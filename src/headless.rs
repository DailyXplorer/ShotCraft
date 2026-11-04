//! Test-only: draws the editor without a window or a GPU, so a test can look at what the user sees.
//!
//! egui turns a frame into textured triangles. `Soft` plays the part of the GPU backend: it keeps
//! the textures egui uploads and fills the triangles on the CPU, blending like the real backends do
//! (premultiplied colours, in the sRGB-encoded space).
use crate::model::{Doc, Image, R};
use crate::render;
use crate::App;
use eframe::egui::{self, epaint, Color32, Context, RawInput, TextureId, ViewportId};
use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

struct Tex {
    size: [usize; 2],
    px: Vec<Color32>,
    options: egui::TextureOptions,
}

/// A CPU stand-in for the egui rendering backend.
#[derive(Default)]
pub struct Soft {
    textures: HashMap<TextureId, Tex>,
    /// Freed by egui, dropped once the frame that may still use them is painted.
    freed: Vec<TextureId>,
}

impl Soft {
    fn apply(&mut self, delta: &epaint::textures::TexturesDelta) {
        self.freed.extend(delta.free.iter().copied());
        for (id, deltas) in &delta.set {
            for d in deltas {
                let egui::ImageData::Color(img) = &d.image;
                match d.pos {
                    None => {
                        self.textures.insert(
                            *id,
                            Tex {
                                size: img.size,
                                px: img.pixels.clone(),
                                options: d.options,
                            },
                        );
                    }
                    Some([x0, y0]) => {
                        if let Some(t) = self.textures.get_mut(id) {
                            for y in 0..img.size[1] {
                                for x in 0..img.size[0] {
                                    t.px[(y0 + y) * t.size[0] + x0 + x] =
                                        img.pixels[y * img.size[0] + x];
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn sample(t: &Tex, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (t.size[0] as i32, t.size[1] as i32);
        let fetch = |x: i32, y: i32| -> [f32; 4] {
            let (x, y) = if t.options.wrap_mode == egui::TextureWrapMode::Repeat {
                (x.rem_euclid(w), y.rem_euclid(h))
            } else {
                (x.clamp(0, w - 1), y.clamp(0, h - 1))
            };
            let c = t.px[(y * w + x) as usize];
            [c.r() as f32, c.g() as f32, c.b() as f32, c.a() as f32]
        };
        let (fx, fy) = (u * w as f32, v * h as f32);
        if t.options.magnification == egui::TextureFilter::Nearest {
            return fetch(fx.floor() as i32, fy.floor() as i32);
        }
        let (gx, gy) = (fx - 0.5, fy - 0.5);
        let (x0, y0) = (gx.floor(), gy.floor());
        let (ax, ay) = (gx - x0, gy - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let (p00, p10, p01, p11) = (
            fetch(x0, y0),
            fetch(x0 + 1, y0),
            fetch(x0, y0 + 1),
            fetch(x0 + 1, y0 + 1),
        );
        let mut out = [0.0; 4];
        for i in 0..4 {
            out[i] = (p00[i] * (1.0 - ax) + p10[i] * ax) * (1.0 - ay)
                + (p01[i] * (1.0 - ax) + p11[i] * ax) * ay;
        }
        out
    }

    /// Paints `prims` over `fb` (RGBA8, `w × h` pixels, already holding the clear colour).
    fn paint(
        &self,
        fb: &mut [f32],
        w: usize,
        h: usize,
        ppp: f32,
        prims: &[egui::ClippedPrimitive],
    ) {
        for prim in prims {
            let epaint::Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            let Some(tex) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            let clip = prim.clip_rect;
            let (cx0, cy0) = (
                (clip.min.x * ppp).round().max(0.0) as usize,
                (clip.min.y * ppp).round().max(0.0) as usize,
            );
            let (cx1, cy1) = (
                ((clip.max.x * ppp).round() as usize).min(w),
                ((clip.max.y * ppp).round() as usize).min(h),
            );
            for tri in mesh.indices.chunks_exact(3) {
                let v = [
                    mesh.vertices[tri[0] as usize],
                    mesh.vertices[tri[1] as usize],
                    mesh.vertices[tri[2] as usize],
                ];
                let p = v.map(|v| (v.pos.x * ppp, v.pos.y * ppp));
                let area =
                    (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
                if area.abs() < 1e-6 {
                    continue;
                }
                let minx = p
                    .iter()
                    .map(|q| q.0)
                    .fold(f32::MAX, f32::min)
                    .floor()
                    .max(cx0 as f32) as usize;
                let maxx = (p
                    .iter()
                    .map(|q| q.0)
                    .fold(f32::MIN, f32::max)
                    .ceil()
                    .max(0.0) as usize)
                    .min(cx1);
                let miny = p
                    .iter()
                    .map(|q| q.1)
                    .fold(f32::MAX, f32::min)
                    .floor()
                    .max(cy0 as f32) as usize;
                let maxy = (p
                    .iter()
                    .map(|q| q.1)
                    .fold(f32::MIN, f32::max)
                    .ceil()
                    .max(0.0) as usize)
                    .min(cy1);
                for y in miny..maxy {
                    for x in minx..maxx {
                        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        let w0 =
                            ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                        let w1 =
                            ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                            continue;
                        }
                        let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                        let t = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                        let texel = Self::sample(tex, u, t);
                        let mut src = [0.0f32; 4];
                        for c in 0..4 {
                            let vc = |k: usize| {
                                [
                                    v[k].color.r(),
                                    v[k].color.g(),
                                    v[k].color.b(),
                                    v[k].color.a(),
                                ][c] as f32
                            };
                            let col = w0 * vc(0) + w1 * vc(1) + w2 * vc(2);
                            src[c] = texel[c] / 255.0 * col / 255.0;
                        }
                        let o = (y * w + x) * 4;
                        for c in 0..4 {
                            fb[o + c] = src[c] + fb[o + c] * (1.0 - src[3]);
                        }
                    }
                }
            }
        }
    }
}

/// One egui context and one backend for every test. The icon cache is process-wide and holds
/// textures of the context that first loaded them, so each test must draw with that same context.
pub struct Rig {
    pub ctx: Context,
    soft: Soft,
    frame: u32,
}

static RIG: std::sync::OnceLock<std::sync::Mutex<Rig>> = std::sync::OnceLock::new();

/// Runs `f` with the shared rig; tests that draw take turns.
pub fn with_rig<T>(f: impl FnOnce(&mut Rig) -> T) -> T {
    let rig = RIG.get_or_init(|| {
        let ctx = Context::default();
        crate::setup_fonts(&ctx);
        std::sync::Mutex::new(Rig {
            ctx,
            soft: Soft::default(),
            frame: 0,
        })
    });
    let mut guard = rig.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

impl Rig {
    /// An app with no window behind it.
    pub fn app(&self) -> App {
        App::headless(self.ctx.clone())
    }

    /// Runs the editor for a few frames and returns what it drew.
    pub fn render_editor(
        &mut self,
        app: &mut App,
        size: (f32, f32),
        ppp: f32,
        dark: bool,
    ) -> Image {
        self.ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        self.ctx.set_pixels_per_point(ppp);
        let mut last = None;
        for _ in 0..4 {
            let mut input = RawInput::default();
            input.screen_rect = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size.0, size.1),
            ));
            self.frame += 1;
            input.time = Some(self.frame as f64 * 0.05);
            input.viewport_id = ViewportId::ROOT;
            let mut out = self.ctx.run_ui(input, |ui| app.editor_ui(ui));
            self.soft.apply(&out.textures_delta);
            out.textures_delta.clear();
            last = Some((out.shapes, out.pixels_per_point));
        }
        let (shapes, ppp) = last.unwrap();
        let prims = self.ctx.tessellate(shapes, ppp);
        let (w, h) = ((size.0 * ppp) as usize, (size.1 * ppp) as usize);
        let mut fb = vec![0.0f32; w * h * 4];
        for px in fb.chunks_exact_mut(4) {
            px[3] = 1.0;
        }
        self.soft.paint(&mut fb, w, h, ppp, &prims);
        for id in std::mem::take(&mut self.soft.freed) {
            self.soft.textures.remove(&id);
        }
        let bytes = fb
            .iter()
            .map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
            .collect();
        Image {
            w: w as u32,
            h: h as u32,
            px: Arc::new(bytes),
        }
    }
}

pub fn save_png(img: &Image, path: &std::path::Path) {
    let rgba = image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).unwrap();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    rgba.save(path).unwrap();
}

/// A picture shaped like what `screencapture -l<id>` writes for a window: the window with
/// rounded, transparent corners, inside a margin that holds its soft drop shadow (transparent
/// where there is no shadow). `w` and `h` are the window size in points.
pub fn window_capture(w: u32, h: u32, scale: u32) -> Image {
    let s = scale as f32;
    let (left, right, top, bottom) = (30.0 * s, 30.0 * s, 20.0 * s, 46.0 * s);
    let (ww, wh) = (w as f32 * s, h as f32 * s);
    let (iw, ih) = ((ww + left + right) as u32, (wh + top + bottom) as u32);
    let radius = 10.0 * s;
    let rounded = |x: f32, y: f32, w: f32, h: f32, r: f32| {
        let mut pb = PathBuilder::new();
        pb.move_to(x + r, y);
        pb.line_to(x + w - r, y);
        pb.quad_to(x + w, y, x + w, y + r);
        pb.line_to(x + w, y + h - r);
        pb.quad_to(x + w, y + h, x + w - r, y + h);
        pb.line_to(x + r, y + h);
        pb.quad_to(x, y + h, x, y + h - r);
        pb.line_to(x, y + r);
        pb.quad_to(x, y, x + r, y);
        pb.close();
        pb.finish().unwrap()
    };
    let paint = |r: u8, g: u8, b: u8, a: u8| {
        let mut p = Paint::default();
        p.set_color(Color::from_rgba8(r, g, b, a));
        p.anti_alias = true;
        p
    };
    let mut pm = Pixmap::new(iw, ih).unwrap();
    // Shadow: the window's outline, a little lower, blurred.
    pm.fill_path(
        &rounded(left, top + 10.0 * s, ww, wh, radius),
        &paint(0, 0, 0, 120),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    render::box_blur(
        &mut pm,
        &R {
            x0: 0.0,
            y0: 0.0,
            x1: iw as f32,
            y1: ih as f32,
        },
        (9.0 * s) as i32,
        3,
    );
    // The window itself: a clear cut-out, then its content.
    let body = rounded(left, top, ww, wh, radius);
    let mut clear = paint(0, 0, 0, 255);
    clear.blend_mode = tiny_skia::BlendMode::Clear;
    pm.fill_path(
        &body,
        &clear,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    pm.fill_path(
        &body,
        &paint(255, 255, 255, 255),
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let mut window = Pixmap::new(iw, ih).unwrap();
    let rect = |x: f32, y: f32, w: f32, h: f32| tiny_skia::Rect::from_xywh(x, y, w, h).unwrap();
    let mut fill = |pm: &mut Pixmap, r: tiny_skia::Rect, c: (u8, u8, u8)| {
        pm.fill_rect(r, &paint(c.0, c.1, c.2, 255), Transform::identity(), None)
    };
    fill(
        &mut window,
        rect(left, top, ww, 52.0 * s),
        (0xEC, 0xEC, 0xEE),
    );
    fill(
        &mut window,
        rect(left, top + 52.0 * s, 190.0 * s, wh - 52.0 * s),
        (0xF1, 0xF1, 0xF4),
    );
    for (i, c) in [(0xFF, 0x5F, 0x57), (0xFE, 0xBC, 0x2E), (0x28, 0xC8, 0x40)]
        .into_iter()
        .enumerate()
    {
        let mut pb = PathBuilder::new();
        pb.push_circle(left + (20.0 + 20.0 * i as f32) * s, top + 26.0 * s, 6.0 * s);
        window.fill_path(
            &pb.finish().unwrap(),
            &paint(c.0, c.1, c.2, 255),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    for row in 0..8 {
        let y = top + (76.0 + 34.0 * row as f32) * s;
        fill(
            &mut window,
            rect(
                left + 212.0 * s,
                y,
                (260.0 + 40.0 * ((row * 5) % 4) as f32) * s,
                10.0 * s,
            ),
            (0x9A, 0x9A, 0xA2),
        );
        fill(
            &mut window,
            rect(left + 16.0 * s, y, 120.0 * s, 10.0 * s),
            (0xB8, 0xB8, 0xC0),
        );
    }
    // Keep the window's content only inside its rounded outline.
    let mask = {
        let mut m = tiny_skia::Mask::new(iw, ih).unwrap();
        m.fill_path(&body, FillRule::Winding, true, Transform::identity());
        m
    };
    pm.draw_pixmap(
        0,
        0,
        window.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        Some(&mask),
    );
    render::from_pixmap(&pm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{th, STATUS_H, TOOLBAR_H};
    use crate::model::{CHECKER_DARK, CHECKER_LIGHT};

    const SIZE: (f32, f32) = (1100.0, 760.0);
    const PPP: f32 = 2.0;

    fn out_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("SHOTCRAFT_TEST_OUT").map(std::path::PathBuf::from)
    }

    struct Shot {
        img: Image,
        /// Where the picture is on screen, in points.
        picture: egui::Rect,
        /// The canvas (the area between the toolbar and the status bar), in points.
        canvas: egui::Rect,
    }

    fn editor_with(img: Image, dark: bool) -> Shot {
        with_rig(|rig| {
            let mut app = rig.app();
            app.doc = Some(Doc::new(img, PPP));
            let shot = rig.render_editor(&mut app, SIZE, PPP, dark);
            let view = app.doc.as_ref().unwrap().view_rect();
            Shot {
                img: shot,
                picture: app.ed.rect_screen(&view),
                canvas: egui::Rect::from_min_max(
                    egui::pos2(0.0, TOOLBAR_H + 1.0),
                    egui::pos2(SIZE.0, SIZE.1 - STATUS_H - 1.0),
                ),
            }
        })
    }

    fn at(shot: &Shot, x: f32, y: f32) -> [u8; 3] {
        let (px, py) = ((x * PPP) as u32, (y * PPP) as u32);
        let i = ((py * shot.img.w + px) * 4) as usize;
        [shot.img.px[i], shot.img.px[i + 1], shot.img.px[i + 2]]
    }

    fn grey(v: f32) -> [u8; 3] {
        let g = (v * 255.0).round() as u8;
        [g, g, g]
    }

    fn canvas_color(dark: bool) -> [u8; 3] {
        let c = th(dark).canvas;
        [c.r(), c.g(), c.b()]
    }

    fn line_color(dark: bool) -> [u8; 3] {
        let c = th(dark).line;
        [c.r(), c.g(), c.b()]
    }

    #[test]
    fn window_capture_has_a_see_through_margin_and_corners() {
        let win = window_capture(640, 380, 2);
        assert!(win.has_alpha());
        assert_eq!(win.get(0, 0)[3], 0, "outer corner is transparent");
        let (l, t) = (60, 40);
        assert!(
            win.get(l, t)[3] < 64,
            "the window's rounded corner is see-through (only shadow shows), got alpha {}",
            win.get(l, t)[3]
        );
        assert_eq!(
            win.get(l + 300, t + 300)[3],
            255,
            "the window body is opaque"
        );
        let shadow = win.get(win.w as i32 / 2, (t + 760 + 30) as i32)[3];
        assert!(
            shadow > 0 && shadow < 255,
            "a soft shadow under the window, got alpha {shadow}"
        );
    }

    #[test]
    fn a_window_capture_sits_on_a_plain_canvas_with_one_checkerboard_behind_it() {
        for dark in [false, true] {
            let shot = editor_with(window_capture(640, 380, 2), dark);
            let (canvas, checker) = (
                canvas_color(dark),
                if dark { CHECKER_DARK } else { CHECKER_LIGHT },
            );
            let p = shot.picture;
            assert!(
                shot.canvas.contains(p.min) && shot.canvas.contains(p.max),
                "the picture fits the canvas: {p:?}"
            );

            // Around the picture the canvas is one flat colour: no checkerboard, no outline.
            for (x, y) in [
                (p.min.x - 40.0, p.center().y),
                (p.max.x + 40.0, p.center().y),
                (p.center().x, p.min.y - 20.0),
                (p.center().x, p.max.y + 20.0),
            ] {
                assert_eq!(at(&shot, x, y), canvas, "canvas at ({x}, {y}), dark={dark}");
            }
            assert_eq!(
                at(&shot, p.min.x - 0.5, p.center().y),
                canvas,
                "no outline on the left edge, dark={dark}"
            );
            assert_eq!(
                at(&shot, p.center().x, p.max.y + 0.5),
                canvas,
                "no outline on the bottom edge, dark={dark}"
            );
            assert_ne!(at(&shot, p.min.x - 0.5, p.center().y), line_color(dark));

            // In the picture's own transparent corner there is one checkerboard: two alternating greys.
            let (a, b) = (
                at(&shot, p.min.x + 2.0, p.min.y + 2.0),
                at(&shot, p.min.x + 10.0, p.min.y + 2.0),
            );
            let (c, d) = (
                at(&shot, p.min.x + 2.0, p.min.y + 10.0),
                at(&shot, p.min.x + 10.0, p.min.y + 10.0),
            );
            assert_eq!(
                a,
                grey(checker.1),
                "first square starts at the picture's corner, dark={dark}"
            );
            assert_eq!(b, grey(checker.0));
            assert_eq!(c, grey(checker.0));
            assert_eq!(d, grey(checker.1));

            // The window body is drawn as is.
            assert_eq!(
                at(&shot, p.center().x + 60.0, p.center().y + 40.0),
                [255, 255, 255],
                "the window body, dark={dark}"
            );

            if let Some(dir) = out_dir() {
                save_png(
                    &shot.img,
                    &dir.join(format!("after-{}.png", if dark { "dark" } else { "light" })),
                );
            }
        }
    }

    #[test]
    fn an_opaque_picture_keeps_its_outline_and_no_checkerboard() {
        let (w, h) = (500u32, 300u32);
        let px: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % w) as u8, 120, 200, 255])
            .collect();
        let shot = editor_with(
            Image {
                w,
                h,
                px: Arc::new(px),
            },
            false,
        );
        let p = shot.picture;
        assert_eq!(at(&shot, p.center().x, p.min.y - 30.0), canvas_color(false));
        assert_eq!(
            at(&shot, p.min.x - 0.5, p.center().y),
            line_color(false),
            "the hairline outline stays around an opaque picture"
        );
        assert_eq!(at(&shot, p.center().x, p.center().y)[1], 120);
    }

    /// Optional: render a real window capture (`SHOTCRAFT_TEST_WINDOW_PNG`) for a visual check.
    #[test]
    fn real_window_capture_for_the_eyes() {
        let (Some(src), Some(dir)) = (std::env::var_os("SHOTCRAFT_TEST_WINDOW_PNG"), out_dir())
        else {
            return;
        };
        let rgba = image::open(src).unwrap().to_rgba8();
        let img = Image {
            w: rgba.width(),
            h: rgba.height(),
            px: Arc::new(rgba.into_raw()),
        };
        let tag = std::env::var("SHOTCRAFT_TEST_TAG").unwrap_or_default();
        let shot = editor_with(img, false);
        save_png(&shot.img, &dir.join(format!("real-{tag}.png")));
    }

    #[test]
    fn export_keeps_the_alpha_of_a_window_capture() {
        let win = window_capture(640, 380, 2);
        let doc = Doc::new(win.clone(), PPP);
        let out = render::export(&doc, &render::Fonts::load());
        assert_eq!((out.w, out.h), (win.w, win.h));
        assert_eq!(out.get(0, 0)[3], 0);
        assert!(out.get(60, 40)[3] < 64, "rounded corner stays see-through");
        assert_eq!(out.get(360, 340)[3], 255);
        let (a, b) = (
            out.get(win.w as i32 / 2, 40 + 760 + 30),
            win.get(win.w as i32 / 2, 40 + 760 + 30),
        );
        assert!(
            (a[3] as i32 - b[3] as i32).abs() <= 1,
            "the shadow keeps its alpha: {a:?} vs {b:?}"
        );
    }
}
