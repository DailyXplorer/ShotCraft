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

/// Preview thumbnail window (0x100016228).
pub const THUMB_WIDTH: f32 = 204.0;
pub const THUMB_MAX_HEIGHT: f32 = 190.0;

/// Arrow line (0x1001af27c, data 0x1002527d8): capped to a tenth of the length, and it grows with
/// the arrow as sqrt((length + 550) / 600), up to 1.5 times.
pub const ARROW_WIDTHS: [f32; 10] = [1., 2., 3., 4., 6., 10., 14., 20., 28., 40.];
pub const ARROW_LINE_DIVISOR: f32 = 10.0;
pub const ARROW_GROW_BASE: f32 = 550.0;
pub const ARROW_GROW_DIVISOR: f32 = 600.0;
pub const ARROW_GROW_MAX: f32 = 1.5;
/// Thin arrow head (0x10007ebbc): branch length, capped to a quarter of the arrow.
pub const ARROW_BRANCH_SCALE: f32 = 0.2;
pub const ARROW_BRANCH_ROOT: f32 = 7.0;
pub const ARROW_BRANCH_LINE: f32 = 0.7;
pub const ARROW_BRANCH_MAX: f32 = 0.25;
pub const ARROW_SHORT: f32 = 20.0;
pub const ARROW_SPREAD: f32 = 0.8726646;
pub const ARROW_SPREAD_SHORT: f32 = 0.78539815;
/// Hand-drawn arrow line (0x1001af27c).
pub const ARROW_HAND_GAIN: f32 = 0.7;
pub const ARROW_HAND_BASE: f32 = 1.3;
pub const ARROW_HAND_DIVISOR: f32 = 15.0;
/// Hand-drawn arrow shaft (0x1001b0adc): stops short of the tip, its curve handles sit a third of
/// the way in and lean by a little noise, and the line thickens from the tail to the head.
pub const ARROW_HAND_STOP: f32 = 0.8;
pub const ARROW_HAND_END: f32 = 1.2;
pub const ARROW_HAND_HANDLE: f32 = 0.33;
pub const ARROW_HAND_LEAN_START: f32 = 0.06;
pub const ARROW_HAND_LEAN_END: f32 = 0.02;
/// Solid arrow (0x1001af27c, 0x10007e35c, data 0x100252760): drawn in units of the size step.
/// Past 520 the head grows and the tail shrinks, down to 0.8; short arrows shrink as a whole.
pub const ARROW_UNITS: [f32; 10] = [0.33, 0.5, 0.7, 0.9, 1.1, 1.4, 2.0, 2.8, 4.2, 6.4];
pub const ARROW_LONG: f32 = 520.0;
pub const ARROW_LONG_RANGE: f32 = 1000.0;
pub const ARROW_LONG_MIN: f32 = 0.8;
pub const ARROW_SPAN: f32 = 56.0;
pub const ARROW_SPAN_LIMIT: f32 = 100.0;
pub const ARROW_SHORT_LIMIT: f32 = 200.0;
/// Proportions of the default solid arrow, in units, measured from 0x100249010.
pub const ARROW_HEAD_LENGTH: f32 = 48.0;
pub const ARROW_HEAD_HALF: f32 = 22.0;
pub const ARROW_NECK_HALF: f32 = 9.0;
pub const ARROW_TAIL_HALF: f32 = 7.0;

/// Text (0x100068230): line height, the larger Goofy font, and the luminance above which a
/// filled label turns black and the weight drops from medium to regular.
pub const TEXT_LINE_HEIGHT: f32 = 1.15;
pub const TEXT_GOOFY_SIZE: f32 = 1.2;
pub const TEXT_GOOFY_LINE_HEIGHT: f32 = 1.324;
pub const TEXT_LIGHT_ABOVE: f32 = 0.6;

/// Plain oval (0x10008a8c4): eight curves as (end, control 1, control 2) in the unit square, from
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

/// Rectangle and oval (0x100088084): the line never exceeds a tenth of the short side, and below
/// that side the hand-drawn brush falls back to a plain stroke.
pub const SHAPE_LINE_DIVISOR: f32 = 10.0;
pub const SHAPE_HAND_MIN_SIDE: f32 = 40.0;
/// Fill segments are -1, 0, 40 and 100 percent (0x10008945c, 0x100089664).
pub const FILL_TRANSLUCENT: f32 = 0.4;
/// Plain corners are twice the line (0x100089adc); hand-drawn ones start from six times the line.
pub const CORNER_LINE_RATIO: f32 = 2.0;
pub const HAND_CORNER_LINE_RATIO: f32 = 6.0;
pub const HAND_CORNER_GROWTH: f32 = 1.1;
pub const HAND_CORNER_DAMPING: f32 = 0.004;
/// Hand-drawn outline (0x100089fec): side bow and corner jitter.
pub const HAND_WOBBLE: f32 = 1.3;
pub const HAND_JITTER_DIVISOR: f32 = 400.0;
pub const HAND_BOW_DIVISOR: f32 = 80000.0;
pub const HAND_BOW_MAX: f32 = 0.7;
pub const CORNER_MAX_RATIO: f32 = 3.0;
/// Hand-drawn brush (0x10008b8a0, 0x100100498, 0x100100e3c, 0x10008b3f8): width varies by
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
/// Curves are flattened every two points (0x1000ffb9c).
pub const BRUSH_SPACING: f32 = 2.0;
pub const BRUSH_STEPS_PER_LENGTH: f32 = 1.2;
/// Hand-drawn oval (0x100089c20): stops a little short of or past the full turn, spiralling.
pub const OVAL_SAMPLES: usize = 1000;
pub const OVAL_TURN_MIN: f32 = 0.98;
pub const OVAL_TURN_RANGE: f32 = 0.06;
pub const OVAL_DRIFT: f32 = 2.6;
pub const OVAL_OVERLAP_LINES: f32 = 5.0;

/// Canvas checkerboard (0x10007b654): squares of 8, (background, square) grey levels per theme.
pub const CHECKER_SQUARE: f32 = 8.0;
pub const CHECKER_LIGHT: (f32, f32) = (1.0, 0.95);
pub const CHECKER_DARK: (f32, f32) = (0.165, 0.2);

/// Blur tool (0x1001087b0): block = round(min(round(side / 6), 5 × scale) × 1.35^size), and past
/// side / 7 each block may take a neighbour's colour so the text underneath can't be rebuilt.
pub const PIXEL_SIDE_DIVISOR: f32 = 6.0;
pub const PIXEL_RETINA_CAP: f32 = 5.0;
pub const PIXEL_GROWTH: f32 = 1.35;
pub const PIXEL_SHUFFLE_DIVISOR: f32 = 7.0;
/// Ruler (0x100027f34, 0x10009da34): pixels match while |dr|+|dg|+|db| <= 2 × sensitivity².
pub const RULER_SENSITIVITY: f32 = 3.54;
pub const RULER_SENSITIVITY_MAX: f32 = 20.0;
/// Zoom (0x100012780 keys, 0x100012b54 wheel, 0x100018bac wheel notch).
pub const ZOOM_KEY_STEP: f32 = 1.414;
pub const ZOOM_WHEEL_STEP: f32 = 1.1892;
pub const ZOOM_WHEEL_NOTCH: f32 = 10.0;
pub const ZOOM_MAX: f32 = 64.0;
/// Counter label (0x1000778a0): diameter × 0.5 × 1.1, × 0.9 from 10 upwards; black above 0.6 brightness.
pub const COUNTER_LABEL_RATIO: f32 = 0.5 * 1.1;
pub const COUNTER_LABEL_TWO_DIGITS: f32 = 0.9;
pub const COUNTER_DARK_TEXT_ABOVE: f32 = 0.6;
/// Selection colour (0x1000778a0).
pub const SELECTION_RGB: (f32, f32, f32) = (0.0, 0.495, 0.85);
/// Mesh gradient (0x100035f0c): one noise octave, blur radius 0.0375 × map size, seed offset 1000.
pub const MESH_BLUR: f32 = 0.0375;
pub const MESH_SEED_BASE: u64 = 1000;
pub const MESH_PERIODS: f32 = 1.3;
pub const MESH_MAP_SIZE: u32 = 256;
/// Spotlight (0x1000bb168): black at the `transparency` setting's default of 0.214.
pub const SPOTLIGHT_DIM: f32 = 0.214;
/// Magnifier (0x1000397a8, 0x100039a88): the lens shows the source at 3×, centred 2.2 widths
/// right and 1.4 heights above it, joined to it by a line.
pub const MAGNIFIER_ZOOM: f32 = 3.0;
pub const MAGNIFIER_LENS: P = P { x: 2.2, y: -1.4 };
pub const DARKEST_AREA: i32 = 20;

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
    /// Pixels per point of the capture: every size table is in points (0x100048538, 0x10006af98, 0x100077b54, 0x1001b30e4).
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
            scale: 1.0,
        }
    }
}

impl Style {
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
        }
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        match &mut self.kind {
            Kind::Arrow { a, b, .. } | Kind::Line { a, b } | Kind::Highlighter { a, b } | Kind::Ruler { a, b } => {
                *a = a.add(dx, dy);
                *b = b.add(dx, dy);
            }
            Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } => {
                *r = r.translate(dx, dy)
            }
            Kind::Text { at, .. } | Kind::Counter { at, .. } => *at = at.add(dx, dy),
            Kind::Freehand { pts } => pts.iter_mut().for_each(|q| *q = q.add(dx, dy)),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.kind {
            Kind::Arrow { a, b, .. } | Kind::Line { a, b } | Kind::Highlighter { a, b } | Kind::Ruler { a, b } => a.dist(*b) < 3.0,
            Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } => r.w() < 3.0 || r.h() < 3.0,
            Kind::Text { text, .. } => text.trim().is_empty(),
            Kind::Freehand { pts } => pts.len() < 2,
            Kind::Counter { .. } => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// Gradient templates, colours and stops as the reference stores them.
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Backdrop {
    pub gradient: usize,
    pub kind: GradientKind,
    pub padding: f32,
    pub shadow: f32,
    pub inset: f32,
    pub outer_radius: f32,
    pub inner_radius: f32,
    pub ratio: Option<f32>,
    pub reverse: bool,
}

impl Default for Backdrop {
    fn default() -> Self {
        Backdrop {
            gradient: 0,
            kind: GradientKind::Mesh,
            padding: 12.0,
            shadow: 20.0,
            inset: 12.0,
            outer_radius: 12.0,
            inner_radius: 20.0,
            ratio: None,
            reverse: false,
        }
    }
}

/// Backdrop sliders run 0…20 (Main storyboard, view t3D-RU-alS); these turn a slider position into pixels.
pub const BACKDROP_SLIDER_MAX: f32 = 20.0;

impl Backdrop {
    /// slidePadding → 0x10014bdfc: round(v² × 0.5)
    pub fn padding_px(&self) -> f32 {
        (self.padding * self.padding * 0.5).round()
    }
    /// slideShadow → 0x1001527fc: v ÷ 20
    pub fn shadow_amount(&self) -> f32 {
        self.shadow / 20.0
    }
    /// slideInset → 0x100152a14: round(v² ÷ 1.8)
    pub fn inset_px(&self) -> f32 {
        (self.inset * self.inset / 1.8).round()
    }
    /// slideOuterRadius → 0x100152d1c: v × 2
    pub fn outer_radius_px(&self) -> f32 {
        self.outer_radius * 2.0
    }
    /// slideInnerRadius → 0x100152f30: v × 2
    pub fn inner_radius_px(&self) -> f32 {
        self.inner_radius * 2.0
    }
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

/// ⌘+ / ⌘-: doubles or halves below 4×, above that ×/÷ 1.414 rounded to an even number (0x100012780).
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

/// One wheel notch ×/÷ 1.1892, then snapped to round levels (0x100012b54).
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

#[derive(Clone)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    /// Straight (not premultiplied) RGBA rows.
    pub px: Arc<Vec<u8>>,
}

impl Image {
    pub fn get(&self, x: i32, y: i32) -> Rgba {
        let x = x.clamp(0, self.w as i32 - 1) as usize;
        let y = y.clamp(0, self.h as i32 - 1) as usize;
        let i = (y * self.w as usize + x) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }
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
        Snapshot { base: self.base.clone(), objs: self.objs.clone(), crop: self.crop, backdrop: self.backdrop }
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

    fn restore(&mut self, s: Snapshot) {
        self.base = s.base;
        self.objs = s.objs;
        self.crop = s.crop;
        self.backdrop = s.backdrop;
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

    pub fn next_counter(&self) -> u32 {
        self.objs.iter().filter_map(|o| if let Kind::Counter { n, .. } = o.kind { Some(n) } else { None }).max().unwrap_or(0) + 1
    }
}
