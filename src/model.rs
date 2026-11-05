use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Versions are unique across documents so texture caches never confuse two pictures.
/// Per-object noise seed, so a hand-drawn shape keeps its look when it moves or resizes.
pub fn next_seed() -> u64 {
    next_version().wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

pub fn next_version() -> u64 {
    static V: AtomicU64 = AtomicU64::new(1);
    V.fetch_add(1, Ordering::Relaxed)
}

pub type Rgba = [u8; 4];

pub fn hex(s: &str) -> Rgba {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).unwrap_or(0);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]
}

pub const PALETTE: [&str; 8] = [
    "#00A490", "#00AFEE", "#1C2B41", "#F7F4E7", "#FF0C01", "#7758EF", "#FFDC2C", "#FA3C63",
];
pub const HIGHLIGHT_PALETTE: [&str; 8] = [
    "#ADF6FF", "#C2EAFE", "#E6E6FF", "#FFDAEF", "#FFF68D", "#F2FE01", "#D8FF55", "#DDF5C6",
];
pub const DEFAULT_COLOR: &str = "#FF0C01";
pub const HIGHLIGHT_DEFAULT: &str = "#FFF68D";

pub const LINE_WIDTHS: [f32; 10] = [1., 2., 3., 4., 6., 8., 11., 15., 21., 30.];
pub const TEXT_SIZES: [f32; 10] = [12., 14., 17., 20., 28., 40., 55., 77., 110., 150.];
pub const COUNTER_SIZES: [f32; 10] = [22., 28., 36., 48., 64., 84., 112., 150., 200., 266.];
pub const HIGHLIGHT_SIZES: [f32; 8] = [14., 20., 28., 38., 54., 76., 108., 154.];
pub const SIZE_MAX: u8 = 7;
pub const SIZE_DEFAULT: u8 = 2;

/// Preview thumbnail window size, in points.
pub const THUMB_WIDTH: f32 = 204.0;
pub const THUMB_MAX_HEIGHT: f32 = 190.0;

/// Arrow line: capped to a tenth of the length, and it grows with
/// the arrow as sqrt((length + 550) / 600), up to 1.5 times.
pub const ARROW_WIDTHS: [f32; 10] = [1., 2., 3., 4., 6., 10., 14., 20., 28., 40.];
pub const ARROW_LINE_DIVISOR: f32 = 10.0;
pub const ARROW_GROW_BASE: f32 = 550.0;
pub const ARROW_GROW_DIVISOR: f32 = 600.0;
pub const ARROW_GROW_MAX: f32 = 1.5;
/// Thin arrow head: branch length, capped to a quarter of the arrow.
pub const ARROW_BRANCH_SCALE: f32 = 0.2;
pub const ARROW_BRANCH_ROOT: f32 = 7.0;
pub const ARROW_BRANCH_LINE: f32 = 0.7;
pub const ARROW_BRANCH_MAX: f32 = 0.25;
pub const ARROW_SHORT: f32 = 20.0;
pub const ARROW_SPREAD: f32 = 0.8726646;
pub const ARROW_SPREAD_SHORT: f32 = 0.78539815;
/// Hand-drawn arrow line.
pub const ARROW_HAND_GAIN: f32 = 0.7;
pub const ARROW_HAND_BASE: f32 = 1.3;
pub const ARROW_HAND_DIVISOR: f32 = 15.0;
/// Hand-drawn arrow shaft: stops short of the tip, its curve handles sit a third of
/// the way in and lean by a little noise, and the line thickens from the tail to the head.
pub const ARROW_HAND_STOP: f32 = 0.8;
pub const ARROW_HAND_END: f32 = 1.2;
pub const ARROW_HAND_HANDLE: f32 = 0.33;
pub const ARROW_HAND_LEAN_START: f32 = 0.06;
pub const ARROW_HAND_LEAN_END: f32 = 0.02;
/// Solid arrow: drawn in units of the size step.
/// Past 520 the head grows and the tail shrinks, down to 0.8; short arrows shrink as a whole.
pub const ARROW_UNITS: [f32; 10] = [0.33, 0.5, 0.7, 0.9, 1.1, 1.4, 2.0, 2.8, 4.2, 6.4];
pub const ARROW_LONG: f32 = 520.0;
pub const ARROW_LONG_RANGE: f32 = 1000.0;
pub const ARROW_LONG_MIN: f32 = 0.8;
pub const ARROW_SPAN: f32 = 56.0;
pub const ARROW_SPAN_LIMIT: f32 = 100.0;
pub const ARROW_SHORT_LIMIT: f32 = 200.0;
/// Proportions of the default solid arrow, in units.
pub const ARROW_HEAD_LENGTH: f32 = 48.0;
pub const ARROW_HEAD_HALF: f32 = 22.0;
pub const ARROW_NECK_HALF: f32 = 9.0;
pub const ARROW_TAIL_HALF: f32 = 7.0;

/// Text: line height, the larger Goofy font, and the luminance above which a
/// filled label turns black and the weight drops from medium to regular.
pub const TEXT_LINE_HEIGHT: f32 = 1.15;
pub const TEXT_GOOFY_SIZE: f32 = 1.2;
pub const TEXT_GOOFY_LINE_HEIGHT: f32 = 1.324;
pub const TEXT_LIGHT_ABOVE: f32 = 0.6;

/// Plain oval: eight curves as (end, control 1, control 2) in the unit square, from
/// (0.22, 0.06). A long shape keeps its corners close to the short side's proportion.
pub const OVAL_START: (f32, f32) = (0.22, 0.06);
pub const OVAL_CURVES: [[(f32, f32); 3]; 8] = [
    [(0.78, 0.06), (0.38, -0.02), (0.62, -0.02)],
    [(0.94, 0.22), (0.8467, 0.0934), (0.9066, 0.1533)],
    [(0.94, 0.78), (1.02, 0.38), (1.02, 0.62)],
    [(0.78, 0.94), (0.9066, 0.8467), (0.8467, 0.9066)],
    [(0.22, 0.94), (0.62, 1.02), (0.38, 1.02)],
    [(0.06, 0.78), (0.1533, 0.9066), (0.0934, 0.8467)],
    [(0.06, 0.22), (-0.02, 0.62), (-0.02, 0.38)],
    [(0.22, 0.06), (0.0934, 0.1533), (0.1533, 0.0934)],
];
pub const OVAL_ASPECT_GAIN: f32 = 0.75;
pub const OVAL_ASPECT_BASE: f32 = 0.25;

/// Rectangle and oval: the line never exceeds a tenth of the short side, and below
/// that side the hand-drawn brush falls back to a plain stroke.
pub const SHAPE_LINE_DIVISOR: f32 = 10.0;
pub const SHAPE_HAND_MIN_SIDE: f32 = 40.0;
/// Fill segments are -1, 0, 40 and 100 percent.
pub const FILL_TRANSLUCENT: f32 = 0.4;
/// Plain corners are twice the line; hand-drawn ones start from six times the line.
pub const CORNER_LINE_RATIO: f32 = 2.0;
pub const HAND_CORNER_LINE_RATIO: f32 = 6.0;
pub const HAND_CORNER_GROWTH: f32 = 1.1;
pub const HAND_CORNER_DAMPING: f32 = 0.004;
/// Hand-drawn outline: side bow and corner jitter.
pub const HAND_WOBBLE: f32 = 1.3;
pub const HAND_JITTER_DIVISOR: f32 = 400.0;
pub const HAND_BOW_DIVISOR: f32 = 80000.0;
pub const HAND_BOW_MAX: f32 = 0.7;
pub const CORNER_MAX_RATIO: f32 = 3.0;
/// Hand-drawn brush: width varies by
/// taper around the line, driven by smoothed noise and swelling in the bends.
pub const BRUSH_TAPER_MAX: f32 = 0.3;
pub const BRUSH_TAPER_MIN: f32 = 0.08;
pub const BRUSH_TAPER_LINE_DIVISOR: f32 = 150.0;
pub const BRUSH_TAPER_OVAL: f32 = 0.25;
pub const BRUSH_BEND_GAIN: f32 = 4000.0;
pub const BRUSH_WAVE: f32 = 9.42;
pub const BRUSH_NOISE: f32 = 12.0;
pub const BRUSH_SMOOTH_WINDOW: usize = 41;
pub const BRUSH_SMOOTH_PASSES: usize = 4;
/// Curves are flattened every two points.
pub const BRUSH_SPACING: f32 = 2.0;
pub const BRUSH_STEPS_PER_LENGTH: f32 = 1.2;
/// Hand-drawn oval: stops a little short of or past the full turn, spiralling.
pub const OVAL_SAMPLES: usize = 1000;
pub const OVAL_TURN_MIN: f32 = 0.98;
pub const OVAL_TURN_RANGE: f32 = 0.06;
pub const OVAL_DRIFT: f32 = 2.6;
pub const OVAL_OVERLAP_LINES: f32 = 5.0;

/// Canvas checkerboard: squares of 8, (background, square) grey levels per theme.
pub const CHECKER_SQUARE: f32 = 8.0;
pub const CHECKER_LIGHT: (f32, f32) = (1.0, 0.95);
pub const CHECKER_DARK: (f32, f32) = (0.165, 0.2);

/// Blur tool: block = round(min(round(side / 6), 5 × scale) × 1.35^size), and past
/// side / 7 each block may take a neighbour's colour so the text underneath can't be rebuilt.
pub const PIXEL_SIDE_DIVISOR: f32 = 6.0;
pub const PIXEL_RETINA_CAP: f32 = 5.0;
pub const PIXEL_GROWTH: f32 = 1.35;
pub const PIXEL_SHUFFLE_DIVISOR: f32 = 7.0;
/// Ruler: pixels match while |dr|+|dg|+|db| <= 2 × sensitivity².
pub const RULER_SENSITIVITY: f32 = 3.54;
pub const RULER_SENSITIVITY_MAX: f32 = 20.0;
/// Zoom steps: one for the keys, one for the wheel, and the wheel notch size.
pub const ZOOM_KEY_STEP: f32 = 1.414;
pub const ZOOM_WHEEL_STEP: f32 = 1.1892;
pub const ZOOM_WHEEL_NOTCH: f32 = 10.0;
pub const ZOOM_MAX: f32 = 64.0;
/// Counter label: diameter × 0.5 × 1.1, × 0.9 from 10 upwards; black above 0.6 brightness.
pub const COUNTER_LABEL_RATIO: f32 = 0.5 * 1.1;
pub const COUNTER_LABEL_TWO_DIGITS: f32 = 0.9;
pub const COUNTER_DARK_TEXT_ABOVE: f32 = 0.6;
/// Selection colour.
pub const SELECTION_RGB: (f32, f32, f32) = (0.0, 0.495, 0.85);
/// Mesh gradient: one noise octave, blur radius 0.0375 × map size, seed offset 1000.
pub const MESH_BLUR: f32 = 0.0375;
pub const MESH_SEED_BASE: u64 = 1000;
pub const MESH_PERIODS: f32 = 1.3;
pub const MESH_MAP_SIZE: u32 = 256;
/// Spotlight: black at the `transparency` setting's default of 0.214.
pub const SPOTLIGHT_DIM: f32 = 0.214;
/// Magnifier: the lens shows the source at 3×, centred 2.2 widths
/// right and 1.4 heights above it, joined to it by a line.
pub const MAGNIFIER_ZOOM: f32 = 3.0;
pub const MAGNIFIER_LENS: P = P { x: 2.2, y: -1.4 };
pub const DARKEST_AREA: i32 = 20;
/// Freehand: ⌥↑ / ⌥↓ change the smoothness (0 keeps the stroke as drawn), and every level runs
/// two passes of a 1-2-1 filter over the points. ⌘Enter steps through the stroke variability:
/// the line gets thinner where the pen moved fast, by 0, 45 or 80 percent at most.
pub const FREEHAND_SMOOTH_MAX: u8 = 6;
pub const FREEHAND_SMOOTH_PASSES: usize = 2;
pub const FREEHAND_VARY: [f32; 3] = [0.0, 0.45, 0.8];
pub const FREEHAND_VARY_SPEED: f32 = 14.0;
pub const FREEHAND_VARY_MIN: f32 = 0.2;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Tool {
    Select,
    Arrow,
    Text,
    Ruler,
    Rectangle,
    Backdrop,
    Freehand,
    Magnifier,
    Blur,
    Highlighter,
    Spotlight,
    Counter,
    Oval,
    Line,
    Crop,
    Cut,
    Ocr,
}

impl Tool {
    pub const TOOLBAR: [Tool; 13] = [
        Tool::Arrow,
        Tool::Text,
        Tool::Ruler,
        Tool::Rectangle,
        Tool::Backdrop,
        Tool::Freehand,
        Tool::Magnifier,
        Tool::Blur,
        Tool::Highlighter,
        Tool::Spotlight,
        Tool::Counter,
        Tool::Oval,
        Tool::Line,
    ];

    pub fn tooltip(self) -> &'static str {
        match self {
            Tool::Arrow => "Arrow (A)",
            Tool::Text => "Text (T)",
            Tool::Ruler => "Ruler (1 or 2)",
            Tool::Rectangle => "Rectangle (R)",
            Tool::Backdrop => "Backdrop (K)",
            Tool::Freehand => "Freehand Drawing (D)",
            Tool::Magnifier => "Magnifier (M)",
            Tool::Blur => "Blur/Erase (B)",
            Tool::Highlighter => "Highlighter (H)",
            Tool::Spotlight => "Spotlight (S)",
            Tool::Counter => "Counter (C)",
            Tool::Oval => "Oval (O)",
            Tool::Line => "Line (L)",
            Tool::Crop => "Crop",
            Tool::Cut => "Cut Out",
            Tool::Ocr => "Recognize Text or QR (⌘O)",
            Tool::Select => "Select",
        }
    }

    pub fn name(self) -> &'static str {
        let t = self.tooltip();
        t.split(" (").next().unwrap_or(t)
    }

    pub fn from_key(c: char) -> Option<Tool> {
        Some(match c {
            'a' => Tool::Arrow,
            't' => Tool::Text,
            'r' => Tool::Rectangle,
            'k' => Tool::Backdrop,
            'd' => Tool::Freehand,
            'm' => Tool::Magnifier,
            'b' => Tool::Blur,
            'h' => Tool::Highlighter,
            's' => Tool::Spotlight,
            'c' => Tool::Counter,
            'o' => Tool::Oval,
            'l' => Tool::Line,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrowStyle {
    Shape,
    HandDrawn,
    Line,
    Double,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fill {
    HandDrawn,
    Transparent,
    Translucent,
    Opaque,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Rect,
    Oval,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontKind {
    Sans,
    Goofy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlurMode {
    Blur,
    BlurText,
    Erase,
    EraseText,
}

impl BlurMode {
    pub const ALL: [BlurMode; 4] = [BlurMode::Blur, BlurMode::BlurText, BlurMode::Erase, BlurMode::EraseText];
    pub fn label(self) -> &'static str {
        match self {
            BlurMode::Blur => "Blur",
            BlurMode::BlurText => "Blur Text",
            BlurMode::Erase => "Erase",
            BlurMode::EraseText => "Erase Text",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub color: Rgba,
    pub size: u8,
    pub dashed: bool,
    pub arrow: ArrowStyle,
    pub fill: Fill,
    pub shape: Shape,
    pub font: FontKind,
    pub blur: BlurMode,
    /// Freehand smoothness, 0 to `FREEHAND_SMOOTH_MAX`.
    pub smooth: u8,
    /// Freehand stroke variability, an index into `FREEHAND_VARY`.
    pub vary: u8,
    /// Highlighter ends: round instead of flat.
    pub round_cap: bool,
    /// Spotlight: how dark the area around the lit part is.
    pub dim: f32,
    /// Pixels per point of the capture: every size table is in points.
    pub scale: f32,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            color: hex(DEFAULT_COLOR),
            size: SIZE_DEFAULT,
            dashed: false,
            arrow: ArrowStyle::Shape,
            fill: Fill::Transparent,
            shape: Shape::Rect,
            font: FontKind::Sans,
            blur: BlurMode::Blur,
            smooth: 0,
            vary: 0,
            round_cap: false,
            dim: SPOTLIGHT_DIM,
            scale: 1.0,
        }
    }
}

impl Style {
    /// ⌥↑ / ⌥↓: freehand smoothness one level up or down.
    pub fn smoother(mut self, up: bool) -> Style {
        self.smooth = if up { (self.smooth + 1).min(FREEHAND_SMOOTH_MAX) } else { self.smooth.saturating_sub(1) };
        self
    }
    /// ⌘Enter on a freehand stroke: the next stroke variability.
    pub fn next_variability(mut self) -> Style {
        self.vary = cycle(self.vary, FREEHAND_VARY.len() as u8);
        self
    }
    /// ⌘Enter on a highlighter: the other cap style.
    pub fn next_cap(mut self) -> Style {
        self.round_cap = !self.round_cap;
        self
    }
    /// Keys 1 to 9 on a spotlight: how dark everything around it is.
    pub fn with_dim(mut self, digit: u8) -> Style {
        self.dim = spotlight_dim(digit);
        self
    }
    pub fn width(&self) -> f32 {
        LINE_WIDTHS[self.size as usize] * self.scale
    }
    pub fn text_size(&self) -> f32 {
        let goofy = if self.font == FontKind::Goofy { TEXT_GOOFY_SIZE } else { 1.0 };
        TEXT_SIZES[self.size as usize] * self.scale * goofy
    }
    pub fn text_line_height(&self) -> f32 {
        self.text_size() * if self.font == FontKind::Goofy { TEXT_GOOFY_LINE_HEIGHT } else { TEXT_LINE_HEIGHT }
    }
    pub fn counter_size(&self) -> f32 {
        COUNTER_SIZES[self.size as usize] * self.scale
    }
    pub fn highlight_size(&self) -> f32 {
        HIGHLIGHT_SIZES[(self.size as usize).min(HIGHLIGHT_SIZES.len() - 1)] * self.scale
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct P {
    pub x: f32,
    pub y: f32,
}

pub fn p(x: f32, y: f32) -> P {
    P { x, y }
}

impl P {
    pub fn dist(self, o: P) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
    pub fn add(self, dx: f32, dy: f32) -> P {
        p(self.x + dx, self.y + dy)
    }
}

/// Axis-aligned box in image pixels, always normalised (x0 <= x1, y0 <= y1).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct R {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl R {
    pub fn from_points(a: P, b: P) -> R {
        R { x0: a.x.min(b.x), y0: a.y.min(b.y), x1: a.x.max(b.x), y1: a.y.max(b.y) }
    }
    pub fn w(&self) -> f32 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }
    pub fn center(&self) -> P {
        p((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }
    pub fn expand(&self, m: f32) -> R {
        R { x0: self.x0 - m, y0: self.y0 - m, x1: self.x1 + m, y1: self.y1 + m }
    }
    /// The box on whole pixels, as the keyboard edits it.
    pub fn rounded(&self) -> R {
        R { x0: self.x0.round(), y0: self.y0.round(), x1: self.x1.round(), y1: self.y1.round() }
    }
    pub fn contains(&self, q: P) -> bool {
        q.x >= self.x0 && q.x <= self.x1 && q.y >= self.y0 && q.y <= self.y1
    }
    pub fn union(&self, o: &R) -> R {
        R { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    pub fn translate(&self, dx: f32, dy: f32) -> R {
        R { x0: self.x0 + dx, y0: self.y0 + dy, x1: self.x1 + dx, y1: self.y1 + dy }
    }
    pub fn clamp_to(&self, w: f32, h: f32) -> R {
        R { x0: self.x0.clamp(0.0, w), y0: self.y0.clamp(0.0, h), x1: self.x1.clamp(0.0, w), y1: self.y1.clamp(0.0, h) }
    }
}

/// Spotlight: key 1 to 9 sets how dark the surroundings are, from 10 to 90 percent.
pub fn spotlight_dim(digit: u8) -> f32 {
    digit.clamp(1, 9) as f32 / 10.0
}

/// Next level of a setting that ⌘Enter steps through, wrapping to the first.
pub fn cycle(level: u8, levels: u8) -> u8 {
    (level + 1) % levels.max(1)
}

/// Smoothed freehand points: `level` times two passes of a 1-2-1 filter, with both ends kept.
pub fn freehand_smooth(pts: &[P], level: u8) -> Vec<P> {
    let mut out = pts.to_vec();
    if out.len() < 3 {
        return out;
    }
    for _ in 0..level.min(FREEHAND_SMOOTH_MAX) as usize * FREEHAND_SMOOTH_PASSES {
        let prev = out.clone();
        for i in 1..prev.len() - 1 {
            out[i] = p((prev[i - 1].x + 2.0 * prev[i].x + prev[i + 1].x) / 4.0, (prev[i - 1].y + 2.0 * prev[i].y + prev[i + 1].y) / 4.0);
        }
    }
    out
}

/// Line width of each segment of a freehand stroke (one less than the points). Level 0 keeps
/// `base`; above that a segment is thinner the longer it is (the pen moved faster there), and the
/// widths are averaged with their neighbours so the line does not step.
pub fn freehand_widths(pts: &[P], base: f32, level: u8, scale: f32) -> Vec<f32> {
    let n = pts.len().saturating_sub(1);
    let amount = FREEHAND_VARY.get(level as usize).copied().unwrap_or(0.0);
    if n == 0 || amount == 0.0 {
        return vec![base; n];
    }
    let raw: Vec<f32> = (0..n)
        .map(|i| {
            let fast = (pts[i].dist(pts[i + 1]) / (FREEHAND_VARY_SPEED * scale)).min(1.0);
            base * (1.0 - amount * fast).max(FREEHAND_VARY_MIN)
        })
        .collect();
    (0..n)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(2), (i + 2).min(n - 1));
            raw[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
        })
        .collect()
}

pub fn magnifier_lens(r: &R, lens: P) -> R {
    let c = r.center();
    let (w, h) = (r.w() * MAGNIFIER_ZOOM, r.h() * MAGNIFIER_ZOOM);
    let (x, y) = (c.x + lens.x * r.w(), c.y + lens.y * r.h());
    R { x0: x - w / 2.0, y0: y - h / 2.0, x1: x + w / 2.0, y1: y + h / 2.0 }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    Arrow { a: P, b: P, seed: u64 },
    Line { a: P, b: P },
    Rect { r: R, seed: u64 },
    Oval { r: R, seed: u64 },
    Text { at: P, text: String },
    Freehand { pts: Vec<P> },
    Highlighter { a: P, b: P },
    /// `text`: boxes found by text detection, relative to the top left of `r`.
    Blur { r: R, text: Option<Vec<R>> },
    Spotlight { r: R },
    Counter { at: P, n: u32 },
    /// `lens`: centre of the magnified view from the centre of `r`, in widths and heights of `r`.
    Magnifier { r: R, lens: P },
    Ruler { a: P, b: P },
    /// A picture laid over the screenshot (pasted from the clipboard), drawn into `r` at `opacity` (0…1).
    Image { pic: Pic, r: R, opacity: f32 },
}

/// A picture held by an image object. Cheap to clone; compared by identity.
#[derive(Clone)]
pub struct Pic {
    pub img: Image,
    id: u64,
}

impl Pic {
    pub fn new(img: Image) -> Self {
        Pic { img, id: next_version() }
    }
}

impl PartialEq for Pic {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
    }
}

impl std::fmt::Debug for Pic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pic({}x{})", self.img.w, self.img.h)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Obj {
    pub kind: Kind,
    pub style: Style,
}

impl Obj {
    pub fn tool(&self) -> Tool {
        match self.kind {
            Kind::Arrow { .. } => Tool::Arrow,
            Kind::Line { .. } => Tool::Line,
            Kind::Rect { .. } => Tool::Rectangle,
            Kind::Oval { .. } => Tool::Oval,
            Kind::Text { .. } => Tool::Text,
            Kind::Freehand { .. } => Tool::Freehand,
            Kind::Highlighter { .. } => Tool::Highlighter,
            Kind::Blur { .. } => Tool::Blur,
            Kind::Spotlight { .. } => Tool::Spotlight,
            Kind::Counter { .. } => Tool::Counter,
            Kind::Magnifier { .. } => Tool::Magnifier,
            Kind::Ruler { .. } => Tool::Ruler,
            Kind::Image { .. } => Tool::Select,
        }
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        match &mut self.kind {
            Kind::Arrow { a, b, .. } | Kind::Line { a, b } | Kind::Highlighter { a, b } | Kind::Ruler { a, b } => {
                *a = a.add(dx, dy);
                *b = b.add(dx, dy);
            }
            Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } | Kind::Image { r, .. } => {
                *r = r.translate(dx, dy)
            }
            Kind::Text { at, .. } | Kind::Counter { at, .. } => *at = at.add(dx, dy),
            Kind::Freehand { pts } => pts.iter_mut().for_each(|q| *q = q.add(dx, dy)),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.kind {
            Kind::Arrow { a, b, .. } | Kind::Line { a, b } | Kind::Highlighter { a, b } | Kind::Ruler { a, b } => a.dist(*b) < 3.0,
            Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } | Kind::Image { r, .. } => r.w() < 3.0 || r.h() < 3.0,
            Kind::Text { text, .. } => text.trim().is_empty(),
            Kind::Freehand { pts } => pts.len() < 2,
            Kind::Counter { .. } => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum GradientKind {
    Mesh,
    Vertical,
    Diagonal,
    Radial,
}

impl GradientKind {
    pub const ALL: [GradientKind; 4] = [GradientKind::Mesh, GradientKind::Vertical, GradientKind::Diagonal, GradientKind::Radial];
    pub fn label(self) -> &'static str {
        match self {
            GradientKind::Mesh => "Mesh gradient",
            GradientKind::Vertical => "Vertical gradient",
            GradientKind::Diagonal => "Diagonal gradient",
            GradientKind::Radial => "Radial gradient",
        }
    }
}

/// Gradient templates: a list of colours and the stops between them.
pub const GRADIENTS: [(&str, &str); 14] = [
    ("#9AE6FF,#56C3FF,#2B91E2,#144CA0,#0D2162,#091022", "0.12,0.37,0.64,0.87"),
    ("#001738,#0E3A69,#004C7E,#005985,#116D96,#3F9ABA,#7CC0D6,#A3D1E7", "0.24,0.40,0.47,0.58,0.77,0.93"),
    ("#0078A6,#004A70,#042952,#2C1E48,#4C1A48,#BA313E,#FF8D00", "0.17,0.33,0.41,0.50,0.77"),
    ("#FFE995,#FF8C7C,#FF6B77,#E92D94,#C2008D,#98008A,#840488,#000177", "0.20,0.27,0.41,0.56,0.67,0.71"),
    ("#6568B5,#AB84C2,#D998B6,#F2ACA2,#F7B39C,#F7B494,#F7AD90,#F29C85,#E08379,#AD506D,#24134B", "0.15,0.25,0.35,0.41,0.45,0.49,0.55,0.62,0.76"),
    ("#A5BCD8,#C0CCD9,#D9DBD8,#E6E1D3,#EFE4CA,#F0E1C5,#E9D7BB,#D9C9B0,#C2B49F,#A0A19A,#7B8894,#576C80,#364759,#0A1621", "0.11,0.24,0.31,0.38,0.44,0.50,0.56,0.63,0.69,0.75,0.81,0.88"),
    ("#000000,#23054E,#320870,#4B1F8C,#6425A1,#8036B3,#9D46C2,#BA57CF,#DA7FE6,#F8A8F8,#FCC8FC,#FFE8FF", "0.14,0.21,0.31,0.36,0.43,0.50,0.56,0.68,0.82,0.92"),
    ("#26608C,#3981AF,#5B9DC5,#7CB8DA,#B6E0FC", "0.00,0.27,0.48,0.80,1.00"),
    ("#015D8E,#1A87B2,#379DC3,#53B4D4,#81DCFA", "0.00,0.27,0.48,0.80,1.00"),
    ("#F996B2,#FFBEB8,#FFE3D1", ""),
    ("#FEB142,#F06A20,#B61B14", ""),
    ("#304B54,#486D78,#618794,#9DBCC6,#D3E7F1", "0.00,0.27,0.48,0.80,1.00"),
    ("#7F3273,#B0457B,#D35A7D,#F49595,#F9BDA9", "0.00,0.27,0.48,0.80,1.00"),
    ("#C3E3E4,#E5F1EC,#EEF0E5,#F7EFDE", "0.00,0.50,0.70,1.00"),
];

pub fn gradient_stops(i: usize) -> Vec<(f32, Rgba)> {
    let (cols, stops) = GRADIENTS[i % GRADIENTS.len()];
    let cols: Vec<Rgba> = cols.split(',').map(hex).collect();
    let mut st: Vec<f32> = stops.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    if st.len() + 2 == cols.len() {
        st.insert(0, 0.0);
        st.push(1.0);
    }
    if st.len() != cols.len() {
        let n = (cols.len() - 1).max(1) as f32;
        st = (0..cols.len()).map(|i| i as f32 / n).collect();
    }
    st.into_iter().zip(cols).collect()
}

/// What the backdrop is painted with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BackdropFill {
    /// One of the preset gradients.
    Preset,
    /// A gradient between two chosen colours.
    Custom,
    /// One flat colour.
    Solid,
}

/// A picture used as the backdrop. Cheap to clone; compared by identity.
#[derive(Clone)]
pub struct BackdropImage {
    pub img: Image,
    id: u64,
}

impl BackdropImage {
    pub fn new(img: Image) -> Self {
        BackdropImage { img, id: next_version() }
    }
}

impl PartialEq for BackdropImage {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id
    }
}

impl std::fmt::Debug for BackdropImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BackdropImage({}x{})", self.img.w, self.img.h)
    }
}

/// The backdrop configuration. Serialisable (the picture itself stays out: only its file path is kept).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Backdrop {
    pub fill: BackdropFill,
    pub gradient: usize,
    pub kind: GradientKind,
    pub padding: f32,
    pub shadow: f32,
    pub inset: f32,
    pub outer_radius: f32,
    pub inner_radius: f32,
    pub ratio: Option<f32>,
    pub reverse: bool,
    pub custom_from: [u8; 3],
    pub custom_to: [u8; 3],
    pub solid: [u8; 3],
    /// Image blur slider, 0…7: Gaussian radius 2 × v.
    pub blur: f32,
    /// File the background picture was loaded from; empty for no picture.
    pub image_path: String,
    #[serde(skip)]
    pub image: Option<BackdropImage>,
    /// Trim the empty border (see-through or one flat colour) off the screenshot first.
    pub autocrop: bool,
    /// Where the screenshot sits relative to the centre of the backdrop, in output pixels.
    pub offset: [f32; 2],
}

impl Default for Backdrop {
    fn default() -> Self {
        Backdrop {
            fill: BackdropFill::Preset,
            gradient: 0,
            kind: GradientKind::Mesh,
            padding: 12.0,
            shadow: 20.0,
            inset: 12.0,
            outer_radius: 12.0,
            inner_radius: 20.0,
            ratio: None,
            reverse: false,
            custom_from: [0x22, 0x14, 0x48],
            custom_to: [0x91, 0x9B, 0xD2],
            solid: [0x60, 0x80, 0xA0],
            blur: 0.0,
            image_path: String::new(),
            image: None,
            autocrop: false,
            offset: [0.0, 0.0],
        }
    }
}

/// Backdrop sliders run 0…20; these turn a slider position into pixels.
pub const BACKDROP_SLIDER_MAX: f32 = 20.0;
/// The image blur slider runs 0…7.
pub const BACKDROP_BLUR_MAX: f32 = 7.0;
/// A backdrop is refused on pictures above this many pixels (crop or resize first).
pub const BACKDROP_MAX_PIXELS: u64 = 15_000_000;

pub fn backdrop_too_large(w: u32, h: u32) -> bool {
    w as u64 * h as u64 > BACKDROP_MAX_PIXELS
}

impl Backdrop {
    /// The settings file can be edited by hand or come from another version: values the sliders
    /// can't produce (and that would make the render allocate absurd sizes) are brought in range.
    pub fn sanitised(mut self) -> Backdrop {
        let slider = |v: f32| if v.is_finite() { v.clamp(0.0, BACKDROP_SLIDER_MAX) } else { 0.0 };
        self.padding = slider(self.padding);
        self.shadow = slider(self.shadow);
        self.inset = slider(self.inset);
        self.outer_radius = slider(self.outer_radius);
        self.inner_radius = slider(self.inner_radius);
        self.blur = if self.blur.is_finite() { self.blur.clamp(0.0, BACKDROP_BLUR_MAX) } else { 0.0 };
        self.ratio = self.ratio.filter(|r| r.is_finite() && (0.1..=10.0).contains(r));
        self.gradient %= GRADIENTS.len();
        if !self.offset.iter().all(|v| v.is_finite()) {
            self.offset = [0.0, 0.0];
        }
        self
    }

    /// Padding: round(v² × 0.5)
    pub fn padding_px(&self) -> f32 {
        (self.padding * self.padding * 0.5).round()
    }
    /// Shadow: v ÷ 20
    pub fn shadow_amount(&self) -> f32 {
        self.shadow / 20.0
    }
    /// Inset: round(v² ÷ 1.8)
    pub fn inset_px(&self) -> f32 {
        (self.inset * self.inset / 1.8).round()
    }
    /// Outer radius: v × 2
    pub fn outer_radius_px(&self) -> f32 {
        self.outer_radius * 2.0
    }
    /// Inner radius: v × 2
    pub fn inner_radius_px(&self) -> f32 {
        self.inner_radius * 2.0
    }
    /// Image blur: Gaussian radius 2 × v
    pub fn blur_radius(&self) -> f32 {
        self.blur * 2.0
    }
    /// Puts the screenshot back at the centre.
    pub fn recenter(&mut self) {
        self.offset = [0.0, 0.0];
    }
    /// The colour stops of the gradient fills (before `reverse`).
    pub fn stops(&self) -> Vec<(f32, Rgba)> {
        match self.fill {
            BackdropFill::Custom => {
                let c = |v: [u8; 3]| [v[0], v[1], v[2], 255];
                vec![(0.0, c(self.custom_from)), (1.0, c(self.custom_to))]
            }
            BackdropFill::Solid => vec![(0.0, [self.solid[0], self.solid[1], self.solid[2], 255]); 2],
            BackdropFill::Preset => gradient_stops(self.gradient),
        }
    }
    /// Forgets the background picture.
    pub fn forget_image(&mut self) {
        self.image = None;
        self.image_path.clear();
    }
    /// Reloads the picture from `image_path` (after the configuration came back from the settings file).
    pub fn restore_image(&mut self) {
        if self.image.is_none() && !self.image_path.is_empty() {
            match load_backdrop_image(std::path::Path::new(&self.image_path)) {
                Some(i) => self.image = Some(i),
                None => self.image_path.clear(),
            }
        }
    }
}

/// Longest side kept for a background picture.
pub const BACKDROP_IMAGE_MAX_SIDE: u32 = 4096;

/// Reads a picture file for use as a backdrop; very large pictures are scaled down.
pub fn load_backdrop_image(path: &std::path::Path) -> Option<BackdropImage> {
    let mut img = image::open(path).ok()?;
    if img.width().max(img.height()) > BACKDROP_IMAGE_MAX_SIDE {
        img = img.resize(BACKDROP_IMAGE_MAX_SIDE, BACKDROP_IMAGE_MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    Some(BackdropImage::new(Image { w: rgba.width(), h: rgba.height(), px: Arc::new(rgba.into_raw()) }))
}

/// Keeps a copy of a picked background picture in `dir` (so it survives the original being moved) and
/// returns its path. Only the newest `keep` copies are kept.
pub fn store_backdrop_image(src: &std::path::Path, dir: &std::path::Path, keep: usize) -> Option<std::path::PathBuf> {
    let img = load_backdrop_image(src)?;
    std::fs::create_dir_all(dir).ok()?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = dir.join(format!("backdrop-{stamp}.jpg"));
    let rgb = image::RgbImage::from_fn(img.img.w, img.img.h, |x, y| {
        let p = img.img.get(x as i32, y as i32);
        let a = p[3] as u32;
        image::Rgb([0, 1, 2].map(|i| ((p[i] as u32 * a + 255 * (255 - a)) / 255) as u8))
    });
    rgb.save_with_format(&path, image::ImageFormat::Jpeg).ok()?;
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("backdrop-"))
        .collect();
    files.sort_by_key(|e| e.file_name());
    while files.len() > keep {
        let old = files.remove(0);
        let _ = std::fs::remove_file(old.path());
    }
    Some(path)
}

pub const RATIOS: [(&str, Option<f32>); 10] = [
    ("Reset", None),
    ("1:1", Some(1.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("3:2", Some(1.5)),
    ("16:9", Some(16.0 / 9.0)),
    ("Twitter", Some(16.0 / 9.0)),
    ("Facebook", Some(1.91)),
    ("YouTube", Some(16.0 / 9.0)),
    ("LinkedIn", Some(1.91)),
    ("Pinterest", Some(2.0 / 3.0)),
];

/// ⌘+ / ⌘-: doubles or halves below 4×, above that ×/÷ 1.414 rounded to an even number.
pub fn zoom_key_target(z: f32, zoom_in: bool) -> f32 {
    let even = |v: f32| (v * 0.5).round() * 2.0;
    match (zoom_in, z >= 4.0) {
        (true, true) => even(z * ZOOM_KEY_STEP),
        (true, false) => z * 2.0,
        (false, true) if z > 4.0 => even(z / ZOOM_KEY_STEP),
        (false, _) => z * 0.5,
    }
    .min(ZOOM_MAX)
}

/// One wheel notch ×/÷ 1.1892, then snapped to round levels.
pub fn zoom_wheel_target(z: f32, zoom_in: bool) -> f32 {
    let z = if zoom_in { z * ZOOM_WHEEL_STEP } else { z / ZOOM_WHEEL_STEP };
    let snapped = if z > 6.0 {
        z.round()
    } else if z > 2.0 {
        (z * 2.0).round() * 0.5
    } else if z > 0.65 {
        (z * 4.0).round() * 0.25
    } else if z >= 0.5 {
        (z * 10.0).round() / 10.0
    } else if z >= 0.2 {
        (z * 20.0).round() / 20.0
    } else {
        z
    };
    snapped.min(ZOOM_MAX)
}

pub const RESIZE_STEPS: [(&str, f32); 6] = [("25%", 0.25), ("33%", 1.0 / 3.0), ("50%", 0.5), ("1×", 1.0), ("2×", 2.0), ("4×", 4.0)];

/// A pasted picture over this many pixels is scaled down first (an 8K screenshot is 33 M).
pub const MAX_PASTE_PIXELS: u64 = 40_000_000;

pub fn too_big_to_paste(img: &Image) -> bool {
    img.w as u64 * img.h as u64 > MAX_PASTE_PIXELS
}

/// The factor that brings a pasted picture down to `MAX_PASTE_PIXELS`.
pub fn paste_shrink_factor(img: &Image) -> f32 {
    ((MAX_PASTE_PIXELS as f64 / (img.w as f64 * img.h as f64).max(1.0)).sqrt() as f32).min(1.0)
}

/// Most pixels a resized picture may have (a 4× enlargement of a 5K screenshot is ~236 M).
pub const MAX_RESIZE_PIXELS: f64 = 300_000_000.0;

/// Whether resizing a `w` × `h` picture by `factor` would make it unreasonably large (the pixel
/// buffer alone is 4 bytes a pixel, and a failed allocation aborts the app).
pub fn resize_too_large(w: f32, h: f32, factor: f32) -> bool {
    let f = factor as f64;
    w as f64 * h as f64 * f * f > MAX_RESIZE_PIXELS
}

#[derive(Clone)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    /// Straight (not premultiplied) RGBA rows.
    pub px: Arc<Vec<u8>>,
}

impl Image {
    /// True when any pixel is not fully opaque.
    pub fn has_alpha(&self) -> bool {
        self.px.chunks_exact(4).any(|p| p[3] != 255)
    }

    pub fn get(&self, x: i32, y: i32) -> Rgba {
        let x = x.clamp(0, self.w as i32 - 1) as usize;
        let y = y.clamp(0, self.h as i32 - 1) as usize;
        let i = (y * self.w as usize + x) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }
}

/// How far annotations may stick out of the picture on one side before the canvas stops growing.
pub const MAX_OVERHANG: f32 = 4096.0;
/// The most pixels an enlarged canvas may have; beyond that the picture keeps its own size.
pub const MAX_CANVAS_PIXELS: f64 = 64_000_000.0;

/// The canvas a picture `base` needs so that every `ink` rectangle (the painted extent of an
/// annotation) fits: `base` grown to hold them all, in whole pixels, each side by at most
/// `MAX_OVERHANG`. Anything over `MAX_CANVAS_PIXELS` is not granted: the result is `base` itself.
pub fn grow_canvas(base: R, inks: impl IntoIterator<Item = R>) -> R {
    let mut r = base;
    for i in inks {
        if [i.x0, i.y0, i.x1, i.y1].iter().all(|v| v.is_finite()) {
            r = r.union(&i);
        }
    }
    // Whole pixels; less than half a pixel of overhang (antialiasing at the edge) grows nothing.
    let r = R {
        x0: r.x0.max(base.x0 - MAX_OVERHANG).round(),
        y0: r.y0.max(base.y0 - MAX_OVERHANG).round(),
        x1: r.x1.min(base.x1 + MAX_OVERHANG).round(),
        y1: r.y1.min(base.y1 + MAX_OVERHANG).round(),
    };
    if r.w() as f64 * r.h() as f64 > MAX_CANVAS_PIXELS {
        base
    } else {
        r
    }
}

/// Where a picture of `w` × `h` goes when pasted over `view`: centred, at 100 % or fitted inside
/// `view` (keeping its proportions) when larger.
pub fn overlay_rect(w: u32, h: u32, view: &R) -> R {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let k = (view.w() / w).min(view.h() / h).min(1.0);
    let (w, h) = (w * k, h * k);
    let c = view.center();
    R { x0: c.x - w / 2.0, y0: c.y - h / 2.0, x1: c.x + w / 2.0, y1: c.y + h / 2.0 }
}

/// The corner opposite `anchor` for a drag that ends at `q`, moved so the box keeps `ratio` (width over
/// height): it follows whichever axis the pointer went furthest on.
pub fn keep_ratio(anchor: P, q: P, ratio: f32) -> P {
    let ratio = ratio.max(1e-3);
    let (dx, dy) = (q.x - anchor.x, q.y - anchor.y);
    let w = dx.abs().max(dy.abs() * ratio);
    let h = w / ratio;
    p(anchor.x + w * if dx < 0.0 { -1.0 } else { 1.0 }, anchor.y + h * if dy < 0.0 { -1.0 } else { 1.0 })
}

/// `left` with `right` beside it, top aligned, on a canvas as tall as the taller of the two.
/// Pixels that belong to neither are fully transparent.
pub fn place_right(left: &Image, right: &Image) -> Image {
    let (w, h) = (left.w + right.w, left.h.max(right.h));
    let mut px = vec![0u8; w as usize * h as usize * 4];
    for (src, x0) in [(left, 0u32), (right, left.w)] {
        for y in 0..src.h as usize {
            let d = (y * w as usize + x0 as usize) * 4;
            let s = y * src.w as usize * 4;
            px[d..d + src.w as usize * 4].copy_from_slice(&src.px[s..s + src.w as usize * 4]);
        }
    }
    Image { w, h, px: Arc::new(px) }
}

#[derive(Clone)]
pub struct Snapshot {
    pub base: Image,
    pub objs: Vec<Obj>,
    pub crop: Option<R>,
    pub backdrop: Option<Backdrop>,
}

pub struct Doc {
    pub base: Image,
    pub objs: Vec<Obj>,
    pub crop: Option<R>,
    pub backdrop: Option<Backdrop>,
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    /// Bumped on every change so cached textures know when to refresh.
    pub version: u64,
    pub scale_factor: f32,
}

impl Doc {
    pub fn new(base: Image, scale_factor: f32) -> Doc {
        Doc { base, objs: vec![], crop: None, backdrop: None, undo: vec![], redo: vec![], version: next_version(), scale_factor }
    }

    fn snap(&self) -> Snapshot {
        Snapshot { base: self.base.clone(), objs: self.objs.clone(), crop: self.crop, backdrop: self.backdrop.clone() }
    }

    /// Call before a user-visible change so it can be undone.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.snap());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.version = next_version();
    }

    pub fn touch(&mut self) {
        self.version = next_version();
    }

    /// Every spotlight shares one dark layer, so the darkness is set on all of them at once, as one
    /// undo step. Returns whether there was a spotlight to change.
    pub fn set_spotlight_dim(&mut self, dim: f32) -> bool {
        let spots = |d: &Doc| d.objs.iter().any(|o| matches!(o.kind, Kind::Spotlight { .. }) && o.style.dim != dim);
        if !spots(self) {
            return false;
        }
        self.checkpoint();
        for o in self.objs.iter_mut().filter(|o| matches!(o.kind, Kind::Spotlight { .. })) {
            o.style.dim = dim;
        }
        true
    }

    fn restore(&mut self, s: Snapshot) {
        self.base = s.base;
        self.objs = s.objs;
        self.crop = s.crop;
        self.backdrop = s.backdrop.clone();
        self.version = next_version();
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(s) => {
                self.redo.push(self.snap());
                self.restore(s);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(s) => {
                self.undo.push(self.snap());
                self.restore(s);
                true
            }
            None => false,
        }
    }

    pub fn view_rect(&self) -> R {
        self.crop.unwrap_or(R { x0: 0.0, y0: 0.0, x1: self.base.w as f32, y1: self.base.h as f32 })
    }

    /// Lays a picture from the clipboard over the screenshot: centred on what is shown, at 100 %
    /// (one picture pixel per image pixel), scaled down to fit when it is larger than that.
    /// One undo step; returns the index of the new object, which is the topmost.
    pub fn paste_image(&mut self, img: Image) -> usize {
        let r = overlay_rect(img.w, img.h, &self.view_rect());
        self.checkpoint();
        self.objs.push(Obj { kind: Kind::Image { pic: Pic::new(img), r, opacity: 1.0 }, style: Style::default() });
        self.objs.len() - 1
    }

    /// Adds a new capture to the right of the picture, side by side and top aligned; the rest of the
    /// enlarged canvas is see-through. Annotations stay where they were. Any crop is dropped so the
    /// whole result shows. One undo step.
    pub fn append_capture(&mut self, img: &Image) {
        self.checkpoint();
        self.base = place_right(&self.base, img);
        self.crop = None;
    }

    pub fn next_counter(&self) -> u32 {
        self.objs.iter().filter_map(|o| if let Kind::Counter { n, .. } = o.kind { Some(n) } else { None }).max().unwrap_or(0) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_edited_backdrop_is_brought_back_in_range() {
        let bd: Backdrop = serde_json::from_str(r#"{"padding":20000,"shadow":-3,"ratio":0.0,"gradient":99,"blur":50,"offset":[1e30,0]}"#).unwrap();
        let ok = bd.sanitised();
        assert_eq!((ok.padding, ok.shadow, ok.blur), (BACKDROP_SLIDER_MAX, 0.0, BACKDROP_BLUR_MAX));
        assert_eq!(ok.ratio, None);
        assert!(ok.gradient < GRADIENTS.len());
        let sane = Backdrop { ratio: Some(1.5), padding: 7.0, ..Backdrop::default() };
        assert_eq!(sane.clone().sanitised(), sane);
        let nan = Backdrop { padding: f32::NAN, ratio: Some(f32::INFINITY), offset: [f32::NAN, 0.0], ..Backdrop::default() }.sanitised();
        assert_eq!((nan.padding, nan.ratio, nan.offset), (0.0, None, [0.0, 0.0]));
    }

    #[test]
    fn a_huge_pasted_picture_is_brought_down_to_the_pixel_budget() {
        let big = Image { w: 20_000, h: 20_000, px: Arc::new(vec![]) };
        assert!(too_big_to_paste(&big));
        let f = paste_shrink_factor(&big);
        let (w, h) = ((20_000.0 * f) as u64, (20_000.0 * f) as u64);
        assert!(w * h <= MAX_PASTE_PIXELS && w * h > MAX_PASTE_PIXELS * 9 / 10, "{w}x{h}");
        let ok = Image { w: 7680, h: 4320, px: Arc::new(vec![]) };
        assert!(!too_big_to_paste(&ok));
        assert_eq!(paste_shrink_factor(&ok), 1.0);
    }

    #[test]
    fn resize_refuses_pictures_that_would_not_fit_in_memory() {
        // Every preset is fine on a 5K screenshot and on an ordinary one.
        for (_, f) in RESIZE_STEPS {
            assert!(!resize_too_large(5120.0, 2880.0, f));
            assert!(!resize_too_large(1440.0, 900.0, f));
        }
        // A very tall scrolling capture doubled or quadrupled is refused, shrinking it is not.
        assert!(resize_too_large(1600.0, 60_000.0, 4.0));
        assert!(!resize_too_large(1600.0, 60_000.0, 0.5));
    }

    #[test]
    fn backdrop_config_round_trips_through_json() {
        let mut bd = Backdrop::default();
        bd.fill = BackdropFill::Custom;
        bd.kind = GradientKind::Radial;
        bd.custom_from = [1, 2, 3];
        bd.blur = 4.0;
        bd.ratio = Some(1.5);
        bd.autocrop = true;
        bd.image_path = "/tmp/x.jpg".into();
        bd.image = Some(BackdropImage::new(Image { w: 1, h: 1, px: Arc::new(vec![0, 0, 0, 255]) }));
        let json = serde_json::to_string(&bd).unwrap();
        assert!(!json.contains("\"image\""), "the picture itself is not written: {json}");
        let back: Backdrop = serde_json::from_str(&json).unwrap();
        assert!(back.image.is_none());
        let mut expect = bd.clone();
        expect.image = None;
        assert_eq!(back, expect);
        // Old or partial files still load.
        let partial: Backdrop = serde_json::from_str("{\"padding\": 5.0}").unwrap();
        assert_eq!(partial.padding, 5.0);
        assert_eq!(partial.shadow, Backdrop::default().shadow);
    }

    #[test]
    fn backdrop_is_refused_above_fifteen_megapixels() {
        assert!(!backdrop_too_large(5000, 3000));
        assert!(backdrop_too_large(5000, 3001));
        assert!(backdrop_too_large(20_000, 1_000));
        assert!(!backdrop_too_large(3840, 2160));
    }

    #[test]
    fn blur_slider_is_a_radius_of_twice_its_value_and_recenter_resets_the_offset() {
        let mut bd = Backdrop::default();
        bd.blur = 3.0;
        assert_eq!(bd.blur_radius(), 6.0);
        bd.offset = [10.0, -4.0];
        bd.recenter();
        assert_eq!(bd.offset, [0.0, 0.0]);
    }

    #[test]
    fn stops_follow_the_fill() {
        let mut bd = Backdrop::default();
        assert_eq!(bd.stops().len(), gradient_stops(0).len());
        bd.fill = BackdropFill::Custom;
        assert_eq!(bd.stops(), vec![(0.0, [0x22, 0x14, 0x48, 255]), (1.0, [0x91, 0x9B, 0xD2, 255])]);
        bd.fill = BackdropFill::Solid;
        assert!(bd.stops().iter().all(|(_, c)| *c == [0x60, 0x80, 0xA0, 255]));
    }

    #[test]
    fn a_picked_picture_is_stored_reloaded_and_old_copies_are_dropped() {
        let dir = std::env::temp_dir().join(format!("shotcraft-bd-{}", next_version()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("pick.png");
        image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255])).save(&src).unwrap();
        let store = dir.join("backdrops");
        let mut last = None;
        for _ in 0..4 {
            std::thread::sleep(std::time::Duration::from_millis(3));
            last = store_backdrop_image(&src, &store, 2);
        }
        let last = last.unwrap();
        assert_eq!(std::fs::read_dir(&store).unwrap().count(), 2, "only the newest copies stay");
        let mut bd = Backdrop::default();
        bd.image_path = last.display().to_string();
        bd.restore_image();
        let img = bd.image.as_ref().expect("reloaded from its path").img.clone();
        assert_eq!((img.w, img.h), (40, 20));
        assert!(img.get(10, 10)[0] > 150, "colour survives the jpeg copy");
        bd.forget_image();
        assert!(bd.image.is_none() && bd.image_path.is_empty());
        bd.image_path = "/no/such/file.png".into();
        bd.restore_image();
        assert!(bd.image.is_none() && bd.image_path.is_empty(), "a missing file is forgotten");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn smoothness_steps_between_zero_and_the_maximum() {
        let st = Style::default();
        assert_eq!(st.smooth, 0);
        assert_eq!(st.smoother(false).smooth, 0, "stops at zero");
        let mut s = st;
        for _ in 0..FREEHAND_SMOOTH_MAX + 3 {
            s = s.smoother(true);
        }
        assert_eq!(s.smooth, FREEHAND_SMOOTH_MAX, "stops at the maximum");
        assert_eq!(s.smoother(false).smooth, FREEHAND_SMOOTH_MAX - 1);
    }

    #[test]
    fn variability_cycles_through_every_level_and_wraps() {
        let mut s = Style::default();
        let seen: Vec<u8> = (0..FREEHAND_VARY.len() + 1)
            .map(|_| {
                let v = s.vary;
                s = s.next_variability();
                v
            })
            .collect();
        assert_eq!(seen, vec![0, 1, 2, 0]);
    }

    #[test]
    fn highlighter_cap_toggles() {
        let s = Style::default();
        assert!(!s.round_cap);
        assert!(s.next_cap().round_cap);
        assert!(!s.next_cap().next_cap().round_cap);
    }

    #[test]
    fn spotlight_keys_set_the_darkness_and_the_default_is_kept() {
        assert_eq!(Style::default().dim, SPOTLIGHT_DIM);
        assert_eq!(Style::default().with_dim(1).dim, 0.1);
        assert_eq!(Style::default().with_dim(5).dim, 0.5);
        assert_eq!(Style::default().with_dim(9).dim, 0.9);
        assert_eq!(spotlight_dim(0), 0.1, "0 is not a level: clamped");
        assert_eq!(spotlight_dim(200), 0.9);
    }

    #[test]
    fn smoothing_keeps_the_ends_and_calms_a_shaky_line() {
        let shaky: Vec<P> = (0..40).map(|i| p(i as f32 * 4.0, ((i * 7919 % 13) as f32 - 6.0) * 1.5)).collect();
        let rough = |v: &[P]| v.windows(3).map(|w| (w[0].y - 2.0 * w[1].y + w[2].y).abs()).sum::<f32>();
        assert_eq!(freehand_smooth(&shaky, 0), shaky);
        let (l1, l3, l6) = (freehand_smooth(&shaky, 1), freehand_smooth(&shaky, 3), freehand_smooth(&shaky, 6));
        assert!(rough(&shaky) > rough(&l1) && rough(&l1) > rough(&l3) && rough(&l3) > rough(&l6), "each level is calmer");
        for out in [&l1, &l3, &l6] {
            assert_eq!(out.len(), shaky.len());
            assert_eq!((out[0], out[out.len() - 1]), (shaky[0], shaky[shaky.len() - 1]), "the ends stay");
        }
        assert_eq!(freehand_smooth(&shaky, 99).len(), shaky.len(), "levels past the maximum are clamped");
        let two = vec![p(0.0, 0.0), p(5.0, 5.0)];
        assert_eq!(freehand_smooth(&two, 4), two);
    }

    #[test]
    fn variability_thins_fast_segments_only() {
        let pts = vec![p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(3.0, 0.0), p(4.0, 0.0), p(60.0, 0.0), p(120.0, 0.0), p(180.0, 0.0), p(240.0, 0.0), p(300.0, 0.0)];
        assert_eq!(freehand_widths(&pts, 10.0, 0, 1.0), vec![10.0; 9]);
        let w = freehand_widths(&pts, 10.0, 2, 1.0);
        assert_eq!(w.len(), 9);
        assert!(w[0] > 8.0, "slow start stays thick: {}", w[0]);
        assert!(w[7] < 4.0, "fast end is thin: {}", w[7]);
        assert!(w.iter().all(|v| *v >= 10.0 * FREEHAND_VARY_MIN - 1e-4 && *v <= 10.0));
        let mild = freehand_widths(&pts, 10.0, 1, 1.0);
        assert!(mild[7] > w[7], "level 1 thins less than level 2");
        assert!(freehand_widths(&[p(0.0, 0.0)], 10.0, 2, 1.0).is_empty());
    }

    #[test]
    fn spotlight_darkness_is_set_on_every_spotlight_in_one_undo_step() {
        let img = Image { w: 4, h: 4, px: std::sync::Arc::new(vec![0; 64]) };
        let mut d = Doc::new(img, 1.0);
        let r = R { x0: 0.0, y0: 0.0, x1: 2.0, y1: 2.0 };
        d.objs.push(Obj { kind: Kind::Spotlight { r }, style: Style::default() });
        d.objs.push(Obj { kind: Kind::Line { a: p(0.0, 0.0), b: p(3.0, 3.0) }, style: Style::default() });
        d.objs.push(Obj { kind: Kind::Spotlight { r }, style: Style::default() });
        assert!(d.set_spotlight_dim(0.6));
        assert_eq!([d.objs[0].style.dim, d.objs[1].style.dim, d.objs[2].style.dim], [0.6, SPOTLIGHT_DIM, 0.6]);
        assert!(!d.set_spotlight_dim(0.6), "nothing changes the second time, so no undo step");
        assert_eq!(d.undo.len(), 1);
        d.undo();
        assert_eq!(d.objs[0].style.dim, SPOTLIGHT_DIM);
    }

    fn flat_img(w: u32, h: u32, c: [u8; 4]) -> Image {
        Image { w, h, px: std::sync::Arc::new((0..w * h).flat_map(|_| c).collect()) }
    }

    #[test]
    fn a_pasted_picture_is_centred_at_100_percent_or_fitted_inside_the_view() {
        let view = R { x0: 0.0, y0: 0.0, x1: 400.0, y1: 200.0 };
        assert_eq!(overlay_rect(100, 50, &view), R { x0: 150.0, y0: 75.0, x1: 250.0, y1: 125.0 });
        // Larger than the view: scaled down to fit, proportions kept, still centred.
        let r = overlay_rect(800, 200, &view);
        assert_eq!((r.w(), r.h()), (400.0, 100.0));
        assert_eq!((r.center().x, r.center().y), (200.0, 100.0));
        let r = overlay_rect(200, 800, &view);
        assert_eq!((r.w(), r.h()), (50.0, 200.0));
    }

    #[test]
    fn pasting_a_picture_adds_the_topmost_object_in_one_undo_step() {
        let mut d = Doc::new(flat_img(40, 20, [255, 255, 255, 255]), 1.0);
        d.objs.push(Obj { kind: Kind::Line { a: p(0.0, 0.0), b: p(5.0, 5.0) }, style: Style::default() });
        let i = d.paste_image(flat_img(10, 10, [255, 0, 0, 255]));
        assert_eq!(i, 1);
        let Kind::Image { r, opacity, .. } = &d.objs[i].kind else { panic!("not an image") };
        assert_eq!((*r, *opacity), (R { x0: 15.0, y0: 5.0, x1: 25.0, y1: 15.0 }, 1.0));
        assert_eq!(d.undo.len(), 1);
        assert!(d.undo());
        assert_eq!(d.objs.len(), 1);
        assert!(d.redo());
        assert_eq!(d.objs.len(), 2);
    }

    #[test]
    fn a_pasted_picture_moves_with_the_move_command() {
        let mut d = Doc::new(flat_img(40, 20, [0, 0, 0, 255]), 1.0);
        let i = d.paste_image(flat_img(10, 10, [255, 0, 0, 255]));
        d.objs[i].translate(3.0, -2.0);
        let Kind::Image { r, .. } = &d.objs[i].kind else { panic!() };
        assert_eq!((r.x0, r.y0), (18.0, 3.0));
    }

    #[test]
    fn keeping_the_ratio_follows_the_longer_drag() {
        let a = p(0.0, 0.0);
        assert_eq!(keep_ratio(a, p(100.0, 10.0), 2.0), p(100.0, 50.0));
        assert_eq!(keep_ratio(a, p(10.0, 100.0), 2.0), p(200.0, 100.0));
        assert_eq!(keep_ratio(a, p(-30.0, -10.0), 1.0), p(-30.0, -30.0));
    }

    #[test]
    fn adding_a_capture_puts_it_to_the_right_on_a_taller_transparent_canvas_and_undoes() {
        let mut d = Doc::new(flat_img(4, 2, [10, 20, 30, 255]), 1.0);
        d.crop = Some(R { x0: 0.0, y0: 0.0, x1: 2.0, y1: 2.0 });
        d.append_capture(&flat_img(3, 5, [200, 100, 50, 255]));
        assert_eq!((d.base.w, d.base.h), (7, 5));
        assert_eq!(d.crop, None);
        assert_eq!(d.base.get(0, 0), [10, 20, 30, 255]);
        assert_eq!(d.base.get(3, 1), [10, 20, 30, 255]);
        assert_eq!(d.base.get(4, 0), [200, 100, 50, 255]);
        assert_eq!(d.base.get(6, 4), [200, 100, 50, 255]);
        assert_eq!(d.base.get(0, 4)[3], 0, "below the shorter picture the canvas is see-through");
        assert!(d.undo());
        assert_eq!((d.base.w, d.base.h), (4, 2));
        assert_eq!(d.crop, Some(R { x0: 0.0, y0: 0.0, x1: 2.0, y1: 2.0 }));
    }

    #[test]
    fn the_canvas_grows_to_hold_what_sticks_out_and_only_that() {
        let base = R { x0: 0.0, y0: 0.0, x1: 100.0, y1: 50.0 };
        assert_eq!(grow_canvas(base, []), base);
        let inside = R { x0: 10.0, y0: 10.0, x1: 20.0, y1: 20.0 };
        assert_eq!(grow_canvas(base, [inside]), base);
        let out = R { x0: 90.0, y0: -7.0, x1: 130.0, y1: 20.0 };
        assert_eq!(grow_canvas(base, [inside, out]), R { x0: 0.0, y0: -7.0, x1: 130.0, y1: 50.0 });
        // Antialiasing fuzz at the edge is not a reason to grow.
        let fuzz = R { x0: -0.4, y0: 0.0, x1: 100.4, y1: 50.3 };
        assert_eq!(grow_canvas(base, [fuzz]), base);
    }

    #[test]
    fn the_canvas_has_limits() {
        let base = R { x0: 0.0, y0: 0.0, x1: 100.0, y1: 50.0 };
        let far = R { x0: 0.0, y0: 0.0, x1: 1.0e6, y1: 10.0 };
        assert_eq!(grow_canvas(base, [far]).x1, 100.0 + MAX_OVERHANG);
        let bad = R { x0: f32::NAN, y0: 0.0, x1: f32::INFINITY, y1: 10.0 };
        assert_eq!(grow_canvas(base, [bad]), base);
        // Too many pixels in all: the picture keeps its own size.
        let big = R { x0: 0.0, y0: 0.0, x1: 7000.0, y1: 7000.0 };
        let wide = R { x0: -MAX_OVERHANG, y0: -MAX_OVERHANG, x1: 7000.0 + MAX_OVERHANG, y1: 7000.0 + MAX_OVERHANG };
        assert_eq!(grow_canvas(big, [wide]), big);
    }
}
