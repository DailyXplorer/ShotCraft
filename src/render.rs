use crate::model::*;
use ab_glyph::{Font, FontVec, OutlineCurve, ScaleFont};
use std::sync::Arc;
use tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, StrokeDash, Transform,
};

pub struct Fonts {
    pub sans: FontVec,
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
        Fonts { sans: dm(), goofy }
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

fn stroke(width: f32, dashed: bool) -> Stroke {
    let mut s = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    if dashed {
        s.dash = StrokeDash::new(vec![width * 2.5 + 2.0, width * 2.0 + 4.0], 0.0);
    }
    s
}

/// Noise behind the hand-drawn shapes: 100 values in 0...1 from an xorshift stream (0x1001b3d58).
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

/// Rounded outline as eight curves, sides then corners, clockwise from the top left (0x100089fec).
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

/// Points along the curves, about `spacing` apart (0x1000ffb9c).
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

/// Hand-drawn rectangle (0x10008b3f8): the whole outline slips by up to half a point, except
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

/// Hand-drawn oval (0x100089c20): one turn from a random angle that overshoots or stops short
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

/// Stroke width at every point (0x10008b8a0, 0x100100498, 0x100100a04, 0x100100e3c): noise and a
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
        Kind::Arrow { a, b } => draw_arrow(pm, *a, *b, st),
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
            let path = if is_oval { oval_path(r) } else { curves_path(&outline_curves(r, wobble, base * CORNER_LINE_RATIO * scale, scale, &n)) };
            let Some(path) = path else { return };
            match st.fill {
                Fill::Translucent => pm.fill_path(&path, &paint(st.color, FILL_TRANSLUCENT), FillRule::Winding, tf, None),
                Fill::Opaque => pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None),
                _ => {}
            }
            pm.stroke_path(&path, &paint(st.color, 1.0), &stroke(line, st.dashed), tf, None);
        }
        Kind::Freehand { pts } => {
            if pts.len() < 2 {
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
                let s = Stroke { width: st.highlight_size(), line_cap: LineCap::Butt, ..Default::default() };
                pm.stroke_path(&path, &pt, &s, tf, None);
            }
        }
        Kind::Blur { r, text } => draw_blur(pm, r, text.as_deref(), st, scale_factor),
        Kind::Spotlight { r } => draw_spotlight(pm, &[(*r, st.shape)]),
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
    }
}

fn draw_arrow(pm: &mut Pixmap, a: P, b: P, st: &Style) {
    let w = st.width();
    let len = a.dist(b);
    if len < 1.0 {
        return;
    }
    let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    let (nx, ny) = (-uy, ux);
    let head = (w * 3.2 + 8.0).min(len * 0.6);
    let half = head * 0.55;
    let tf = Transform::identity();
    let pt = paint(st.color, 1.0);
    let filled_head = |pm: &mut Pixmap, tip: P, dx: f32, dy: f32| {
        let base = p(tip.x - dx * head, tip.y - dy * head);
        let mut pb = PathBuilder::new();
        pb.move_to(tip.x, tip.y);
        pb.line_to(base.x - dy * half, base.y + dx * half);
        pb.line_to(base.x + dy * half, base.y - dx * half);
        pb.close();
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None);
        }
    };
    match st.arrow {
        ArrowStyle::Shape => {
            let base = p(b.x - ux * head * 0.85, b.y - uy * head * 0.85);
            let tail = w * 0.25;
            let neck = w * 0.6;
            let mut pb = PathBuilder::new();
            pb.move_to(a.x + nx * tail, a.y + ny * tail);
            pb.line_to(base.x + nx * neck, base.y + ny * neck);
            pb.line_to(base.x + nx * half, base.y + ny * half);
            pb.line_to(b.x, b.y);
            pb.line_to(base.x - nx * half, base.y - ny * half);
            pb.line_to(base.x - nx * neck, base.y - ny * neck);
            pb.line_to(a.x - nx * tail, a.y - ny * tail);
            pb.close();
            if let Some(path) = pb.finish() {
                pm.fill_path(&path, &pt, FillRule::Winding, tf, None);
                let s = Stroke { width: 1.0, line_join: LineJoin::Round, ..Default::default() };
                pm.stroke_path(&path, &pt, &s, tf, None);
            }
        }
        ArrowStyle::HandDrawn | ArrowStyle::Line => {
            let mut pb = PathBuilder::new();
            if st.arrow == ArrowStyle::HandDrawn {
                let bend = len * 0.12;
                let c = p((a.x + b.x) / 2.0 + nx * bend, (a.y + b.y) / 2.0 + ny * bend);
                pb.move_to(a.x, a.y);
                pb.quad_to(c.x, c.y, b.x, b.y);
                let (tx, ty) = (b.x - c.x, b.y - c.y);
                let tl = (tx * tx + ty * ty).sqrt().max(1.0);
                let (dx, dy) = (tx / tl, ty / tl);
                for s in [1.0f32, -1.0] {
                    pb.move_to(b.x, b.y);
                    pb.line_to(b.x - dx * head + s * -dy * half, b.y - dy * head + s * dx * half);
                }
            } else {
                pb.move_to(a.x, a.y);
                pb.line_to(b.x, b.y);
                for s in [1.0f32, -1.0] {
                    pb.move_to(b.x, b.y);
                    pb.line_to(b.x - ux * head + s * nx * half, b.y - uy * head + s * ny * half);
                }
            }
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &pt, &stroke(w, st.dashed), tf, None);
            }
        }
        ArrowStyle::Double => {
            let mut pb = PathBuilder::new();
            pb.move_to(a.x + ux * head * 0.8, a.y + uy * head * 0.8);
            pb.line_to(b.x - ux * head * 0.8, b.y - uy * head * 0.8);
            if let Some(path) = pb.finish() {
                pm.stroke_path(&path, &pt, &stroke(w, st.dashed), tf, None);
            }
            filled_head(pm, b, ux, uy);
            filled_head(pm, a, -ux, -uy);
        }
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

/// Average-colour blocks, see the PIXEL_ constants (0x1001087b0).
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
/// edges are the most uniform (0x10010954c).
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

pub fn draw_spotlight(pm: &mut Pixmap, holes: &[(R, Shape)]) {
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
        pm.fill_path(&path, &paint([0, 0, 0, 255], SPOTLIGHT_DIM), FillRule::EvenOdd, Transform::identity(), None);
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

pub fn text_box(at: P, text: &str, st: &Style, fonts: &Fonts) -> R {
    let font = fonts.get(st.font);
    let size = st.text_size();
    let line_h = size * 1.2;
    let lines: Vec<&str> = text.split('\n').collect();
    let w = lines.iter().map(|l| measure(font, l, size).0).fold(size * 0.3, f32::max);
    let pad = size * 0.3;
    R { x0: at.x - pad, y0: at.y - pad * 0.6, x1: at.x + w + pad, y1: at.y + line_h * lines.len() as f32 + pad * 0.6 }
}

fn draw_text(pm: &mut Pixmap, at: P, text: &str, st: &Style, fonts: &Fonts) {
    let font = fonts.get(st.font);
    let size = st.text_size();
    let line_h = size * 1.2;
    let tf = Transform::identity();
    let bx = text_box(at, text, st, fonts);
    let (fg, outline) = match st.fill {
        Fill::Opaque => {
            if let Some(path) = round_rect_path(&bx, size * 0.25) {
                pm.fill_path(&path, &paint(st.color, 1.0), FillRule::Winding, tf, None);
            }
            (if is_light(st.color) { hex("#1C2B41") } else { [255, 255, 255, 255] }, None)
        }
        Fill::Translucent => {
            if let Some(path) = round_rect_path(&bx, size * 0.25) {
                pm.fill_path(&path, &paint(st.color, 0.25), FillRule::Winding, tf, None);
            }
            (st.color, None)
        }
        Fill::Transparent => {
            let oc = if is_light(st.color) { [0, 0, 0, 200] } else { [255, 255, 255, 255] };
            (st.color, Some((oc, (size * 0.14).max(2.0))))
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
    let mut spot_done = false;
    for (i, o) in doc.objs.iter().enumerate() {
        if Some(i) == skip {
            continue;
        }
        if let Kind::Spotlight { .. } = o.kind {
            if !spot_done {
                draw_spotlight(&mut pm, &holes);
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

/// Seeded single-octave Perlin noise, the source of the mesh gradient (0x100035f0c).
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
/// a Gaussian blur then a crop (0x100035f0c, 0x100035c6c).
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

pub fn gradient_pixmap(w: u32, h: u32, bd: &Backdrop) -> Pixmap {
    let mut stops = gradient_stops(bd.gradient);
    if bd.reverse {
        stops = stops.into_iter().rev().map(|(s, c)| (1.0 - s, c)).collect();
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

pub fn apply_backdrop(content: &Pixmap, bd: &Backdrop) -> Pixmap {
    let (cw, ch) = (content.width() as f32, content.height() as f32);
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
    let tf = Transform::identity();
    let pp = tiny_skia::PixmapPaint::default();
    let mut pm = gradient_pixmap(w, h, bd);
    let ox = ((w as f32 - iw) / 2.0).round();
    let oy = ((h as f32 - ih) / 2.0).round();
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
    let pm = compose(doc, fonts, None);
    let pm = match doc.crop {
        Some(r) => crop_pixmap(&pm, &r),
        None => pm,
    };
    let pm = match &doc.backdrop {
        Some(bd) => apply_backdrop(&pm, bd),
        None => pm,
    };
    from_pixmap(&pm)
}

/// Flatten every object into the base picture (used by Rasterize, Resize and Cut Out).
pub fn rasterize(doc: &mut Doc, fonts: &Fonts) {
    let pm = compose(doc, fonts, None);
    doc.base = from_pixmap(&pm);
    doc.objs.clear();
}

/// Interpolation follows the reference resize (0x100071c18): high quality by
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

pub fn format_color(c: Rgba, fmt: usize) -> String {
    let f = |v: u8| v as f32 / 255.0;
    match fmt {
        1 => format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
        2 => format!("{}, {}, {}", c[0], c[1], c[2]),
        3 => format!("({:.2}, {:.2}, {:.2})", f(c[0]), f(c[1]), f(c[2])),
        4 => format!("UIColor(red: {:.2}, green: {:.2}, blue: {:.2}, alpha: 1)", f(c[0]), f(c[1]), f(c[2])),
        5 => format!("rgba({}, {}, {}, 1)", c[0], c[1], c[2]),
        6 => format!("Color.FromArgb({}, {}, {});", c[0], c[1], c[2]),
        7 => format!("0xFF{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
        _ => format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
    }
}

pub const COLOR_FORMATS: [&str; 8] = [
    "HEX #FFC200",
    "HEX FFC200",
    "RGB 255, 194, 0",
    "RGB (1.00, 0.76, 0.00)",
    "UIColor (Swift)",
    "CSS rgba()",
    "C# Color",
    "Android 0xFFFFC200",
];

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

    #[test]
    fn zoom_steps_match_the_original() {
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
        assert!(boxes[0].y0 > 220.0 && boxes[0].y1 < 400.0, "box is near the text and measured from the top: {:?}", boxes[0]);
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
            Obj { kind: Kind::Arrow { a: p(60.0, 300.0), b: p(240.0, 180.0) }, style: st },
            Obj { kind: Kind::Arrow { a: p(300.0, 320.0), b: p(440.0, 200.0) }, style: hand },
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
}
