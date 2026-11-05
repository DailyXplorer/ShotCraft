use crate::model::*;
use ab_glyph::{Font, FontVec, OutlineCurve, ScaleFont};
use std::sync::Arc;
use tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, PathSegment, Pixmap, PixmapPaint, Rect, Stroke, StrokeDash, Transform,
};

pub struct Fonts {
    pub sans: FontVec,
    pub regular: FontVec,
    pub medium: FontVec,
    pub goofy: FontVec,
}

fn load_font(path: &str, index: u32) -> Option<FontVec> {
    let data = std::fs::read(path).ok()?;
    FontVec::try_from_vec_and_index(data, index).ok()
}

impl Fonts {
    pub fn load() -> Fonts {
        let dm = || FontVec::try_from_vec(include_bytes!("../assets/fonts/DMSans-Bold.ttf").to_vec()).expect("bundled DM Sans");
        let goofy = load_font("/System/Library/Fonts/Supplemental/ChalkboardSE.ttc", 2)
            .or_else(|| load_font("/System/Library/Fonts/Supplemental/Comic Sans MS Bold.ttf", 0))
            .unwrap_or_else(dm);
        let face = |bytes: &[u8]| FontVec::try_from_vec(bytes.to_vec()).expect("bundled font");
        Fonts {
            sans: dm(),
            regular: face(include_bytes!("../assets/fonts/DMSans-Regular.ttf")),
            medium: face(include_bytes!("../assets/fonts/DMSans-Medium.ttf")),
            goofy,
        }
    }
    /// Annotation text: regular on light colours, medium on dark ones.
    pub fn text(&self, st: &Style) -> &FontVec {
        match st.font {
            FontKind::Sans if luminance(st.color) > TEXT_LIGHT_ABOVE => &self.regular,
            FontKind::Sans => &self.medium,
            FontKind::Goofy => &self.goofy,
        }
    }
    fn get(&self, k: FontKind) -> &FontVec {
        match k {
            FontKind::Sans => &self.sans,
            FontKind::Goofy => &self.goofy,
        }
    }
}

pub fn color(c: Rgba, alpha: f32) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], ((c[3] as f32) * alpha).round().clamp(0.0, 255.0) as u8)
}

fn paint(c: Rgba, alpha: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(c, alpha));
    p.anti_alias = true;
    p
}

pub fn luminance(c: Rgba) -> f32 {
    (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32) / 255.0
}

pub fn is_light(c: Rgba) -> bool {
    (0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32) > 170.0
}

pub fn to_pixmap(img: &Image) -> Pixmap {
    let mut pm = Pixmap::new(img.w.max(1), img.h.max(1)).unwrap();
    let dst = pm.data_mut();
    for (d, s) in dst.chunks_exact_mut(4).zip(img.px.chunks_exact(4)) {
        let a = s[3] as u32;
        d[0] = ((s[0] as u32 * a + 127) / 255) as u8;
        d[1] = ((s[1] as u32 * a + 127) / 255) as u8;
        d[2] = ((s[2] as u32 * a + 127) / 255) as u8;
        d[3] = s[3];
    }
    pm
}

pub fn from_pixmap(pm: &Pixmap) -> Image {
    let mut out = Vec::with_capacity(pm.data().len());
    for s in pm.data().chunks_exact(4) {
        let a = s[3] as u32;
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            out.push(((s[0] as u32 * 255 + a / 2) / a).min(255) as u8);
            out.push(((s[1] as u32 * 255 + a / 2) / a).min(255) as u8);
            out.push(((s[2] as u32 * 255 + a / 2) / a).min(255) as u8);
            out.push(s[3]);
        }
    }
    Image { w: pm.width(), h: pm.height(), px: Arc::new(out) }
}

fn rect(r: &R) -> Option<Rect> {
    Rect::from_ltrb(r.x0, r.y0, r.x1.max(r.x0 + 0.5), r.y1.max(r.y0 + 0.5))
}

fn round_rect_path(r: &R, rad: f32) -> Option<Path> {
    let rad = rad.min(r.w() / 2.0).min(r.h() / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    if rad < 0.5 {
        pb.push_rect(rect(r)?);
        return pb.finish();
    }
    let k = 0.5523 * rad;
    pb.move_to(r.x0 + rad, r.y0);
    pb.line_to(r.x1 - rad, r.y0);
    pb.cubic_to(r.x1 - rad + k, r.y0, r.x1, r.y0 + rad - k, r.x1, r.y0 + rad);
    pb.line_to(r.x1, r.y1 - rad);
    pb.cubic_to(r.x1, r.y1 - rad + k, r.x1 - rad + k, r.y1, r.x1 - rad, r.y1);
    pb.line_to(r.x0 + rad, r.y1);
    pb.cubic_to(r.x0 + rad - k, r.y1, r.x0, r.y1 - rad + k, r.x0, r.y1 - rad);
    pb.line_to(r.x0, r.y0 + rad);
    pb.cubic_to(r.x0, r.y0 + rad - k, r.x0 + rad - k, r.y0, r.x0 + rad, r.y0);
    pb.close();
    pb.finish()
}

fn oval_path(r: &R) -> Option<Path> {
    PathBuilder::from_oval(rect(r)?)
}

/// Unit-square coordinate of the plain oval, pulled towards the edge on the long axis.
fn oval_axis(f: f32, along: f32, across: f32) -> f32 {
    if along <= across {
        return f;
    }
    let k = across / along * OVAL_ASPECT_GAIN + OVAL_ASPECT_BASE;
    if f < 0.5 {
        f * k
    } else {
        1.0 - (1.0 - f) * k
    }
}

fn plain_oval_curves(r: &R) -> [Curve; 8] {
    let (w, h) = (r.w().max(1.0), r.h().max(1.0));
    let at = |(x, y): (f32, f32)| p(r.x0 + oval_axis(x, w, h) * w, r.y0 + oval_axis(y, h, w) * h);
    let mut c = at(OVAL_START);
    OVAL_CURVES.map(|[b, c1, c2]| {
        let curve = Curve { a: c, c1: at(c1), c2: at(c2), b: at(b) };
        c = curve.b;
        curve
    })
}

fn stroke(width: f32, dashed: bool) -> Stroke {
    let mut s = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    if dashed {
        s.dash = StrokeDash::new(vec![width * 2.5 + 2.0, width * 2.0 + 4.0], 0.0);
    }
    s
}

/// Noise behind the hand-drawn shapes: 100 values in 0...1 from an xorshift stream.
fn noise(seed: u64) -> [f32; 100] {
    let mut x = seed | 1;
    std::array::from_fn(|_| {
        x ^= x >> 12;
        x ^= x << 25;
        x = (x ^ (x >> 27)).wrapping_mul(0x2545_f491_4f6c_dd1d);
        (x & 0xffff_ffff) as f32 / 4294967295.0
    })
}

#[derive(Clone, Copy)]
struct Curve {
    a: P,
    c1: P,
    c2: P,
    b: P,
}

/// Rounded outline as eight curves, sides then corners, clockwise from the top left.
/// With `wobble` at zero it is a plain rounded rectangle.
fn outline_curves(r: &R, wobble: f32, radius: f32, scale: f32, n: &[f32; 100]) -> [Curve; 8] {
    let (w, h) = (r.w().max(1.0), r.h().max(1.0));
    let cut = radius.min(w.min(h) / CORNER_MAX_RATIO);
    let (rx, ry) = (cut / w, cut / h);
    let (cx, cy) = (rx + (1.0 - 2.0 * rx) * 0.25, ry + (1.0 - 2.0 * ry) * 0.25);
    let bow = wobble * (w / h).min(h / w);
    let bow_x = (h / scale / HAND_BOW_DIVISOR).min(scale * HAND_BOW_MAX / w);
    let bow_y = (w / scale / HAND_BOW_DIVISOR).min(scale * HAND_BOW_MAX / h);
    let (side_x, side_y) = (bow * bow_x, bow * bow_y);
    let q = ry * bow * bow_x / (cy - ry).max(0.1);
    let e = rx * bow * bow_y / (cx - rx).max(0.1);
    let jitter = |i: usize| (h * wobble * (n[56 + i * 2] - 0.5) / HAND_JITTER_DIVISOR, w * wobble * (n[57 + i * 2] - 0.5) / HAND_JITTER_DIVISOR);
    let at = |x: f32, y: f32| {
        let (jx, jy) = jitter(match (x > 0.5, y > 0.5) {
            (false, false) => 0,
            (false, true) => 1,
            (true, true) => 2,
            (true, false) => 3,
        });
        p(r.x0 + x * w + jx, r.y0 + y * h + jy)
    };
    let mut c = at(rx, 0.0);
    [
        (at(1.0 - rx, 0.0), at(cx, -side_y), at(1.0 - cx, -side_y)),
        (at(1.0, ry), at(1.0 - q, e), at(1.0 - q, e)),
        (at(1.0, 1.0 - ry), at(1.0 + side_x, cy), at(1.0 + side_x, 1.0 - cy)),
        (at(1.0 - rx, 1.0), at(1.0 - q, 1.0 - e), at(1.0 - q, 1.0 - e)),
        (at(rx, 1.0), at(1.0 - cx, 1.0 + side_y), at(cx, 1.0 + side_y)),
        (at(0.0, 1.0 - ry), at(q, 1.0 - e), at(q, 1.0 - e)),
        (at(0.0, ry), at(-side_x, 1.0 - cy), at(-side_x, cy)),
        (at(rx, 0.0), at(q, e), at(q, e)),
    ]
    .map(|(b, c1, c2)| {
        let curve = Curve { a: c, c1, c2, b };
        c = b;
        curve
    })
}

fn curves_path(curves: &[Curve]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(curves[0].a.x, curves[0].a.y);
    for c in curves {
        pb.cubic_to(c.c1.x, c.c1.y, c.c2.x, c.c2.y, c.b.x, c.b.y);
    }
    pb.close();
    pb.finish()
}

/// Points along the curves, about `spacing` apart.
fn flatten(curves: &[Curve], spacing: f32) -> Vec<P> {
    let mut out = vec![];
    for c in curves {
        let steps = (c.a.dist(c.b) * BRUSH_STEPS_PER_LENGTH / spacing) as usize;
        let mut last = c.a;
        out.push(last);
        for i in 1..steps {
            let t = i as f32 / steps as f32;
            let u = 1.0 - t;
            let (k0, k1, k2, k3) = (u * u * u, 3.0 * t * u * u, 3.0 * t * t * u, t * t * t);
            let q = p(k0 * c.a.x + k1 * c.c1.x + k2 * c.c2.x + k3 * c.b.x, k0 * c.a.y + k1 * c.c1.y + k2 * c.c2.y + k3 * c.b.y);
            if q.dist(last) >= spacing {
                out.push(q);
                last = q;
            }
        }
    }
    out.push(curves[curves.len() - 1].b);
    out
}

/// Hand-drawn rectangle: the whole outline slips by up to half a point, except
/// where it starts and ends, which leaves the small kink of a pen closing its loop.
fn hand_rect(r: &R, base: f32, scale: f32, n: &[f32; 100]) -> Vec<P> {
    let d = base * HAND_CORNER_LINE_RATIO;
    let radius = (d * HAND_CORNER_GROWTH - d * d * HAND_CORNER_DAMPING) * scale;
    let mut curves = outline_curves(r, HAND_WOBBLE, radius, scale, n);
    let (dx, dy) = ((n[27] - 0.5) * scale, (n[28] - 0.5) * scale);
    let last = curves.len() - 1;
    for (i, c) in curves.iter_mut().enumerate() {
        if i > 0 {
            c.a = c.a.add(dx, dy);
            c.c1 = c.c1.add(dx, dy);
        }
        if i < last {
            c.b = c.b.add(dx, dy);
            c.c2 = c.c2.add(dx, dy);
        }
    }
    flatten(&curves, BRUSH_SPACING * scale)
}

/// Hand-drawn oval: one turn from a random angle that overshoots or stops short
/// while the radius drifts, so the two ends of the stroke do not meet.
fn hand_oval(r: &R, line: f32, scale: f32, n: &[f32; 100]) -> Vec<P> {
    let (w, h) = (r.w().max(1.0), r.h().max(1.0));
    let short = w.min(h);
    let inner = short * 0.5;
    let a = (w - inner) * 0.5;
    let k = (h - inner) / (w - inner);
    let tau = std::f32::consts::TAU;
    let (turn, drift) = if line * OVAL_OVERLAP_LINES < short { ((n[8] * OVAL_TURN_RANGE + OVAL_TURN_MIN) * tau, line * OVAL_DRIFT) } else { (tau, 0.0) };
    let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    let spacing = BRUSH_SPACING * scale;
    let mut out: Vec<P> = vec![];
    for i in 0..OVAL_SAMPLES {
        let t = i as f32 / (OVAL_SAMPLES - 1) as f32;
        let (sin, cos) = (n[37] * tau + turn * t).sin_cos();
        let radius = inner * 0.5 + drift * (t - 0.5);
        let norm = (cos * cos + sin * sin / (k * k)).sqrt();
        let q = p(cx + a * cos + radius * cos / norm, cy + a * k * sin + radius * sin / (k * norm));
        if out.last().is_none_or(|l| q.dist(*l) >= spacing) {
            out.push(q);
        }
    }
    out
}

/// Stroke width at every point: noise and a
/// slow wave, plus a swell in the bends when `bends` is set, smoothed around the loop, then
/// spread between line × (1 - taper) and line × (1 + taper).
fn brush_widths(pts: &[P], line: f32, taper: f32, bends: bool, n: &[f32; 100]) -> Vec<f32> {
    let count = pts.len();
    let mut ink: Vec<f32> = (0..count)
        .map(|i| {
            let bend = if bends {
                let (q, a, b) = (pts[i], pts[i.saturating_sub(1)], pts[(i + 1).min(count - 1)]);
                let (la, lb) = (q.dist(a), q.dist(b));
                let cos = if la * lb != 0.0 { ((a.x - q.x) * (b.x - q.x) + (a.y - q.y) * (b.y - q.y)) / la / lb } else { -1.0 };
                (1.0 - cos.abs()) * BRUSH_BEND_GAIN
            } else {
                0.0
            };
            bend + (i as f32 * BRUSH_WAVE / count as f32).cos() + 1.0 + n[(84 + i) % 100] * BRUSH_NOISE
        })
        .collect();
    let half = BRUSH_SMOOTH_WINDOW.min(count - 1) / 2;
    let span = half * 2 + 1;
    for _ in 0..BRUSH_SMOOTH_PASSES {
        let mut sum: f32 = (0..span).map(|j| ink[(j + count - half) % count]).sum();
        let mut next = Vec::with_capacity(count);
        for i in 0..count {
            next.push(sum / span as f32);
            sum += ink[(i + half + 1) % count] - ink[(i + count - half) % count];
        }
        ink = next;
    }
    let (lo, hi) = ink.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    ink.iter()
        .map(|v| {
            let level = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            line * (1.0 - taper + 2.0 * taper * level)
        })
        .collect()
}

/// Stroke of varying width as discs joined by quads, all wound the same way so they merge.
fn brush_path(pts: &[P], widths: &[f32]) -> Option<Path> {
    const SIDES: usize = 12;
    let mut pb = PathBuilder::new();
    for (i, (q, w)) in pts.iter().zip(widths).enumerate() {
        let radius = w / 2.0;
        for j in 0..SIDES {
            let (sin, cos) = (-(j as f32) * std::f32::consts::TAU / SIDES as f32).sin_cos();
            let (x, y) = (q.x + radius * cos, q.y + radius * sin);
            if j == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        pb.close();
        let Some(next) = pts.get(i + 1) else { break };
        let len = q.dist(*next);
        if len > 0.0 {
            let (nx, ny) = (-(next.y - q.y) / len, (next.x - q.x) / len);
            let far = widths[i + 1] / 2.0;
            pb.move_to(q.x + nx * radius, q.y + ny * radius);
            pb.line_to(next.x + nx * far, next.y + ny * far);
            pb.line_to(next.x - nx * far, next.y - ny * far);
            pb.line_to(q.x - nx * radius, q.y - ny * radius);
            pb.close();
        }
    }
    pb.finish()
}

pub fn draw_obj(pm: &mut Pixmap, obj: &Obj, fonts: &Fonts, scale_factor: f32) {
    let st = &obj.style;
    let w = st.width();
    let tf = Transform::identity();
    match &obj.kind {
        Kind::Arrow { a, b, seed } => draw_arrow(pm, *a, *b, *seed, st),
        Kind::Line { a, b } => {
            let mut pb = PathBuilder::new();
            pb.move_to(a.x, a.y);
            pb.line_to(b.x, b.y);
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &paint(st.color, 1.0), &stroke(w, st.dashed), tf, None);
            }
        }
        Kind::Rect { r, seed } | Kind::Oval { r, seed } => {
            let is_oval = matches!(obj.kind, Kind::Oval { .. });
            let n = noise(*seed);
            let scale = st.scale;
            let base = w / scale;
            let short = r.w().min(r.h());
            let line = base.min(short / SHAPE_LINE_DIVISOR) * scale;
            let hand = st.fill == Fill::HandDrawn;
            if hand && short > SHAPE_HAND_MIN_SIDE && !st.dashed {
                let (pts, taper, bends) = if is_oval {
                    (hand_oval(r, line, scale, &n), BRUSH_TAPER_OVAL, false)
                } else {
                    (hand_rect(r, base, scale, &n), (BRUSH_TAPER_MAX - line / BRUSH_TAPER_LINE_DIVISOR).clamp(BRUSH_TAPER_MIN, BRUSH_TAPER_MAX), true)
                };
                if pts.len() >= 2 {
                    let widths = brush_widths(&pts, line, taper, bends, &n);
                    if let Some(path) = brush_path(&pts, &widths) {
                        pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None);
                    }
                }
                return;
            }
            let wobble = if hand { HAND_WOBBLE } else { 0.0 };
            let curves = if is_oval { plain_oval_curves(r) } else { outline_curves(r, wobble, base * CORNER_LINE_RATIO * scale, scale, &n) };
            let path = curves_path(&curves);
            let Some(path) = path else { return };
            match st.fill {
                Fill::Translucent => pm.fill_path(&path, &paint(st.color, FILL_TRANSLUCENT), FillRule::Winding, tf, None),
                Fill::Opaque => pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None),
                _ => {}
            }
            pm.stroke_path(&path, &paint(st.color, 1.0), &stroke(line, st.dashed), tf, None);
        }
        Kind::Freehand { pts } => {
            if let [only] = pts.as_slice() {
                // A single click draws a point.
                let mut pb = PathBuilder::new();
                pb.push_circle(only.x, only.y, w / 2.0);
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None);
                }
                return;
            }
            if pts.len() < 2 {
                return;
            }
            let pts = freehand_smooth(pts, st.smooth);
            if st.vary > 0 && !st.dashed {
                for (i, seg_w) in freehand_widths(&pts, w, st.vary, st.scale).into_iter().enumerate() {
                    let mut pb = PathBuilder::new();
                    pb.move_to(pts[i].x, pts[i].y);
                    pb.line_to(pts[i + 1].x, pts[i + 1].y);
                    if let Some(path) = pb.finish() {
                        pm.stroke_path(&path, &paint(st.color, 1.0), &stroke(seg_w, false), tf, None);
                    }
                }
                return;
            }
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].x, pts[0].y);
            for i in 1..pts.len() - 1 {
                let m = p((pts[i].x + pts[i + 1].x) / 2.0, (pts[i].y + pts[i + 1].y) / 2.0);
                pb.quad_to(pts[i].x, pts[i].y, m.x, m.y);
            }
            let l = pts[pts.len() - 1];
            pb.line_to(l.x, l.y);
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &paint(st.color, 1.0), &stroke(w, st.dashed), tf, None);
            }
        }
        Kind::Highlighter { a, b } => {
            let mut pb = PathBuilder::new();
            pb.move_to(a.x, a.y);
            pb.line_to(b.x, b.y);
            if let Some(path) = pb.finish() {
                let mut pt = paint(st.color, 1.0);
                pt.blend_mode = BlendMode::Multiply;
                let cap = if st.round_cap { LineCap::Round } else { LineCap::Butt };
                let s = Stroke { width: st.highlight_size(), line_cap: cap, ..Default::default() };
                pm.stroke_path(&path, &pt, &s, tf, None);
            }
        }
        Kind::Blur { r, text } => draw_blur(pm, r, text.as_deref(), st, scale_factor),
        Kind::Spotlight { r } => draw_spotlight(pm, &[(*r, st.shape)], st.dim),
        Kind::Counter { at, n } => {
            let d = st.counter_size();
            let r = R { x0: at.x - d / 2.0, y0: at.y - d / 2.0, x1: at.x + d / 2.0, y1: at.y + d / 2.0 };
            if let Some(path) = oval_path(&r) {
                pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None);
            }
            let bright = (0.299 * st.color[0] as f32 + 0.587 * st.color[1] as f32 + 0.114 * st.color[2] as f32) / 255.0;
            let txt = if bright > COUNTER_DARK_TEXT_ABOVE { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
            let s = n.to_string();
            let size = d * COUNTER_LABEL_RATIO * if *n > 9 { COUNTER_LABEL_TWO_DIGITS } else { 1.0 };
            let (tw, _) = measure(fonts.get(FontKind::Sans), &s, size);
            draw_text_line(pm, fonts.get(FontKind::Sans), &s, p(at.x - tw / 2.0, at.y + size * 0.36), size, txt, None);
        }
        Kind::Magnifier { r, lens } => draw_magnifier(pm, r, *lens, st),
        Kind::Ruler { a, b } => draw_ruler(pm, *a, *b, st, fonts, scale_factor),
        Kind::Text { at, text } => draw_text(pm, *at, text, st, fonts),
        Kind::Image { pic, r, opacity } => draw_image(pm, &pic.img, r, *opacity),
    }
}

/// Draws a picture into `r` (stretched to it) at `opacity`. Whole-number enlargements and 1:1 stay
/// sharp; anything else is smoothed.
fn draw_image(pm: &mut Pixmap, img: &Image, r: &R, opacity: f32) {
    if img.w == 0 || img.h == 0 || r.w() < 0.5 || r.h() < 0.5 {
        return;
    }
    let (sx, sy) = (r.w() / img.w as f32, r.h() / img.h as f32);
    let sharp = (sx - sx.round()).abs() < 1e-3 && (sy - sy.round()).abs() < 1e-3 && sx >= 1.0;
    let src = to_pixmap(img);
    let paint = PixmapPaint {
        opacity: opacity.clamp(0.0, 1.0),
        quality: if sharp { tiny_skia::FilterQuality::Nearest } else { tiny_skia::FilterQuality::Bicubic },
        ..Default::default()
    };
    pm.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_row(sx, 0.0, 0.0, sy, r.x0, r.y0), None);
}

/// Line of the thin arrows: capped to a tenth of the length, then growing with it.
/// `len` is in image pixels here and below; the rules themselves are in points.
fn arrow_line(len: f32, st: &Style) -> f32 {
    let len = len / st.scale;
    let line = ARROW_WIDTHS[st.size as usize].min(len / ARROW_LINE_DIVISOR) * st.scale;
    let grow = ((len + ARROW_GROW_BASE) / ARROW_GROW_DIVISOR).sqrt();
    line * grow.clamp(1.0, ARROW_GROW_MAX)
}

/// Branch length and half angle of the thin arrow head.
fn arrow_branch(len: f32, line: f32, scale: f32) -> (f32, f32) {
    let (len, line) = (len / scale, line / scale);
    let branch = ARROW_BRANCH_SCALE + line.sqrt() * ARROW_BRANCH_ROOT + line * ARROW_BRANCH_LINE;
    (branch.min(len * ARROW_BRANCH_MAX) * scale, if len < ARROW_SHORT { ARROW_SPREAD_SHORT } else { ARROW_SPREAD })
}

/// Solid arrow stretch as (head, tail) factors, from a length in points.
fn arrow_stretch(len: f32, unit: f32) -> (f32, f32) {
    let mut tail = 1.0;
    let mut head = 1.0;
    if len > ARROW_LONG {
        tail = (1.0 - (len - ARROW_LONG) / ARROW_LONG_RANGE).max(ARROW_LONG_MIN);
        head = 1.0 / tail;
    }
    let span = unit * ARROW_SPAN;
    let limit = if span >= ARROW_SPAN_LIMIT { span * 2.0 } else { ARROW_SHORT_LIMIT };
    if len < limit {
        head = len / limit;
        tail = head;
    }
    (head, tail)
}

/// Shaft of the hand-drawn arrow, nearly straight with a slight lean at each end.
fn hand_arrow_shaft(a: P, b: P, line: f32, n: &[f32; 100]) -> Curve {
    let len = a.dist(b);
    let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    let reach = (len - line * ARROW_HAND_STOP).max(1.0);
    let end = p(a.x + ux * reach, a.y + uy * reach);
    let handle = reach * ARROW_HAND_HANDLE;
    let lean = |q: P, dx: f32, dy: f32, turn: f32| {
        let (sin, cos) = (turn * std::f32::consts::PI).sin_cos();
        p(q.x + handle * (dx * cos - dy * sin), q.y + handle * (dy * cos + dx * sin))
    };
    let c1 = lean(a, ux, uy, n[11] * ARROW_HAND_LEAN_START - ARROW_HAND_LEAN_START / 2.0);
    let c2 = lean(end, -ux, -uy, n[12] * ARROW_HAND_LEAN_END - ARROW_HAND_LEAN_END / 2.0);
    Curve { a, c1, c2, b: end }
}

/// What an arrow paints, shared by drawing, bounds and hit tests so they cannot disagree.
enum ArrowGeometry {
    Solid { outline: [P; 7], edge: f32 },
    /// Solid shafts paint `widths`, one per point, and dashed ones `line`; heads run wing, tip, wing.
    Lines { shaft: Vec<P>, widths: Vec<f32>, heads: Vec<[P; 3]>, line: f32 },
}

fn arrow_head(tip: P, dx: f32, dy: f32, branch: f32, spread: f32) -> [P; 3] {
    let (sin, cos) = spread.sin_cos();
    let wing = |side: f32| p(tip.x - branch * (dx * cos - side * dy * sin), tip.y - branch * (dy * cos + side * dx * sin));
    [wing(1.0), tip, wing(-1.0)]
}

fn arrow_geometry(a: P, b: P, seed: u64, st: &Style) -> Option<ArrowGeometry> {
    let len = a.dist(b);
    if len < 1.0 {
        return None;
    }
    let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    let (nx, ny) = (-uy, ux);
    let thin = arrow_line(len, st);
    let (branch, spread) = arrow_branch(len, thin, st.scale);
    Some(match st.arrow {
        ArrowStyle::Shape => {
            let unit = ARROW_UNITS[st.size as usize];
            let (head, tail) = arrow_stretch(len / st.scale, unit);
            let (head, tail) = (head * unit * st.scale, tail * unit * st.scale);
            let back = p(b.x - ux * ARROW_HEAD_LENGTH * head, b.y - uy * ARROW_HEAD_LENGTH * head);
            let across = |q: P, half: f32| p(q.x + nx * half, q.y + ny * half);
            let outline = [
                across(a, ARROW_TAIL_HALF * tail),
                across(back, ARROW_NECK_HALF * head),
                across(back, ARROW_HEAD_HALF * head),
                b,
                across(back, -ARROW_HEAD_HALF * head),
                across(back, -ARROW_NECK_HALF * head),
                across(a, -ARROW_TAIL_HALF * tail),
            ];
            ArrowGeometry::Solid { outline, edge: st.scale }
        }
        ArrowStyle::HandDrawn => {
            let line = ((ARROW_WIDTHS[st.size as usize] * ARROW_HAND_GAIN + ARROW_HAND_BASE) * st.scale).min(len / ARROW_HAND_DIVISOR);
            let shaft = flatten(&[hand_arrow_shaft(a, b, line, &noise(seed))], BRUSH_SPACING * st.scale);
            let last = (shaft.len() - 1).max(1) as f32;
            let widths = (0..shaft.len()).map(|i| line * (ARROW_HAND_STOP + (ARROW_HAND_END - ARROW_HAND_STOP) * i as f32 / last)).collect();
            ArrowGeometry::Lines { shaft, widths, heads: vec![arrow_head(b, ux, uy, branch, spread)], line }
        }
        ArrowStyle::Line | ArrowStyle::Double => {
            let mut heads = vec![arrow_head(b, ux, uy, branch, spread)];
            if st.arrow == ArrowStyle::Double {
                heads.push(arrow_head(a, -ux, -uy, branch, spread));
            }
            ArrowGeometry::Lines { shaft: vec![a, b], widths: vec![thin; 2], heads, line: thin }
        }
    })
}

fn polyline(pb: &mut PathBuilder, pts: &[P]) {
    pb.move_to(pts[0].x, pts[0].y);
    for q in &pts[1..] {
        pb.line_to(q.x, q.y);
    }
}

fn draw_arrow(pm: &mut Pixmap, a: P, b: P, seed: u64, st: &Style) {
    let Some(geometry) = arrow_geometry(a, b, seed, st) else { return };
    let [red, green, blue, alpha] = st.color;
    if alpha == 255 {
        paint_arrow(pm, geometry, st, Transform::identity());
        return;
    }
    // Shaft and head overlap, so a translucent arrow is painted opaque and faded once.
    let area = arrow_area(&geometry, a, b).clamp_to(pm.width() as f32, pm.height() as f32);
    let (x, y) = (area.x0.floor(), area.y0.floor());
    let Some(mut layer) = Pixmap::new((area.x1.ceil() - x) as u32, (area.y1.ceil() - y) as u32) else { return };
    paint_arrow(&mut layer, geometry, &Style { color: [red, green, blue, 255], ..*st }, Transform::from_translate(-x, -y));
    let fade = PixmapPaint { opacity: alpha as f32 / 255.0, ..Default::default() };
    pm.draw_pixmap(x as i32, y as i32, layer.as_ref(), &fade, Transform::identity(), None);
}

fn paint_arrow(pm: &mut Pixmap, geometry: ArrowGeometry, st: &Style, tf: Transform) {
    let pt = paint(st.color, 1.0);
    match geometry {
        ArrowGeometry::Solid { outline, edge } => {
            let mut pb = PathBuilder::new();
            polyline(&mut pb, &outline);
            pb.close();
            if let Some(path) = pb.finish() {
                pm.fill_path(&path, &pt, FillRule::Winding, tf, None);
                let s = Stroke { width: edge, line_join: LineJoin::Round, ..Default::default() };
                pm.stroke_path(&path, &pt, &s, tf, None);
            }
        }
        ArrowGeometry::Lines { shaft, widths, heads, line } => {
            let hand = st.arrow == ArrowStyle::HandDrawn;
            let pen = if hand { stroke(line, false) } else { Stroke { line_cap: LineCap::Square, line_join: LineJoin::Miter, ..stroke(line, false) } };
            if hand && !st.dashed {
                if let Some(path) = brush_path(&shaft, &widths) {
                    pm.fill_path(&path, &pt, FillRule::Winding, tf, None);
                }
            } else {
                let mut pb = PathBuilder::new();
                polyline(&mut pb, &shaft);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &pt, &Stroke { dash: stroke(line, st.dashed).dash, ..pen.clone() }, tf, None);
                }
            }
            let mut pb = PathBuilder::new();
            for head in &heads {
                polyline(&mut pb, head);
            }
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &pt, &pen, tf, None);
            }
        }
    }
}

fn arrow_area(geometry: &ArrowGeometry, a: P, b: P) -> R {
    let around = |pts: &mut dyn Iterator<Item = &P>, margin: f32| pts.fold(R::from_points(a, b), |r, q| r.union(&R::from_points(*q, *q))).expand(margin);
    match geometry {
        ArrowGeometry::Solid { outline, edge } => around(&mut outline.iter(), edge + 2.0),
        ArrowGeometry::Lines { shaft, heads, line, .. } => around(&mut shaft.iter().chain(heads.iter().flatten()), line + 2.0),
    }
}

pub fn seg_dist(q: P, a: P, b: P) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = (dx * dx + dy * dy).max(1e-6);
    let t = (((q.x - a.x) * dx + (q.y - a.y) * dy) / l2).clamp(0.0, 1.0);
    q.dist(p(a.x + dx * t, a.y + dy * t))
}

/// Within a stroke of width `w` with square ends, which also covers round ends and miter joins.
fn on_stroke(q: P, s: P, e: P, w: f32, tol: f32) -> bool {
    let reach = w / 2.0 + tol;
    let len = s.dist(e);
    if len < 1e-6 {
        return q.dist(s) <= reach;
    }
    let (ux, uy) = ((e.x - s.x) / len, (e.y - s.y) / len);
    let (dx, dy) = (q.x - s.x, q.y - s.y);
    (dx * uy - dy * ux).abs() <= reach && (-reach..=len + reach).contains(&(dx * ux + dy * uy))
}

fn inside(polygon: &[P], q: P) -> bool {
    let edges = polygon.iter().zip(polygon.iter().cycle().skip(1));
    edges.filter(|(s, e)| (s.y > q.y) != (e.y > q.y) && q.x < s.x + (q.y - s.y) / (e.y - s.y) * (e.x - s.x)).count() % 2 == 1
}

/// The dashes of a shaft as segments, from the same dashing that paints them.
fn dash_segments(shaft: &[P], dash: &StrokeDash) -> Vec<[P; 2]> {
    let mut pb = PathBuilder::new();
    polyline(&mut pb, shaft);
    let Some(dashed) = pb.finish().and_then(|path| path.dash(dash, 1.0)) else { return vec![] };
    let mut last = p(0.0, 0.0);
    let mut out = vec![];
    for segment in dashed.segments() {
        match segment {
            PathSegment::MoveTo(q) => last = p(q.x, q.y),
            PathSegment::LineTo(q) => {
                out.push([last, p(q.x, q.y)]);
                last = p(q.x, q.y);
            }
            _ => {}
        }
    }
    out
}

/// True on anything the arrow paints, within `tol`. An arrow too short to paint stays selectable.
pub fn arrow_hit(a: P, b: P, seed: u64, st: &Style, q: P, tol: f32) -> bool {
    match arrow_geometry(a, b, seed, st) {
        None => seg_dist(q, a, b) <= st.width() + tol,
        Some(ArrowGeometry::Solid { outline, edge }) => {
            inside(&outline, q) || outline.iter().zip(outline.iter().cycle().skip(1)).any(|(s, e)| on_stroke(q, *s, *e, edge, tol))
        }
        Some(ArrowGeometry::Lines { shaft, widths, heads, line }) => {
            let on_shaft = match stroke(line, st.dashed).dash {
                Some(dash) => dash_segments(&shaft, &dash).iter().any(|s| on_stroke(q, s[0], s[1], line, tol)),
                None => shaft.windows(2).zip(&widths).any(|(s, w)| on_stroke(q, s[0], s[1], *w, tol)),
            };
            on_shaft
                || heads.iter().flat_map(|h| h.windows(2)).any(|s| on_stroke(q, s[0], s[1], line, tol))
        }
    }
}

/// Area the object paints on, used for hit tests and partial redraws.
pub fn bounds(obj: &Obj, fonts: &Fonts) -> R {
    let st = &obj.style;
    let w = st.width();
    match &obj.kind {
        Kind::Arrow { a, b, seed } => match arrow_geometry(*a, *b, *seed, st) {
            Some(geometry) => arrow_area(&geometry, *a, *b),
            None => R::from_points(*a, *b).expand(w + 2.0),
        },
        Kind::Line { a, b } | Kind::Ruler { a, b } => R::from_points(*a, *b).expand(w * 2.0 + 24.0),
        Kind::Rect { r, .. } | Kind::Oval { r, .. } => r.expand(w * 2.0 + 4.0),
        Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Image { r, .. } => *r,
        Kind::Magnifier { r, lens } => r.union(&magnifier_lens(r, *lens)).expand(w + 4.0),
        Kind::Highlighter { a, b } => R::from_points(*a, *b).expand(st.highlight_size() / 2.0 + 2.0),
        Kind::Freehand { pts } => {
            let mut r = R { x0: f32::MAX, y0: f32::MAX, x1: f32::MIN, y1: f32::MIN };
            for q in pts {
                r = r.union(&R { x0: q.x, y0: q.y, x1: q.x, y1: q.y });
            }
            r.expand(w + 2.0)
        }
        Kind::Counter { at, .. } => {
            let s = st.counter_size() / 2.0 + 2.0;
            R { x0: at.x - s, y0: at.y - s, x1: at.x + s, y1: at.y + s }
        }
        Kind::Text { at, text } => text_box(*at, text, st, fonts).expand(text_outline(st.text_size()) + 2.0),
    }
}

/// Box blur on premultiplied pixels in place, `passes` times (3 passes ≈ gaussian).
pub fn box_blur(pm: &mut Pixmap, r: &R, radius: i32, passes: usize) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let x0 = (r.x0.floor() as i32).clamp(0, w);
    let y0 = (r.y0.floor() as i32).clamp(0, h);
    let x1 = (r.x1.ceil() as i32).clamp(0, w);
    let y1 = (r.y1.ceil() as i32).clamp(0, h);
    let (rw, rh) = (x1 - x0, y1 - y0);
    if rw <= 0 || rh <= 0 || radius <= 0 {
        return;
    }
    let data = pm.data_mut();
    let mut buf: Vec<[f32; 4]> = (0..rh)
        .flat_map(|y| (0..rw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let i = (((y0 + y) * w + x0 + x) * 4) as usize;
            [data[i] as f32, data[i + 1] as f32, data[i + 2] as f32, data[i + 3] as f32]
        })
        .collect();
    let mut tmp = buf.clone();
    let rad = radius.min(rw.max(rh));
    for _ in 0..passes {
        for horizontal in [true, false] {
            let (outer, inner) = if horizontal { (rh, rw) } else { (rw, rh) };
            for o in 0..outer {
                let idx = |i: i32| -> usize {
                    let i = i.clamp(0, inner - 1);
                    if horizontal { (o * rw + i) as usize } else { (i * rw + o) as usize }
                };
                let mut acc = [0f32; 4];
                for i in -rad..=rad {
                    let v = buf[idx(i)];
                    for c in 0..4 {
                        acc[c] += v[c];
                    }
                }
                let n = (2 * rad + 1) as f32;
                for i in 0..inner {
                    tmp[idx(i)] = [acc[0] / n, acc[1] / n, acc[2] / n, acc[3] / n];
                    let add = buf[idx(i + rad + 1)];
                    let sub = buf[idx(i - rad)];
                    for c in 0..4 {
                        acc[c] += add[c] - sub[c];
                    }
                }
            }
            std::mem::swap(&mut buf, &mut tmp);
        }
    }
    for y in 0..rh {
        for x in 0..rw {
            let i = (((y0 + y) * w + x0 + x) * 4) as usize;
            let v = buf[(y * rw + x) as usize];
            let a = v[3].round().clamp(0.0, 255.0);
            data[i + 3] = a as u8;
            for c in 0..3 {
                data[i + c] = v[c].round().clamp(0.0, a) as u8;
            }
        }
    }
}

fn region(pm: &Pixmap, r: &R) -> (i32, i32, i32, i32) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    ((r.x0.floor() as i32).clamp(0, w), (r.y0.floor() as i32).clamp(0, h), (r.x1.ceil() as i32).clamp(0, w), (r.y1.ceil() as i32).clamp(0, h))
}

fn border_average(pm: &Pixmap, r: &R) -> [u8; 4] {
    let (x0, y0, x1, y1) = region(pm, r);
    let w = pm.width() as i32;
    let d = pm.data();
    let mut acc = [0u64; 4];
    let mut n = 0u64;
    let mut add = |x: i32, y: i32| {
        let i = ((y * w + x) * 4) as usize;
        for c in 0..4 {
            acc[c] += d[i + c] as u64;
        }
        n += 1;
    };
    if x1 <= x0 || y1 <= y0 {
        return [0, 0, 0, 0];
    }
    for x in x0..x1 {
        add(x, y0);
        add(x, y1 - 1);
    }
    for y in y0..y1 {
        add(x0, y);
        add(x1 - 1, y);
    }
    let n = n.max(1);
    [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (acc[3] / n) as u8]
}

/// Average-colour blocks, see the PIXEL_ constants.
fn pixelate(pm: &mut Pixmap, r: &R, size: u8, scale: f32) {
    let (x0, y0, x1, y1) = region(pm, r);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let w = pm.width() as i32;
    let side = (x1 - x0).min(y1 - y0) as f32;
    let cell = ((side / PIXEL_SIDE_DIVISOR).round().min(PIXEL_RETINA_CAP * scale.max(1.0)) * PIXEL_GROWTH.powi(size as i32)).round().max(1.0) as i32;
    let (cols, rows) = ((x1 - x0 + cell - 1) / cell, (y1 - y0 + cell - 1) / cell);
    let d = pm.data_mut();
    let mut means = vec![[0u8; 4]; (cols * rows) as usize];
    for gy in 0..rows {
        for gx in 0..cols {
            let (sx, sy) = (x0 + gx * cell, y0 + gy * cell);
            let (ex, ey) = ((sx + cell).min(x1), (sy + cell).min(y1));
            let mut acc = [0u32; 4];
            for y in sy..ey {
                for x in sx..ex {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += d[i + c] as u32;
                    }
                }
            }
            let n = ((ex - sx) * (ey - sy)).max(1) as u32;
            means[(gy * cols + gx) as usize] = [0, 1, 2, 3].map(|c| (acc[c] / n) as u8);
        }
    }
    let shuffle = side / PIXEL_SHUFFLE_DIVISOR < cell as f32;
    let mut seed = (x0 as u64 + 1).wrapping_mul(0x9E37_79B9) ^ (y0 as u64 + 1).wrapping_mul(0x85EB_CA6B) | 1;
    for gy in 0..rows {
        for gx in 0..cols {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let (nx, ny) = match (shuffle, seed % 5) {
                (true, 0) => (gx, gy + 1),
                (true, 1) => (gx + 1, gy),
                (true, 2) => (gx, gy - 1),
                (true, 3) => (gx - 1, gy),
                _ => (gx, gy),
            };
            let inside = nx >= 0 && ny >= 0 && nx < cols && ny < rows;
            let c = means[if inside { (ny * cols + nx) as usize } else { (gy * cols + gx) as usize }];
            for y in y0 + gy * cell..(y0 + (gy + 1) * cell).min(y1) {
                for x in x0 + gx * cell..(x0 + (gx + 1) * cell).min(x1) {
                    let i = ((y * w + x) * 4) as usize;
                    d[i..i + 4].copy_from_slice(&c);
                }
            }
        }
    }
}

/// Line-by-line gradient between two opposite edges, across the direction whose other two
/// edges are the most uniform.
fn erase(pm: &mut Pixmap, r: &R) {
    let (x0, y0, x1, y1) = region(pm, r);
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return;
    }
    let w = pm.width() as i32;
    let d = pm.data_mut();
    let px = |d: &[u8], x: i32, y: i32| -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [d[i], d[i + 1], d[i + 2], d[i + 3]]
    };
    let changes = |d: &[u8], pts: Vec<(i32, i32)>| -> f32 {
        let n = pts.len().max(2) as f32 - 1.0;
        let hits = pts.windows(2).filter(|s| {
            let (a, b) = (px(d, s[0].0, s[0].1), px(d, s[1].0, s[1].1));
            (0..3).map(|c| (a[c] as i32 - b[c] as i32).abs()).sum::<i32>() > 6
        });
        hits.count() as f32 / n
    };
    let horizontal_edges = changes(d, (x0..x1).map(|x| (x, y0)).collect()) + changes(d, (x0..x1).map(|x| (x, y1 - 1)).collect());
    let vertical_edges = changes(d, (y0..y1).map(|y| (x0, y)).collect()) + changes(d, (y0..y1).map(|y| (x1 - 1, y)).collect());
    let mix = |a: [u8; 4], b: [u8; 4], t: f32| [0, 1, 2, 3].map(|c| (a[c] as f32 + (b[c] as f32 - a[c] as f32) * t).round() as u8);
    if horizontal_edges <= vertical_edges {
        for y in y0..y1 {
            let (a, b) = (px(d, x0, y), px(d, x1 - 1, y));
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                d[i..i + 4].copy_from_slice(&mix(a, b, (x - x0) as f32 / (x1 - x0 - 1) as f32));
            }
        }
    } else {
        for x in x0..x1 {
            let (a, b) = (px(d, x, y0), px(d, x, y1 - 1));
            for y in y0..y1 {
                let i = ((y * w + x) * 4) as usize;
                d[i..i + 4].copy_from_slice(&mix(a, b, (y - y0) as f32 / (y1 - y0 - 1) as f32));
            }
        }
    }
}

fn draw_blur(pm: &mut Pixmap, area: &R, text: Option<&[R]>, st: &Style, scale: f32) {
    let (pw, ph) = (pm.width() as f32, pm.height() as f32);
    let r = area.clamp_to(pw, ph);
    let (x0, y0, x1, y1) = region(pm, &r);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let before = if st.shape == Shape::Oval { Some(pm.clone()) } else { None };
    let w = pm.width() as i32;
    let only_text = matches!(st.blur, BlurMode::BlurText | BlurMode::EraseText);
    let targets: Vec<R> = match (only_text, text) {
        (true, Some(boxes)) => boxes.iter().map(|b| b.translate(area.x0, area.y0).clamp_to(pw, ph)).collect(),
        _ => vec![r],
    };
    for t in &targets {
        match st.blur {
            BlurMode::Blur | BlurMode::BlurText => pixelate(pm, t, st.size, scale),
            BlurMode::Erase | BlurMode::EraseText => erase(pm, t),
        }
    }
    if let Some(before) = before {
        // Oval blur: keep the original pixels outside the ellipse.
        let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
        let (rx, ry) = ((r.w() / 2.0).max(1.0), (r.h() / 2.0).max(1.0));
        let src = before.data();
        let d = pm.data_mut();
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                if dx * dx + dy * dy > 1.0 {
                    let i = ((y * w + x) * 4) as usize;
                    d[i..i + 4].copy_from_slice(&src[i..i + 4]);
                }
            }
        }
    }
}

pub fn draw_spotlight(pm: &mut Pixmap, holes: &[(R, Shape)], dim: f32) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    let mut pb = PathBuilder::new();
    if let Some(full) = Rect::from_xywh(0.0, 0.0, w, h) {
        pb.push_rect(full);
    }
    for (r, shape) in holes {
        if let Some(rc) = rect(r) {
            match shape {
                Shape::Rect => pb.push_rect(rc),
                Shape::Oval => pb.push_oval(rc),
            }
        }
    }
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &paint([0, 0, 0, 255], dim), FillRule::EvenOdd, Transform::identity(), None);
    }
}

fn draw_magnifier(pm: &mut Pixmap, r: &R, lens: P, st: &Style) {
    let src = pm.clone();
    let view = magnifier_lens(r, lens);
    let (from, to) = (r.center(), view.center());
    let tf = Transform::identity();
    let line = stroke(st.width().max(2.0), false);
    let mut pb = PathBuilder::new();
    pb.move_to(from.x, from.y);
    pb.line_to(to.x, to.y);
    if let Some(path) = pb.finish() {
        pm.stroke_path(&path, &paint(st.color, 1.0), &line, tf, None);
    }
    if let Some(path) = oval_path(r) {
        pm.stroke_path(&path, &paint(st.color, 1.0), &line, tf, None);
    }
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let (cx, cy, rx, ry) = (to.x, to.y, (view.w() / 2.0).max(1.0), (view.h() / 2.0).max(1.0));
    let sd = src.data();
    let d = pm.data_mut();
    for y in (view.y0.floor() as i32).clamp(0, h)..(view.y1.ceil() as i32).clamp(0, h) {
        for x in (view.x0.floor() as i32).clamp(0, w)..(view.x1.ceil() as i32).clamp(0, w) {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (dx, dy) = ((px - cx) / rx, (py - cy) / ry);
            if dx * dx + dy * dy > 1.0 {
                continue;
            }
            let sx = ((r.x0 + (px - view.x0) / MAGNIFIER_ZOOM).floor() as i32).clamp(0, w - 1);
            let sy = ((r.y0 + (py - view.y0) / MAGNIFIER_ZOOM).floor() as i32).clamp(0, h - 1);
            let i = ((y * w + x) * 4) as usize;
            let j = ((sy * w + sx) * 4) as usize;
            d[i..i + 4].copy_from_slice(&sd[j..j + 4]);
        }
    }
    if let Some(path) = oval_path(&view) {
        pm.stroke_path(&path, &paint(st.color, 1.0), &line, tf, None);
    }
}

pub fn ruler_label(a: P, b: P, scale_factor: f32, points: bool) -> String {
    let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    let s = if points { scale_factor.max(1.0) } else { 1.0 };
    let unit = if points { "pt" } else { "px" };
    if dx < 1.0 || dy < 1.0 {
        format!("{} {}", ((dx.max(dy)) / s).round() as i64, unit)
    } else {
        format!("{}×{} {}", (dx / s).round() as i64, (dy / s).round() as i64, unit)
    }
}

fn draw_ruler(pm: &mut Pixmap, a: P, b: P, st: &Style, fonts: &Fonts, scale_factor: f32) {
    let tf = Transform::identity();
    let pt = paint(st.color, 1.0);
    let len = a.dist(b).max(1.0);
    let (nx, ny) = (-(b.y - a.y) / len, (b.x - a.x) / len);
    let tick = 6.0;
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x, b.y);
    for q in [a, b] {
        pb.move_to(q.x - nx * tick, q.y - ny * tick);
        pb.line_to(q.x + nx * tick, q.y + ny * tick);
    }
    if let Some(path) = pb.finish() {
        pm.stroke_path(&path, &pt, &Stroke { width: 1.5, line_cap: LineCap::Butt, ..Default::default() }, tf, None);
    }
    let label = ruler_label(a, b, scale_factor, st.dashed);
    let size = 13.0;
    let (tw, _) = measure(&fonts.sans, &label, size);
    let m = p((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let bx = R { x0: m.x - tw / 2.0 - 6.0, y0: m.y - size * 0.75 - 4.0, x1: m.x + tw / 2.0 + 6.0, y1: m.y + size * 0.75 + 4.0 };
    if let Some(path) = round_rect_path(&bx, 5.0) {
        pm.fill_path(&path, &pt, FillRule::Winding, tf, None);
    }
    let txt = if is_light(st.color) { [0, 0, 0, 255] } else { [255, 255, 255, 255] };
    draw_text_line(pm, &fonts.sans, &label, p(m.x - tw / 2.0, m.y + size * 0.36), size, txt, None);
}

pub fn measure(font: &FontVec, s: &str, size: f32) -> (f32, f32) {
    let sf = font.as_scaled(size);
    let mut x = 0.0;
    let mut prev = None;
    for c in s.chars() {
        let id = sf.glyph_id(c);
        if let Some(pv) = prev {
            x += sf.kern(pv, id);
        }
        x += sf.h_advance(id);
        prev = Some(id);
    }
    (x, sf.height())
}

fn text_path(font: &FontVec, s: &str, origin: P, size: f32) -> Option<Path> {
    let sf = font.as_scaled(size);
    let (hs, vs) = (sf.h_scale_factor(), sf.v_scale_factor());
    let mut pb = PathBuilder::new();
    let mut x = origin.x;
    let mut prev = None;
    for c in s.chars() {
        let id = sf.glyph_id(c);
        if let Some(pv) = prev {
            x += sf.kern(pv, id);
        }
        if let Some(outline) = font.outline(id) {
            let map = |pt: ab_glyph::Point| (x + pt.x * hs, origin.y - pt.y * vs);
            let mut last: Option<(f32, f32)> = None;
            for curve in &outline.curves {
                let (start, end) = match curve {
                    OutlineCurve::Line(a, b) => (map(*a), map(*b)),
                    OutlineCurve::Quad(a, _, b) => (map(*a), map(*b)),
                    OutlineCurve::Cubic(a, _, _, b) => (map(*a), map(*b)),
                };
                let gap = last.map_or(true, |l| (l.0 - start.0).abs() > 0.01 || (l.1 - start.1).abs() > 0.01);
                if gap {
                    if last.is_some() {
                        pb.close();
                    }
                    pb.move_to(start.0, start.1);
                }
                match curve {
                    OutlineCurve::Line(_, _) => pb.line_to(end.0, end.1),
                    OutlineCurve::Quad(_, c, _) => {
                        let c = map(*c);
                        pb.quad_to(c.0, c.1, end.0, end.1)
                    }
                    OutlineCurve::Cubic(_, c1, c2, _) => {
                        let (c1, c2) = (map(*c1), map(*c2));
                        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1)
                    }
                }
                last = Some(end);
            }
            if last.is_some() {
                pb.close();
            }
        }
        x += sf.h_advance(id);
        prev = Some(id);
    }
    pb.finish()
}

fn draw_text_line(pm: &mut Pixmap, font: &FontVec, s: &str, baseline: P, size: f32, c: Rgba, outline: Option<(Rgba, f32)>) {
    if let Some(path) = text_path(font, s, baseline, size) {
        let tf = Transform::identity();
        if let Some((oc, ow)) = outline {
            let st = Stroke { width: ow, line_join: LineJoin::Round, ..Default::default() };
            pm.stroke_path(&path, &paint(oc, 1.0), &st, tf, None);
        }
        pm.fill_path(&path, &paint(c, 1.0), FillRule::Winding, tf, None);
    }
}

fn text_outline(size: f32) -> f32 {
    (size * 0.14).max(2.0)
}

pub fn text_box(at: P, text: &str, st: &Style, fonts: &Fonts) -> R {
    let font = fonts.text(st);
    let size = st.text_size();
    let line_h = st.text_line_height();
    let lines: Vec<&str> = text.split('\n').collect();
    let w = lines.iter().map(|l| measure(font, l, size).0).fold(size * 0.3, f32::max);
    let pad = size * 0.3;
    R { x0: at.x - pad, y0: at.y - pad * 0.6, x1: at.x + w + pad, y1: at.y + line_h * lines.len() as f32 + pad * 0.6 }
}

fn draw_text(pm: &mut Pixmap, at: P, text: &str, st: &Style, fonts: &Fonts) {
    let font = fonts.text(st);
    let size = st.text_size();
    let line_h = st.text_line_height();
    let tf = Transform::identity();
    let bx = text_box(at, text, st, fonts);
    let (fg, outline) = match st.fill {
        Fill::Opaque => {
            if let Some(path) = round_rect_path(&bx, size * 0.25) {
                pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None);
            }
            (if luminance(st.color) > TEXT_LIGHT_ABOVE { [0, 0, 0, 255] } else { [255, 255, 255, 255] }, None)
        }
        Fill::Translucent => {
            if let Some(path) = round_rect_path(&bx, size * 0.25) {
                pm.fill_path(&path, &paint(st.color, 0.25), FillRule::Winding, tf, None);
            }
            (st.color, None)
        }
        Fill::Transparent => {
            let oc = if is_light(st.color) { [0, 0, 0, 200] } else { [255, 255, 255, 255] };
            (st.color, Some((oc, text_outline(size))))
        }
        Fill::HandDrawn => (st.color, None),
    };
    for (i, line) in text.split('\n').enumerate() {
        let baseline = p(at.x, at.y + size * 0.95 + i as f32 * line_h);
        draw_text_line(pm, font, line, baseline, size, fg, outline);
    }
}

/// Base image plus every object (optionally skipping one), no crop or backdrop.
pub fn compose(doc: &Doc, fonts: &Fonts, skip: Option<usize>) -> Pixmap {
    let mut pm = to_pixmap(&doc.base);
    let holes: Vec<(R, Shape)> = doc
        .objs
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != skip)
        .filter_map(|(_, o)| if let Kind::Spotlight { r } = o.kind { Some((r, o.style.shape)) } else { None })
        .collect();
    // Every spotlight shares one dark layer, as dark as the newest spotlight says.
    let dim = doc.objs.iter().enumerate().rev().find(|(i, o)| Some(*i) != skip && matches!(o.kind, Kind::Spotlight { .. })).map_or(SPOTLIGHT_DIM, |(_, o)| o.style.dim);
    let mut spot_done = false;
    for (i, o) in doc.objs.iter().enumerate() {
        if Some(i) == skip {
            continue;
        }
        if let Kind::Spotlight { .. } = o.kind {
            if !spot_done {
                draw_spotlight(&mut pm, &holes, dim);
                spot_done = true;
            }
            continue;
        }
        draw_obj(&mut pm, o, fonts, doc.scale_factor);
    }
    pm
}

pub fn crop_pixmap(pm: &Pixmap, r: &R) -> Pixmap {
    let (x0, y0, x1, y1) = region(pm, r);
    let (w, h) = ((x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32);
    let mut out = Pixmap::new(w, h).unwrap();
    let sw = pm.width() as usize;
    let s = pm.data();
    let d = out.data_mut();
    for y in 0..h as usize {
        let si = ((y0 as usize + y) * sw + x0 as usize) * 4;
        let di = y * w as usize * 4;
        d[di..di + w as usize * 4].copy_from_slice(&s[si..si + w as usize * 4]);
    }
    out
}

fn sample_stops(stops: &[(f32, Rgba)], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let mut prev = stops[0];
    for &(s, c) in stops {
        if t <= s {
            let span = (s - prev.0).max(1e-4);
            let k = ((t - prev.0) / span).clamp(0.0, 1.0);
            return [0, 1, 2].map(|i| prev.1[i] as f32 + (c[i] as f32 - prev.1[i] as f32) * k);
        }
        prev = (s, c);
    }
    let c = stops[stops.len() - 1].1;
    [c[0] as f32, c[1] as f32, c[2] as f32]
}

/// Seeded single-octave Perlin noise, the source of the mesh gradient.
struct Perlin {
    perm: [u8; 512],
}

impl Perlin {
    fn new(seed: u64) -> Perlin {
        let mut p: Vec<u8> = (0..=255).collect();
        let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        for i in (1..256).rev() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            p.swap(i, (s % (i as u64 + 1)) as usize);
        }
        let mut perm = [0u8; 512];
        for i in 0..512 {
            perm[i] = p[i & 255];
        }
        Perlin { perm }
    }

    fn grad(h: u8, x: f32, y: f32) -> f32 {
        match h & 7 {
            0 => x + y,
            1 => x - y,
            2 => -x + y,
            3 => -x - y,
            4 => x,
            5 => -x,
            6 => y,
            _ => -y,
        }
    }

    fn at(&self, x: f32, y: f32) -> f32 {
        let (xi, yi) = (x.floor() as i32 & 255, y.floor() as i32 & 255);
        let (xf, yf) = (x - x.floor(), y - y.floor());
        let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let (u, v) = (fade(xf), fade(yf));
        let p = &self.perm;
        let h = |dx: i32, dy: i32| p[(p[((xi + dx) & 255) as usize] as i32 + yi + dy) as usize & 511];
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let x1 = lerp(Self::grad(h(0, 0), xf, yf), Self::grad(h(1, 0), xf - 1.0, yf), u);
        let x2 = lerp(Self::grad(h(0, 1), xf, yf - 1.0), Self::grad(h(1, 1), xf - 1.0, yf - 1.0), u);
        lerp(x1, x2, v)
    }
}

/// Mesh gradient: noise normalised to its own min…max picks a colour from a 256-step table,
/// then the map is Gaussian-blurred (radius MESH_BLUR × size) and trimmed by twice the radius
/// a Gaussian blur then a crop.
fn mesh_pixmap(w: u32, h: u32, stops: &[(f32, Rgba)], seed: u64) -> Pixmap {
    let n = MESH_MAP_SIZE;
    let radius = (n as f32 * MESH_BLUR).round() as i32;
    let margin = (radius * 2) as u32;
    let full = n + margin * 2;
    let noise = Perlin::new(seed);
    let freq = MESH_PERIODS / n as f32;
    let vals: Vec<f32> = (0..full * full)
        .map(|i| noise.at((i % full) as f32 * freq, (i / full) as f32 * freq))
        .collect();
    let (lo, hi) = vals.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let span = (hi - lo).max(1e-6);
    let table: Vec<[f32; 3]> = (0..256).map(|i| sample_stops(stops, i as f32 / 255.0)).collect();
    let mut map = Pixmap::new(full, full).unwrap();
    for (px, v) in map.data_mut().chunks_exact_mut(4).zip(&vals) {
        let c = table[(((v - lo) / span) * 255.0).round().clamp(0.0, 255.0) as usize];
        px.copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
    }
    box_blur(&mut map, &R { x0: 0.0, y0: 0.0, x1: full as f32, y1: full as f32 }, radius, 3);
    let map = crop_pixmap(&map, &R { x0: margin as f32, y0: margin as f32, x1: (margin + n) as f32, y1: (margin + n) as f32 });
    let mut out = Pixmap::new(w.max(1), h.max(1)).unwrap();
    let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
    let tf = Transform::from_scale(w.max(1) as f32 / n as f32, h.max(1) as f32 / n as f32);
    out.draw_pixmap(0, 0, map.as_ref(), &paint, tf, None);
    out
}

/// The flat or gradient paint behind the screenshot (no picture).
pub fn gradient_pixmap(w: u32, h: u32, bd: &Backdrop) -> Pixmap {
    let mut stops = bd.stops();
    if bd.reverse {
        stops = stops.into_iter().rev().map(|(s, c)| (1.0 - s, c)).collect();
    }
    if bd.fill == BackdropFill::Solid {
        let mut pm = Pixmap::new(w.max(1), h.max(1)).unwrap();
        pm.fill(color([bd.solid[0], bd.solid[1], bd.solid[2], 255], 1.0));
        return pm;
    }
    if bd.kind == GradientKind::Mesh {
        return mesh_pixmap(w, h, &stops, MESH_SEED_BASE + bd.gradient as u64);
    }
    let mut pm = Pixmap::new(w.max(1), h.max(1)).unwrap();
    let (fw, fh) = (w.max(1) as f32, h.max(1) as f32);
    let d = pm.data_mut();
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / fw, y as f32 / fh);
            let c = match bd.kind {
                GradientKind::Vertical => sample_stops(&stops, v),
                GradientKind::Diagonal => sample_stops(&stops, (u + v) / 2.0),
                _ => {
                    let (dx, dy) = (u - 0.5, v - 0.5);
                    sample_stops(&stops, (dx * dx + dy * dy).sqrt() / 0.7071)
                }
            };
            let i = ((y * w + x) * 4) as usize;
            d[i] = c[0] as u8;
            d[i + 1] = c[1] as u8;
            d[i + 2] = c[2] as u8;
            d[i + 3] = 255;
        }
    }
    pm
}

/// A picture scaled to cover `w` × `h` (cropped around its centre), then blurred by `blur_radius`
/// (a Gaussian sigma, in 1000-pixel units so a preview and an export look alike).
pub fn image_backdrop_pixmap(w: u32, h: u32, img: &Image, blur_radius: f32) -> Pixmap {
    let (w, h) = (w.max(1), h.max(1));
    let src = image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).unwrap_or_else(|| image::RgbaImage::new(1, 1));
    let up = (w as f32 / img.w.max(1) as f32).max(h as f32 / img.h.max(1) as f32) > 1.0;
    let filter = if up { image::imageops::FilterType::CatmullRom } else { image::imageops::FilterType::Triangle };
    let dynimg = image::DynamicImage::ImageRgba8(src).resize_to_fill(w, h, filter).to_rgba8();
    let mut pm = to_pixmap(&Image { w, h, px: Arc::new(dynimg.into_raw()) });
    let sigma = blur_radius * (w.max(h) as f32 / 1000.0).max(1.0);
    if sigma >= 0.5 {
        box_blur(&mut pm, &R { x0: 0.0, y0: 0.0, x1: w as f32, y1: h as f32 }, sigma.round().max(1.0) as i32, 3);
    }
    pm
}

/// The part of `pm` left after trimming the border rows and columns that are all the colour of the
/// top-left pixel (a see-through margin, a flat page edge).
pub fn autocrop_rect(pm: &Pixmap) -> R {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let d = pm.data();
    let reference: [u8; 4] = [d[0], d[1], d[2], d[3]];
    let same = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        (0..4).all(|c| (d[i + c] as i32 - reference[c] as i32).abs() <= 6)
    };
    let row_empty = |y: usize| (0..w).all(|x| same(x, y));
    let col_empty = |x: usize, y0: usize, y1: usize| (y0..y1).all(|y| same(x, y));
    let mut top = 0;
    while top < h && row_empty(top) {
        top += 1;
    }
    if top == h {
        return R { x0: 0.0, y0: 0.0, x1: w as f32, y1: h as f32 };
    }
    let mut bottom = h;
    while bottom > top + 1 && row_empty(bottom - 1) {
        bottom -= 1;
    }
    let mut left = 0;
    while left + 1 < w && col_empty(left, top, bottom) {
        left += 1;
    }
    let mut right = w;
    while right > left + 1 && col_empty(right - 1, top, bottom) {
        right -= 1;
    }
    R { x0: left as f32, y0: top as f32, x1: right as f32, y1: bottom as f32 }
}

/// How far the picture can be moved off-centre in each direction, in output pixels.
pub fn backdrop_max_offset(cw: f32, ch: f32, bd: &Backdrop) -> (f32, f32) {
    let mut flat = bd.clone();
    flat.offset = [0.0, 0.0];
    let (w, h, _, _, iw, ih) = backdrop_layout(cw, ch, &flat);
    (((w as f32 - iw) / 2.0).max(0.0), ((h as f32 - ih) / 2.0).max(0.0))
}

/// The output size of a backdrop around a picture of `cw` × `ch`, and where the framed picture
/// (inset included, `iw` × `ih`) sits in it.
pub fn backdrop_layout(cw: f32, ch: f32, bd: &Backdrop) -> (u32, u32, f32, f32, f32, f32) {
    let inset = bd.inset_px();
    let (iw, ih) = (cw + inset * 2.0, ch + inset * 2.0);
    let pad = bd.padding_px();
    let (mut w, mut h) = (iw + pad * 2.0, ih + pad * 2.0);
    if let Some(ratio) = bd.ratio {
        if w / h < ratio {
            w = h * ratio;
        } else {
            h = w / ratio;
        }
    }
    let (w, h) = (w.round() as u32, h.round() as u32);
    // The picture may be moved off-centre, but not out of the backdrop.
    let max_x = ((w as f32 - iw) / 2.0).max(0.0);
    let max_y = ((h as f32 - ih) / 2.0).max(0.0);
    let ox = (((w as f32 - iw) / 2.0) + bd.offset[0].clamp(-max_x, max_x)).round();
    let oy = (((h as f32 - ih) / 2.0) + bd.offset[1].clamp(-max_y, max_y)).round();
    (w, h, ox, oy, iw, ih)
}

pub fn apply_backdrop(content: &Pixmap, bd: &Backdrop) -> Pixmap {
    let trimmed;
    let content = if bd.autocrop {
        trimmed = crop_pixmap(content, &autocrop_rect(content));
        &trimmed
    } else {
        content
    };
    let (cw, ch) = (content.width() as f32, content.height() as f32);
    let inset = bd.inset_px();
    let (w, h, ox, oy, iw, ih) = backdrop_layout(cw, ch, bd);
    let tf = Transform::identity();
    let pp = tiny_skia::PixmapPaint::default();
    let mut pm = match &bd.image {
        Some(i) => image_backdrop_pixmap(w, h, &i.img, bd.blur_radius()),
        None => gradient_pixmap(w, h, bd),
    };
    let frame = R { x0: ox, y0: oy, x1: ox + iw, y1: oy + ih };
    let shadow = bd.shadow_amount();
    let (inner_r, outer_r) = (bd.inner_radius_px(), bd.outer_radius_px());
    if shadow > 0.0 {
        let blur = (iw.max(ih) * 0.03 * shadow).max(2.0);
        let mut sh = Pixmap::new(w, h).unwrap();
        if let Some(path) = round_rect_path(&frame.translate(0.0, blur * 0.4), inner_r) {
            sh.fill_path(&path, &paint([0, 0, 0, 255], (0.35 + 0.4 * shadow).min(0.8)), FillRule::Winding, tf, None);
        }
        box_blur(&mut sh, &R { x0: 0.0, y0: 0.0, x1: w as f32, y1: h as f32 }, (blur / 1.7) as i32, 3);
        pm.draw_pixmap(0, 0, sh.as_ref(), &pp, tf, None);
    }
    let mut inner = Pixmap::new(iw.max(1.0) as u32, ih.max(1.0) as u32).unwrap();
    if inset > 0.0 {
        let bg = border_average(content, &R { x0: 0.0, y0: 0.0, x1: cw, y1: ch });
        let un = |v: u8| if bg[3] == 0 { 255 } else { ((v as u32 * 255) / bg[3] as u32).min(255) as u8 };
        inner.fill(color([un(bg[0]), un(bg[1]), un(bg[2]), 255], 1.0));
    }
    inner.draw_pixmap(inset as i32, inset as i32, content.as_ref(), &pp, tf, None);
    let mut mask = tiny_skia::Mask::new(w, h).unwrap();
    if let Some(path) = round_rect_path(&frame, inner_r) {
        mask.fill_path(&path, FillRule::Winding, true, tf);
    }
    pm.draw_pixmap(ox as i32, oy as i32, inner.as_ref(), &pp, tf, Some(&mask));
    if outer_r > 0.0 {
        let mut out = Pixmap::new(w, h).unwrap();
        let mut m = tiny_skia::Mask::new(w, h).unwrap();
        if let Some(path) = round_rect_path(&R { x0: 0.0, y0: 0.0, x1: w as f32, y1: h as f32 }, outer_r) {
            m.fill_path(&path, FillRule::Winding, true, tf);
        }
        out.draw_pixmap(0, 0, pm.as_ref(), &pp, tf, Some(&m));
        pm = out;
    }
    pm
}

/// Final picture: objects, crop, then backdrop.
pub fn export(doc: &Doc, fonts: &Fonts) -> Image {
    export_without(doc, fonts, None, false)
}

/// `export` with one object left out, and optionally the backdrop's automatic crop off.
pub fn export_without(doc: &Doc, fonts: &Fonts, skip: Option<usize>, keep_size: bool) -> Image {
    let pm = compose(doc, fonts, skip);
    let pm = match doc.crop {
        Some(r) => crop_pixmap(&pm, &r),
        None => pm,
    };
    let pm = match &doc.backdrop {
        Some(bd) if keep_size && bd.autocrop => apply_backdrop(&pm, &Backdrop { autocrop: false, ..bd.clone() }),
        Some(bd) => apply_backdrop(&pm, bd),
        None => pm,
    };
    from_pixmap(&pm)
}

/// Why no before/after GIF can be made.
pub const GIF_NEEDS_OVERLAY: &str = "A before/after GIF needs a pasted picture placed over the screenshot. Paste one with ⌘V and position it first.";

/// The two frames of a before/after GIF: the screenshot as it is without the topmost pasted picture,
/// then with it. The picture has to overlap what the document shows.
pub fn gif_frames(doc: &Doc, fonts: &Fonts) -> Result<(Image, Image), String> {
    let Some((i, r)) = doc.objs.iter().enumerate().rev().find_map(|(i, o)| if let Kind::Image { r, .. } = &o.kind { Some((i, *r)) } else { None }) else {
        return Err(GIF_NEEDS_OVERLAY.into());
    };
    let v = doc.view_rect();
    if r.x1.min(v.x1) - r.x0.max(v.x0) < 1.0 || r.y1.min(v.y1) - r.y0.max(v.y0) < 1.0 {
        return Err("The pasted picture must overlap the screenshot. Move it over the screenshot first.".into());
    }
    let (before, after) = (export_without(doc, fonts, Some(i), false), export(doc, fonts));
    if (before.w, before.h) == (after.w, after.h) {
        return Ok((before, after));
    }
    // The backdrop's automatic crop trimmed the two differently: leave it off so the frames match.
    Ok((export_without(doc, fonts, Some(i), true), export_without(doc, fonts, None, true)))
}

/// Delay between the two frames of the before/after GIF.
pub const GIF_DELAY_MS: u32 = 1000;

/// Encodes frames (all the same size) as a looping GIF, `GIF_DELAY_MS` apart. GIF has no partial
/// transparency, so frames are laid on white first.
pub fn encode_gif(frames: &[Image]) -> Result<Vec<u8>, String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    if frames.windows(2).any(|p| (p[0].w, p[0].h) != (p[1].w, p[1].h)) {
        return Err("The GIF frames have different sizes".into());
    }
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new_with_speed(&mut out, 10);
        enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
        for f in frames {
            let flat = crate::windowbg::flatten(f, [255, 255, 255]);
            let rgba = image::RgbaImage::from_raw(flat.w, flat.h, flat.px.to_vec()).ok_or("bad image")?;
            let delay = image::Delay::from_numer_denom_ms(GIF_DELAY_MS, 1);
            enc.encode_frame(image::Frame::from_parts(rgba, 0, 0, delay)).map_err(|e| e.to_string())?;
        }
    }
    Ok(out)
}

/// Flatten every object into the base picture (used by Rasterize, Resize and Cut Out).
pub fn rasterize(doc: &mut Doc, fonts: &Fonts) {
    let pm = compose(doc, fonts, None);
    doc.base = from_pixmap(&pm);
    doc.objs.clear();
}

/// Interpolation: high quality by
/// default, medium at 50 % or below, none for whole-number enlargements so pixels stay sharp.
pub fn resize(img: &Image, factor: f32) -> Image {
    use image::imageops::FilterType;
    let nw = ((img.w as f32 * factor).round() as u32).max(1);
    let nh = ((img.h as f32 * factor).round() as u32).max(1);
    let filter = if factor >= 1.0 && factor.round() == factor {
        FilterType::Nearest
    } else if factor <= 0.5 {
        FilterType::Triangle
    } else {
        FilterType::Lanczos3
    };
    let src = image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).unwrap();
    let out = image::imageops::resize(&src, nw, nh, filter);
    Image { w: nw, h: nh, px: Arc::new(out.into_raw()) }
}

/// Remove a horizontal band (wide selection) or a vertical band (tall selection) and join the rest.
pub fn cut_out(img: &Image, r: &R) -> Image {
    let r = r.clamp_to(img.w as f32, img.h as f32);
    let (w, h) = (img.w as usize, img.h as usize);
    if r.w() >= r.h() {
        let (y0, y1) = (r.y0.round() as usize, r.y1.round() as usize);
        let mut out = Vec::with_capacity(img.px.len());
        for y in (0..h).filter(|y| *y < y0 || *y >= y1) {
            out.extend_from_slice(&img.px[y * w * 4..(y + 1) * w * 4]);
        }
        let nh = (out.len() / (w * 4)).max(1) as u32;
        Image { w: img.w, h: nh, px: Arc::new(out) }
    } else {
        let (x0, x1) = (r.x0.round() as usize, r.x1.round() as usize);
        let nw = (w - (x1 - x0)).max(1);
        let mut out = Vec::with_capacity(nw * h * 4);
        for y in 0..h {
            for x in (0..w).filter(|x| *x < x0 || *x >= x1) {
                let i = (y * w + x) * 4;
                out.extend_from_slice(&img.px[i..i + 4]);
            }
        }
        Image { w: nw as u32, h: img.h, px: Arc::new(out) }
    }
}

/// Horizontal (or vertical) run of pixels similar to the one under the cursor: the Ruler's span.
pub fn ruler_span(img: &Image, at: P, horizontal: bool, sensitivity: f32) -> (P, P) {
    let (x, y) = (at.x.floor() as i32, at.y.floor() as i32);
    let c = img.get(x, y);
    let limit = (2.0 * sensitivity * sensitivity) as i32;
    let same = |q: Rgba| (0..3).map(|i| (q[i] as i32 - c[i] as i32).abs()).sum::<i32>() <= limit;
    if horizontal {
        let mut l = x;
        while l > 0 && same(img.get(l - 1, y)) {
            l -= 1;
        }
        let mut r = x;
        while r < img.w as i32 - 1 && same(img.get(r + 1, y)) {
            r += 1;
        }
        (p(l as f32, y as f32 + 0.5), p(r as f32 + 1.0, y as f32 + 0.5))
    } else {
        let mut t = y;
        while t > 0 && same(img.get(x, t - 1)) {
            t -= 1;
        }
        let mut b = y;
        while b < img.h as i32 - 1 && same(img.get(x, b + 1)) {
            b += 1;
        }
        (p(x as f32 + 0.5, t as f32), p(x as f32 + 0.5, b as f32 + 1.0))
    }
}

/// Darkest colour in the square around the cursor (Shift+Tab "Copy Text Color").
pub fn darkest_near(img: &Image, at: P) -> Rgba {
    let (cx, cy) = (at.x as i32, at.y as i32);
    let half = DARKEST_AREA / 2;
    let mut best = img.get(cx, cy);
    let lum = |c: Rgba| c[0] as u32 * 299 + c[1] as u32 * 587 + c[2] as u32 * 114;
    for y in cy - half..=cy + half {
        for x in cx - half..=cx + half {
            let c = img.get(x, y);
            if lum(c) < lum(best) {
                best = c;
            }
        }
    }
    best
}

pub fn average(img: &Image, r: &R) -> Rgba {
    let r = r.clamp_to(img.w as f32, img.h as f32);
    let mut acc = [0u64; 3];
    let mut n = 0u64;
    for y in r.y0 as i32..r.y1.ceil() as i32 {
        for x in r.x0 as i32..r.x1.ceil() as i32 {
            let c = img.get(x, y);
            for i in 0..3 {
                acc[i] += c[i] as u64;
            }
            n += 1;
        }
    }
    let n = n.max(1);
    [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, 255]
}

pub use crate::color::{format_color, COLOR_FORMATS};

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Image {
        let (w, h) = (900u32, 560u32);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let band = (y / 70) % 2 == 0;
                let c = if x > 520 && y > 60 && y < 260 { [40, 44, 52] } else if band { [245, 246, 248] } else { [228, 233, 240] };
                px.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        Image { w, h, px: Arc::new(px) }
    }

    fn flat(w: u32, h: u32, c: [u8; 4]) -> Image {
        Image { w, h, px: Arc::new((0..w * h).flat_map(|_| c).collect()) }
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * pm.width() + x) * 4) as usize;
        [pm.data()[i], pm.data()[i + 1], pm.data()[i + 2], pm.data()[i + 3]]
    }

    #[test]
    fn autocrop_trims_a_see_through_or_flat_border() {
        let mut pm = Pixmap::new(100, 60).unwrap();
        // Transparent margin around an opaque block at x 20..70, y 10..40.
        for y in 10..40 {
            for x in 20..70 {
                let i = ((y * 100 + x) * 4) as usize;
                pm.data_mut()[i..i + 4].copy_from_slice(&[10, 20, 30, 255]);
            }
        }
        let r = autocrop_rect(&pm);
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (20.0, 10.0, 70.0, 40.0));
        // A flat page-coloured border goes too.
        let mut pm = Pixmap::new(30, 30).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(250, 250, 250, 255));
        let i = ((12 * 30 + 5) * 4) as usize;
        pm.data_mut()[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
        let r = autocrop_rect(&pm);
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (5.0, 12.0, 6.0, 13.0));
        // Nothing but border: left alone.
        let mut pm = Pixmap::new(8, 8).unwrap();
        pm.fill(tiny_skia::Color::WHITE);
        let r = autocrop_rect(&pm);
        assert_eq!((r.w(), r.h()), (8.0, 8.0));
    }

    #[test]
    fn autocrop_changes_the_backdrop_size() {
        let mut pm = Pixmap::new(200, 100).unwrap();
        let i = ((50 * 200 + 100) * 4) as usize;
        pm.data_mut()[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
        let mut bd = Backdrop::default();
        bd.padding = 0.0;
        bd.inset = 0.0;
        bd.outer_radius = 0.0;
        bd.inner_radius = 0.0;
        assert_eq!((apply_backdrop(&pm, &bd).width(), apply_backdrop(&pm, &bd).height()), (200, 100));
        bd.autocrop = true;
        let out = apply_backdrop(&pm, &bd);
        assert_eq!((out.width(), out.height()), (1, 1));
    }

    #[test]
    fn picture_backdrop_covers_the_canvas_and_blur_smooths_it() {
        // Left half red, right half blue, wider than the canvas: cover crops the sides.
        let mut img = flat(200, 100, [255, 0, 0, 255]);
        {
            let px = Arc::make_mut(&mut img.px);
            for y in 0..100 {
                for x in 100..200 {
                    let i = (y * 200 + x) * 4;
                    px[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
                }
            }
        }
        let sharp = image_backdrop_pixmap(100, 100, &img, 0.0);
        assert_eq!((sharp.width(), sharp.height()), (100, 100));
        assert!(px(&sharp, 5, 50)[0] > 240 && px(&sharp, 94, 50)[2] > 240);
        assert!(px(&sharp, 5, 0)[3] == 255 && px(&sharp, 99, 99)[3] == 255, "no transparent edges");
        let blurred = image_backdrop_pixmap(100, 100, &img, 4.0);
        let (a, b) = (px(&sharp, 46, 50), px(&blurred, 46, 50));
        assert!(a[2] < 30 && b[2] > 30 + 20, "blur bleeds the edge: sharp {a:?} blurred {b:?}");
        assert!(px(&blurred, 0, 0)[3] == 255 && px(&blurred, 99, 99)[3] == 255, "edges stay opaque");
    }

    #[test]
    fn backdrop_paints_a_picture_a_flat_colour_and_a_custom_gradient() {
        let content = Pixmap::new(40, 40).unwrap();
        let mut bd = Backdrop::default();
        bd.padding = 10.0;
        bd.shadow = 0.0;
        bd.outer_radius = 0.0;
        bd.kind = GradientKind::Vertical;
        // Flat colour.
        bd.fill = BackdropFill::Solid;
        let out = apply_backdrop(&content, &bd);
        assert_eq!(px(&out, 2, 2), [0x60, 0x80, 0xA0, 255]);
        assert_eq!(px(&out, out.width() - 3, out.height() - 3), [0x60, 0x80, 0xA0, 255]);
        // Two colours: top is the first, bottom the second.
        bd.fill = BackdropFill::Custom;
        let out = apply_backdrop(&content, &bd);
        let (top, bottom) = (px(&out, 2, 2), px(&out, 2, out.height() - 3));
        assert!(top[2] < bottom[2] && top[0] < bottom[0], "{top:?} -> {bottom:?}");
        bd.reverse = true;
        let rev = apply_backdrop(&content, &bd);
        assert!(px(&rev, 2, 2)[0] > px(&rev, 2, rev.height() - 3)[0], "reverse swaps the colours");
        // A picture replaces the gradient.
        bd.reverse = false;
        bd.image = Some(BackdropImage::new(flat(64, 64, [0, 255, 0, 255])));
        let out = apply_backdrop(&content, &bd);
        assert_eq!(px(&out, 2, 2), [0, 255, 0, 255]);
    }

    #[test]
    fn backdrop_layout_centres_and_keeps_the_offset_inside() {
        let mut bd = Backdrop::default();
        bd.padding = 10.0; // 50 px
        bd.inset = 0.0;
        let (w, h, ox, oy, iw, ih) = backdrop_layout(100.0, 60.0, &bd);
        assert_eq!((w, h, iw, ih), (200, 160, 100.0, 60.0));
        assert_eq!((ox, oy), (50.0, 50.0));
        bd.offset = [20.0, -10.0];
        let (_, _, ox, oy, _, _) = backdrop_layout(100.0, 60.0, &bd);
        assert_eq!((ox, oy), (70.0, 40.0));
        bd.offset = [500.0, -500.0];
        let (_, _, ox, oy, _, _) = backdrop_layout(100.0, 60.0, &bd);
        assert_eq!((ox, oy), (100.0, 0.0), "cannot leave the backdrop");
        bd.offset = [0.0, 0.0];
        assert_eq!(backdrop_max_offset(100.0, 60.0, &bd), (50.0, 50.0));
        bd.ratio = Some(2.0);
        let (w, h, ..) = backdrop_layout(100.0, 60.0, &bd);
        assert_eq!((w, h), (320, 160), "ratio widens the canvas, picture stays centred");
    }

    #[test]
    fn zoom_keys_and_wheel_step_to_round_levels() {
        assert_eq!(zoom_key_target(1.0, true), 2.0);
        assert_eq!(zoom_key_target(1.0, false), 0.5);
        assert_eq!(zoom_key_target(8.0, true), 12.0);
        assert_eq!(zoom_wheel_target(1.0, true), 1.25);
        assert_eq!(zoom_wheel_target(60.0, true), ZOOM_MAX);
    }

    #[test]
    fn ruler_stops_where_colours_differ_past_the_sensitivity() {
        let (w, h) = (100u32, 4u32);
        let px: Vec<u8> = (0..w * h).flat_map(|i| if i % w < 60 { [200, 200, 200, 255] } else { [120, 120, 120, 255] }).collect();
        let img = Image { w, h, px: Arc::new(px) };
        let (a, b) = ruler_span(&img, p(10.0, 1.0), true, RULER_SENSITIVITY);
        assert_eq!((a.x, b.x), (0.0, 60.0));
    }

    fn ink_rows(pm: &Pixmap, x: u32) -> usize {
        (0..pm.height()).filter(|y| pm.pixel(x, *y).unwrap().alpha() > 128).count()
    }

    #[test]
    fn freehand_with_one_point_paints_a_dot() {
        let fonts = Fonts::load();
        let mut st = Style::default();
        st.size = 4;
        let pm = painted(&Obj { kind: Kind::Freehand { pts: vec![p(50.0, 50.0)] }, style: st }, &fonts, 100, 100);
        let ink = ink(&pm);
        assert!(!ink.is_empty());
        let (minx, maxx) = (ink.iter().map(|q| q.0).min().unwrap(), ink.iter().map(|q| q.0).max().unwrap());
        assert!(((maxx - minx + 1) as f32 - st.width()).abs() <= 2.0, "as wide as the line: {} vs {}", maxx - minx + 1, st.width());
        assert!(painted(&Obj { kind: Kind::Freehand { pts: vec![] }, style: st }, &fonts, 100, 100).pixels().iter().all(|c| c.alpha() == 0));
    }

    #[test]
    fn freehand_variability_thins_the_fast_part_of_a_stroke() {
        let fonts = Fonts::load();
        let mut st = Style::default();
        st.size = 5;
        let pts: Vec<P> = [10.0, 14.0, 18.0, 22.0, 26.0, 30.0, 34.0, 38.0, 90.0, 142.0, 194.0, 246.0, 298.0].iter().map(|x| p(*x, 50.0)).collect();
        let stroke = |vary| painted(&Obj { kind: Kind::Freehand { pts: pts.clone() }, style: Style { vary, ..st } }, &fonts, 320, 100);
        let plain = stroke(0);
        let varied = stroke(2);
        assert_eq!(ink_rows(&plain, 20), ink_rows(&plain, 250), "one width without variability");
        assert!(ink_rows(&varied, 20) > ink_rows(&varied, 250) + 2, "slow part {} fast part {}", ink_rows(&varied, 20), ink_rows(&varied, 250));
    }

    #[test]
    fn freehand_smoothness_rounds_off_a_corner() {
        let fonts = Fonts::load();
        let st = Style::default();
        let corner: Vec<P> = (0..=10).map(|i| p(10.0 + i as f32 * 8.0, 70.0)).chain((1..=10).map(|i| p(90.0, 70.0 - i as f32 * 8.0))).collect();
        let sharp = painted(&Obj { kind: Kind::Freehand { pts: corner.clone() }, style: st }, &fonts, 120, 100);
        let smooth = painted(&Obj { kind: Kind::Freehand { pts: corner }, style: Style { smooth: 6, ..st } }, &fonts, 120, 100);
        let outer = |pm: &Pixmap| ink(pm).iter().filter(|q| q.0 >= 90 && q.1 >= 68).count();
        assert!(outer(&smooth) < outer(&sharp), "the smoothed stroke cuts the corner: {} vs {}", outer(&smooth), outer(&sharp));
    }

    #[test]
    fn highlighter_cap_style_changes_the_ends() {
        let fonts = Fonts::load();
        let obj = |round_cap| Obj { kind: Kind::Highlighter { a: p(40.0, 50.0), b: p(160.0, 50.0) }, style: Style { round_cap, ..Style::default() } };
        let flat = painted(&obj(false), &fonts, 200, 100);
        let round = painted(&obj(true), &fonts, 200, 100);
        assert_eq!(flat.pixel(35, 50).unwrap().alpha(), 0, "flat ends stop at the points");
        assert!(round.pixel(35, 50).unwrap().alpha() > 0, "round ends reach past them");
        assert_eq!(round.pixel(40, 50).unwrap().alpha(), flat.pixel(40, 50).unwrap().alpha());
    }

    #[test]
    fn spotlight_darkness_follows_the_style() {
        let fonts = Fonts::load();
        let hole = R { x0: 20.0, y0: 20.0, x1: 60.0, y1: 60.0 };
        let shade = |dim: f32| {
            let mut doc = Doc::new(Image { w: 100, h: 100, px: Arc::new([255u8; 40000].to_vec()) }, 1.0);
            doc.objs = vec![Obj { kind: Kind::Spotlight { r: hole }, style: Style { dim, ..Style::default() } }];
            let pm = compose(&doc, &fonts, None);
            (pm.pixel(80, 80).unwrap().red(), pm.pixel(40, 40).unwrap().red())
        };
        let (light_out, light_in) = shade(0.2);
        let (dark_out, dark_in) = shade(0.8);
        assert_eq!((light_in, dark_in), (255, 255), "the lit part is untouched");
        assert!(dark_out < light_out, "{dark_out} < {light_out}");
        assert!((light_out as f32 - 255.0 * 0.8).abs() < 3.0 && (dark_out as f32 - 255.0 * 0.2).abs() < 3.0);
    }

    #[test]
    fn blur_pixelates_and_erase_blends_the_edges() {
        let fonts = Fonts::load();
        let mut doc = Doc::new(sample(), 2.0);
        let area = R { x0: 100.0, y0: 150.0, x1: 400.0, y1: 400.0 };
        let mut st = Style::default();
        doc.objs = vec![Obj { kind: Kind::Blur { r: area, text: None }, style: st }];
        let blurred = export(&doc, &fonts);
        assert_eq!(blurred.get(100, 150), blurred.get(102, 151), "pixels of one block share a colour");
        st.blur = BlurMode::Erase;
        doc.objs = vec![Obj { kind: Kind::Blur { r: area, text: None }, style: st }];
        let erased = export(&doc, &fonts);
        assert_eq!(erased.get(250, 200), sample().get(100, 200), "a flat row stays flat");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ocr_reads_text_from_a_picture() {
        let fonts = Fonts::load();
        let mut doc = Doc::new(sample(), 2.0);
        let mut st = Style::default();
        st.size = 6;
        st.fill = Fill::HandDrawn;
        st.color = [0, 0, 0, 255];
        doc.objs = vec![Obj { kind: Kind::Text { at: p(40.0, 280.0), text: "Hello ShotCraft".into() }, style: st }];
        let res = crate::system::recognize(&export(&doc, &fonts)).expect("OCR runs");
        assert!(res.lines.join(" ").contains("ShotCraft"), "read: {:?}", res.lines);
        for (w, h) in [(1u32, 1u32), (3, 3), (2000, 2), (2, 2000)] {
            let tiny = Image { w, h, px: Arc::new(vec![255; (w * h * 4) as usize]) };
            let _ = crate::system::recognize(&tiny);
            let _ = crate::system::text_boxes(&tiny);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finds_text_for_blur_text() {
        let fonts = Fonts::load();
        let mut doc = Doc::new(sample(), 2.0);
        let mut st = Style::default();
        st.size = 6;
        st.fill = Fill::HandDrawn;
        st.color = [0, 0, 0, 255];
        doc.objs = vec![Obj { kind: Kind::Text { at: p(40.0, 280.0), text: "Hello ShotCraft".into() }, style: st }];
        let boxes = crate::system::text_boxes(&export(&doc, &fonts));
        assert!(!boxes.is_empty(), "the Vision framework sees the text");
        assert!(boxes[0].y0 > 220.0 && boxes[0].y1 < 400.0, "box is near the text and counted from the top: {:?}", boxes[0]);
    }

    #[test]
    fn hand_drawn_brush_stays_within_its_taper() {
        let n = noise(42);
        let r = R { x0: 10.0, y0: 20.0, x1: 410.0, y1: 320.0 };
        let pts = hand_rect(&r, 3.0, 2.0, &n);
        assert!(pts.len() > 100, "the outline is sampled every two points");
        assert!(pts.iter().all(|q| q.x > 0.0 && q.x < 420.0 && q.y > 10.0 && q.y < 330.0), "the sketch hugs the rectangle");
        let widths = brush_widths(&pts, 6.0, 0.25, true, &n);
        assert!(widths.iter().all(|w| *w >= 4.49 && *w <= 7.51), "{widths:?}");
        let (lo, hi) = widths.iter().fold((f32::MAX, 0.0f32), |(l, h), w| (l.min(*w), h.max(*w)));
        assert!(hi - lo > 2.9, "the width uses the whole range");
        let corner = pts.iter().zip(&widths).filter(|(q, _)| q.x < 40.0 && q.y < 50.0).map(|(_, w)| *w).fold(0.0, f32::max);
        let middle = pts.iter().zip(&widths).filter(|(q, _)| (q.x - 210.0).abs() < 20.0 && q.y < 50.0).map(|(_, w)| *w).fold(0.0, f32::max);
        assert!(corner > middle, "bends swell: {corner} vs {middle}");
    }

    #[test]
    fn hand_drawn_oval_leaves_its_ends_apart() {
        let r = R { x0: 0.0, y0: 0.0, x1: 300.0, y1: 200.0 };
        let pts = hand_oval(&r, 6.0, 2.0, &noise(7));
        let (first, last) = (pts[0], pts[pts.len() - 1]);
        assert!(first.dist(last) > 1.0, "the stroke does not close on itself");
        assert!(pts.iter().all(|q| q.x > -20.0 && q.x < 320.0 && q.y > -20.0 && q.y < 220.0));
        let plain = hand_oval(&r, 50.0, 2.0, &noise(7));
        assert!(plain.iter().all(|q| q.x >= -0.01 && q.x <= 300.01 && q.y >= -0.01 && q.y <= 200.01), "a thick line keeps the exact ellipse");
    }

    #[test]
    fn plain_outline_is_a_rounded_rectangle() {
        let r = R { x0: 0.0, y0: 0.0, x1: 200.0, y1: 100.0 };
        let curves = outline_curves(&r, 0.0, 12.0, 2.0, &noise(1));
        assert_eq!((curves[0].a.x, curves[0].a.y), (12.0, 0.0));
        assert_eq!((curves[1].c1.x, curves[1].c1.y), (200.0, 0.0), "corners pull towards the exact corner");
        assert_eq!((curves[7].b.x, curves[7].b.y), (12.0, 0.0), "the outline closes");
    }

    #[test]
    fn arrows_follow_their_size_rules() {
        let st = Style { size: 2, scale: 2.0, ..Style::default() };
        assert_eq!(arrow_line(20.0, &st), 2.0, "a short arrow is capped to a tenth of its length");
        assert_eq!(arrow_line(100.0, &st), 6.0, "below the growth threshold the table applies");
        let long = arrow_line(4000.0, &st);
        assert_eq!(long, 9.0, "growth stops at 1.5 times");
        let (branch, spread) = arrow_branch(10.0, 2.0, 2.0);
        assert_eq!((branch, spread), (2.5, ARROW_SPREAD_SHORT), "branches stop at a quarter of the arrow");
        assert_eq!(arrow_branch(300.0, 6.0, 2.0).1, ARROW_SPREAD);
        assert_eq!(arrow_stretch(100.0, 0.7), (0.5, 0.5), "short arrows shrink as a whole");
        assert_eq!(arrow_stretch(300.0, 0.7), (1.0, 1.0));
        let (head, tail) = arrow_stretch(1020.0, 0.7);
        assert!((tail - 0.8).abs() < 1e-6 && (head - 1.25).abs() < 1e-6, "long arrows widen the head: {head} {tail}");
    }

    #[test]
    fn hand_drawn_arrow_is_nearly_straight() {
        let (a, b) = (p(0.0, 0.0), p(300.0, 0.0));
        let c = hand_arrow_shaft(a, b, 10.0, &noise(3));
        assert_eq!((c.b.x, c.b.y), (292.0, 0.0), "the shaft stops short of the tip");
        assert!((c.c1.x - 96.36).abs() < 1.0 && c.c1.y.abs() < 9.1, "handles sit a third of the way in: {:?}", c.c1);
        assert!(c.c2.y.abs() < 3.1);
    }

    #[test]
    fn plain_oval_keeps_its_proportions() {
        let square = plain_oval_curves(&R { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0 });
        assert!((square[0].a.x - 22.0).abs() < 1e-3 && (square[0].a.y - 6.0).abs() < 1e-3);
        assert_eq!((square[7].b.x, square[7].b.y), (square[0].a.x, square[0].a.y), "the outline closes");
        let wide = plain_oval_curves(&R { x0: 0.0, y0: 0.0, x1: 400.0, y1: 100.0 });
        let k = 0.25 * 0.75 + 0.25;
        assert!((wide[0].a.x - 400.0 * 0.22 * k).abs() < 1e-2, "corners stay near the ends of a long oval");
        assert!((wide[0].a.y - 6.0).abs() < 1e-3, "the short axis keeps its fractions");
    }

    const ARROWS: [ArrowStyle; 4] = [ArrowStyle::Line, ArrowStyle::Double, ArrowStyle::HandDrawn, ArrowStyle::Shape];

    fn arrow(a: P, b: P, st: Style) -> Obj {
        Obj { kind: Kind::Arrow { a, b, seed: 7 }, style: st }
    }

    fn painted(obj: &Obj, fonts: &Fonts, w: u32, h: u32) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        draw_obj(&mut pm, obj, fonts, 1.0);
        pm
    }

    fn ink(pm: &Pixmap) -> Vec<(u32, u32)> {
        let w = pm.width();
        pm.pixels().iter().enumerate().filter(|(_, c)| c.alpha() > 0).map(|(i, _)| (i as u32 % w, i as u32 / w)).collect()
    }

    fn ink_height(pm: &Pixmap) -> f32 {
        let rows: Vec<u32> = ink(pm).iter().map(|q| q.1).collect();
        (rows.iter().max().unwrap() - rows.iter().min().unwrap() + 1) as f32
    }

    #[test]
    fn arrows_keep_their_proportions_on_retina_captures() {
        let fonts = Fonts::load();
        for style in ARROWS {
            for size in [0, 2, 7] {
                for len in [10.0, 60.0, 150.0, 800.0] {
                    let st = Style { size, arrow: style, ..Style::default() };
                    let one = painted(&arrow(p(150.0, 300.0), p(150.0 + len, 300.0), Style { scale: 1.0, ..st }), &fonts, 1200, 600);
                    let two = painted(&arrow(p(300.0, 600.0), p(300.0 + len * 2.0, 600.0), Style { scale: 2.0, ..st }), &fonts, 2400, 1200);
                    let (h1, h2) = (ink_height(&one), ink_height(&two));
                    assert!((h2 - h1 * 2.0).abs() <= 4.0, "{style:?} size {size} length {len}: {h1} at 1x, {h2} at 2x");
                }
            }
        }
    }

    #[test]
    fn hand_drawn_arrow_keeps_its_curve_when_moved() {
        let fonts = Fonts::load();
        let st = Style { arrow: ArrowStyle::HandDrawn, ..Style::default() };
        let mut obj = arrow(p(120.0, 140.0), p(420.0, 260.0), st);
        let before = painted(&obj, &fonts, 500, 400);
        obj.translate(-60.0, -80.0);
        let moved = painted(&obj, &fonts, 500, 400);
        assert_eq!(ink(&before).len(), ink(&moved).len());
        let shifted: Vec<(u32, u32)> = ink(&moved).iter().map(|q| (q.0 + 60, q.1 + 80)).collect();
        assert_eq!(ink(&before), shifted, "the same pixels, 60 and 80 further");
    }

    #[test]
    fn hand_drawn_arrows_differ_from_one_another() {
        let fonts = Fonts::load();
        let st = Style { arrow: ArrowStyle::HandDrawn, ..Style::default() };
        let first = painted(&Obj { kind: Kind::Arrow { a: p(20.0, 100.0), b: p(400.0, 100.0), seed: 1 }, style: st }, &fonts, 450, 200);
        let second = painted(&Obj { kind: Kind::Arrow { a: p(20.0, 100.0), b: p(400.0, 100.0), seed: 2 }, style: st }, &fonts, 450, 200);
        assert_ne!(ink(&first), ink(&second));
    }

    #[test]
    fn arrows_paint_inside_their_bounds_and_can_be_clicked_anywhere() {
        let fonts = Fonts::load();
        for style in ARROWS {
            for size in 0..=SIZE_MAX {
                for (scale, dashed) in [(1.0, false), (2.0, false), (1.0, true), (2.0, true)] {
                    for (a, b) in [(p(300.25, 300.25), p(330.25, 300.25)), (p(300.0, 500.0), p(900.0, 250.0)), (p(900.0, 300.0), p(300.0, 400.0))] {
                        let st = Style { size, scale, dashed, arrow: style, ..Style::default() };
                        let obj = arrow(a, b, st);
                        let area = bounds(&obj, &fonts);
                        for (x, y) in ink(&painted(&obj, &fonts, 1200, 800)) {
                            let q = p(x as f32 + 0.5, y as f32 + 0.5);
                            assert!(area.contains(q), "{style:?} size {size} scale {scale}: {x},{y} outside {area:?}");
                            assert!(arrow_hit(a, b, 7, &st, q, 1.0), "{style:?} size {size} scale {scale}: {x},{y} cannot be clicked");
                        }
                        let len = a.dist(b);
                        let beside = p((a.x + b.x) / 2.0 - (b.y - a.y) / len * 200.0, (a.y + b.y) / 2.0 + (b.x - a.x) / len * 200.0);
                        assert!(!arrow_hit(a, b, 7, &st, beside, 1.0), "{style:?} size {size} scale {scale}: far from the arrow is a miss");
                    }
                }
            }
        }
    }

    #[test]
    fn clicks_beside_a_thick_arrow_reach_what_is_next_to_it() {
        let st = Style { size: 7, scale: 2.0, arrow: ArrowStyle::Line, ..Style::default() };
        let (a, b) = (p(100.0, 500.0), p(2100.0, 500.0));
        let line = arrow_line(a.dist(b), &st);
        assert!(arrow_hit(a, b, 7, &st, p(1000.0, 500.0 + line / 2.0 + 10.0), 12.0), "inside the tolerance");
        assert!(!arrow_hit(a, b, 7, &st, p(1000.0, 500.0 + line / 2.0 + 13.0), 12.0), "past the line and the tolerance");
    }

    #[test]
    fn clicks_in_a_wide_dash_gap_reach_what_is_behind() {
        let st = Style { size: 7, scale: 2.0, dashed: true, arrow: ArrowStyle::Line, ..Style::default() };
        let (a, b) = (p(100.0, 180.0), p(2100.0, 180.0));
        assert!(arrow_hit(a, b, 7, &st, p(150.5, 180.5), 12.0), "on the first dash");
        assert!(!arrow_hit(a, b, 7, &st, p(314.5, 180.5), 12.0), "in the first gap");
    }

    #[test]
    fn thin_dashed_arrows_can_be_clicked_along_their_whole_length() {
        for style in [ArrowStyle::Line, ArrowStyle::Double, ArrowStyle::HandDrawn] {
            for size in 0..=2 {
                let st = Style { size, dashed: true, arrow: style, ..Style::default() };
                let (a, b) = (p(100.0, 180.0), p(700.0, 180.0));
                for x in 100..700 {
                    assert!(arrow_hit(a, b, 7, &st, p(x as f32, 180.0), 6.0), "{style:?} size {size}: gap at {x}");
                }
            }
        }
    }

    #[test]
    fn clicks_on_blank_canvas_never_select_a_dashed_arrow() {
        let st = Style { size: 1, scale: 1.0, dashed: true, arrow: ArrowStyle::Line, ..Style::default() };
        assert!(!arrow_hit(p(300.25, 300.25), p(296.8854, 270.43927), 7, &st, p(50.5, 50.5), 6.0), "a dash can end up with no length");
        for style in [ArrowStyle::Line, ArrowStyle::Double, ArrowStyle::HandDrawn] {
            for size in 0..=SIZE_MAX {
                for i in 0..2000 {
                    let turn = i as f32 * 0.37;
                    let (a, len) = (p(600.0, 600.0), 20.0 + i as f32 * 0.173);
                    let b = p(a.x + turn.cos() * len, a.y + turn.sin() * len);
                    let st = Style { size, dashed: true, arrow: style, ..Style::default() };
                    assert!(!arrow_hit(a, b, 7, &st, p(-1500.0, -1500.0), 6.0), "{style:?} size {size} step {i}");
                }
            }
        }
    }

    #[test]
    fn a_stroke_with_no_length_is_only_its_point() {
        let q = p(10.0, 10.0);
        assert!(on_stroke(p(12.0, 10.0), q, q, 2.0, 1.0));
        assert!(!on_stroke(p(14.0, 10.0), q, q, 2.0, 1.0));
    }

    #[test]
    fn at_high_zoom_clicks_just_past_an_arrow_reach_its_neighbour() {
        let st = Style { size: 7, arrow: ArrowStyle::Shape, ..Style::default() };
        assert!(!arrow_hit(p(100.0, 180.4), p(600.0, 180.4), 7, &st, p(465.5, 243.5), 6.0 / 64.0));
    }

    #[test]
    fn translucent_arrows_stay_even_where_their_parts_overlap() {
        let fonts = Fonts::load();
        for style in ARROWS {
            let st = Style { size: 4, arrow: style, color: [255, 0, 0, 128], ..Style::default() };
            let pm = painted(&arrow(p(60.0, 200.0), p(460.0, 120.0), st), &fonts, 520, 320);
            let most = pm.pixels().iter().map(|c| c.alpha()).max().unwrap();
            assert!((127..=129).contains(&most), "{style:?}: alpha reaches {most}");
            let opaque = painted(&arrow(p(60.0, 200.0), p(460.0, 120.0), Style { color: [255, 0, 0, 255], ..st }), &fonts, 520, 320);
            let (faded, full) = (ink(&pm), ink(&opaque));
            let lookup: std::collections::HashSet<_> = full.iter().collect();
            let shared = faded.iter().filter(|q| lookup.contains(q)).count();
            assert!(shared as f32 > full.len() as f32 * 0.98, "{style:?}: the translucent arrow sits where the opaque one does");
        }
    }

    #[test]
    fn translucent_arrow_partly_off_the_picture_still_paints() {
        let fonts = Fonts::load();
        for style in ARROWS {
            let st = Style { size: 4, arrow: style, color: [255, 0, 0, 128], ..Style::default() };
            assert!(!ink(&painted(&arrow(p(-200.0, -80.0), p(160.0, 90.0), st), &fonts, 300, 200)).is_empty(), "{style:?}");
            assert!(ink(&painted(&arrow(p(-900.0, -900.0), p(-600.0, -700.0), st), &fonts, 300, 200)).is_empty(), "{style:?}");
        }
    }

    #[test]
    fn long_hand_drawn_arrow_stays_inside_its_bounds() {
        let fonts = Fonts::load();
        for (seed, (size, dashed)) in [(0, false), (7, false), (7, true)].into_iter().cycle().take(6).enumerate() {
            let seed = seed as u64;
            let st = Style { size, dashed, scale: 2.0, arrow: ArrowStyle::HandDrawn, ..Style::default() };
            let (a, b) = (p(100.0, 500.0), p(6100.0, 500.0));
            let obj = Obj { kind: Kind::Arrow { a, b, seed }, style: st };
            let area = bounds(&obj, &fonts);
            for (i, (x, y)) in ink(&painted(&obj, &fonts, 6200, 1000)).into_iter().enumerate() {
                let q = p(x as f32 + 0.5, y as f32 + 0.5);
                assert!(area.contains(q), "seed {seed}: {x},{y} outside {area:?}");
                assert!(i % 97 != 0 || arrow_hit(a, b, seed, &st, q, 1.0), "seed {seed} size {size} dashed {dashed}: {x},{y} cannot be clicked");
            }
        }
    }

    #[test]
    fn text_paints_inside_its_bounds() {
        let fonts = Fonts::load();
        let lines = vec!["gjpqy WWWW"; 10].join("\n");
        for font in [FontKind::Sans, FontKind::Goofy] {
            for fill in [Fill::Opaque, Fill::Translucent, Fill::Transparent, Fill::HandDrawn] {
                let st = Style { font, fill, size: 2, scale: 2.0, ..Style::default() };
                let obj = Obj { kind: Kind::Text { at: p(60.0, 60.0), text: lines.clone() }, style: st };
                let area = bounds(&obj, &fonts);
                for (x, y) in ink(&painted(&obj, &fonts, 1400, 1200)) {
                    assert!(area.contains(p(x as f32 + 0.5, y as f32 + 0.5)), "{font:?} {fill:?}: {x},{y} outside {area:?}");
                }
            }
        }
    }

    #[test]
    fn dashed_hand_drawn_arrow_shows_gaps() {
        let fonts = Fonts::load();
        let st = Style { arrow: ArrowStyle::HandDrawn, ..Style::default() };
        let (a, b) = (p(20.0, 100.0), p(420.0, 100.0));
        let solid = ink(&painted(&arrow(a, b, st), &fonts, 450, 200)).len();
        let dashed = ink(&painted(&arrow(a, b, Style { dashed: true, ..st }), &fonts, 450, 200)).len();
        assert!((dashed as f32) < solid as f32 * 0.8, "{dashed} dashed pixels, {solid} solid");
    }

    #[test]
    fn dashed_arrows_keep_their_tips() {
        let fonts = Fonts::load();
        for style in [ArrowStyle::Line, ArrowStyle::Double, ArrowStyle::HandDrawn] {
            for size in 0..=SIZE_MAX {
                for len in (40..500).step_by(7) {
                    let (a, b) = (p(100.0, 200.0), p(100.0 + len as f32, 200.0));
                    let st = Style { size, arrow: style, dashed: true, ..Style::default() };
                    let pm = painted(&arrow(a, b, st), &fonts, 700, 400);
                    assert!(pm.pixel(b.x as u32, 200).unwrap().alpha() > 0, "{style:?} size {size} length {len}: no tip");
                    if style == ArrowStyle::Double {
                        assert!(pm.pixel(100, 200).unwrap().alpha() > 0, "size {size} length {len}: no tail tip");
                    }
                }
            }
        }
    }

    #[test]
    fn renders_every_tool() {
        let fonts = Fonts::load();
        let mut doc = Doc::new(sample(), 2.0);
        let st = Style::default();
        let mut hl = st;
        hl.color = hex(HIGHLIGHT_DEFAULT);
        let mut filled = st;
        filled.fill = Fill::Translucent;
        filled.color = hex("#00AFEE");
        let mut text = st;
        text.size = 4;
        let mut blur = st;
        let sketch = Style { fill: Fill::HandDrawn, scale: 2.0, ..st };
        let mut hand = st;
        hand.arrow = ArrowStyle::HandDrawn;
        hand.color = hex("#7758EF");
        let mut dashed = st;
        dashed.dashed = true;
        dashed.color = hex("#00A490");
        let objs = vec![
            Obj { kind: Kind::Highlighter { a: p(40.0, 100.0), b: p(300.0, 100.0) }, style: hl },
            Obj { kind: Kind::Arrow { a: p(60.0, 300.0), b: p(240.0, 180.0), seed: 9 }, style: st },
            Obj { kind: Kind::Arrow { a: p(300.0, 320.0), b: p(440.0, 200.0), seed: 10 }, style: hand },
            Obj { kind: Kind::Rect { r: R { x0: 520.0, y0: 300.0, x1: 760.0, y1: 420.0 }, seed: 11 }, style: filled },
            Obj { kind: Kind::Oval { r: R { x0: 60.0, y0: 360.0, x1: 240.0, y1: 480.0 }, seed: 12 }, style: dashed },
            Obj { kind: Kind::Rect { r: R { x0: 640.0, y0: 30.0, x1: 880.0, y1: 190.0 }, seed: 13 }, style: sketch },
            Obj { kind: Kind::Oval { r: R { x0: 640.0, y0: 375.0, x1: 880.0, y1: 535.0 }, seed: 14 }, style: sketch },
            Obj { kind: Kind::Text { at: p(300.0, 400.0), text: "Hello ShotCraft".into() }, style: text },
            Obj { kind: Kind::Counter { at: p(800.0, 470.0), n: 1 }, style: st },
            Obj { kind: Kind::Blur { r: R { x0: 540.0, y0: 80.0, x1: 860.0, y1: 140.0 }, text: None }, style: blur },
            Obj { kind: Kind::Magnifier { r: R { x0: 430.0, y0: 240.0, x1: 490.0, y1: 300.0 }, lens: MAGNIFIER_LENS }, style: st },
            Obj { kind: Kind::Ruler { a: p(40.0, 520.0), b: p(480.0, 520.0) }, style: st },
            Obj { kind: Kind::Freehand { pts: (0..40).map(|i| p(320.0 + i as f32 * 5.0, 470.0 + (i as f32 * 0.5).sin() * 20.0)).collect() }, style: st },
        ];
        doc.objs = objs;
        let flat = export(&doc, &fonts);
        assert_eq!((flat.w, flat.h), (900, 560));
        assert_ne!(flat.get(150, 240), sample().get(150, 240), "arrow painted over the picture");
        doc.objs.push(Obj { kind: Kind::Spotlight { r: R { x0: 500.0, y0: 280.0, x1: 780.0, y1: 440.0 } }, style: st });
        doc.backdrop = Some(Backdrop::default());
        let framed = export(&doc, &fonts);
        assert!(framed.w > 900 && framed.h > 560, "backdrop adds padding");
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shots");
        std::fs::create_dir_all(&out).unwrap();
        for (name, img) in [("render-tools.png", &flat), ("render-backdrop.png", &framed)] {
            image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).unwrap().save(out.join(name)).unwrap();
        }
    }

    fn doc_of(w: u32, h: u32, c: [u8; 4]) -> Doc {
        Doc::new(flat(w, h, c), 1.0)
    }

    #[test]
    fn an_image_object_is_drawn_into_its_rectangle_and_exported() {
        let fonts = Fonts::load();
        let mut doc = doc_of(40, 20, [255, 255, 255, 255]);
        let i = doc.paste_image(flat(10, 10, [255, 0, 0, 255]));
        let out = export(&doc, &fonts);
        assert_eq!((out.w, out.h), (40, 20));
        assert_eq!(out.get(20, 10), [255, 0, 0, 255]);
        assert_eq!(out.get(14, 10), [255, 255, 255, 255]);
        assert_eq!(out.get(25, 10), [255, 255, 255, 255]);
        // Resized to twice as big: still centred where its rectangle says.
        if let Kind::Image { r, .. } = &mut doc.objs[i].kind {
            *r = R { x0: 0.0, y0: 0.0, x1: 20.0, y1: 20.0 };
        }
        let out = export(&doc, &fonts);
        assert_eq!(out.get(19, 19), [255, 0, 0, 255]);
        assert_eq!(out.get(20, 10), [255, 255, 255, 255]);
    }

    #[test]
    fn opacity_blends_the_picture_with_what_is_below() {
        let fonts = Fonts::load();
        let mut doc = doc_of(20, 20, [0, 0, 0, 255]);
        let i = doc.paste_image(flat(20, 20, [255, 255, 255, 255]));
        if let Kind::Image { opacity, .. } = &mut doc.objs[i].kind {
            *opacity = 0.5;
        }
        let px = export(&doc, &fonts).get(10, 10);
        assert!((px[0] as i32 - 128).abs() <= 2 && px[3] == 255, "{px:?}");
    }

    #[test]
    fn objects_stack_in_order_so_a_later_picture_covers_an_earlier_one_and_a_shape_covers_a_picture() {
        let fonts = Fonts::load();
        let mut doc = doc_of(40, 40, [255, 255, 255, 255]);
        doc.paste_image(flat(30, 30, [255, 0, 0, 255]));
        doc.paste_image(flat(20, 20, [0, 0, 255, 255]));
        assert_eq!(export(&doc, &fonts).get(20, 20), [0, 0, 255, 255], "newest picture on top");
        assert_eq!(export(&doc, &fonts).get(7, 20), [255, 0, 0, 255], "older one visible around it");
        let mut st = Style::default();
        st.fill = Fill::Opaque;
        st.color = [0, 255, 0, 255];
        doc.objs.push(Obj { kind: Kind::Rect { r: R { x0: 15.0, y0: 15.0, x1: 25.0, y1: 25.0 }, seed: 1 }, style: st });
        let out = export(&doc, &fonts);
        assert_ne!(out.get(20, 20), [0, 0, 255, 255], "a rectangle drawn after the picture covers it");
    }

    #[test]
    fn a_picture_hanging_over_the_edge_is_cut_at_the_edge() {
        let fonts = Fonts::load();
        let mut doc = doc_of(20, 20, [255, 255, 255, 255]);
        let i = doc.paste_image(flat(10, 10, [255, 0, 0, 255]));
        if let Kind::Image { r, .. } = &mut doc.objs[i].kind {
            *r = R { x0: 15.0, y0: 15.0, x1: 25.0, y1: 25.0 };
        }
        let out = export(&doc, &fonts);
        assert_eq!((out.w, out.h), (20, 20));
        assert_eq!(out.get(19, 19), [255, 0, 0, 255]);
    }

    #[test]
    fn the_bounds_of_a_picture_are_its_rectangle() {
        let fonts = Fonts::load();
        let mut doc = doc_of(40, 20, [255, 255, 255, 255]);
        let i = doc.paste_image(flat(10, 10, [1, 2, 3, 255]));
        assert_eq!(bounds(&doc.objs[i], &fonts), R { x0: 15.0, y0: 5.0, x1: 25.0, y1: 15.0 });
    }

    #[test]
    fn the_gif_has_two_frames_the_screenshot_then_with_the_picture_a_second_apart() {
        use image::AnimationDecoder;
        let fonts = Fonts::load();
        let mut doc = doc_of(40, 20, [255, 255, 255, 255]);
        doc.paste_image(flat(10, 10, [255, 0, 0, 255]));
        let (before, after) = gif_frames(&doc, &fonts).unwrap();
        assert_eq!(before.get(20, 10), [255, 255, 255, 255]);
        assert_eq!(after.get(20, 10), [255, 0, 0, 255]);
        let bytes = encode_gif(&[before, after]).unwrap();
        let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).unwrap();
        let frames = dec.into_frames().collect_frames().unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].delay().numer_denom_ms(), (GIF_DELAY_MS, 1));
        assert_eq!((frames[0].buffer().width(), frames[0].buffer().height()), (40, 20));
        let (a, b) = (frames[0].buffer().get_pixel(20, 10).0, frames[1].buffer().get_pixel(20, 10).0);
        assert!(a[0] > 200 && a[1] > 200, "first frame is white there: {a:?}");
        assert!(b[0] > 200 && b[1] < 80, "second frame is red there: {b:?}");
    }

    #[test]
    fn no_gif_without_a_picture_or_when_it_misses_the_screenshot() {
        let fonts = Fonts::load();
        let mut doc = doc_of(40, 20, [255, 255, 255, 255]);
        assert_eq!(gif_frames(&doc, &fonts).err().unwrap(), GIF_NEEDS_OVERLAY);
        let i = doc.paste_image(flat(10, 10, [255, 0, 0, 255]));
        if let Kind::Image { r, .. } = &mut doc.objs[i].kind {
            *r = R { x0: 100.0, y0: 100.0, x1: 110.0, y1: 110.0 };
        }
        assert!(gif_frames(&doc, &fonts).err().unwrap().contains("overlap"));
    }

    #[test]
    fn the_two_gif_frames_keep_one_size_even_with_an_autocropping_backdrop() {
        let fonts = Fonts::load();
        // A see-through margin the picture covers in the "after" frame only.
        let mut doc = doc_of(60, 40, [0, 0, 0, 0]);
        let mut base = (*doc.base.px).clone();
        for y in 10..30 {
            for x in 10..30 {
                base[((y * 60 + x) * 4) as usize..][..4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        doc.base = Image { w: 60, h: 40, px: Arc::new(base) };
        let i = doc.paste_image(flat(10, 10, [255, 0, 0, 255]));
        if let Kind::Image { r, .. } = &mut doc.objs[i].kind {
            *r = R { x0: 20.0, y0: 20.0, x1: 50.0, y1: 40.0 };
        }
        let mut bd = Backdrop::default();
        bd.autocrop = true;
        doc.backdrop = Some(bd);
        let (raw_a, raw_b) = (export_without(&doc, &fonts, Some(i), false), export(&doc, &fonts));
        assert_ne!((raw_a.w, raw_a.h), (raw_b.w, raw_b.h), "the set-up must trim the frames differently");
        let (a, b) = gif_frames(&doc, &fonts).unwrap();
        assert_eq!((a.w, a.h), (b.w, b.h));
        assert!(encode_gif(&[a, b]).is_ok());
        assert!(encode_gif(&[flat(4, 4, [0, 0, 0, 255]), flat(5, 4, [0, 0, 0, 255])]).is_err());
    }
}
