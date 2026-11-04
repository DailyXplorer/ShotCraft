//! What a window capture looks like once it is taken: the "Window Screenshot Background" setting.
//!
//! `screencapture -l<id>` delivers the window inside a margin that holds its soft drop shadow
//! (transparent where there is no shadow) with rounded, transparent corners. Four looks:
//!
//! * Transparent: that picture as it is, without the fully transparent rows and columns.
//! * Trim shadow: captured with `-o` (no shadow) and cut tight, corners stay rounded.
//! * Solid / Wallpaper: the window and its shadow laid on a colour or on the desktop wallpaper.
//!   The picture is opaque and rectangular. Its padding is the measured shadow margin; the shadow
//!   falls further down than up, so top and bottom (and left and right) get the larger of the two:
//!   the window ends up centred in the picture.
//!
//! Everything here is pure, so it runs and is tested on any OS.

use crate::model::Image;
use std::sync::Arc;

/// Alpha from which a pixel belongs to the window itself rather than to its shadow.
const WINDOW_ALPHA: u8 = 250;

/// Pixels of transparent or shadow margin around the window, per side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Margins {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

/// Bounding box `(x0, y0, x1, y1)`, right and bottom exclusive, of the pixels with `alpha >= min`.
fn alpha_bounds(img: &Image, min: u8) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (img.w as usize, img.h as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        let row = &img.px[y * w * 4..(y + 1) * w * 4];
        let first = row.chunks_exact(4).position(|p| p[3] >= min);
        let Some(first) = first else { continue };
        let last = row.chunks_exact(4).rposition(|p| p[3] >= min).unwrap_or(first);
        x0 = x0.min(first);
        x1 = x1.max(last + 1);
        y0 = y0.min(y);
        y1 = y + 1;
    }
    (x0 != usize::MAX).then_some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

/// How much shadow margin each side of a window capture has: the distance between the window
/// (the opaque part, rounded corners included) and the edge of the picture. Zero everywhere for a
/// picture without a margin, and for one with no opaque pixel at all.
pub fn shadow_paddings(img: &Image) -> Margins {
    match alpha_bounds(img, WINDOW_ALPHA) {
        Some((x0, y0, x1, y1)) => Margins { top: y0, right: img.w - x1, bottom: img.h - y1, left: x0 },
        None => Margins::default(),
    }
}

/// Does the picture have a see-through margin (a shadow) around its window?
pub fn is_edge_transparent(img: &Image) -> bool {
    shadow_paddings(img) != Margins::default()
}

/// The picture without its fully transparent outer rows and columns. The shadow stays.
pub fn trim_transparent(img: &Image) -> Image {
    match alpha_bounds(img, 1) {
        Some((x0, y0, x1, y1)) if (x0, y0, x1, y1) != (0, 0, img.w, img.h) => crop(img, x0, y0, x1 - x0, y1 - y0),
        _ => img.clone(),
    }
}

fn crop(img: &Image, x: u32, y: u32, w: u32, h: u32) -> Image {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * img.w + x) * 4) as usize;
        out.extend_from_slice(&img.px[start..start + (w * 4) as usize]);
    }
    Image { w, h, px: Arc::new(out) }
}

/// Where the capture goes on the canvas of a Solid or Wallpaper picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub w: u32,
    pub h: u32,
    /// Top left corner of the capture on the canvas.
    pub x: u32,
    pub y: u32,
    pub margins: Margins,
}

/// The canvas: the window with the larger of its two margins on both opposite sides.
pub fn layout(img: &Image) -> Layout {
    let m = shadow_paddings(img);
    let (pad_x, pad_y) = (m.left.max(m.right), m.top.max(m.bottom));
    let (x, y) = (pad_x - m.left, pad_y - m.top);
    Layout { w: img.w + x + (pad_x - m.right), h: img.h + y + (pad_y - m.bottom), x, y, margins: m }
}

/// The capture over `bg(x, y)` (canvas coordinates): an opaque picture of the canvas size.
fn compose(img: &Image, lay: &Layout, bg: impl Fn(u32, u32) -> [u8; 3]) -> Image {
    let mut out = Vec::with_capacity((lay.w * lay.h * 4) as usize);
    for y in 0..lay.h {
        for x in 0..lay.w {
            let b = bg(x, y);
            let inside = x >= lay.x && y >= lay.y && x < lay.x + img.w && y < lay.y + img.h;
            if inside {
                let i = (((y - lay.y) * img.w + (x - lay.x)) * 4) as usize;
                let s = &img.px[i..i + 4];
                let a = s[3] as u32;
                for c in 0..3 {
                    out.push(((s[c] as u32 * a + b[c] as u32 * (255 - a) + 127) / 255) as u8);
                }
            } else {
                out.extend_from_slice(&b);
            }
            out.push(255);
        }
    }
    Image { w: lay.w, h: lay.h, px: Arc::new(out) }
}

/// Solid mode: window and shadow on a colour.
pub fn on_color(img: &Image, rgb: [u8; 3]) -> Image {
    compose(img, &layout(img), |_, _| rgb)
}

/// Wallpaper mode: window and shadow on `backdrop`, a picture of the canvas size (`layout(img)`).
pub fn on_picture(img: &Image, backdrop: &Image) -> Image {
    let lay = layout(img);
    debug_assert_eq!((backdrop.w, backdrop.h), (lay.w, lay.h));
    compose(img, &lay, |x, y| {
        let p = backdrop.get(x as i32, y as i32);
        [p[0], p[1], p[2]]
    })
}

/// Flat colour under a picture that may have see-through pixels (for formats without alpha).
pub fn flatten(img: &Image, bg: [u8; 3]) -> Image {
    let lay = Layout { w: img.w, h: img.h, x: 0, y: 0, margins: Margins::default() };
    compose(img, &lay, |_, _| bg)
}

/// A rectangle in some pixel or point space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The wallpaper area that fills a `w` × `h` picture when no position is known: scaled to cover, centred.
fn cover_region(src: (u32, u32), w: u32, h: u32) -> Region {
    let (sw, sh) = (src.0 as f64, src.1 as f64);
    let k = (sw / w as f64).min(sh / h as f64);
    let (rw, rh) = (w as f64 * k, h as f64 * k);
    Region { x: (sw - rw) / 2.0, y: (sh - rh) / 2.0, w: rw, h: rh }
}

/// The desktop picture is scaled to fill its display and centred; `r`, in display pixels, is mapped to
/// the matching area of the wallpaper file.
fn fill_region(src: (u32, u32), display: (f64, f64), r: Region) -> Region {
    let (sw, sh) = (src.0 as f64, src.1 as f64);
    let s = (display.0 / sw).max(display.1 / sh);
    let (off_x, off_y) = ((sw * s - display.0) / 2.0, (sh * s - display.1) / 2.0);
    Region { x: (r.x + off_x) / s, y: (r.y + off_y) / s, w: r.w / s, h: r.h / s }
}

/// `region` of `src` resampled to `w` × `h`: bilinear, averaged over several taps when it shrinks.
/// Positions outside the source take its nearest edge pixel.
fn sample_region(src: &Image, region: Region, w: u32, h: u32) -> Image {
    let (step_x, step_y) = (region.w / w as f64, region.h / h as f64);
    let taps = |step: f64| (step.ceil() as usize).clamp(1, 4);
    let (nx, ny) = (taps(step_x), taps(step_y));
    let bilinear = |fx: f64, fy: f64| -> [f64; 3] {
        let (fx, fy) = (fx.clamp(0.0, src.w as f64 - 1.0), fy.clamp(0.0, src.h as f64 - 1.0));
        let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
        let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
        let mut acc = [0.0; 3];
        for (dx, dy, wt) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
            let p = src.get(x0 + dx, y0 + dy);
            for c in 0..3 {
                acc[c] += p[c] as f64 * wt;
            }
        }
        acc
    };
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for oy in 0..h {
        for ox in 0..w {
            let mut sum = [0.0; 3];
            for j in 0..ny {
                for i in 0..nx {
                    let fx = region.x + (ox as f64 + (i as f64 + 0.5) / nx as f64) * step_x - 0.5;
                    let fy = region.y + (oy as f64 + (j as f64 + 0.5) / ny as f64) * step_y - 0.5;
                    let p = bilinear(fx, fy);
                    for c in 0..3 {
                        sum[c] += p[c];
                    }
                }
            }
            let n = (nx * ny) as f64;
            out.extend(sum.iter().map(|v| (v / n).round().clamp(0.0, 255.0) as u8));
            out.push(255);
        }
    }
    Image { w, h, px: Arc::new(out) }
}

/// Where a window capture sits in global points, when macOS gave the rectangle it stored on the
/// file: either the window's frame or the whole capture with its shadow. Returns the top left corner
/// of the canvas of `lay` in global points. `None` when the rectangle fits neither.
pub fn canvas_origin(frame: (f64, f64, f64, f64), px_per_point: f64, img: &Image, lay: &Layout) -> Option<(f64, f64)> {
    let m = lay.margins;
    let (core_w, core_h) = ((img.w - m.left - m.right) as f64, (img.h - m.top - m.bottom) as f64);
    let (fw, fh) = (frame.2 * px_per_point, frame.3 * px_per_point);
    let near = |a: f64, b: f64| (a - b).abs() <= 3.0;
    let (cx, cy) = if near(fw, core_w) && near(fh, core_h) {
        // The window's own frame.
        (lay.margins.left + lay.x, lay.margins.top + lay.y)
    } else if near(fw, img.w as f64) && near(fh, img.h as f64) {
        // The whole capture, shadow included.
        (lay.x, lay.y)
    } else {
        return None;
    };
    Some((frame.0 - cx as f64 / px_per_point, frame.1 - cy as f64 / px_per_point))
}

/// The display a wallpaper is on, for `wallpaper_canvas`.
#[derive(Clone, Copy, Debug)]
pub struct DisplayPlace {
    /// Frame in global points.
    pub frame: (f64, f64, f64, f64),
    pub px_per_point: f64,
}

/// The wallpaper area behind the canvas, as a picture of the canvas size. With the canvas position
/// (`origin`, global points) it is what lies under the window on its display; without it, the wallpaper
/// scaled to cover the picture.
pub fn wallpaper_canvas(wall: &Image, lay: &Layout, origin: Option<(f64, f64)>, display: Option<DisplayPlace>) -> Image {
    let src = (wall.w, wall.h);
    let region = match (origin, display) {
        (Some(o), Some(d)) => {
            let s = d.px_per_point;
            let on_display = Region { x: (o.0 - d.frame.0) * s, y: (o.1 - d.frame.1) * s, w: lay.w as f64, h: lay.h as f64 };
            fill_region(src, (d.frame.2 * s, d.frame.3 * s), on_display)
        }
        _ => cover_region(src, lay.w, lay.h),
    };
    sample_region(wall, region, lay.w, lay.h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::{save_png, window_capture, with_rig};
    use crate::model::Doc;

    /// A window capture shaped like the real thing: an opaque rounded body, a soft shadow that falls
    /// further down than up, transparent corners.
    fn synthetic() -> Image {
        window_capture(300, 200, 1)
    }

    fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
        img.get(x as i32, y as i32)
    }

    fn flat(w: u32, h: u32, c: [u8; 4]) -> Image {
        Image { w, h, px: Arc::new((0..w * h).flat_map(|_| c).collect()) }
    }

    #[test]
    fn margins_of_the_synthetic_window_are_the_shadow_margins() {
        let img = synthetic();
        let m = shadow_paddings(&img);
        // window_capture: 30 left and right, 20 top, 46 bottom.
        assert!((29..=31).contains(&m.left), "{m:?}");
        assert!((29..=31).contains(&m.right), "{m:?}");
        assert!((19..=21).contains(&m.top), "{m:?}");
        assert!((45..=47).contains(&m.bottom), "{m:?}");
        assert!(is_edge_transparent(&img));
    }

    #[test]
    fn a_picture_without_margin_has_no_paddings() {
        let img = flat(50, 40, [10, 20, 30, 255]);
        assert_eq!(shadow_paddings(&img), Margins::default());
        assert!(!is_edge_transparent(&img));
        // Rounded corners alone are not a shadow: the window still touches the four edges.
        let mut corner = flat(50, 40, [10, 20, 30, 255]);
        Arc::make_mut(&mut corner.px)[3] = 0;
        assert!(!is_edge_transparent(&corner));
        // Nothing opaque at all: no window to measure.
        assert_eq!(shadow_paddings(&flat(20, 20, [0, 0, 0, 0])), Margins::default());
        assert_eq!(shadow_paddings(&flat(20, 20, [0, 0, 0, 100])), Margins::default());
    }

    #[test]
    fn measures_a_margin_made_of_a_gaussian_like_falloff() {
        // 10 px of falloff on every side of an opaque 20 × 10 body.
        let (w, h) = (40u32, 30u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = ((10 - x as i32).max(x as i32 - 29).max(0), (10 - y as i32).max(y as i32 - 19).max(0));
                let d = dx.max(dy);
                let a = if d == 0 { 255 } else { (200 - d * 20).max(0) as u8 };
                px[((y * w + x) * 4 + 3) as usize] = a;
            }
        }
        let img = Image { w, h, px: Arc::new(px) };
        assert_eq!(shadow_paddings(&img), Margins { top: 10, right: 10, bottom: 10, left: 10 });
    }

    #[test]
    fn trimming_cuts_only_fully_transparent_margins() {
        // 3 empty columns left, 2 empty rows top, a faint shadow pixel at the far corner.
        let mut img = flat(20, 12, [0, 0, 0, 0]);
        {
            let px = Arc::make_mut(&mut img.px);
            for y in 2..12usize {
                for x in 3..19usize {
                    px[(y * 20 + x) * 4 + 3] = 255;
                }
            }
            px[(11 * 20 + 19) * 4 + 3] = 5;
        }
        let t = trim_transparent(&img);
        assert_eq!((t.w, t.h), (17, 10), "the shadow pixel at the corner is kept");
        assert_eq!(px(&t, 0, 0)[3], 255);
        assert_eq!(px(&t, 16, 9)[3], 5);
        // Nothing to trim: same size. Nothing visible at all: untouched.
        assert_eq!((trim_transparent(&t).w, trim_transparent(&t).h), (17, 10));
        assert_eq!(trim_transparent(&flat(9, 9, [0, 0, 0, 0])).w, 9);
    }

    #[test]
    fn solid_background_is_opaque_and_centres_the_window() {
        let img = synthetic();
        let m = shadow_paddings(&img);
        let bg = [196, 198, 200];
        let out = on_color(&img, bg);
        let (pad_x, pad_y) = (m.left.max(m.right), m.top.max(m.bottom));
        let (core_w, core_h) = (img.w - m.left - m.right, img.h - m.top - m.bottom);
        assert_eq!((out.w, out.h), (core_w + 2 * pad_x, core_h + 2 * pad_y));
        assert!(!out.has_alpha(), "opaque rectangle");
        // Top and bottom (and left and right) paddings are the same.
        let lay = layout(&img);
        assert_eq!(lay.y + m.top, pad_y);
        assert_eq!(out.h - (lay.y + img.h - m.bottom), pad_y);
        // Corner: the colour. Centre: the window pixel. Rounded window corner: the colour too.
        assert_eq!(&px(&out, 0, 0)[..3], &bg);
        assert_eq!(&px(&out, out.w - 1, out.h - 1)[..3], &bg);
        let (cx, cy) = (img.w / 2, img.h / 2);
        assert_eq!(px(&out, lay.x + cx, lay.y + cy), px(&img, cx, cy));
        let (wx, wy) = (lay.x + m.left, lay.y + m.top);
        let corner = px(&out, wx, wy);
        assert!((0..3).all(|c| (corner[c] as i32 - bg[c] as i32).abs() <= 16), "the rounded corner shows the colour (plus a little shadow): {corner:?}");
    }

    #[test]
    fn shadow_pixels_are_blended_with_the_colour() {
        // Body 4 × 4 opaque white; one margin pixel of black at alpha 51 (20 %).
        let mut img = flat(8, 8, [0, 0, 0, 0]);
        {
            let px = Arc::make_mut(&mut img.px);
            for y in 2..6usize {
                for x in 2..6usize {
                    px[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
            px[(6 * 8 + 3) * 4..(6 * 8 + 3) * 4 + 4].copy_from_slice(&[0, 0, 0, 51]);
        }
        let out = on_color(&img, [200, 100, 50]);
        let lay = layout(&img);
        // Margins are 2 / 2 / 2 / 2: nothing to add. Blend = 0.2 * black + 0.8 * colour.
        assert_eq!((lay.w, lay.h), (8, 8));
        assert_eq!(&px(&out, 3, 6)[..3], &[160, 80, 40]);
        assert_eq!(&px(&out, 3, 3)[..3], &[255, 255, 255]);
    }

    #[test]
    fn opaque_picture_without_a_shadow_keeps_its_size() {
        let img = flat(30, 20, [9, 9, 9, 255]);
        let out = on_color(&img, [1, 2, 3]);
        assert_eq!((out.w, out.h), (30, 20));
        assert_eq!(out.px, img.px);
    }

    #[test]
    fn flatten_puts_transparent_pixels_on_the_colour() {
        let mut img = flat(2, 1, [200, 100, 0, 255]);
        Arc::make_mut(&mut img.px)[4..8].copy_from_slice(&[0, 0, 0, 0]);
        let out = flatten(&img, [255, 255, 255]);
        assert_eq!(px(&out, 0, 0), [200, 100, 0, 255]);
        assert_eq!(px(&out, 1, 0), [255, 255, 255, 255]);
    }

    /// A wallpaper whose colour tells the position: red grows to the right, green downwards.
    fn gradient(w: u32, h: u32) -> Image {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 0, 255]);
            }
        }
        Image { w, h, px: Arc::new(px) }
    }

    #[test]
    fn wallpaper_without_a_position_covers_the_picture() {
        let wall = gradient(400, 200);
        let img = synthetic();
        let lay = layout(&img);
        let out = wallpaper_canvas(&wall, &lay, None, None);
        assert_eq!((out.w, out.h), (lay.w, lay.h));
        // Centred cover: the middle of the wallpaper is in the middle of the picture, nothing stretched.
        let mid = px(&out, out.w / 2, out.h / 2);
        assert!((mid[0] as i32 - 127).abs() <= 3 && (mid[1] as i32 - 127).abs() <= 3, "{mid:?}");
        let (a, b) = (px(&out, 0, 0), px(&out, out.w - 1, out.h - 1));
        assert!(a[0] < b[0] && a[1] < b[1]);
    }

    #[test]
    fn wallpaper_with_a_position_shows_what_is_under_the_window() {
        // A 2× display of 100 × 50 points with a wallpaper of the same size in pixels (scale 2 → 200 × 100).
        let wall = gradient(200, 100);
        let display = DisplayPlace { frame: (0.0, 0.0, 100.0, 50.0), px_per_point: 2.0 };
        let img = synthetic();
        let lay = layout(&img);
        // Canvas placed at (20, 10) points: its pixel (0, 0) is display pixel (40, 20).
        let out = wallpaper_canvas(&wall, &lay, Some((20.0, 10.0)), Some(display));
        let a = px(&out, 0, 0);
        assert!((a[0] as i32 - 40 * 255 / 199).abs() <= 2 && (a[1] as i32 - 20 * 255 / 99).abs() <= 2, "{a:?}");
        // A display on the right of the main one: coordinates are relative to its own frame.
        let right = DisplayPlace { frame: (100.0, 0.0, 100.0, 50.0), px_per_point: 2.0 };
        let b = wallpaper_canvas(&wall, &lay, Some((120.0, 10.0)), Some(right));
        assert_eq!(px(&b, 0, 0), a);
    }

    #[test]
    fn a_wallpaper_of_another_aspect_is_scaled_to_fill_the_display() {
        // Square wallpaper on a 2:1 display: filled by width, the top and bottom quarter cut off.
        let wall = gradient(100, 100);
        let display = DisplayPlace { frame: (0.0, 0.0, 200.0, 100.0), px_per_point: 1.0 };
        let lay = Layout { w: 20, h: 10, x: 0, y: 0, margins: Margins::default() };
        let out = wallpaper_canvas(&wall, &lay, Some((0.0, 0.0)), Some(display));
        let a = px(&out, 0, 0);
        assert!(a[0] < 5 && (a[1] as i32 - 64).abs() <= 6, "top left of the display = 25 % down the wallpaper: {a:?}");
    }

    #[test]
    fn the_stored_rectangle_places_the_canvas() {
        let img = synthetic();
        let lay = layout(&img);
        let m = lay.margins;
        let ppp = 1.0;
        // The window frame only.
        let frame = (500.0, 300.0, (img.w - m.left - m.right) as f64, (img.h - m.top - m.bottom) as f64);
        let o = canvas_origin(frame, ppp, &img, &lay).unwrap();
        let pad_x = m.left.max(m.right) as f64;
        let pad_y = m.top.max(m.bottom) as f64;
        assert_eq!(o, (500.0 - pad_x, 300.0 - pad_y));
        // The whole capture with its shadow.
        let full = (470.0, 280.0, img.w as f64, img.h as f64);
        let o = canvas_origin(full, ppp, &img, &lay).unwrap();
        assert_eq!(o, (470.0 - lay.x as f64, 280.0 - lay.y as f64));
        // Retina: the same rectangle in points.
        let o2 = canvas_origin((235.0, 140.0, img.w as f64 / 2.0, img.h as f64 / 2.0), 2.0, &img, &lay).unwrap();
        assert_eq!(o2, (235.0 - lay.x as f64 / 2.0, 140.0 - lay.y as f64 / 2.0));
        // Something else (a rectangle of unknown meaning): no position.
        assert_eq!(canvas_origin((0.0, 0.0, 17.0, 9.0), ppp, &img, &lay), None);
    }

    /// Renders the editor with a window capture in each mode, for the eyes. Optional:
    /// `SHOTCRAFT_TEST_OUT` is the folder, `SHOTCRAFT_TEST_WINDOW_PNG` a real capture with its shadow
    /// (without it a synthetic window), `SHOTCRAFT_TEST_WALLPAPER` any picture.
    #[test]
    fn every_mode_in_the_editor_for_the_eyes() {
        let Some(dir) = std::env::var_os("SHOTCRAFT_TEST_OUT").map(std::path::PathBuf::from) else { return };
        let load = |var: &str| {
            std::env::var_os(var).map(|p| {
                let rgba = image::open(p).unwrap().to_rgba8();
                Image { w: rgba.width(), h: rgba.height(), px: Arc::new(rgba.into_raw()) }
            })
        };
        let shadowed = load("SHOTCRAFT_TEST_WINDOW_PNG").unwrap_or_else(|| window_capture(640, 380, 2));
        let wall = load("SHOTCRAFT_TEST_WALLPAPER").unwrap_or_else(|| gradient(1600, 900));
        let lay = layout(&shadowed);
        let modes: [(&str, Image); 4] = [
            ("transparent", trim_transparent(&shadowed)),
            // Stand-in for the `-o` capture: the window cut out of the shadowed one.
            ("trimmed", {
                let m = lay.margins;
                trim_transparent(&crop(&shadowed, m.left, m.top, shadowed.w - m.left - m.right, shadowed.h - m.top - m.bottom))
            }),
            ("solid", on_color(&shadowed, [0xC4, 0xC6, 0xC8])),
            ("wallpaper", on_picture(&shadowed, &wallpaper_canvas(&wall, &lay, None, None))),
        ];
        for (name, img) in modes {
            let out = with_rig(|rig| {
                let mut app = rig.app();
                app.doc = Some(Doc::new(img.clone(), 2.0));
                rig.render_editor(&mut app, (1100.0, 760.0), 2.0, false)
            });
            save_png(&out, &dir.join(format!("{name}.png")));
            save_png(&img, &dir.join(format!("{name}-picture.png")));
        }
    }
}
