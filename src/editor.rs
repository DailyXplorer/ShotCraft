use crate::icons::{self, Icon};
use crate::model::*;
use crate::render;
use crate::selection::{self, snap45, square, Corner};
use crate::snap;
use crate::tiled::{Alpha, TiledTexture};
use crate::{Action, App};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Key, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use std::collections::HashMap;
use tiny_skia::Pixmap;

pub use crate::sizing::{OPTIONS_H, STATUS_H, TOOLBAR_H};
use crate::sizing::{seg_group_w, toolbar_cluster_left, toolbar_fit, TOOLBAR_DRAG_W, TOOLBAR_EDGE, TOOLBAR_GROUP_GAP as GROUP_GAP, TOOLBAR_SEG_W as SEG_W};
/// Narrowest width the editor window can have.
pub const MIN_W: f32 = crate::sizing::EDITOR_MIN_SIZE.0;
pub const MIN_H: f32 = crate::sizing::EDITOR_MIN_SIZE.1;

#[derive(Clone, Copy)]
pub struct Th {
    pub bar: Color32,
    pub bar2: Color32,
    pub canvas: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub hover: Color32,
    pub seg: Color32,
    pub line: Color32,
}

pub fn th(dark: bool) -> Th {
    if dark {
        Th {
            bar: Color32::from_rgb(0x2C, 0x2C, 0x2E),
            bar2: Color32::from_rgb(0x25, 0x25, 0x27),
            canvas: Color32::from_rgb(0x1A, 0x1A, 0x1C),
            text: Color32::from_rgb(0xE8, 0xE8, 0xEA),
            dim: Color32::from_rgb(0x98, 0x98, 0x9D),
            accent: Color32::from_rgb(0x0A, 0x84, 0xFF),
            hover: Color32::from_rgb(0x3A, 0x3A, 0x3D),
            seg: Color32::from_rgb(0x36, 0x36, 0x39),
            line: Color32::from_rgb(0x44, 0x44, 0x48),
        }
    } else {
        Th {
            bar: Color32::from_rgb(0xEC, 0xEC, 0xEE),
            bar2: Color32::from_rgb(0xF6, 0xF6, 0xF7),
            canvas: Color32::from_rgb(0xD8, 0xD8, 0xDC),
            text: Color32::from_rgb(0x1D, 0x1D, 0x1F),
            dim: Color32::from_rgb(0x6E, 0x6E, 0x73),
            accent: Color32::from_rgb(0x00, 0x7A, 0xFF),
            hover: Color32::from_rgb(0xDC, 0xDC, 0xE0),
            seg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            line: Color32::from_rgb(0xC6, 0xC6, 0xCB),
        }
    }
}

fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    A,
    B,
    Corner(u8),
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    None,
    Create { start: P },
    Move { idx: usize, last: P },
    Handle { idx: usize, h: Handle, anchor: P },
    Select { start: P },
    Crop { start: P },
    Pan { last: Pos2 },
    /// Z + drag: zoom to the dragged area.
    Zoom { start: P },
}

#[derive(Clone, Copy, PartialEq)]
pub enum Popover {
    None,
    Color(Rect),
    Picker(Rect),
    Size(Rect),
    Zoom(Rect),
    Resize(Rect),
}

struct Comp {
    key: (u64, Option<usize>),
    /// Where the composite's pixel (0, 0) lies in picture coordinates: the canvas can reach past
    /// the picture's top and left edges.
    origin: P,
    pm: Pixmap,
    tex: TiledTexture,
}

struct Overlay {
    obj: Obj,
    version: u64,
    rect: R,
    tex: TiledTexture,
}

pub struct Editor {
    pub tool: Tool,
    /// Tool a new screenshot opens with; none keeps the one in use.
    pub default_tool: Option<Tool>,
    styles: HashMap<Tool, Style>,
    zoom: f32,
    pan: Vec2,
    fit: bool,
    prefer_100: bool,
    scale: f32,
    selected: Option<usize>,
    editing_text: bool,
    drag: Drag,
    pub selection: Option<R>,
    hover: Option<P>,
    pub(crate) hover_color: Rgba,
    comp: Option<Comp>,
    /// The canvas the document exports to (see `render::canvas_rect`), kept per document version.
    extent: Option<(u64, R)>,
    overlay: Option<Overlay>,
    preview: Option<(u64, (u32, u32), TiledTexture)>,
    pub popover: Popover,
    crop_draft: Option<R>,
    resize_w: String,
    resize_h: String,
    custom_color: Color32,
    canvas: Rect,
    options_w: f32,
    ruler_sensitivity: f32,
    wheel_acc: f32,
    checker: Option<(bool, egui::TextureHandle)>,
    /// Whether the picture has see-through pixels, for the document version in the first field.
    alpha: Option<(u64, bool)>,
    /// How far snapping has moved the dragged annotation off the pointer, to undo before the next step.
    snap_carry: (f32, f32),
    /// The alignment guides of the last snap, drawn while the drag lasts.
    snap_guides: Vec<snap::Guide>,
}

impl Default for Editor {
    fn default() -> Self {
        let mut styles = HashMap::new();
        let mut hl = Style::default();
        hl.color = hex(HIGHLIGHT_DEFAULT);
        styles.insert(Tool::Highlighter, hl);
        let shape = Style { fill: Fill::HandDrawn, ..Style::default() };
        styles.insert(Tool::Rectangle, shape);
        styles.insert(Tool::Oval, Style { shape: Shape::Oval, ..shape });
        Editor {
            tool: Tool::Select,
            default_tool: None,
            styles,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit: true,
            prefer_100: false,
            scale: 1.0,
            selected: None,
            editing_text: false,
            drag: Drag::None,
            selection: None,
            snap_carry: (0.0, 0.0),
            snap_guides: vec![],
            hover: None,
            hover_color: [255, 255, 255, 255],
            comp: None,
            extent: None,
            overlay: None,
            preview: None,
            popover: Popover::None,
            crop_draft: None,
            resize_w: String::new(),
            resize_h: String::new(),
            custom_color: Color32::RED,
            canvas: Rect::NOTHING,
            options_w: 0.0,
            ruler_sensitivity: RULER_SENSITIVITY,
            wheel_acc: 0.0,
            checker: None,
            alpha: None,
        }
    }
}

/// The style a tool had when the settings were saved. Anything out of range (a hand-edited file,
/// a newer version's value) falls back to `base`'s.
fn style_from_stored(st: &crate::settings::ToolStyle, base: &Style) -> Style {
    fn pick<T: Copy>(all: &[T], i: u8, fallback: T) -> T {
        all.get(i as usize).copied().unwrap_or(fallback)
    }
    let color = if st.color.len() == 7 && st.color.starts_with('#') && st.color[1..].chars().all(|c| c.is_ascii_hexdigit()) { hex(&st.color) } else { base.color };
    Style {
        color,
        size: if (st.size as usize) < LINE_WIDTHS.len() { st.size } else { base.size },
        dashed: st.dashed,
        arrow: pick(&ARROW_STYLES, st.arrow, base.arrow),
        fill: pick(&FILLS, st.fill, base.fill),
        shape: pick(&SHAPES, st.shape, base.shape),
        font: pick(&FONTS, st.font, base.font),
        blur: pick(&BlurMode::ALL, st.blur, base.blur),
        smooth: st.smooth.min(FREEHAND_SMOOTH_MAX),
        vary: st.vary.min(FREEHAND_VARY.len() as u8 - 1),
        round_cap: st.round_cap,
        dim: if st.dim.is_finite() { st.dim.clamp(0.0, 1.0) } else { base.dim },
        scale: base.scale,
    }
}

fn style_to_stored(s: &Style) -> crate::settings::ToolStyle {
    fn index<T: PartialEq>(all: &[T], v: &T) -> u8 {
        all.iter().position(|x| x == v).unwrap_or(0) as u8
    }
    crate::settings::ToolStyle {
        color: format!("#{:02X}{:02X}{:02X}", s.color[0], s.color[1], s.color[2]),
        size: s.size,
        dashed: s.dashed,
        arrow: index(&ARROW_STYLES, &s.arrow),
        fill: index(&FILLS, &s.fill),
        shape: index(&SHAPES, &s.shape),
        font: index(&FONTS, &s.font),
        blur: index(&BlurMode::ALL, &s.blur),
        smooth: s.smooth,
        vary: s.vary,
        round_cap: s.round_cap,
        dim: s.dim,
    }
}

// The order the stored choices count in. Append only: stored files point into these.
const ARROW_STYLES: [ArrowStyle; 4] = [ArrowStyle::Shape, ArrowStyle::HandDrawn, ArrowStyle::Line, ArrowStyle::Double];
const FILLS: [Fill; 4] = [Fill::HandDrawn, Fill::Transparent, Fill::Translucent, Fill::Opaque];
const SHAPES: [Shape; 2] = [Shape::Rect, Shape::Oval];
const FONTS: [FontKind; 2] = [FontKind::Sans, FontKind::Goofy];

impl Editor {
    pub fn reset_for_new_doc(&mut self, prefer_100: bool) {
        self.fit = true;
        self.prefer_100 = prefer_100;
        self.selected = None;
        self.editing_text = false;
        self.drag = Drag::None;
        self.selection = None;
        self.crop_draft = None;
        self.overlay = None;
        self.popover = Popover::None;
        if matches!(self.tool, Tool::Crop | Tool::Cut | Tool::Ocr | Tool::Backdrop) {
            self.tool = Tool::Select;
        }
        if let Some(t) = self.default_tool {
            self.tool = t;
        }
    }

    /// The editor as the settings left it: the last style of each tool.
    pub fn from_settings(settings: &crate::settings::Settings) -> Editor {
        let mut ed = Editor::default();
        ed.load_styles(settings);
        ed
    }

    /// Replaces the tools' styles by the factory ones with the stored ones on top.
    pub fn load_styles(&mut self, settings: &crate::settings::Settings) {
        self.styles = Editor::default().styles;
        for tool in Tool::TOOLBAR {
            if let Some(stored) = settings.tool_styles.get(tool.name()) {
                let base = self.styles.get(&tool).copied().unwrap_or_default();
                self.styles.insert(tool, style_from_stored(stored, &base));
            }
        }
        self.default_tool = Tool::TOOLBAR.into_iter().filter(|t| *t != Tool::Backdrop).find(|t| t.name() == settings.default_tool);
    }

    /// What `load_styles` would read back: the toolbar tools' styles, as the settings keep them.
    fn stored_styles(&self) -> std::collections::BTreeMap<String, crate::settings::ToolStyle> {
        Tool::TOOLBAR.into_iter().filter_map(|t| self.styles.get(&t).map(|s| (t.name().to_string(), style_to_stored(s)))).collect()
    }

    fn style_for(&self, t: Tool) -> Style {
        Style { scale: self.scale, ..self.styles.get(&t).copied().unwrap_or_default() }
    }

    fn to_screen(&self, q: P) -> Pos2 {
        self.canvas.min + self.pan + vec2(q.x, q.y) * self.zoom
    }

    /// The canvas of `d`: its picture, grown to hold annotations that reach past the edges.
    fn canvas_of(&mut self, d: &Doc, fonts: &render::Fonts) -> R {
        match self.extent {
            Some((v, r)) if v == d.version => r,
            _ => {
                let r = render::canvas_rect(d, fonts);
                self.extent = Some((d.version, r));
                r
            }
        }
    }

    /// What the editor shows and exports: the crop, else the canvas as of the last repaint.
    fn view_area(&self, d: &Doc) -> R {
        d.crop.unwrap_or_else(|| match self.extent {
            Some((v, r)) if v == d.version => r,
            _ => d.view_rect(),
        })
    }

    fn to_img(&self, s: Pos2) -> P {
        let v = (s - self.canvas.min - self.pan) / self.zoom;
        p(v.x, v.y)
    }

    pub(crate) fn rect_screen(&self, r: &R) -> Rect {
        Rect::from_min_max(self.to_screen(p(r.x0, r.y0)), self.to_screen(p(r.x1, r.y1)))
    }

    fn zoom_around(&mut self, factor: f32, around: Pos2) {
        let before = self.to_img(around);
        self.zoom = (self.zoom * factor).max(0.02);
        let after = self.to_screen(before);
        self.pan += around - after;
        self.fit = false;
    }
}

fn icon_btn(ui: &mut Ui, icon: Icon, selected: bool, tip: &str, t: &Th, w: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let bg = if selected {
        t.accent
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, bg);
    let fg = if selected { Color32::WHITE } else { t.text };
    icons::draw(ui.painter(), Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), icon, fg);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// The drag-out handle: pressing it and moving starts a native drag of the edited picture as a
/// file. True on the frame the drag begins.
fn drag_handle(ui: &mut Ui, t: &Th, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(TOOLBAR_DRAG_W, 28.0), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    ui.painter().rect_filled(rect, 7.0, t.seg);
    ui.painter().rect_stroke(rect, 7.0, Stroke::new(0.5, t.line), StrokeKind::Inside);
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect.shrink(2.0), 5.0, t.hover);
    }
    let ink = if enabled { t.text } else { t.dim };
    icons::draw(ui.painter(), Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), Icon::DragFile, ink);
    let tip = if enabled { "Drag the picture into another app or a folder" } else { "Nothing to drag yet" };
    let resp = resp.on_hover_text(tip);
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::Grab) } else { resp };
    enabled && resp.drag_started()
}

/// A row of icon segments framed like a macOS segmented control; returns the clicked index.
fn segments(ui: &mut Ui, items: &[(Icon, &str, bool)], t: &Th, seg_w: f32) -> Option<usize> {
    let w = seg_w * items.len() as f32 + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    ui.painter().rect_filled(rect, 7.0, t.seg);
    ui.painter().rect_stroke(rect, 7.0, Stroke::new(0.5, t.line), StrokeKind::Inside);
    let mut clicked = None;
    for (i, (icon, tip, sel)) in items.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + 2.0 + i as f32 * seg_w, rect.top() + 2.0), vec2(seg_w, 24.0));
        let resp = ui.interact(r, ui.id().with(("seg", tip, i)), Sense::click());
        let bg = if *sel {
            t.accent
        } else if resp.hovered() {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(r, 5.0, bg);
        icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(17.0, 17.0)), *icon, if *sel { Color32::WHITE } else { t.text });
        let resp = if tip.is_empty() { resp } else { resp.on_hover_text(*tip) };
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

fn text_segments(ui: &mut Ui, labels: &[&str], selected: usize, t: &Th) -> Option<usize> {
    let font = FontId::new(12.0, FontFamily::Proportional);
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), t.text).size().x) + 18.0)
        .collect();
    let (rect, _) = ui.allocate_exact_size(vec2(widths.iter().sum::<f32>() + 4.0, 28.0), Sense::hover());
    ui.painter().rect_filled(rect, 7.0, t.seg);
    ui.painter().rect_stroke(rect, 7.0, Stroke::new(0.5, t.line), StrokeKind::Inside);
    let mut x = rect.left() + 2.0;
    let mut clicked = None;
    for (i, l) in labels.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(widths[i], 24.0));
        let resp = ui.interact(r, ui.id().with(("tseg", l, i)), Sense::click());
        let sel = i == selected;
        ui.painter().rect_filled(r, 5.0, if sel { t.accent } else if resp.hovered() { t.hover } else { Color32::TRANSPARENT });
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *l, font.clone(), if sel { Color32::WHITE } else { t.text });
        if resp.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}

/// Frame of the floating panels (Keyboard Shortcuts, About): the bar colour, fully opaque, so the
/// editor behind never shows through.
fn panel_frame(ctx: &egui::Context, t: &Th) -> egui::Frame {
    let mut frame = egui::Frame::window(&ctx.global_style());
    frame.fill = Color32::from_rgb(t.bar.r(), t.bar.g(), t.bar.b());
    frame.stroke = Stroke::new(1.0, t.line);
    frame
}

/// One row of the toolbar's overflow menu: icon, name, highlighted when it is the active tool.
fn menu_row(ui: &mut Ui, icon: Icon, label: &str, selected: bool, enabled: bool, t: &Th) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(140.0), 26.0), if enabled { Sense::click() } else { Sense::hover() });
    let fg = if !enabled {
        t.dim
    } else if selected {
        Color32::WHITE
    } else {
        t.text
    };
    if selected {
        ui.painter().rect_filled(rect, 6.0, t.accent);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 6.0, t.hover);
    }
    icons::draw(ui.painter(), Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(17.0, 17.0)), icon, fg);
    ui.painter().text(pos2(rect.left() + 36.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::new(13.0, FontFamily::Proportional), fg);
    resp
}

fn hit(obj: &Obj, q: P, tol: f32, fonts: &render::Fonts) -> bool {
    let seg_dist = |a: P, b: P| render::seg_dist(q, a, b);
    let w = obj.style.width();
    match &obj.kind {
        Kind::Arrow { a, b, seed } => render::arrow_hit(*a, *b, *seed, &obj.style, q, tol),
        Kind::Line { a, b } | Kind::Ruler { a, b } => seg_dist(*a, *b) <= w * 1.5 + tol,
        Kind::Highlighter { a, b } => seg_dist(*a, *b) <= obj.style.highlight_size() / 2.0 + tol,
        Kind::Freehand { pts } if pts.len() == 1 => pts[0].dist(q) <= w / 2.0 + tol,
        Kind::Freehand { pts } => pts.windows(2).any(|s| seg_dist(s[0], s[1]) <= w + tol),
        Kind::Rect { r, .. } | Kind::Oval { r, .. } => {
            if matches!(obj.style.fill, Fill::Translucent | Fill::Opaque) {
                r.expand(tol).contains(q)
            } else {
                r.expand(w + tol).contains(q) && !r.expand(-(w + tol)).contains(q)
            }
        }
        _ => render::bounds(obj, fonts).expand(tol).contains(q),
    }
}

fn handles(obj: &Obj) -> Vec<(Handle, P)> {
    match &obj.kind {
        Kind::Arrow { a, b, .. } | Kind::Line { a, b } | Kind::Ruler { a, b } | Kind::Highlighter { a, b } => vec![(Handle::A, *a), (Handle::B, *b)],
        Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } | Kind::Image { r, .. } => vec![
            (Handle::Corner(0), p(r.x0, r.y0)),
            (Handle::Corner(1), p(r.x1, r.y0)),
            (Handle::Corner(2), p(r.x1, r.y1)),
            (Handle::Corner(3), p(r.x0, r.y1)),
        ],
        _ => vec![],
    }
}

fn set_end(kind: &mut Kind, start: P, q: P, shift: bool) {
    match kind {
        Kind::Arrow { b, .. } | Kind::Line { b, .. } | Kind::Ruler { b, .. } => *b = if shift { snap45(start, q) } else { q },
        Kind::Highlighter { b, .. } => *b = if shift { q } else { p(q.x, start.y) },
        Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } | Kind::Image { r, .. } => {
            *r = R::from_points(start, if shift { square(start, q) } else { q })
        }
        Kind::Freehand { pts } => {
            if pts.last().map_or(true, |l| l.dist(q) > 0.8) {
                pts.push(q)
            }
        }
        Kind::Counter { at, .. } | Kind::Text { at, .. } => *at = q,
    }
}

fn new_kind(tool: Tool, q: P, n: u32) -> Option<Kind> {
    let z = R::from_points(q, q);
    Some(match tool {
        Tool::Arrow => Kind::Arrow { a: q, b: q, seed: next_seed() },
        Tool::Line => Kind::Line { a: q, b: q },
        Tool::Ruler => Kind::Ruler { a: q, b: q },
        Tool::Highlighter => Kind::Highlighter { a: q, b: q },
        Tool::Rectangle => Kind::Rect { r: z, seed: next_seed() },
        Tool::Oval => Kind::Oval { r: z, seed: next_seed() },
        Tool::Blur => Kind::Blur { r: z, text: None },
        Tool::Spotlight => Kind::Spotlight { r: z },
        Tool::Magnifier => Kind::Magnifier { r: z, lens: MAGNIFIER_LENS },
        Tool::Freehand => Kind::Freehand { pts: vec![q] },
        Tool::Counter => Kind::Counter { at: q, n },
        Tool::Text => Kind::Text { at: q, text: String::new() },
        _ => return None,
    })
}

/// Pictures taller than this many times their width open fitted to the width.
const TALL_ASPECT: f32 = 3.0;

const TEX: egui::TextureOptions = egui::TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

impl App {
    pub fn editor_action(&mut self, a: Action) {
        let ppp = self.ctx.pixels_per_point();
        let center = self.ed.canvas.center();
        match a {
            Action::Undo | Action::Redo => {
                if let Some(d) = &mut self.doc {
                    let before = (d.base.w, d.base.h, d.crop);
                    if matches!(a, Action::Undo) {
                        d.undo();
                    } else {
                        d.redo();
                    }
                    self.ed.selected = None;
                    self.ed.editing_text = false;
                    if matches!(self.ed.drag, Drag::Move { .. } | Drag::Handle { .. }) {
                        self.ed.drag = Drag::None;
                        self.ed.snap_guides.clear();
                    }
                    // A step that changed the picture's size or crop (Add Capture, Rasterize, Cut Out,
                    // Crop) moves what the old area and crop frame refer to: drop them.
                    if before != (d.base.w, d.base.h, d.crop) {
                        self.ed.selection = None;
                        self.ed.crop_draft = None;
                        self.ed.fit = true;
                    }
                }
            }
            Action::ResetCrop => {
                if let Some(d) = &mut self.doc {
                    if d.crop.is_some() {
                        d.checkpoint();
                        d.crop = None;
                        self.ed.fit = true;
                    }
                }
            }
            Action::Rasterize => {
                if let Some(d) = &mut self.doc {
                    d.checkpoint();
                    render::rasterize(d, &self.fonts);
                    self.ed.selected = None;
                    // A grown canvas moved the picture's origin: nothing in the old coordinates holds.
                    self.ed.selection = None;
                    self.ed.fit = true;
                }
            }
            Action::ZoomIn | Action::ZoomOut => {
                let z = self.ed.zoom * ppp;
                let target = zoom_key_target(z, matches!(a, Action::ZoomIn));
                self.ed.zoom_around(target / z, center);
            }
            Action::ZoomFit => self.ed.fit = true,
            Action::Zoom100 => {
                let f = (1.0 / ppp) / self.ed.zoom;
                self.ed.zoom_around(f, center);
            }
            Action::ZoomSelection | Action::SelectionTopLeft | Action::SelectionBottomRight => {
                let Some(r) = self.ed.selection else {
                    self.say("Select an area first");
                    return;
                };
                let canvas = (self.ed.canvas.width(), self.ed.canvas.height());
                let (zoom, pan) = match a {
                    Action::ZoomSelection => selection::zoom_to_rect(canvas, r, ppp),
                    Action::SelectionTopLeft => selection::zoom_to_corner(canvas, self.ed.zoom, r, Corner::TopLeft, ppp),
                    _ => selection::zoom_to_corner(canvas, self.ed.zoom, r, Corner::BottomRight, ppp),
                };
                self.ed.zoom = zoom;
                self.ed.pan = vec2(pan.0, pan.1);
                self.ed.fit = false;
            }
            Action::Tool(t) => self.set_tool(t),
            _ => {}
        }
    }

    /// For the menus: which plain keys the editor itself takes now, so the Draw items that share a
    /// letter with them step aside. Mirrors the conditions in `keyboard`.
    pub fn editor_key_state(&self) -> crate::menu_model::MenuState {
        let area = self.ed.selection.is_some();
        crate::menu_model::MenuState {
            typing: self.ed.editing_text,
            c_copies_colour: self.ed.tool == Tool::Select && area,
            area_keys: self.ed.tool == Tool::Select && self.ed.selected.is_none() && area,
            has_area: area,
            ..Default::default()
        }
    }

    fn set_tool(&mut self, t: Tool) {
        self.finish_text();
        self.ed.selected = None;
        self.ed.popover = Popover::None;
        if t == Tool::Backdrop {
            if self.doc.as_ref().map_or(false, |d| d.backdrop.is_none()) {
                if self.backdrop_allowed() {
                    let bd = self.new_backdrop();
                    if let Some(d) = &mut self.doc {
                        d.checkpoint();
                        d.backdrop = Some(bd);
                    }
                } else {
                    return;
                }
            }
        }
        if t == Tool::Crop {
            self.ed.crop_draft = self.ed.selection.take().or(self.doc.as_ref().and_then(|d| d.crop));
        }
        self.ed.tool = if self.ed.tool == t && t != Tool::Select { Tool::Select } else { t };
        if self.ed.tool != Tool::Crop {
            self.ed.crop_draft = None;
        }
    }

    fn finish_text(&mut self) {
        if !self.ed.editing_text {
            return;
        }
        self.ed.editing_text = false;
        if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
            if d.objs.get(i).map_or(false, |o| o.is_empty()) {
                d.objs.remove(i);
                d.touch();
                self.ed.selected = None;
            }
        }
    }

    /// Stops a drag of an object whose index may no longer mean the same one.
    fn cancel_object_drag(&mut self) {
        if matches!(self.ed.drag, Drag::Move { .. } | Drag::Handle { .. }) {
            self.ed.drag = Drag::None;
            self.ed.snap_guides.clear();
        }
    }

    fn delete_selected(&mut self) {
        if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
            if i < d.objs.len() {
                d.checkpoint();
                d.objs.remove(i);
            }
            self.ed.selected = None;
            self.ed.editing_text = false;
            self.cancel_object_drag();
        }
    }

    fn apply_crop(&mut self) {
        if let (Some(d), Some(r)) = (&mut self.doc, self.ed.crop_draft) {
            let r = r.clamp_to(d.base.w as f32, d.base.h as f32);
            if r.w() >= 2.0 && r.h() >= 2.0 {
                d.checkpoint();
                d.crop = Some(R { x0: r.x0.round(), y0: r.y0.round(), x1: r.x1.round(), y1: r.y1.round() });
            }
        }
        self.ed.crop_draft = None;
        self.ed.tool = Tool::Select;
        self.ed.fit = true;
    }

    fn cut_out(&mut self, r: R) {
        if let Some(d) = &mut self.doc {
            d.checkpoint();
            // Rasterizing onto a grown canvas moves the origin: the selection moves with it.
            let cv = render::canvas_rect(d, &self.fonts);
            let r = r.translate(-cv.x0, -cv.y0);
            render::rasterize(d, &self.fonts);
            d.base = render::cut_out(&d.base, &r);
            d.crop = None;
            self.ed.selected = None;
            self.ed.fit = true;
        }
    }

    fn resize_doc(&mut self, factor: f32) {
        if let Some(d) = &mut self.doc {
            if (factor - 1.0).abs() < 1e-3 {
                return;
            }
            let cv = render::canvas_rect(d, &self.fonts);
            if resize_too_large(cv.w(), cv.h(), factor) {
                return self.error("The picture would be too large: resize it by a smaller factor.");
            }
            render::rasterize(d, &self.fonts);
            if let Some(c) = d.crop.take() {
                let pm = render::crop_pixmap(&render::to_pixmap(&d.base), &c);
                d.base = render::from_pixmap(&pm);
            }
            d.base = render::resize(&d.base, factor);
            d.undo.clear();
            d.redo.clear();
            d.touch();
            self.ed.selected = None;
            self.ed.fit = true;
        }
    }

    fn update_style(&mut self, f: impl Fn(&mut Style)) {
        let tool = self.context_tool();
        if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
            if let Some(o) = d.objs.get(i) {
                let mut st = o.style;
                f(&mut st);
                if st != o.style {
                    d.checkpoint();
                    d.objs[i].style = st;
                }
            }
        }
        let mut st = self.ed.style_for(tool);
        f(&mut st);
        self.ed.styles.insert(tool, st);
    }

    fn context_tool(&self) -> Tool {
        match (&self.doc, self.ed.selected) {
            (Some(d), Some(i)) if i < d.objs.len() => d.objs[i].tool(),
            _ => self.ed.tool,
        }
    }

    fn context_style(&self) -> Style {
        match (&self.doc, self.ed.selected) {
            (Some(d), Some(i)) if i < d.objs.len() => d.objs[i].style,
            _ => self.ed.style_for(self.ed.tool),
        }
    }

    fn keyboard(&mut self, ui: &mut Ui) {
        if self.doc.is_none() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        if self.ed.editing_text {
            let Some(i) = self.ed.selected else {
                self.ed.editing_text = false;
                return;
            };
            let mut stop = false;
            if let Some(d) = &mut self.doc {
                for e in &events {
                    let Some(Obj { kind: Kind::Text { text, .. }, .. }) = d.objs.get_mut(i) else {
                        stop = true;
                        break;
                    };
                    match e {
                        egui::Event::Text(s) => {
                            text.push_str(s);
                            d.version = next_version();
                        }
                        egui::Event::Key { key: Key::Backspace, pressed: true, .. } => {
                            text.pop();
                            d.version = next_version();
                        }
                        egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } => {
                            if modifiers.command {
                                stop = true;
                            } else {
                                text.push('\n');
                                d.version = next_version();
                            }
                        }
                        egui::Event::Key { key: Key::Escape, pressed: true, .. } => stop = true,
                        egui::Event::Paste(s) => {
                            text.push_str(s);
                            d.version = next_version();
                        }
                        // Cmd+C: the whole text (there is no text selection to copy from).
                        egui::Event::Copy => crate::system::copy_text(text),
                        _ => {}
                    }
                }
            }
            if stop {
                self.finish_text();
            }
            return;
        }
        // Cmd+V may arrive as a paste event and as a key in one frame: it pastes once.
        let mut pasted = false;
        for e in &events {
            // Cmd+C reaches egui as `Event::Copy` and never as a key (egui-winit swallows the key event),
            // so this is the only place the shortcut is seen.
            if matches!(e, egui::Event::Copy) {
                self.push(Action::Copy);
                continue;
            }
            // Cmd+V reaches egui as `Event::Paste` when the clipboard holds text, as nothing when it
            // holds a picture (the Edit ▸ Paste menu item takes that one); the picture is wanted here.
            if matches!(e, egui::Event::Paste(_)) {
                if !std::mem::replace(&mut pasted, true) {
                    self.push(Action::Paste);
                }
                continue;
            }
            let egui::Event::Key { key, pressed: true, modifiers, repeat, .. } = e else { continue };
            let (cmd, shift, alt) = (modifiers.command, modifiers.shift, modifiers.alt);
            let plain = !cmd && !modifiers.ctrl && !alt;
            let sel_keys = self.ed.tool == Tool::Select && self.ed.selected.is_none() && self.ed.selection.is_some();
            let tool_now = self.context_tool();
            match key {
                Key::Z if cmd && shift => self.push(Action::Redo),
                Key::Z if cmd => self.push(Action::Undo),
                Key::S if cmd && shift => self.push(Action::SaveAs),
                Key::S if cmd => self.push(Action::Save),
                Key::V if cmd && !shift && !alt => {
                    if !std::mem::replace(&mut pasted, true) {
                        self.push(Action::Paste);
                    }
                }
                Key::P if cmd && !shift && !alt => self.push(Action::Pin),
                Key::O if cmd && !shift => self.set_tool(Tool::Ocr),
                Key::Equals | Key::Plus if cmd => self.push(Action::ZoomIn),
                Key::Minus if cmd => self.push(Action::ZoomOut),
                Key::Num0 if cmd && !shift && !alt => self.push(Action::Zoom100),
                Key::Num1 if cmd && !shift && !alt => self.push(Action::ZoomFit),
                Key::Num2 if cmd && !shift && !alt => self.push(Action::ZoomSelection),
                Key::Q if plain && self.ed.selection.is_some() => self.push(Action::SelectionTopLeft),
                Key::W if plain && self.ed.selection.is_some() => self.push(Action::SelectionBottomRight),
                Key::A if plain && sel_keys => self.smart_select(),
                Key::B if plain && sel_keys => self.blur_selection(),
                Key::W if cmd && !shift && !alt => self.push(Action::Close),
                Key::Tab => {
                    if let (Some(q), Some(d)) = (self.ed.hover, &self.doc) {
                        let c = if shift { render::darkest_near(&d.base, q) } else { self.ed.hover_color };
                        let s = render::format_color(c, self.settings.color_format);
                        crate::system::copy_text(&s);
                        self.say(format!("{s} copied"));
                    }
                }
                Key::Escape => {
                    if self.ed.popover != Popover::None {
                        self.ed.popover = Popover::None;
                    } else if self.ed.tool == Tool::Crop {
                        self.ed.crop_draft = None;
                        self.ed.tool = Tool::Select;
                    } else if self.ed.selected.is_some() {
                        self.ed.selected = None;
                    } else if self.ed.selection.is_some() {
                        self.ed.selection = None;
                    } else if self.ed.tool != Tool::Select {
                        self.ed.tool = Tool::Select;
                    } else {
                        if self.settings.esc_copy {
                            self.copy();
                        }
                        if self.settings.esc_save {
                            self.save();
                        }
                        self.push(Action::Close);
                    }
                }
                Key::Enter if cmd && tool_now == Tool::Freehand => self.update_style(|st| *st = st.next_variability()),
                Key::Enter if cmd && tool_now == Tool::Highlighter => self.update_style(|st| *st = st.next_cap()),
                Key::Enter if self.ed.tool == Tool::Crop => self.apply_crop(),
                Key::Enter if plain && sel_keys => self.crop_selection(),
                Key::Backspace | Key::Delete => {
                    if self.ed.selected.is_some() {
                        self.delete_selected();
                    } else if let (Some(r), Some(d)) = (self.ed.selection, &mut self.doc) {
                        d.checkpoint();
                        let mut st = Style::default();
                        st.blur = BlurMode::Erase;
                        d.objs.push(Obj { kind: Kind::Blur { r, text: None }, style: st });
                        self.ed.selection = None;
                    }
                }
                Key::ArrowUp | Key::ArrowDown if alt && !cmd && tool_now == Tool::Freehand => {
                    let up = *key == Key::ArrowUp;
                    self.update_style(|st| *st = st.smoother(up));
                }
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown if self.ed.selected.is_none() && !alt => {
                    if let Some(dir) = selection::Arrow::from_key(*key) {
                        self.edit_selection(|r, bounds| selection::arrow_edit(r, dir, cmd, shift, bounds));
                    }
                }
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown if self.ed.selected.is_some() => {
                    let s = if shift { 10.0 } else { 1.0 };
                    let (dx, dy) = match key {
                        Key::ArrowLeft => (-s, 0.0),
                        Key::ArrowRight => (s, 0.0),
                        Key::ArrowUp => (0.0, -s),
                        _ => (0.0, s),
                    };
                    if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
                        if !repeat {
                            d.checkpoint();
                        } else {
                            d.touch();
                        }
                        d.objs[i].translate(dx, dy);
                    }
                }
                Key::OpenBracket | Key::CloseBracket | Key::OpenCurlyBracket | Key::CloseCurlyBracket if !cmd && !alt => {
                    if let Some(n) = selection::bracket_delta(*key, shift) {
                        self.edit_selection(|r, bounds| selection::grow(r, n, bounds));
                    }
                }
                k if plain && selection::digit(*k).is_some() && tool_now == Tool::Spotlight => {
                    let st = self.ed.style_for(Tool::Spotlight).with_dim(selection::digit(*k).unwrap());
                    self.ed.styles.insert(Tool::Spotlight, st);
                    if let Some(d) = &mut self.doc {
                        d.set_spotlight_dim(st.dim);
                    }
                }
                Key::P if !cmd => self.settings.logical_points = !self.settings.logical_points,
                Key::C if !cmd && self.ed.tool == Tool::Select && self.ed.selection.is_some() => {
                    if let (Some(r), Some(d)) = (self.ed.selection, &self.doc) {
                        let s = render::format_color(render::average(&d.base, &r), self.settings.color_format);
                        crate::system::copy_text(&s);
                        self.say(format!("Average color {s} copied"));
                    }
                }
                _ if !cmd && !modifiers.ctrl && !modifiers.alt => {
                    let name = format!("{key:?}").to_lowercase();
                    if name.len() == 1 {
                        if let Some(t) = Tool::from_key(name.chars().next().unwrap()) {
                            self.set_tool(t);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The selection (or the crop frame while cropping) edited by the keyboard; `f` gets the area
    /// and the bounds of the picture.
    fn edit_selection(&mut self, f: impl Fn(R, R) -> R) {
        let Some(d) = &self.doc else { return };
        let bounds = R { x0: 0.0, y0: 0.0, x1: d.base.w as f32, y1: d.base.h as f32 };
        let slot = if self.ed.tool == Tool::Crop { &mut self.ed.crop_draft } else { &mut self.ed.selection };
        if let Some(r) = *slot {
            *slot = Some(f(r, bounds));
        }
    }

    /// A: the selection snaps to the content inside it.
    fn smart_select(&mut self) {
        if let (Some(r), Some(d)) = (self.ed.selection, &self.doc) {
            self.ed.selection = Some(selection::smart_adjust(&d.base, r));
        }
    }

    /// B: blur the selected area with the Blur tool's current mode.
    fn blur_selection(&mut self) {
        let (Some(r), Some(d)) = (self.ed.selection, &mut self.doc) else { return };
        let style = self.ed.style_for(Tool::Blur);
        d.checkpoint();
        d.objs.push(Obj { kind: Kind::Blur { r, text: None }, style });
        self.ed.selection = None;
        self.ed.selected = Some(d.objs.len() - 1);
        self.detect_blur_text();
        self.ed.selected = None;
    }

    /// Enter: crop to the selection.
    fn crop_selection(&mut self) {
        self.ed.crop_draft = self.ed.selection.take();
        self.apply_crop();
    }

    pub fn editor_ui(&mut self, ui: &mut Ui) {
        self.ed.scale = self.doc.as_ref().map(|d| d.scale_factor).unwrap_or(1.0).max(1.0);
        let dark = ui.visuals().dark_mode;
        let t = th(dark);
        self.keyboard(ui);
        egui::Panel::top("toolbar")
            .exact_size(TOOLBAR_H)
            .frame(egui::Frame::new().fill(t.bar))
            .show(ui, |ui| self.toolbar(ui, &t));
        if self.doc.is_some() && (self.selected_image().is_some() || !matches!(self.context_tool(), Tool::Select | Tool::Cut | Tool::Ocr)) {
            egui::Panel::top("options")
                .exact_size(OPTIONS_H)
                .frame(egui::Frame::new().fill(t.bar2).inner_margin(egui::Margin::symmetric(10, 0)))
                .show(ui, |ui| self.options_bar(ui, &t));
        }
        egui::Panel::bottom("status")
            .exact_size(STATUS_H)
            .frame(egui::Frame::new().fill(t.bar))
            .show(ui, |ui| self.status_bar(ui, &t));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| self.canvas_ui(ui, &t));
        self.popovers(ui.ctx(), &t);
        self.overlays(ui.ctx(), &t);
        self.remember_styles();
    }

    /// Keeps the tools' latest styles in the settings, so the next launch starts from them.
    fn remember_styles(&mut self) {
        let now = self.ed.stored_styles();
        if now == self.settings.tool_styles {
            return;
        }
        self.settings.tool_styles = now;
        // The tests draw the editor all the time: they must not write the user's settings.
        if !cfg!(test) {
            self.settings.save();
        }
    }

    /// The selection actions at the right of the tools: icon, tooltip, active.
    fn toolbar_actions(&self) -> [(Icon, &'static str, bool); crate::sizing::TOOLBAR_ACTION_ITEMS] {
        let current = self.ed.tool;
        [
            (Icon::Crop, "Crop", current == Tool::Crop),
            (Icon::Resize, "Resize", matches!(self.ed.popover, Popover::Resize(_))),
            (Icon::Ocr, "Recognize Text or QR (⌘O)", current == Tool::Ocr),
            (Icon::SelSize, "Imprint Selection Size", false),
            (Icon::Erase, "Erase (Del)", false),
            (Icon::Scissors, "Cut Out", current == Tool::Cut),
            (Icon::Reset, "Reset Selection (Esc)", false),
        ]
    }

    /// Runs the `i`th selection action of the toolbar; `anchor` is where the Resize popover opens.
    fn toolbar_action(&mut self, i: usize, anchor: Rect) {
        let current = self.ed.tool;
        match i {
            0 if current == Tool::Crop => self.apply_crop(),
            0 => self.set_tool(Tool::Crop),
            1 => {
                if let Some(d) = &self.doc {
                    let v = self.ed.view_area(d);
                    self.ed.resize_w = format!("{}", v.w().round());
                    self.ed.resize_h = format!("{}", v.h().round());
                }
                self.ed.popover = if matches!(self.ed.popover, Popover::Resize(_)) { Popover::None } else { Popover::Resize(anchor) };
            }
            2 => self.set_tool(Tool::Ocr),
            3 => {
                if let (Some(r), Some(d)) = (self.ed.selection, &mut self.doc) {
                    d.checkpoint();
                    let mut st = self.ed.style_for(Tool::Ruler);
                    st.dashed = self.settings.logical_points;
                    d.objs.push(Obj { kind: Kind::Ruler { a: p(r.x0, r.y1 + 14.0), b: p(r.x1, r.y1 + 14.0) }, style: st });
                    d.objs.push(Obj { kind: Kind::Ruler { a: p(r.x1 + 14.0, r.y0), b: p(r.x1 + 14.0, r.y1) }, style: st });
                } else {
                    self.say("Select an area first");
                }
            }
            4 => {
                if self.ed.selected.is_some() || self.ed.selection.is_some() {
                    self.delete_selected_or_erase();
                } else {
                    self.say("Select an area or an object first");
                }
            }
            5 => self.set_tool(Tool::Cut),
            _ => {
                self.ed.selection = None;
                self.ed.selected = None;
                self.ed.tool = Tool::Select;
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, t: &Th) {
        let rect = ui.max_rect();
        let bg = ui.interact(rect, ui.id().with("titlebar-drag"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let max = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.line));
        let has_doc = self.doc.is_some();
        // Every tool is shown when the window is wide enough; a narrower one moves the last ones
        // (the selection actions first) into the "More" menu.
        let fit = toolbar_fit(rect.width());
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(0.0, 5.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = GROUP_GAP;
                // The groups are centred in the window (the Preferences button is pinned right).
                ui.add_space(toolbar_cluster_left(rect.width(), fit));
                if let Some(i) = segments(
                    ui,
                    &[(Icon::Copy, "Copy (Esc or ⌘C)", false), (Icon::Save, "Save (⌘S)", false), (Icon::Pin, "Pin to Screen (⌘P)", false)],
                    t,
                    SEG_W,
                ) {
                    self.push([Action::Copy, Action::Save, Action::Pin].into_iter().nth(i).unwrap());
                }
                if drag_handle(ui, t, has_doc) {
                    let at = ui.input(|i| i.pointer.interact_pos()).unwrap_or_default();
                    self.drag_out(at);
                }
                let current = self.ed.tool;
                let tools: Vec<(Icon, &str, bool)> = Tool::TOOLBAR.iter().map(|x| (Icon::for_tool(*x), x.tooltip(), current == *x)).collect();
                if fit.tools() > 0 {
                    if let (Some(i), true) = (segments(ui, &tools[..fit.tools()], t, SEG_W), has_doc) {
                        self.set_tool(Tool::TOOLBAR[i]);
                    }
                }
                let actions = self.toolbar_actions();
                if fit.actions() > 0 {
                    let picked = segments(ui, &actions[..fit.actions()], t, SEG_W);
                    if let (Some(i), true) = (picked, has_doc) {
                        let anchor = Rect::from_min_size(ui.min_rect().right_bottom() + vec2(-180.0, 4.0), vec2(1.0, 1.0));
                        self.toolbar_action(i, anchor);
                    }
                }
                if fit.overflows() {
                    let (r, resp) = ui.allocate_exact_size(vec2(seg_group_w(1), 28.0), Sense::click());
                    ui.painter().rect_filled(r, 7.0, t.seg);
                    ui.painter().rect_stroke(r, 7.0, Stroke::new(0.5, t.line), StrokeKind::Inside);
                    let hidden_active = Tool::TOOLBAR[fit.tools()..].contains(&current) || actions[fit.actions()..].iter().any(|a| a.2);
                    if hidden_active {
                        ui.painter().rect_filled(r.shrink(2.0), 5.0, t.accent);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r.shrink(2.0), 5.0, t.hover);
                    }
                    icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(17.0, 17.0)), Icon::ChevronDown, if hidden_active { Color32::WHITE } else { t.text });
                    let resp = resp.on_hover_text("More tools");
                    let mut chosen = None;
                    let hidden_tools = &Tool::TOOLBAR[fit.tools()..];
                    let hidden_actions = &actions[fit.actions()..];
                    // Two columns keep the menu short enough for the smallest window.
                    egui::Popup::menu(&resp)
                        .align(egui::RectAlign::BOTTOM_END)
                        .align_alternatives(&[])
                        .frame(panel_frame(ui.ctx(), t).inner_margin(6).corner_radius(10.0))
                        .show(|ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
                            let rows: Vec<(Icon, String, bool, Result<usize, usize>)> = hidden_tools
                                .iter()
                                .enumerate()
                                .map(|(i, x)| (Icon::for_tool(*x), x.name().to_string(), current == *x, Err(fit.tools() + i)))
                                .chain(hidden_actions.iter().enumerate().map(|(i, (icon, tip, sel))| {
                                    (*icon, tip.split(" (").next().unwrap_or(tip).to_string(), *sel, Ok(fit.actions() + i))
                                }))
                                .collect();
                            let per_col = rows.len().div_ceil(2);
                            ui.horizontal_top(|ui| {
                                for col in rows.chunks(per_col) {
                                    ui.vertical(|ui| {
                                        ui.set_width(180.0);
                                        for (icon, label, sel, id) in col {
                                            if menu_row(ui, *icon, label, *sel, has_doc, t).clicked() {
                                                chosen = Some(*id);
                                            }
                                        }
                                    });
                                }
                            });
                        });
                    if has_doc {
                        match chosen {
                            Some(Err(i)) => self.set_tool(Tool::TOOLBAR[i]),
                            Some(Ok(i)) => self.toolbar_action(i, Rect::from_min_size(resp.rect.left_bottom() + vec2(-180.0, 4.0), vec2(1.0, 1.0))),
                            None => {}
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(TOOLBAR_EDGE);
                    if segments(ui, &[(Icon::Gear, "Preferences… (⌘,)", false)], t, SEG_W).is_some() {
                        self.open_prefs();
                    }
                });
            });
        });
    }

    fn delete_selected_or_erase(&mut self) {
        if self.ed.selected.is_some() {
            self.delete_selected();
        } else if let (Some(r), Some(d)) = (self.ed.selection, &mut self.doc) {
            d.checkpoint();
            let mut st = Style::default();
            st.blur = BlurMode::Erase;
            d.objs.push(Obj { kind: Kind::Blur { r, text: None }, style: st });
            self.ed.selection = None;
        }
    }

    fn options_bar(&mut self, ui: &mut Ui, t: &Th) {
        let rect = ui.max_rect();
        ui.painter().line_segment([pos2(rect.left() - 10.0, rect.bottom()), pos2(rect.right() + 10.0, rect.bottom())], Stroke::new(1.0, t.line));
        // The contents are centred; in a window too narrow for them the bar scrolls sideways
        // instead of cutting them off.
        egui::ScrollArea::horizontal().id_salt("options-scroll").auto_shrink([false, false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.add_space(((rect.width() - self.ed.options_w) / 2.0).max(0.0));
                let start = ui.cursor().left();
                self.options_content(ui, t);
                let w = ui.min_rect().right() - start;
                if (w - self.ed.options_w).abs() > 0.5 {
                    self.ed.options_w = w;
                    ui.ctx().request_discard("centre the options bar");
                }
            });
        });
    }

    /// Fits the view to the picture again (headless pictures).
    #[cfg(test)]
    pub(crate) fn ed_fit(&mut self) {
        self.ed.fit = true;
    }

    /// Selects object `i` with the Select tool (a freshly pasted picture).
    pub fn select_object(&mut self, i: usize) {
        self.ed.selected = Some(i);
        self.ed.tool = Tool::Select;
        self.ed.editing_text = false;
    }

    /// The canvas got another size (Add Capture): drop what pointed into the old one and fit the view.
    pub fn canvas_changed(&mut self) {
        self.ed.selected = None;
        self.ed.selection = None;
        self.ed.crop_draft = None;
        self.ed.fit = true;
    }

    /// The selected object, when it is a pasted picture.
    fn selected_image(&self) -> Option<usize> {
        let (d, i) = (self.doc.as_ref()?, self.ed.selected?);
        matches!(d.objs.get(i)?.kind, Kind::Image { .. }).then_some(i)
    }

    /// Options of a pasted picture: how see-through it is, and the before/after GIF.
    fn image_options(&mut self, ui: &mut Ui, i: usize, t: &Th) {
        if icon_btn(ui, Icon::Back, false, "Back", t, 26.0).clicked() {
            self.ed.selected = None;
            self.ed.tool = Tool::Select;
            return;
        }
        let Some(Kind::Image { opacity, .. }) = self.doc.as_ref().and_then(|d| d.objs.get(i)).map(|o| o.kind.clone()) else { return };
        ui.label("Opacity");
        let mut pct = (opacity * 100.0).round();
        let r = ui.add_sized([120.0, 24.0], egui::Slider::new(&mut pct, 0.0..=100.0).show_value(false)).on_hover_text("Opacity");
        ui.label(format!("{pct:.0}%"));
        // One undo step per drag of the slider, one per click.
        let id = ui.id().with("opacity-undo");
        let saved = ui.data(|m| m.get_temp::<bool>(id)).unwrap_or(false);
        ui.data_mut(|m| m.insert_temp(id, r.dragged() && (saved || r.changed())));
        if r.changed() {
            if let Some(d) = &mut self.doc {
                if !r.dragged() || !saved {
                    d.checkpoint();
                }
                if let Kind::Image { opacity, .. } = &mut d.objs[i].kind {
                    *opacity = pct / 100.0;
                }
                d.touch();
            }
        }
        if ui.button("Save GIF").on_hover_text("Save a two-frame GIF: the screenshot, then with this picture on top").clicked() {
            self.push(Action::SaveGif);
        }
        if ui.button("Delete").clicked() {
            self.delete_selected();
        }
    }

    fn options_content(&mut self, ui: &mut Ui, t: &Th) {
        if let Some(i) = self.selected_image() {
            return self.image_options(ui, i, t);
        }
        let tool = self.context_tool();
        match tool {
            Tool::Crop => {
                if ui.button("Apply (Enter)").clicked() {
                    self.apply_crop();
                }
                if ui.button("Cancel").clicked() {
                    self.ed.crop_draft = None;
                    self.ed.tool = Tool::Select;
                }
                if self.doc.as_ref().map_or(false, |d| d.crop.is_some()) && ui.button("Reset Crop").clicked() {
                    self.ed.tool = Tool::Select;
                    self.push(Action::ResetCrop);
                }
                return;
            }
            Tool::Backdrop => {
                self.backdrop_options(ui, t);
                return;
            }
            _ => {}
        }
        if icon_btn(ui, Icon::Back, false, "Back", t, 26.0).clicked() {
            self.ed.selected = None;
            self.ed.tool = Tool::Select;
        }
        let st = self.context_style();
        let (cr, cresp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
        ui.painter().circle_filled(cr.center(), 10.0, c32(st.color));
        ui.painter().circle_stroke(cr.center(), 11.0, Stroke::new(1.0, t.line));
        if cresp.on_hover_text("Color").clicked() {
            self.ed.popover = if matches!(self.ed.popover, Popover::Picker(_)) { Popover::None } else { Popover::Picker(cr) };
        }
        if tool != Tool::Spotlight {
            let mut size = st.size;
            let max = if tool == Tool::Highlighter { (HIGHLIGHT_SIZES.len() - 1) as u8 } else { SIZE_MAX };
            let r = ui.add_sized([96.0, 24.0], egui::Slider::new(&mut size, 0..=max).show_value(false)).on_hover_text("Size");
            if r.changed() {
                self.update_style(|s| s.size = size);
            }
        }
        let dash_items = |st: &Style| [(Icon::Solid, "Solid", !st.dashed), (Icon::Dash, "Dashed", st.dashed)];
        match tool {
            Tool::Arrow => {
                if let Some(i) = segments(ui, &dash_items(&st), t, 26.0) {
                    self.update_style(|s| s.dashed = i == 1);
                }
                let styles = [ArrowStyle::Shape, ArrowStyle::HandDrawn, ArrowStyle::Line, ArrowStyle::Double];
                let items = [
                    (Icon::ArrowShape, "Shape", st.arrow == ArrowStyle::Shape),
                    (Icon::ArrowHand, "Hand-drawn", st.arrow == ArrowStyle::HandDrawn),
                    (Icon::ArrowLine, "Line", st.arrow == ArrowStyle::Line),
                    (Icon::ArrowDouble, "Double", st.arrow == ArrowStyle::Double),
                ];
                if let Some(i) = segments(ui, &items, t, 26.0) {
                    self.update_style(|s| s.arrow = styles[i]);
                }
            }
            Tool::Line | Tool::Freehand => {
                if let Some(i) = segments(ui, &dash_items(&st), t, 26.0) {
                    self.update_style(|s| s.dashed = i == 1);
                }
            }
            Tool::Rectangle | Tool::Oval | Tool::Text => {
                if tool != Tool::Text {
                    if let Some(i) = segments(ui, &dash_items(&st), t, 26.0) {
                        self.update_style(|s| s.dashed = i == 1);
                    }
                }
                let fills = [Fill::HandDrawn, Fill::Transparent, Fill::Translucent, Fill::Opaque];
                let items = [
                    (Icon::FillHand, "Hand-drawn", st.fill == Fill::HandDrawn),
                    (Icon::FillNone, "Transparent", st.fill == Fill::Transparent),
                    (Icon::FillHalf, "Translucent", st.fill == Fill::Translucent),
                    (Icon::FillFull, "Opaque", st.fill == Fill::Opaque),
                ];
                if let Some(i) = segments(ui, &items, t, 26.0) {
                    self.update_style(|s| s.fill = fills[i]);
                }
                if tool == Tool::Text {
                    if let Some(i) = text_segments(ui, &["Sans", "Goofy"], if st.font == FontKind::Sans { 0 } else { 1 }, t) {
                        self.update_style(|s| s.font = if i == 0 { FontKind::Sans } else { FontKind::Goofy });
                    }
                } else {
                    let items = [(Icon::Rect, "Rectangle", tool == Tool::Rectangle), (Icon::Oval, "Oval", tool == Tool::Oval)];
                    if let Some(i) = segments(ui, &items, t, 26.0) {
                        self.switch_shape(i == 1);
                    }
                }
            }
            Tool::Blur => {
                let cur = BlurMode::ALL.iter().position(|m| *m == st.blur).unwrap_or(0);
                let labels: Vec<&str> = BlurMode::ALL.iter().map(|m| m.label()).collect();
                if let Some(i) = text_segments(ui, &labels, cur, t) {
                    self.update_style(|s| s.blur = BlurMode::ALL[i]);
                    self.detect_blur_text();
                }
                let items = [(Icon::Rect, "Rectangle", st.shape == Shape::Rect), (Icon::Oval, "Oval", st.shape == Shape::Oval)];
                if let Some(i) = segments(ui, &items, t, 26.0) {
                    self.update_style(|s| s.shape = if i == 0 { Shape::Rect } else { Shape::Oval });
                }
            }
            Tool::Spotlight => {
                let items = [(Icon::Rect, "Rectangle", st.shape == Shape::Rect), (Icon::Oval, "Oval", st.shape == Shape::Oval)];
                if let Some(i) = segments(ui, &items, t, 26.0) {
                    self.update_style(|s| s.shape = if i == 0 { Shape::Rect } else { Shape::Oval });
                }
            }
            Tool::Ruler => {
                if let Some(i) = text_segments(ui, &["PX", "PT"], if st.dashed { 1 } else { 0 }, t) {
                    self.update_style(|s| s.dashed = i == 1);
                }
            }
            _ => {}
        }
    }

    fn switch_shape(&mut self, oval: bool) {
        let target = if oval { Tool::Oval } else { Tool::Rectangle };
        if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
            let kind = match d.objs[i].kind {
                Kind::Rect { r, seed } if oval => Some(Kind::Oval { r, seed }),
                Kind::Oval { r, seed } if !oval => Some(Kind::Rect { r, seed }),
                _ => None,
            };
            if let Some(k) = kind {
                d.checkpoint();
                d.objs[i].kind = k;
            }
        }
        let st = self.ed.style_for(self.ed.tool);
        self.ed.styles.entry(target).or_insert(st);
        if self.ed.selected.is_none() {
            self.ed.tool = target;
        }
    }

    /// A fresh backdrop: the last configuration used (kept in the settings), with its picture reloaded.
    fn new_backdrop(&self) -> Backdrop {
        let mut bd = self.settings.backdrop.clone().unwrap_or_default().sanitised();
        bd.restore_image();
        bd.recenter();
        bd
    }

    /// Backdrops are refused on pictures above 15 megapixels: says so and returns false.
    fn backdrop_allowed(&mut self) -> bool {
        let Some(d) = &self.doc else { return false };
        let v = d.crop.unwrap_or_else(|| render::canvas_rect(d, &self.fonts));
        let (w, h) = (v.w() as u32, v.h() as u32);
        if backdrop_too_large(w, h) {
            self.error("The image is too large. Crop or resize to 15 megapixel first.");
            return false;
        }
        true
    }

    fn pick_backdrop_image(&mut self, bd: &mut Backdrop) {
        let Some(path) = rfd::FileDialog::new().add_filter("Images", crate::system::OPEN_EXTENSIONS).pick_file() else { return };
        let dir = crate::system::home().join("Library/Application Support/ShotCraft/backdrops");
        let stored = store_backdrop_image(&path, &dir, 12);
        let src = stored.clone().unwrap_or(path);
        match load_backdrop_image(&src) {
            Some(img) => {
                bd.image = Some(img);
                bd.image_path = src.display().to_string();
            }
            None => self.error("Could not read that picture."),
        }
    }

    fn backdrop_options(&mut self, ui: &mut Ui, t: &Th) {
        let Some(d) = &self.doc else { return };
        let Some(mut bd) = d.backdrop.clone() else {
            if ui.button("Add Gradient Background").clicked() && self.backdrop_allowed() {
                let bd = self.new_backdrop();
                if let Some(d) = &mut self.doc {
                    d.checkpoint();
                    d.backdrop = Some(bd);
                }
            }
            return;
        };
        let before = bd.clone();
        let mut pick_image = false;
        let mode = |bd: &Backdrop| match (&bd.image, bd.fill) {
            (Some(_), _) => 3,
            (None, BackdropFill::Preset) => 0,
            (None, BackdropFill::Custom) => 1,
            (None, BackdropFill::Solid) => 2,
        };
        let mode_names = ["Gradient", "Two colors", "Color", "Picture"];
        let mut cur = mode(&bd);
        crate::icons::combo("bdfill").width(96.0).selected_text(mode_names[cur]).show_ui(ui, |ui| {
            for (i, n) in mode_names.iter().enumerate() {
                if ui.selectable_label(cur == i, *n).clicked() {
                    cur = i;
                }
            }
        });
        if cur != mode(&bd) {
            match cur {
                0 | 1 | 2 => {
                    // The picture is forgotten, so it does not come back with the next backdrop.
                    bd.forget_image();
                    bd.fill = [BackdropFill::Preset, BackdropFill::Custom, BackdropFill::Solid][cur];
                }
                _ => {
                    if bd.image_path.is_empty() {
                        pick_image = true;
                    } else {
                        bd.restore_image();
                    }
                }
            }
        }
        if bd.image.is_none() && bd.fill != BackdropFill::Solid {
            crate::icons::combo("gradkind").width(110.0).selected_text(bd.kind.label().trim_end_matches(" gradient")).show_ui(ui, |ui| {
                for k in GradientKind::ALL {
                    ui.selectable_value(&mut bd.kind, k, k.label());
                }
            });
        }
        if bd.image.is_none() && bd.fill == BackdropFill::Preset {
            let (sw, sresp) = ui.allocate_exact_size(vec2(44.0, 24.0), Sense::click());
            let stops = gradient_stops(bd.gradient);
            let n = 12;
            for i in 0..n {
                let tt = i as f32 / (n - 1) as f32;
                let c = stops.iter().rev().find(|(s, _)| *s <= tt).or(stops.first()).map(|x| x.1).unwrap_or([0, 0, 0, 255]);
                let x0 = sw.left() + sw.width() * i as f32 / n as f32;
                ui.painter().rect_filled(Rect::from_min_max(pos2(x0, sw.top()), pos2(x0 + sw.width() / n as f32 + 0.5, sw.bottom())), 0.0, c32(c));
            }
            ui.painter().rect_stroke(sw, 4.0, Stroke::new(1.0, t.line), StrokeKind::Inside);
            if sresp.on_hover_text("Next gradient").clicked() {
                bd.gradient = (bd.gradient + 1) % GRADIENTS.len();
            }
            if icon_btn(ui, Icon::Randomize, false, "Randomize", t, 26.0).clicked() {
                let seed = (ui.input(|i| i.time) * 1000.0) as usize;
                bd.gradient = seed % GRADIENTS.len();
                bd.kind = GradientKind::ALL[(seed / 7) % 4];
            }
        }
        if bd.image.is_none() && bd.fill == BackdropFill::Custom {
            ui.color_edit_button_srgb(&mut bd.custom_from).on_hover_text("From");
            ui.color_edit_button_srgb(&mut bd.custom_to).on_hover_text("To");
        }
        if bd.image.is_none() && bd.fill == BackdropFill::Solid {
            ui.color_edit_button_srgb(&mut bd.solid).on_hover_text("Color");
        }
        let slider = |ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>| {
            ui.label(egui::RichText::new(label).color(t.dim).size(12.0));
            ui.add_sized([64.0, 20.0], egui::Slider::new(v, range).show_value(false));
        };
        if bd.image.is_some() {
            if ui.button("Choose…").clicked() {
                pick_image = true;
            }
            slider(ui, "Image Blur", &mut bd.blur, 0.0..=BACKDROP_BLUR_MAX);
        }
        slider(ui, "Backdrop", &mut bd.padding, 0.0..=BACKDROP_SLIDER_MAX);
        slider(ui, "Shadow", &mut bd.shadow, 0.0..=BACKDROP_SLIDER_MAX);
        slider(ui, "Inset", &mut bd.inset, 0.0..=BACKDROP_SLIDER_MAX);
        if ui.add_enabled(bd.offset != [0.0, 0.0], egui::Button::new("Recenter")).on_hover_text("Drag the picture to move it; this puts it back in the middle").clicked() {
            bd.recenter();
        }
        ui.menu_button("•••", |ui| {
            ui.set_min_width(190.0);
            for (name, r) in RATIOS {
                if ui.selectable_label(bd.ratio == r && r.is_some(), name).clicked() {
                    bd.ratio = r;
                    ui.close();
                }
            }
            ui.separator();
            if bd.image.is_none() && bd.fill != BackdropFill::Solid {
                ui.checkbox(&mut bd.reverse, "Reverse colors");
            }
            ui.checkbox(&mut bd.autocrop, "Autocrop").on_hover_text("Trims the empty border off the screenshot first");
            ui.separator();
            for (label, v) in [("Outer Radius", &mut bd.outer_radius), ("Inner Radius", &mut bd.inner_radius)] {
                ui.horizontal(|ui| {
                    ui.add_sized([86.0, 18.0], egui::Label::new(label));
                    ui.add(egui::Slider::new(v, 0.0..=BACKDROP_SLIDER_MAX).show_value(false));
                });
            }
        });
        let remove_image = bd.image.is_some() && ui.button("Remove").on_hover_text("Forget the background picture").clicked();
        let remove = bd.image.is_none() && ui.button("Remove").clicked();
        let ok = ui.button("Ok").clicked();
        if pick_image {
            self.pick_backdrop_image(&mut bd);
        }
        if remove_image {
            bd.forget_image();
        }
        let Some(d) = &mut self.doc else { return };
        if remove {
            d.checkpoint();
            d.backdrop = None;
            self.ed.tool = Tool::Select;
            return;
        }
        // One undo step per drag of a slider (the first change of it), one per click.
        let using = self.ed.drag != Drag::None || ui.ctx().egui_is_using_pointer();
        let id = ui.id().with("backdrop-undo");
        let in_drag = ui.data(|m| m.get_temp::<bool>(id)).unwrap_or(false);
        let changed = bd != before;
        if changed {
            if !in_drag {
                d.checkpoint();
            } else {
                d.touch();
            }
            self.settings.backdrop = Some(bd.clone());
            if !using {
                self.settings.save();
            }
            d.backdrop = Some(bd.clone());
        } else if in_drag && !using {
            // The drag ended: keep its last value.
            self.settings.save();
        }
        ui.data_mut(|m| m.insert_temp(id, using && (in_drag || changed)));
        if ok {
            self.settings.backdrop = Some(bd);
            self.settings.save();
            self.ed.tool = Tool::Select;
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Th) {
        let rect = ui.max_rect();
        ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, t.line));
        let small = FontId::new(10.5, FontFamily::Proportional);
        let value = FontId::new(12.0, FontFamily::Name("medium".into()));
        let mut x = rect.left() + 8.0;
        let cell = |ui: &mut Ui, x: &mut f32, w: f32, id: &str| -> (Rect, Response) {
            let r = Rect::from_min_size(pos2(*x, rect.top()), vec2(w, rect.height()));
            let resp = ui.interact(r, ui.id().with(id), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r.shrink(2.0), 5.0, t.hover);
            }
            *x += w;
            (r, resp)
        };
        let sep = |ui: &mut Ui, x: &mut f32| {
            ui.painter().line_segment([pos2(*x + 2.0, rect.top() + 6.0), pos2(*x + 2.0, rect.bottom() - 6.0)], Stroke::new(1.0, t.line));
            *x += 6.0;
        };
        let txt = render::format_color(self.ed.hover_color, self.settings.color_format);
        // Some formats (OKLCH, UIColor) are wider than the default cell.
        // Sized from a few wide samples, not from the colour under the cursor, so the cells to
        // the right do not slide around while the mouse moves.
        let txt_w = [[255, 255, 255, 255], [0, 0, 255, 255], [200, 100, 255, 255], [255, 0, 255, 255]]
            .iter()
            .map(|c| ui.painter().layout_no_wrap(render::format_color(*c, self.settings.color_format), value.clone(), t.text).size().x)
            .fold(0.0f32, f32::max);
        let (r, resp) = cell(ui, &mut x, (txt_w + 44.0).max(140.0), "st-color");
        let sw = Rect::from_min_size(pos2(r.left() + 6.0, r.center().y - 9.0), vec2(18.0, 18.0));
        ui.painter().rect_filled(sw, 4.0, c32(self.ed.hover_color));
        ui.painter().rect_stroke(sw, 4.0, Stroke::new(1.0, t.line), StrokeKind::Inside);
        ui.painter().text(pos2(r.left() + 30.0, r.top() + 4.0), Align2::LEFT_TOP, txt, value.clone(), t.text);
        ui.painter().text(pos2(r.left() + 30.0, r.bottom() - 4.0), Align2::LEFT_BOTTOM, "Tab to copy", small.clone(), t.dim);
        if resp.clicked() {
            self.ed.popover = if matches!(self.ed.popover, Popover::Color(_)) { Popover::None } else { Popover::Color(r) };
        }
        sep(ui, &mut x);
        let sel = self.ed.selection.or(self.ed.crop_draft).or_else(|| self.doc.as_ref().map(|d| self.ed.view_area(d)));
        let pts = self.settings.logical_points;
        let scale = self.doc.as_ref().map(|d| d.scale_factor).unwrap_or(1.0);
        let size_txt = sel.map_or("—".to_string(), |s| {
            let k = if pts { scale.max(1.0) } else { 1.0 };
            format!("{}×{}{}", (s.w() / k).round(), (s.h() / k).round(), if pts { "pt" } else { "px" })
        });
        let (r, resp) = cell(ui, &mut x, 104.0, "st-size");
        ui.painter().text(pos2(r.left() + 8.0, r.top() + 4.0), Align2::LEFT_TOP, size_txt, value.clone(), t.text);
        ui.painter().text(pos2(r.left() + 8.0, r.bottom() - 4.0), Align2::LEFT_BOTTOM, "Selection Size", small.clone(), t.dim);
        if resp.clicked() {
            self.ed.popover = if matches!(self.ed.popover, Popover::Size(_)) { Popover::None } else { Popover::Size(r) };
        }
        sep(ui, &mut x);
        // Contrast of the selection box: text colour against background colour.
        // The contrast cell and the picture size give way when the window is narrow: the three
        // main cells always fit.
        let zoom_w = 64.0;
        let room_for_contrast = x + 136.0 + 6.0 + zoom_w <= rect.right() - 8.0;
        if let (Some(br), Some(d), true) = (self.ed.selection.or(self.ed.crop_draft), &self.doc, room_for_contrast) {
            let key = (d.version, [br.x0.to_bits(), br.y0.to_bits(), br.x1.to_bits(), br.y1.to_bits()]);
            let cache_id = ui.id().with("st-contrast-cache");
            let cached = ui.data(|dd| dd.get_temp::<((u64, [u32; 4]), Option<(Rgba, Rgba)>)>(cache_id));
            let pair = match cached {
                Some((k, p)) if k == key => p,
                _ => {
                    let p = crate::color::contrast_pair(&d.base, &br);
                    ui.data_mut(|dd| dd.insert_temp(cache_id, (key, p)));
                    p
                }
            };
            if let Some((fg, bg)) = pair {
                let (wcag, apca) = (crate::color::contrast_wcag(fg, bg), crate::color::contrast_apca(fg, bg));
                let apca_first = self.settings.contrast_type == crate::color::ContrastType::Apca;
                let (val, cap) = if apca_first { (format!("Lc {apca:.1}"), "APCA Contrast") } else { (format!("{wcag:.2}:1"), "WCAG 2.0 Contrast") };
                let (r, resp) = cell(ui, &mut x, 136.0, "st-contrast");
                let sw = |ui: &mut Ui, c: Rgba, at: Pos2| {
                    let b = Rect::from_min_size(at, vec2(8.0, 8.0));
                    ui.painter().rect_filled(b, 2.0, c32(c));
                    ui.painter().rect_stroke(b, 2.0, Stroke::new(1.0, t.line), StrokeKind::Inside);
                };
                sw(ui, fg, pos2(r.right() - 18.0, r.top() + 6.0));
                sw(ui, bg, pos2(r.right() - 18.0, r.top() + 17.0));
                ui.painter().text(pos2(r.left() + 8.0, r.top() + 4.0), Align2::LEFT_TOP, val, value.clone(), t.text);
                ui.painter().text(pos2(r.left() + 8.0, r.bottom() - 4.0), Align2::LEFT_BOTTOM, cap, small.clone(), t.dim);
                let tip = format!(
                    "WCAG 2.0 Contrast Ratio: {wcag:.2}\nAPCA Contrast: Lc {apca:.1}\n\nWCAG recommends a ratio of at least 4.5 for regular text and 3.0 for large text and UI elements.\nOption+click switches the measure shown here."
                );
                let alt = ui.input(|i| i.modifiers.alt);
                resp.clone().on_hover_text(tip);
                if resp.clicked() && alt {
                    self.settings.contrast_type = self.settings.contrast_type.toggled();
                    self.settings.save();
                }
                sep(ui, &mut x);
            }
        }
        let ppp = ui.ctx().pixels_per_point();
        let (r, resp) = cell(ui, &mut x, zoom_w, "st-zoom");
        let zt = if self.doc.is_some() { format!("{}%", (self.ed.zoom * ppp * 100.0).round()) } else { "—".into() };
        ui.painter().text(pos2(r.left() + 8.0, r.top() + 4.0), Align2::LEFT_TOP, zt, value, t.text);
        ui.painter().text(pos2(r.left() + 8.0, r.bottom() - 4.0), Align2::LEFT_BOTTOM, "Zoom", small.clone(), t.dim);
        if resp.clicked() {
            self.ed.popover = if matches!(self.ed.popover, Popover::Zoom(_)) { Popover::None } else { Popover::Zoom(r) };
        }
        if let Some(d) = &self.doc {
            let v = self.ed.view_area(d);
            let info = format!("{} × {} px", v.w().round(), v.h().round());
            let info_w = ui.painter().layout_no_wrap(info.clone(), small.clone(), t.dim).size().x;
            if x + 16.0 + info_w <= rect.right() - 12.0 {
                ui.painter().text(pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, info, small, t.dim);
            }
        }
    }

    fn refresh_textures(&mut self, ctx: &egui::Context) {
        let Some(d) = &self.doc else {
            self.ed.comp = None;
            return;
        };
        let sel = self.ed.selected.filter(|i| *i < d.objs.len());
        let skip = sel.filter(|i| !matches!(d.objs[*i].kind, Kind::Spotlight { .. }));
        let key = (d.version, skip);
        if self.ed.comp.as_ref().map_or(true, |c| c.key != key) {
            let cv = self.ed.canvas_of(d, &self.fonts);
            let pm = render::compose_canvas(d, &self.fonts, skip, &cv);
            let origin = p(cv.x0, cv.y0);
            let size = (pm.width(), pm.height());
            match &mut self.ed.comp {
                Some(c) if c.tex.fits(ctx, size) => {
                    c.tex.update(Some(c.pm.data()), pm.data(), Alpha::Premultiplied, TEX);
                    c.pm = pm;
                    c.key = key;
                    c.origin = origin;
                }
                _ => {
                    let tex = TiledTexture::new(ctx, "composite", size, pm.data(), Alpha::Premultiplied, TEX);
                    self.ed.comp = Some(Comp { key, origin, pm, tex });
                }
            }
        }
        let comp = self.ed.comp.as_ref().unwrap();
        match skip {
            Some(i) => {
                let obj = &d.objs[i];
                let fresh = self.ed.overlay.as_ref().map_or(true, |o| o.obj != *obj || o.version != comp.key.0);
                if fresh {
                    let (w, h) = (comp.pm.width() as f32, comp.pm.height() as f32);
                    let o = comp.origin;
                    // `b` is in composite pixels; the overlay's `rect` goes back to picture coordinates.
                    let b = render::bounds(obj, &self.fonts).expand(2.0).translate(-o.x, -o.y).clamp_to(w, h);
                    let b = R { x0: b.x0.floor(), y0: b.y0.floor(), x1: b.x1.ceil(), y1: b.y1.ceil() };
                    if b.w() >= 1.0 && b.h() >= 1.0 {
                        let mut region = render::crop_pixmap(&comp.pm, &b);
                        let mut local = obj.clone();
                        local.translate(-b.x0 - o.x, -b.y0 - o.y);
                        render::draw_obj(&mut region, &local, &self.fonts, d.scale_factor);
                        let size = (region.width(), region.height());
                        let tex = match self.ed.overlay.take() {
                            Some(mut o) if o.tex.fits(ctx, size) => {
                                o.tex.update(None, region.data(), Alpha::Premultiplied, TEX);
                                o.tex
                            }
                            _ => TiledTexture::new(ctx, "overlay", size, region.data(), Alpha::Premultiplied, TEX),
                        };
                        self.ed.overlay = Some(Overlay { obj: obj.clone(), version: comp.key.0, rect: b.translate(o.x, o.y), tex });
                    } else {
                        self.ed.overlay = None;
                    }
                }
            }
            None => self.ed.overlay = None,
        }
        if self.ed.tool == Tool::Backdrop && d.backdrop.is_some() {
            if self.ed.preview.as_ref().map_or(true, |p| p.0 != d.version) {
                let img = render::export(d, &self.fonts);
                let tex = TiledTexture::new(ctx, "backdrop-preview", (img.w, img.h), &img.px, Alpha::Straight, egui::TextureOptions::LINEAR);
                self.ed.preview = Some((d.version, (img.w, img.h), tex));
            }
        }
    }

    fn canvas_ui(&mut self, ui: &mut Ui, t: &Th) {
        let rect = ui.max_rect();
        self.ed.canvas = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        if self.doc.is_none() {
            self.empty_state(ui, rect, t);
            return;
        }
        self.refresh_textures(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let crop_mode = self.ed.tool == Tool::Crop;
        let backdrop_mode = self.ed.tool == Tool::Backdrop && self.doc.as_ref().map_or(false, |d| d.backdrop.is_some());
        let (iw, ih, view) = {
            let d = self.doc.as_ref().unwrap();
            let full = R { x0: 0.0, y0: 0.0, x1: d.base.w as f32, y1: d.base.h as f32 };
            let v = if crop_mode { full } else { self.ed.view_area(d) };
            (d.base.w as f32, d.base.h as f32, v)
        };
        let fit_target = if backdrop_mode {
            let (w, h) = self.ed.preview.as_ref().map(|p| (p.1 .0 as f32, p.1 .1 as f32)).unwrap_or((iw, ih));
            R { x0: 0.0, y0: 0.0, x1: w, y1: h }
        } else {
            view
        };
        // While an annotation is being dragged the view stays put even if the canvas grows under it;
        // "fit" catches up on release.
        let editing = matches!(self.ed.drag, Drag::Create { .. } | Drag::Move { .. } | Drag::Handle { .. });
        if self.ed.fit && !editing {
            let avail = rect.size() - vec2(48.0, 48.0);
            let fitz = (avail.x / fit_target.w()).min(avail.y / fit_target.h()).max(0.01);
            let hundred = 1.0 / ppp;
            self.ed.zoom = if self.ed.prefer_100 || self.settings.prefer_100 { hundred.min(fitz.max(hundred * 0.8)) } else { fitz.min(hundred) };
            // A tall picture (a scrolling capture) shrunk to fit its height would be unreadable: fit its
            // width instead and start at the top; scrolling or zooming moves down the picture.
            let tall = fit_target.h() / fit_target.w() > TALL_ASPECT;
            if tall {
                self.ed.zoom = (avail.x / fit_target.w()).min(hundred).max(0.01);
            }
            let content = vec2(fit_target.w(), fit_target.h()) * self.ed.zoom;
            let free = rect.size() - content;
            let offset = if tall { vec2(free.x / 2.0, 24.0) } else { free / 2.0 };
            self.ed.pan = offset - vec2(fit_target.x0, fit_target.y0) * self.ed.zoom;
        }

        let painter = ui.painter_at(rect);
        if backdrop_mode {
            if let Some((_, (w, h), tex)) = &self.ed.preview {
                let r = self.ed.rect_screen(&R { x0: 0.0, y0: 0.0, x1: *w as f32, y1: *h as f32 });
                tex.paint_in(&painter, r, Color32::WHITE);
            }
            self.canvas_zoom_pan(ui, &resp);
            // Dragging moves the screenshot inside the backdrop; Recenter puts it back.
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            }
            if let Some(d) = &mut self.doc {
                if resp.drag_started() {
                    d.checkpoint();
                }
                if resp.dragged() {
                    let delta = resp.drag_delta() / self.ed.zoom;
                    let v = d.crop.unwrap_or_else(|| render::canvas_rect(d, &self.fonts));
                    let (cw, ch) = (v.w(), v.h());
                    if let Some(bd) = &mut d.backdrop {
                        let (mx, my) = render::backdrop_max_offset(cw, ch, bd);
                        bd.offset = [(bd.offset[0] + delta.x).clamp(-mx, mx), (bd.offset[1] + delta.y).clamp(-my, my)];
                    }
                    d.touch();
                }
            }
            return;
        }

        let view_screen = self.ed.rect_screen(&view);
        // The canvas is a plain colour. A checkerboard shows only behind the picture itself, and
        // only when the picture has see-through pixels (a window with its shadow, a logo): there it
        // is what tells transparent from white. An opaque picture covers it and gets an outline.
        let grown = !crop_mode && self.doc.as_ref().map_or(false, |d| d.crop.is_none() && view != R { x0: 0.0, y0: 0.0, x1: d.base.w as f32, y1: d.base.h as f32 });
        let see_through = grown || self.picture_has_alpha();
        if see_through {
            self.paint_checker(ui, view_screen.intersect(rect), view_screen.min);
        }
        if let Some(c) = &self.ed.comp {
            // The composite starts at `origin` in picture coordinates.
            let (ox, oy) = (c.origin.x, c.origin.y);
            c.tex.paint(&painter, &view.translate(-ox, -oy), |r| self.ed.rect_screen(&r.translate(ox, oy)), Color32::WHITE);
        }
        if !see_through {
            painter.rect_stroke(view_screen, 0.0, Stroke::new(1.0, t.line), StrokeKind::Outside);
        }
        let clip = painter.with_clip_rect(view_screen.intersect(rect));
        if let Some(o) = &self.ed.overlay {
            o.tex.paint_in(&clip, self.ed.rect_screen(&o.rect), Color32::WHITE);
        }

        let pointer = ui.input(|i| i.pointer.hover_pos());
        self.ed.hover = pointer.filter(|p| view_screen.contains(*p) && rect.contains(*p)).map(|p| self.ed.to_img(p));
        if let (Some(q), Some(c)) = (self.ed.hover, &self.ed.comp) {
            let (x, y) = ((q.x - c.origin.x).floor() as i32, (q.y - c.origin.y).floor() as i32);
            if x >= 0 && y >= 0 && (x as u32) < c.pm.width() && (y as u32) < c.pm.height() {
                let px = c.pm.pixel(x as u32, y as u32).unwrap().demultiply();
                self.ed.hover_color = [px.red(), px.green(), px.blue(), 255];
            }
        }

        self.canvas_input(ui, &resp);
        self.canvas_zoom_pan(ui, &resp);
        self.canvas_decorations(ui, &painter, crop_mode, iw, ih);
        let hover_cursor = match self.ed.tool {
            Tool::Select => egui::CursorIcon::Crosshair,
            Tool::Text => egui::CursorIcon::Text,
            _ => egui::CursorIcon::Crosshair,
        };
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if matches!(self.ed.drag, Drag::Move { .. } | Drag::Pan { .. }) { egui::CursorIcon::Grabbing } else { hover_cursor });
        }
    }

    /// Whether the open picture has any see-through pixel. Cached per document version, since
    /// the scan reads every pixel.
    fn picture_has_alpha(&mut self) -> bool {
        let Some(d) = &self.doc else { return false };
        match self.ed.alpha {
            Some((v, a)) if v == d.version => a,
            _ => {
                let a = d.base.has_alpha();
                self.ed.alpha = Some((d.version, a));
                a
            }
        }
    }

    /// Fills `area` with a checkerboard of two greys whose squares start at `origin`.
    fn paint_checker(&mut self, ui: &mut Ui, area: Rect, origin: Pos2) {
        if area.width() <= 0.0 || area.height() <= 0.0 {
            return;
        }
        let dark = ui.visuals().dark_mode;
        if self.ed.checker.as_ref().map_or(true, |c| c.0 != dark) {
            let (base, square) = if dark { CHECKER_DARK } else { CHECKER_LIGHT };
            let g = |v: f32| Color32::from_gray((v * 255.0).round() as u8);
            let img = egui::ColorImage::new([2, 2], vec![g(square), g(base), g(base), g(square)]);
            let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::NEAREST };
            self.ed.checker = Some((dark, ui.ctx().load_texture("checker", img, options)));
        }
        if let Some((_, tex)) = &self.ed.checker {
            let tile = CHECKER_SQUARE * 2.0;
            let uv = Rect::from_min_size(pos2((area.min.x - origin.x) / tile, (area.min.y - origin.y) / tile), area.size() / tile);
            ui.painter().image(tex.id(), area, uv, Color32::WHITE);
        }
    }

    fn canvas_zoom_pan(&mut self, ui: &mut Ui, resp: &Response) {
        if !resp.hovered() && !resp.dragged() {
            return;
        }
        let (zoom_delta, scroll, pos, events) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos(), i.raw.events.clone()));
        let around = pos.unwrap_or(self.ed.canvas.center());
        if (zoom_delta - 1.0).abs() > 1e-4 {
            self.ed.zoom_around(zoom_delta, around);
        }
        let wheel_lines = events.iter().any(|e| matches!(e, egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, .. }));
        if scroll != Vec2::ZERO && (zoom_delta - 1.0).abs() < 1e-4 {
            if self.ruler_axis(ui).is_some() {
                self.ed.ruler_sensitivity = (self.ed.ruler_sensitivity + scroll.y).clamp(0.0, RULER_SENSITIVITY_MAX);
            } else if wheel_lines {
                let dir = if self.settings.reverse_zoom { -1.0 } else { 1.0 };
                self.ed.wheel_acc += scroll.y * dir;
                if self.ed.wheel_acc.abs() > ZOOM_WHEEL_NOTCH {
                    let zoom_in = self.ed.wheel_acc > 0.0;
                    self.ed.wheel_acc -= ZOOM_WHEEL_NOTCH.copysign(self.ed.wheel_acc);
                    let ppp = ui.ctx().pixels_per_point();
                    let z = self.ed.zoom * ppp;
                    self.ed.zoom_around(zoom_wheel_target(z, zoom_in) / z, around);
                }
            } else {
                self.ed.pan += scroll;
                self.ed.fit = false;
            }
        }
    }

    fn canvas_input(&mut self, ui: &mut Ui, resp: &Response) {
        let (press, cur, shift, space) = ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos(), i.modifiers.shift, i.key_down(Key::Space)));
        let (alt, cmd, zoom_key) = ui.input(|i| (i.modifiers.alt, i.modifiers.command, i.key_down(Key::Z) && !i.modifiers.command));
        let zoom_key = zoom_key && !self.ed.editing_text;
        let tol = 6.0 / self.ed.zoom;
        let ruler_hold = self.ruler_axis(ui);

        if resp.drag_started() {
            self.ed.popover = Popover::None;
            let start_s = press.unwrap_or_default();
            let q = self.ed.to_img(start_s);
            if space || ui.input(|i| i.pointer.middle_down()) {
                self.ed.drag = Drag::Pan { last: start_s };
            } else if zoom_key {
                self.ed.drag = Drag::Zoom { start: q };
            } else {
                self.finish_text();
                self.ed.snap_carry = (0.0, 0.0);
                self.ed.snap_guides.clear();
                self.begin_drag(q, tol);
            }
        }
        // The object being dragged can be gone (Delete or Undo during the drag).
        if let Drag::Move { idx, .. } | Drag::Handle { idx, .. } = self.ed.drag {
            if self.doc.as_ref().map_or(true, |d| idx >= d.objs.len()) {
                self.ed.drag = Drag::None;
            }
        }
        if resp.dragged() {
            if let Some(cs) = cur {
                let q = self.ed.to_img(cs);
                match self.ed.drag {
                    Drag::Pan { last } => {
                        self.ed.pan += cs - last;
                        self.ed.fit = false;
                        self.ed.drag = Drag::Pan { last: cs };
                    }
                    Drag::Create { start } => {
                        if let Some(i) = self.ed.selected {
                            let free = self.doc.as_ref().map_or(true, |d| matches!(d.objs[i].kind, Kind::Freehand { .. } | Kind::Counter { .. } | Kind::Text { .. }));
                            let rect = self.doc.as_ref().map_or(false, |d| handles(&d.objs[i]).len() == 4);
                            let q = if shift || free {
                                self.ed.snap_guides.clear();
                                q
                            } else {
                                self.snap_pointer(q, i, rect.then_some(start))
                            };
                            if let Some(d) = &mut self.doc {
                                set_end(&mut d.objs[i].kind, start, q, shift);
                                d.touch();
                            }
                        }
                    }
                    Drag::Move { idx, last } => {
                        self.snap_move_object(idx, q, last, !shift);
                        self.ed.drag = Drag::Move { idx, last: q };
                    }
                    Drag::Handle { idx, h, anchor } => {
                        let rect = matches!(h, Handle::Corner(_));
                        let q = if shift {
                            self.ed.snap_guides.clear();
                            q
                        } else {
                            self.snap_pointer(q, idx, rect.then_some(anchor))
                        };
                        if let Some(d) = &mut self.doc {
                            match (&mut d.objs[idx].kind, h) {
                                (Kind::Arrow { a, .. } | Kind::Line { a, .. } | Kind::Ruler { a, .. } | Kind::Highlighter { a, .. }, Handle::A) => *a = q,
                                (Kind::Arrow { b, .. } | Kind::Line { b, .. } | Kind::Ruler { b, .. } | Kind::Highlighter { b, .. }, Handle::B) => *b = q,
                                (Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. }, Handle::Corner(_)) => {
                                    *r = R::from_points(anchor, if shift { square(anchor, q) } else { q })
                                }
                                // A picture keeps its proportions; ⇧ stretches it freely.
                                (Kind::Image { pic, r, .. }, Handle::Corner(_)) => {
                                    *r = R::from_points(anchor, if shift { q } else { keep_ratio(anchor, q, pic.img.w as f32 / pic.img.h.max(1) as f32) })
                                }
                                _ => {}
                            }
                            d.touch();
                        }
                    }
                    Drag::Select { start } => {
                        // ⇧ makes it square; holding ⌥ shows the area the selection would snap to.
                        let mut r = R::from_points(start, if shift { square(start, q) } else { q });
                        if let (true, Some(d)) = (alt, &self.doc) {
                            r = selection::smart_adjust(&d.base, r);
                        }
                        self.ed.selection = Some(r);
                    }
                    Drag::Crop { start } => self.ed.crop_draft = Some(R::from_points(start, if shift { square(start, q) } else { q })),
                    Drag::Zoom { start } => {
                        let r = self.ed.rect_screen(&R::from_points(start, q));
                        ui.painter_at(self.ed.canvas).rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
                    }
                    Drag::None => {}
                }
            }
        }
        if resp.drag_stopped() {
            self.ed.snap_guides.clear();
            self.ed.snap_carry = (0.0, 0.0);
            if matches!(self.ed.drag, Drag::Create { .. } | Drag::Handle { .. }) {
                self.detect_blur_text();
                self.keep_lens_inside();
            }
            if let (Drag::Zoom { start }, Some(cs)) = (self.ed.drag, cur) {
                let end = self.ed.to_img(cs);
                let r = R::from_points(start, end);
                if r.w().max(r.h()) * self.ed.zoom >= selection::ZOOM_DRAG_MIN {
                    let (zoom, pan) = selection::zoom_to_rect((self.ed.canvas.width(), self.ed.canvas.height()), r, ui.ctx().pixels_per_point());
                    self.ed.zoom = zoom;
                    self.ed.pan = vec2(pan.0, pan.1);
                    self.ed.fit = false;
                }
            }
            match self.ed.drag {
                Drag::Create { .. } => {
                    if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
                        if d.objs[i].is_empty() && !matches!(d.objs[i].kind, Kind::Text { .. }) {
                            d.objs.remove(i);
                            d.undo.pop();
                            d.touch();
                            self.ed.selected = None;
                        }
                    }
                }
                Drag::Select { .. } => {
                    let r = self.ed.selection.filter(|r| r.w() >= 2.0 && r.h() >= 2.0);
                    self.ed.selection = r;
                    match (self.ed.tool, r) {
                        (Tool::Cut, Some(r)) => {
                            self.ed.selection = None;
                            self.cut_out(r);
                        }
                        (Tool::Ocr, Some(r)) => {
                            self.ed.selection = None;
                            self.ed.tool = Tool::Select;
                            if let Some(d) = &self.doc {
                                let pm = render::crop_pixmap(&render::compose(d, &self.fonts, None), &r);
                                let img = render::from_pixmap(&pm);
                                self.ocr_image(&img);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            self.ed.drag = Drag::None;
        }
        if resp.clicked() {
            let q = self.ed.to_img(cur.unwrap_or_default());
            self.ed.popover = Popover::None;
            if let (Some(h), Some(d)) = (ruler_hold, &mut self.doc) {
                let (a, b) = render::ruler_span(&d.base, q, h, self.ed.ruler_sensitivity);
                d.checkpoint();
                let mut st = self.ed.style_for(Tool::Ruler);
                st.dashed = self.settings.logical_points;
                d.objs.push(Obj { kind: Kind::Ruler { a, b }, style: st });
                return;
            }
            if zoom_key {
                // Z + click: zoom in one step on that spot.
                let ppp = ui.ctx().pixels_per_point();
                let target = zoom_key_target(self.ed.zoom * ppp, true) / ppp;
                let (zoom, pan) = selection::zoom_to_point((self.ed.canvas.width(), self.ed.canvas.height()), target, q, ppp);
                self.ed.zoom = zoom;
                self.ed.pan = vec2(pan.0, pan.1);
                self.ed.fit = false;
                return;
            }
            if cmd && self.ed.tool == Tool::Select {
                // ⌘-click: the flat area under the pointer becomes the selection.
                if let Some(d) = &self.doc {
                    self.ed.selected = None;
                    self.ed.selection = selection::flood_select(&d.base, q.x.floor() as i32, q.y.floor() as i32, selection::FLOOD_TOLERANCE);
                }
                return;
            }
            self.click(q, tol, resp.double_clicked());
        }
    }

    /// Ruler keys: 2 and ←→ measure across, 1 and ↑↓ measure down.
    /// `Some(true)` is horizontal.
    fn ruler_axis(&self, ui: &Ui) -> Option<bool> {
        if self.ed.editing_text {
            return None;
        }
        // Keys that already do something else are not ruler keys: the arrows move a selected object, a
        // selected area or the crop frame; 1-9 set the Spotlight darkness; Cmd+1 and Cmd+2 are zoom.
        let moves_something = self.ed.selected.is_some() || self.ed.selection.is_some() || (self.ed.tool == Tool::Crop && self.ed.crop_draft.is_some());
        let digits = self.context_tool() != Tool::Spotlight;
        ui.input(|i| {
            let m = i.modifiers;
            let digits = digits && !m.command && !m.alt && !m.ctrl;
            let arrows = !moves_something && !m.alt;
            if (digits && i.key_down(Key::Num2)) || (arrows && (i.key_down(Key::ArrowLeft) || i.key_down(Key::ArrowRight))) {
                Some(true)
            } else if (digits && i.key_down(Key::Num1)) || (arrows && (i.key_down(Key::ArrowUp) || i.key_down(Key::ArrowDown))) {
                Some(false)
            } else {
                None
            }
        })
    }

    /// The default lens spot can fall outside a small picture; flip it to the other side then.
    fn keep_lens_inside(&mut self) {
        let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) else { return };
        let (w, h) = (d.base.w as f32, d.base.h as f32);
        let Some(Obj { kind: Kind::Magnifier { r, lens }, .. }) = d.objs.get_mut(i) else { return };
        let view = magnifier_lens(r, *lens);
        if view.x1 > w || view.x0 < 0.0 {
            lens.x = -lens.x;
        }
        if view.y0 < 0.0 || view.y1 > h {
            lens.y = -lens.y;
        }
        d.touch();
    }

    /// Blur Text and Erase Text only touch the text found inside their area.
    fn detect_blur_text(&mut self) {
        let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) else { return };
        let Some(Obj { kind: Kind::Blur { r, text }, style }) = d.objs.get_mut(i) else { return };
        if !matches!(style.blur, BlurMode::BlurText | BlurMode::EraseText) {
            return;
        }
        let area = r.clamp_to(d.base.w as f32, d.base.h as f32);
        if area.w() < 4.0 || area.h() < 4.0 {
            return;
        }
        let crop = render::from_pixmap(&render::crop_pixmap(&render::to_pixmap(&d.base), &area));
        let (dx, dy) = (area.x0 - r.x0, area.y0 - r.y0);
        *text = Some(crate::system::text_boxes(&crop).iter().map(|b| b.translate(dx, dy)).collect());
        d.touch();
    }

    fn top_hit(&self, q: P, tol: f32) -> Option<usize> {
        let d = self.doc.as_ref()?;
        (0..d.objs.len()).rev().find(|i| hit(&d.objs[*i], q, tol, &self.fonts))
    }

    /// `top_hit` as `tool` sees it: Text and Counter place new marks on a pasted picture instead of
    /// grabbing it, so a picture laid over the whole screenshot does not swallow every click.
    fn top_hit_for(&self, tool: Tool, q: P, tol: f32) -> Option<usize> {
        let d = self.doc.as_ref()?;
        let skip_pictures = matches!(tool, Tool::Text | Tool::Counter);
        (0..d.objs.len()).rev().find(|i| !(skip_pictures && matches!(d.objs[*i].kind, Kind::Image { .. })) && hit(&d.objs[*i], q, tol, &self.fonts))
    }

    fn begin_drag(&mut self, q: P, tol: f32) {
        let tool = self.ed.tool;
        if tool == Tool::Crop {
            self.ed.drag = Drag::Crop { start: q };
            return;
        }
        if matches!(tool, Tool::Select | Tool::Cut | Tool::Ocr) {
            if tool == Tool::Select {
                if let Some(i) = self.ed.selected {
                    if let Some(h) = self.handle_at(i, q, tol) {
                        return self.start_handle(i, h);
                    }
                }
                if let Some(i) = self.top_hit(q, tol) {
                    self.ed.selected = Some(i);
                    self.doc.as_mut().unwrap().checkpoint();
                    self.ed.drag = Drag::Move { idx: i, last: q };
                    return;
                }
            }
            self.ed.selected = None;
            self.ed.drag = Drag::Select { start: q };
            self.ed.selection = None;
            return;
        }
        if let Some(i) = self.ed.selected {
            if let Some(h) = self.handle_at(i, q, tol) {
                return self.start_handle(i, h);
            }
            if self.doc.as_ref().map_or(false, |d| i < d.objs.len() && hit(&d.objs[i], q, tol, &self.fonts)) {
                self.doc.as_mut().unwrap().checkpoint();
                self.ed.drag = Drag::Move { idx: i, last: q };
                return;
            }
        }
        if let Some(i) = self.top_hit_for(tool, q, tol) {
            if self.doc.as_ref().unwrap().objs[i].tool() == tool || matches!(tool, Tool::Counter | Tool::Text) {
                self.ed.selected = Some(i);
                self.doc.as_mut().unwrap().checkpoint();
                self.ed.drag = Drag::Move { idx: i, last: q };
                return;
            }
        }
        let style = self.ed.style_for(tool);
        let d = self.doc.as_mut().unwrap();
        let Some(kind) = new_kind(tool, q, d.next_counter()) else { return };
        d.checkpoint();
        d.objs.push(Obj { kind, style });
        self.ed.selected = Some(d.objs.len() - 1);
        self.ed.drag = Drag::Create { start: q };
        if tool == Tool::Text {
            self.ed.editing_text = true;
        }
    }

    /// Drags the annotation at `idx` as the pointer would, for the headless pictures of the guides.
    #[cfg(test)]
    pub(crate) fn preview_snap_drag(&mut self, idx: usize, from: P, to: P) {
        self.snap_move_object(idx, to, from, true);
    }

    fn snap_opts(&self) -> snap::Opts {
        snap::Opts { objects: self.settings.snap_objects, similar: self.settings.snap_similar }
    }

    /// The other annotations as snap targets for the one at `idx`.
    fn snap_targets(&self, idx: usize) -> Vec<snap::Other> {
        match &self.doc {
            Some(d) if idx < d.objs.len() => snap::others_of(&d.objs, Some(idx), &d.objs[idx], &self.fonts),
            _ => vec![],
        }
    }

    /// Moves the annotation by the pointer's step and lets its edges and centres fall onto the others.
    fn snap_move_object(&mut self, idx: usize, q: P, last: P, snapping: bool) {
        let opts = if snapping { self.snap_opts() } else { snap::Opts::default() };
        let tol = snap::SNAP_POINTS / self.ed.zoom;
        let others = if opts.any() { self.snap_targets(idx) } else { vec![] };
        let (cx, cy) = self.ed.snap_carry;
        let Some(d) = &mut self.doc else { return };
        d.objs[idx].translate(q.x - last.x - cx, q.y - last.y - cy);
        let s = snap::snap_move(snap::object_rect(&d.objs[idx], &self.fonts), &others, opts, tol);
        d.objs[idx].translate(s.dx, s.dy);
        d.touch();
        self.ed.snap_carry = (s.dx, s.dy);
        self.ed.snap_guides = s.guides;
    }

    /// A dragged corner or end point `q` of the annotation at `idx`, pulled onto the others' lines.
    fn snap_pointer(&mut self, q: P, idx: usize, anchor: Option<P>) -> P {
        let opts = self.snap_opts();
        self.ed.snap_guides.clear();
        if !opts.any() {
            return q;
        }
        let others = self.snap_targets(idx);
        let s = snap::snap_point(q, anchor, &others, opts, snap::SNAP_POINTS / self.ed.zoom);
        self.ed.snap_guides = s.guides;
        p(q.x + s.dx, q.y + s.dy)
    }

    fn handle_at(&self, i: usize, q: P, tol: f32) -> Option<Handle> {
        let o = self.doc.as_ref()?.objs.get(i)?;
        handles(o).into_iter().find(|(_, hp)| hp.dist(q) <= tol * 1.4).map(|(h, _)| h)
    }

    fn start_handle(&mut self, i: usize, h: Handle) {
        let d = self.doc.as_mut().unwrap();
        let anchor = match (&d.objs[i].kind, h) {
            (Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } | Kind::Image { r, .. }, Handle::Corner(c)) => match c {
                0 => p(r.x1, r.y1),
                1 => p(r.x0, r.y1),
                2 => p(r.x0, r.y0),
                _ => p(r.x1, r.y0),
            },
            _ => P::default(),
        };
        d.checkpoint();
        self.ed.drag = Drag::Handle { idx: i, h, anchor };
    }

    fn click(&mut self, q: P, tol: f32, double: bool) {
        let tool = self.ed.tool;
        let was_editing = self.ed.editing_text;
        self.finish_text();
        let hit_i = self.top_hit_for(tool, q, tol);
        if let Some(i) = hit_i {
            let is_text = matches!(self.doc.as_ref().unwrap().objs[i].kind, Kind::Text { .. });
            self.ed.selected = Some(i);
            if is_text && (double || tool == Tool::Text) {
                self.ed.editing_text = true;
            }
            return;
        }
        if was_editing {
            self.ed.selected = None;
            return;
        }
        match tool {
            // A click with Freehand draws a point.
            Tool::Counter | Tool::Text | Tool::Freehand => {
                let style = self.ed.style_for(tool);
                let d = self.doc.as_mut().unwrap();
                let kind = new_kind(tool, q, d.next_counter()).unwrap();
                d.checkpoint();
                d.objs.push(Obj { kind, style });
                self.ed.selected = Some(d.objs.len() - 1);
                self.ed.editing_text = tool == Tool::Text;
            }
            _ => {
                self.ed.selected = None;
                self.ed.selection = None;
            }
        }
    }

    fn canvas_decorations(&mut self, ui: &mut Ui, painter: &egui::Painter, crop_mode: bool, iw: f32, ih: f32) {
        let (sr, sg, sb) = SELECTION_RGB;
        let accent = Color32::from_rgb((sr * 255.0) as u8, (sg * 255.0) as u8, (sb * 255.0) as u8);
        if crop_mode {
            let full = self.ed.rect_screen(&R { x0: 0.0, y0: 0.0, x1: iw, y1: ih });
            if let Some(c) = self.ed.crop_draft {
                let cs = self.ed.rect_screen(&c);
                let dim = Color32::from_black_alpha(150);
                painter.rect_filled(Rect::from_min_max(full.min, pos2(full.max.x, cs.min.y)), 0.0, dim);
                painter.rect_filled(Rect::from_min_max(pos2(full.min.x, cs.max.y), full.max), 0.0, dim);
                painter.rect_filled(Rect::from_min_max(pos2(full.min.x, cs.min.y), pos2(cs.min.x, cs.max.y)), 0.0, dim);
                painter.rect_filled(Rect::from_min_max(pos2(cs.max.x, cs.min.y), pos2(full.max.x, cs.max.y)), 0.0, dim);
                painter.rect_stroke(cs, 0.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Outside);
                for i in 1..3 {
                    let fx = cs.left() + cs.width() * i as f32 / 3.0;
                    let fy = cs.top() + cs.height() * i as f32 / 3.0;
                    painter.line_segment([pos2(fx, cs.top()), pos2(fx, cs.bottom())], Stroke::new(0.5, Color32::from_white_alpha(120)));
                    painter.line_segment([pos2(cs.left(), fy), pos2(cs.right(), fy)], Stroke::new(0.5, Color32::from_white_alpha(120)));
                }
            } else {
                painter.rect_filled(full, 0.0, Color32::from_black_alpha(60));
            }
        }
        if let Some(r) = self.ed.selection {
            let rs = self.ed.rect_screen(&r);
            painter.rect_filled(rs, 0.0, accent.gamma_multiply(0.12));
            painter.rect_stroke(rs, 0.0, Stroke::new(1.0, accent), StrokeKind::Outside);
            let label = format!("{}×{}", r.w().round(), r.h().round());
            let font = FontId::new(11.0, FontFamily::Name("medium".into()));
            let pos = pos2(rs.left(), rs.bottom() + 6.0);
            let g = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), Color32::WHITE));
            painter.rect_filled(Rect::from_min_size(pos, g.size() + vec2(10.0, 4.0)), 4.0, accent);
            painter.text(pos + vec2(5.0, 2.0), Align2::LEFT_TOP, label, font, Color32::WHITE);
        }
        if let (Some(d), Some(i)) = (&self.doc, self.ed.selected) {
            if let Some(o) = d.objs.get(i) {
                let b = match &o.kind {
                    Kind::Text { at, text } => render::text_box(*at, text, &o.style, &self.fonts),
                    _ => render::bounds(o, &self.fonts),
                };
                let bs = self.ed.rect_screen(&b);
                painter.rect_stroke(bs, 3.0, Stroke::new(1.0, accent.gamma_multiply(0.8)), StrokeKind::Outside);
                for (_, hp) in handles(o) {
                    let s = self.ed.to_screen(hp);
                    painter.circle_filled(s, 5.0, Color32::WHITE);
                    painter.circle_stroke(s, 5.0, Stroke::new(1.5, accent));
                }
                if self.ed.editing_text {
                    let blink = (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0;
                    if blink {
                        if let Kind::Text { at, text } = &o.kind {
                            let size = o.style.text_size();
                            let last = text.split('\n').last().unwrap_or("");
                            let font = match o.style.font {
                                FontKind::Sans => self.fonts.text(&o.style),
                                FontKind::Goofy => &self.fonts.goofy,
                            };
                            let (w, _) = render::measure(font, last, size);
                            let lines = text.split('\n').count().max(1) as f32 - 1.0;
                            let top = p(at.x + w + 1.0, at.y + lines * o.style.text_line_height());
                            let a = self.ed.to_screen(top);
                            let bpt = self.ed.to_screen(top.add(0.0, size * 1.15));
                            painter.line_segment([a, bpt], Stroke::new(2.0, accent));
                        }
                    }
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
                }
            }
        }
        for g in &self.ed.snap_guides {
            let (a, b) = (self.ed.to_screen(g.a), self.ed.to_screen(g.b));
            let guide = Color32::from_rgb(0xFF, 0x2D, 0x78);
            match g.kind {
                snap::GuideKind::Edge | snap::GuideKind::Centre => {
                    painter.line_segment([a, b], Stroke::new(1.5, guide));
                }
                snap::GuideKind::Gap | snap::GuideKind::Size => {
                    painter.line_segment([a, b], Stroke::new(1.5, guide));
                    let cap = if (b.x - a.x).abs() >= (b.y - a.y).abs() { vec2(0.0, 4.0) } else { vec2(4.0, 0.0) };
                    for e in [a, b] {
                        painter.line_segment([e - cap, e + cap], Stroke::new(1.5, guide));
                    }
                }
            }
        }
        let axis = self.ruler_axis(ui);
        if axis.is_none() {
            self.ed.ruler_sensitivity = RULER_SENSITIVITY;
        }
        if let (Some(q), Some(d), Some(horizontal)) = (self.ed.hover, &self.doc, axis) {
            let (a, b) = render::ruler_span(&d.base, q, horizontal, self.ed.ruler_sensitivity);
            let (sa, sb) = (self.ed.to_screen(a), self.ed.to_screen(b));
            painter.line_segment([sa, sb], Stroke::new(1.5, Color32::from_rgb(0xFF, 0x0C, 0x01)));
            let label = render::ruler_label(a, b, d.scale_factor, self.settings.logical_points);
            let font = FontId::new(12.0, FontFamily::Name("medium".into()));
            let mid = pos2((sa.x + sb.x) / 2.0, (sa.y + sb.y) / 2.0) + vec2(8.0, 8.0);
            let g = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), Color32::WHITE));
            painter.rect_filled(Rect::from_min_size(mid, g.size() + vec2(10.0, 4.0)), 4.0, Color32::from_rgb(0xFF, 0x0C, 0x01));
            painter.text(mid + vec2(5.0, 2.0), Align2::LEFT_TOP, label, font, Color32::WHITE);
            ui.ctx().request_repaint();
        }
    }

    fn empty_state(&mut self, ui: &mut Ui, rect: Rect, t: &Th) {
        let card = Rect::from_center_size(rect.center(), vec2(420.0, 260.0));
        ui.painter().rect_filled(card, 14.0, t.bar);
        ui.painter().rect_stroke(card, 14.0, Stroke::new(1.0, t.line), StrokeKind::Inside);
        icons::draw(ui.painter(), Rect::from_center_size(card.center_top() + vec2(0.0, 44.0), vec2(34.0, 34.0)), Icon::Ocr, t.accent);
        ui.painter().text(card.center_top() + vec2(0.0, 82.0), Align2::CENTER_CENTER, "ShotCraft", FontId::new(20.0, FontFamily::Name("medium".into())), t.text);
        ui.painter().text(
            card.center_top() + vec2(0.0, 106.0),
            Align2::CENTER_CENTER,
            "Capture your screen, then annotate, blur and share.",
            FontId::new(12.5, FontFamily::Proportional),
            t.dim,
        );
        let inner = Rect::from_min_size(card.left_top() + vec2(30.0, 132.0), vec2(360.0, 110.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
            let btn = |ui: &mut Ui, label: &str, key: &str| {
                ui.add_sized(
                    [175.0, 34.0],
                    egui::Button::new(egui::RichText::new(format!("{label}   {key}")).size(13.0)).fill(t.seg).stroke(Stroke::new(0.5, t.line)).corner_radius(8.0),
                )
                .clicked()
            };
            ui.horizontal(|ui| {
                if btn(ui, "Area Capture", "⌘⇧2") {
                    self.push(Action::Capture(crate::system::CaptureMode::Area));
                }
                if btn(ui, "Fullscreen", "⌘⇧1") {
                    self.push(Action::Capture(crate::system::CaptureMode::Fullscreen));
                }
            });
            ui.horizontal(|ui| {
                if btn(ui, "Any Window", "") {
                    self.push(Action::Capture(crate::system::CaptureMode::Window));
                }
                if btn(ui, "Open File", "⌘⇧O") {
                    self.push(Action::OpenFile);
                }
            });
        });
        let _ = CornerRadius::ZERO;
    }

    fn popovers(&mut self, ctx: &egui::Context, t: &Th) {
        let pop = self.ed.popover;
        let (anchor, above) = match pop {
            Popover::None => return,
            Popover::Color(r) | Popover::Size(r) | Popover::Zoom(r) => (r.left_top() + vec2(0.0, -6.0), true),
            Popover::Picker(r) | Popover::Resize(r) => (r.left_bottom() + vec2(0.0, 6.0), false),
        };
        let frame = egui::Frame::new().fill(t.bar).stroke(Stroke::new(1.0, t.line)).corner_radius(10.0).inner_margin(14).shadow(egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(90),
        });
        let area = egui::Area::new(egui::Id::new("editor-popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .pivot(if above { Align2::LEFT_BOTTOM } else { Align2::LEFT_TOP })
            .show(ctx, |ui| {
                frame.show(ui, |ui| match pop {
                    Popover::Color(_) => self.color_helper(ui, t),
                    Popover::Size(_) => self.size_helper(ui, t),
                    Popover::Zoom(_) => self.zoom_helper(ui),
                    Popover::Picker(_) => self.color_picker(ui, t),
                    Popover::Resize(_) => self.resize_helper(ui),
                    Popover::None => {}
                });
            });
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx.input(|i| i.pointer.interact_pos()).map_or(false, |p| !area.response.rect.contains(p))
            && !matches!(pop, Popover::Picker(r) | Popover::Color(r) | Popover::Size(r) | Popover::Zoom(r) if ctx.input(|i| i.pointer.interact_pos()).map_or(false, |p| r.contains(p)));
        if clicked_outside && !ctx.any_popup_open() {
            self.ed.popover = Popover::None;
        }
    }

    fn color_helper(&mut self, ui: &mut Ui, t: &Th) {
        ui.set_width(300.0);
        ui.horizontal(|ui| {
            ui.label("Color Format");
            let cur = self.settings.color_format;
            crate::icons::combo("colorfmt").width(170.0).selected_text(render::COLOR_FORMATS[cur.min(render::COLOR_FORMATS.len() - 1)]).show_ui(ui, |ui| {
                for (i, name) in render::COLOR_FORMATS.iter().enumerate() {
                    if ui.selectable_label(cur == i, *name).clicked() {
                        self.settings.color_format = i;
                        self.settings.save();
                    }
                }
            });
        });
        ui.separator();
        for (title, body) in [
            ("Copy Pixel Color", "Hover mouse over any pixel and press Tab"),
            ("Copy Text Color", "Press Shift+Tab to copy the darkest color in the 20px area around the cursor"),
            ("Copy Average Color", "Select an area and press C to copy its average color"),
        ] {
            ui.label(egui::RichText::new(title).family(FontFamily::Name("medium".into())).color(t.text));
            ui.label(egui::RichText::new(body).color(t.dim).size(12.0));
            ui.add_space(4.0);
        }
    }

    fn size_helper(&mut self, ui: &mut Ui, t: &Th) {
        ui.set_width(260.0);
        ui.horizontal(|ui| {
            ui.label("Show Size in Logical Points");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.checkbox(&mut self.settings.logical_points, "").changed() {
                    self.settings.save();
                }
            });
        });
        ui.label(egui::RichText::new("One logical point is equal two physical pixels on Retina displays. Use shortcut P to switch modes.").color(t.dim).size(12.0));
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Resize Image");
            ui.label(egui::RichText::new("(can't be undone!)").color(t.dim).size(12.0));
        });
        let labels: Vec<&str> = RESIZE_STEPS.iter().map(|s| s.0).collect();
        if let Some(i) = text_segments(ui, &labels, 3, t) {
            self.resize_doc(RESIZE_STEPS[i].1);
            self.ed.popover = Popover::None;
        }
    }

    fn zoom_helper(&mut self, ui: &mut Ui) {
        let ppp = ui.ctx().pixels_per_point();
        ui.horizontal(|ui| {
            if ui.button(" − ").clicked() {
                self.push(Action::ZoomOut);
            }
            ui.label(format!("{}%", (self.ed.zoom * ppp * 100.0).round()));
            if ui.button(" + ").clicked() {
                self.push(Action::ZoomIn);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("100%").clicked() {
                self.push(Action::Zoom100);
            }
            if ui.button("Zoom To Fit").clicked() {
                self.push(Action::ZoomFit);
            }
        });
    }

    fn color_picker(&mut self, ui: &mut Ui, t: &Th) {
        let tool = self.context_tool();
        let palette: Vec<Rgba> = if tool == Tool::Highlighter { HIGHLIGHT_PALETTE.iter().map(|s| hex(s)).collect() } else { PALETTE.iter().map(|s| hex(s)).collect() };
        let cur = self.context_style().color;
        ui.horizontal(|ui| {
            for c in palette {
                let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                ui.painter().circle_filled(r.center(), 11.0, c32(c));
                if c[..3] == cur[..3] {
                    ui.painter().circle_stroke(r.center(), 12.5, Stroke::new(2.0, t.accent));
                } else {
                    ui.painter().circle_stroke(r.center(), 11.0, Stroke::new(0.5, t.line));
                }
                if resp.clicked() {
                    self.update_style(|s| s.color = c);
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Custom");
            self.ed.custom_color = c32(cur);
            if ui.color_edit_button_srgba(&mut self.ed.custom_color).changed() {
                let c = self.ed.custom_color.to_srgba_unmultiplied();
                self.update_style(|s| s.color = c);
            }
        });
    }

    fn resize_helper(&mut self, ui: &mut Ui) {
        let Some((w0, h0)) = self.doc.as_ref().map(|d| { let v = self.ed.view_area(d); (v.w(), v.h()) }) else { return };
        ui.horizontal(|ui| {
            let wr = ui.add_sized([60.0, 22.0], egui::TextEdit::singleline(&mut self.ed.resize_w));
            ui.label("×");
            let hr = ui.add_sized([60.0, 22.0], egui::TextEdit::singleline(&mut self.ed.resize_h));
            ui.label("px");
            if wr.changed() {
                if let Ok(w) = self.ed.resize_w.trim().parse::<f32>() {
                    self.ed.resize_h = format!("{}", (w * h0 / w0).round());
                }
            }
            if hr.changed() {
                if let Ok(h) = self.ed.resize_h.trim().parse::<f32>() {
                    self.ed.resize_w = format!("{}", (h * w0 / h0).round());
                }
            }
            if ui.button("Resize").clicked() {
                if let Ok(w) = self.ed.resize_w.trim().parse::<f32>() {
                    if w >= 1.0 {
                        self.resize_doc(w / w0);
                    }
                }
                self.ed.popover = Popover::None;
            }
        });
    }

    fn overlays(&mut self, ctx: &egui::Context, t: &Th) {
        let now = ctx.input(|i| i.time);
        if let Some((msg, until)) = &self.toast {
            if now > *until {
                self.toast = None;
            } else {
                let msg = msg.clone();
                egui::Area::new(egui::Id::new("toast"))
                    .order(egui::Order::Tooltip)
                    .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -(STATUS_H + 18.0)))
                    .show(ctx, |ui| {
                        egui::Frame::new().fill(Color32::from_black_alpha(215)).corner_radius(10.0).inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
                            ui.label(egui::RichText::new(msg).color(Color32::WHITE).size(13.0));
                        });
                    });
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        if self.about_open {
            let mut open = true;
            egui::Window::new("About ShotCraft").open(&mut open).frame(panel_frame(ctx, t)).fade_in(false).fade_out(false).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.heading("ShotCraft");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label(egui::RichText::new("A screenshot tool with an annotation editor. Everything stays on this computer.").color(t.dim));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Font: DM Sans (SIL Open Font License). Icons: Hugeicons free set (MIT).").size(11.0).color(t.dim));
            });
            self.about_open = open;
        }
        if self.shortcuts_open {
            let mut open = true;
            egui::Window::new("Keyboard Shortcuts").open(&mut open).frame(panel_frame(ctx, t)).fade_in(false).fade_out(false).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                // The list is long: a small window scrolls it instead of cutting it off.
                let room = (ctx.viewport_rect().height() - 90.0).max(120.0);
                egui::ScrollArea::vertical().max_height(room).show(ui, |ui| egui::Grid::new("keys").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                    for (k, v) in [
                        ("A T R O L", "Arrow, Text, Rectangle, Oval, Line"),
                        ("D H B S C M K", "Freehand, Highlighter, Blur, Spotlight, Counter, Magnifier, Backdrop"),
                        ("Hold 2 or ←→ / 1 or ↑↓", "Ruler: horizontal / vertical span under the cursor (arrows: nothing selected)"),
                        ("Tab / ⇧Tab", "Copy pixel colour / darkest nearby colour"),
                        ("C (with a selection)", "Copy the average colour"),
                        ("Del", "Delete object or erase the selection"),
                        ("Esc", "Deselect, or copy and close"),
                        ("Esc (scrolling capture)", "Finish it and keep what was captured, from any app"),
                        ("⌘C  ⌘⇧C  ⌘S  ⌘⇧S  ⌘P", "Copy, Copy Image, Save, Save As, Pin to Screen"),
                        ("⌘V", "Paste the clipboard image over the screenshot"),
                        ("⌘⇧P", "Print"),
                        ("⌘W  ⌘⌥W", "Hide the editor, Close All"),
                        ("⌘⇧O  ⌘⇧V", "Open a file, open the clipboard image"),
                        ("⌘,  ⌘Q", "Preferences, Quit"),
                        ("⌘O", "Recognize Text or QR"),
                        ("⌘Z  ⌘⇧Z", "Undo, Redo"),
                        ("⌘+  ⌘-  ⌘0  ⌘1", "Zoom in, out, 100%, fit"),
                        ("⌘2  Q  W", "Zoom to the selection, to its top left, bottom right corner"),
                        ("Z + click or drag", "Zoom in on a spot or an area"),
                        ("Space + drag", "Move around the picture"),
                        ("⌘ + click", "Select the flat area under the pointer"),
                        ("Enter", "Crop to the selection"),
                        ("A  (hold ⌥ while selecting)", "Snap the selection to its content (preview)"),
                        ("B", "Blur the selection"),
                        ("← → ↑ ↓  (⇧ ×10)", "Move the selection or the selected object"),
                        ("⌘ + ← → ↑ ↓  (⇧ ×10)", "Resize the selection"),
                        ("[  ]  (⇧ ×10)", "Shrink, grow the selection"),
                        ("⇧ while drawing", "Square, circle or 45° line"),
                        ("Freehand: ⌘Enter, ⌥↑ ⌥↓", "Stroke variability, smoothness"),
                        ("Highlighter: ⌘Enter", "Round or flat ends"),
                        ("Spotlight: 1 to 9", "How dark the surroundings are"),
                    ] {
                        ui.label(egui::RichText::new(k).family(FontFamily::Name("medium".into())));
                        ui.label(v);
                        ui.end_row();
                    }
                }));
            });
            self.shortcuts_open = open;
        }
    }
}

/// Key and pointer events through the whole editor, the way a user's input reaches it.
#[cfg(test)]
mod key_tests {
    use super::*;
    use crate::selection;
    use egui::{Event, Modifiers, PointerButton};
    use std::sync::Arc;

    const SIZE: (f32, f32) = (1100.0, 760.0);
    const PPP: f32 = 2.0;
    const NONE: Modifiers = Modifiers::NONE;

    fn rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&f(x, y));
            }
        }
        Image { w, h, px: Arc::new(px) }
    }

    /// White picture 400×300 with a blue block at (100..220, 80..160).
    fn picture() -> Image {
        rgba(400, 300, |x, y| if (100..220).contains(&x) && (80..160).contains(&y) { [20, 60, 200, 255] } else { [255, 255, 255, 255] })
    }

    struct Rig {
        ctx: egui::Context,
        app: App,
        t: f64,
    }

    impl Rig {
        fn new() -> Rig {
            let ctx = egui::Context::default();
            crate::setup_fonts(&ctx);
            ctx.set_pixels_per_point(PPP);
            let mut app = App::headless(ctx.clone());
            app.doc = Some(Doc::new(picture(), PPP));
            let mut rig = Rig { ctx, app, t: 0.0 };
            for _ in 0..3 {
                rig.frame(vec![], NONE);
            }
            rig
        }

        fn frame(&mut self, events: Vec<Event>, mods: Modifiers) {
            self.t += 0.05;
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(SIZE.0, SIZE.1)));
            input.time = Some(self.t);
            input.events = vec![Event::ModifiersChanged(mods)];
            input.events.extend(events);
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(input, |ui| app.editor_ui(ui));
            out.textures_delta.clear();
            let queued: Vec<Action> = std::mem::take(&mut *self.app.queue.lock().unwrap());
            for a in queued {
                self.app.editor_action(a);
            }
        }

        fn key(&mut self, key: Key, mods: Modifiers) {
            let ev = |pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: mods };
            self.frame(vec![ev(true)], mods);
            self.frame(vec![ev(false)], NONE);
        }

        fn hold(&mut self, key: Key) {
            self.frame(vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: NONE }], NONE);
        }

        fn release(&mut self, key: Key) {
            self.frame(vec![Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: NONE }], NONE);
        }

        fn at(&self, x: f32, y: f32) -> Pos2 {
            self.app.ed.to_screen(p(x, y))
        }

        fn click(&mut self, to: Pos2, mods: Modifiers) {
            let button = |pressed| Event::PointerButton { pos: to, button: PointerButton::Primary, pressed, modifiers: mods };
            self.frame(vec![Event::PointerMoved(to)], mods);
            self.frame(vec![button(true)], mods);
            self.frame(vec![button(false)], mods);
        }

        fn drag(&mut self, from: Pos2, to: Pos2, mods: Modifiers) {
            let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: mods };
            self.frame(vec![Event::PointerMoved(from)], mods);
            self.frame(vec![button(from, true)], mods);
            for i in 1..=4 {
                self.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 4.0))], mods);
            }
            self.frame(vec![button(to, false)], mods);
            self.frame(vec![], mods);
        }

        fn sel(&self) -> Option<R> {
            self.app.ed.selection
        }
    }

    fn cmd() -> Modifiers {
        Modifiers::COMMAND
    }
    fn shift() -> Modifiers {
        Modifiers::SHIFT
    }
    fn alt() -> Modifiers {
        Modifiers::ALT
    }
    fn cmd_shift() -> Modifiers {
        Modifiers { shift: true, ..Modifiers::COMMAND }
    }
    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> R {
        R { x0, y0, x1, y1 }
    }

    #[test]
    fn cut_out_and_rasterize_on_a_grown_canvas_keep_the_selection_where_it_was() {
        let mut t = Rig::new();
        let (w0, h0) = {
            let d = t.app.doc.as_ref().unwrap();
            (d.base.w, d.base.h)
        };
        let before = {
            let d = t.app.doc.as_mut().unwrap();
            let mut st = Style::default();
            st.color = [230, 40, 40, 255];
            d.objs.push(Obj { kind: Kind::Line { a: p(30.0, 20.0), b: p(-100.0, 20.0) }, style: st });
            d.touch();
            d.base.clone()
        };
        let cv = render::canvas_rect(t.app.doc.as_ref().unwrap(), &t.app.fonts);
        assert!(cv.x0 <= -100.0, "{cv:?}");
        // Cut out a band of the picture's own pixels (columns 40..50 in picture coordinates).
        t.app.cut_out(r(40.0, 0.0, 50.0, h0 as f32 + 40.0));
        let d = t.app.doc.as_ref().unwrap();
        let grown_w = (w0 as f32 - cv.x0) as u32;
        assert_eq!(d.base.w, grown_w - 10);
        // Column 39 of the picture is the column left of the cut; it sits `-cv.x0` into the canvas.
        let ox = (-cv.x0) as i32;
        assert_eq!(d.base.get(ox + 39, 100), before.get(39, 100));
        assert_eq!(d.base.get(ox + 40, 100), before.get(50, 100), "what was right of the cut moved in");
        assert!(w0 > 50);
    }

    fn two_boxes(t: &mut Rig) {
        let d = t.app.doc.as_mut().unwrap();
        d.objs.push(Obj { kind: Kind::Rect { r: r(10.0, 10.0, 60.0, 50.0), seed: 1 }, style: Style::default() });
        d.objs.push(Obj { kind: Kind::Rect { r: r(200.0, 100.0, 250.0, 140.0), seed: 2 }, style: Style::default() });
        t.app.ed.tool = Tool::Select;
        // Selected up front: the options bar changes with the selection, which would move the canvas mid-drag.
        t.app.ed.selected = Some(0);
        t.app.ed.zoom = 1.0;
        t.app.ed.fit = false;
        t.frame(vec![], NONE);
    }

    fn rect_of(t: &Rig, i: usize) -> R {
        match t.app.doc.as_ref().unwrap().objs[i].kind {
            Kind::Rect { r, .. } => r,
            _ => panic!("not a rectangle"),
        }
    }

    #[test]
    fn dragging_an_annotation_snaps_its_edges_to_another_and_shows_guides() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        t.app.settings.snap_objects = true;
        // Grab the top edge and move by (192, 87): left would be 202, top 97.
        let (a, b) = (t.at(35.0, 10.0), t.at(35.0 + 192.0, 10.0 + 87.0));
        t.drag(a, b, NONE);
        let m = rect_of(&t, 0);
        assert_eq!((m.x0, m.y0), (200.0, 100.0), "{m:?}");
        assert_eq!((m.w(), m.h()), (50.0, 40.0), "the size is untouched");
        assert!(t.app.ed.snap_guides.is_empty(), "the guides go with the drag");
    }

    #[test]
    fn guides_are_there_while_the_drag_lasts() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        t.app.settings.snap_objects = true;
        let from = t.at(35.0, 10.0);
        let to = t.at(35.0 + 192.0, 10.0 + 87.0);
        let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: NONE };
        t.frame(vec![Event::PointerMoved(from)], NONE);
        t.frame(vec![button(from, true)], NONE);
        t.frame(vec![Event::PointerMoved(from + (to - from) * 0.5)], NONE);
        t.frame(vec![Event::PointerMoved(to)], NONE);
        assert!(!t.app.ed.snap_guides.is_empty(), "snapped guides while dragging");
        t.frame(vec![button(to, false)], NONE);
        t.frame(vec![], NONE);
        assert!(t.app.ed.snap_guides.is_empty());
    }

    #[test]
    fn deleting_or_undoing_during_a_drag_does_not_leave_a_stale_drag() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        let from = t.at(35.0, 10.0);
        let to = t.at(80.0, 40.0);
        let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: NONE };
        t.frame(vec![Event::PointerMoved(from)], NONE);
        t.frame(vec![button(from, true)], NONE);
        t.frame(vec![Event::PointerMoved(to)], NONE);
        assert!(matches!(t.app.ed.drag, Drag::Move { .. }));
        t.app.doc.as_mut().unwrap().objs.clear();
        // Two frames more with the pointer still down must not index a missing object.
        t.frame(vec![Event::PointerMoved(from)], NONE);
        t.frame(vec![Event::PointerMoved(to)], NONE);
        t.frame(vec![button(to, false)], NONE);
        assert!(matches!(t.app.ed.drag, Drag::None));
    }

    #[test]
    fn a_pasted_picture_does_not_swallow_clicks_of_the_text_and_counter_tools() {
        let mut t = Rig::new();
        let (w, h) = (t.app.doc.as_ref().unwrap().base.w as f32, t.app.doc.as_ref().unwrap().base.h as f32);
        let d = t.app.doc.as_mut().unwrap();
        d.objs.push(Obj { kind: Kind::Image { pic: Pic::new(rgba(10, 10, |_, _| [1, 2, 3, 255])), r: r(0.0, 0.0, w, h), opacity: 1.0 }, style: Style::default() });
        let q = P { x: 50.0, y: 50.0 };
        assert_eq!(t.app.top_hit_for(Tool::Text, q, 4.0), None);
        assert_eq!(t.app.top_hit_for(Tool::Counter, q, 4.0), None);
        assert_eq!(t.app.top_hit_for(Tool::Select, q, 4.0), Some(0));
    }

    #[test]
    fn shift_moves_an_annotation_without_snapping() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        t.app.settings.snap_objects = true;
        let (a, b) = (t.at(35.0, 10.0), t.at(35.0 + 192.0, 10.0 + 87.0));
        t.drag(a, b, shift());
        let m = rect_of(&t, 0);
        assert!((m.x0 - 202.0).abs() < 0.01 && (m.y0 - 97.0).abs() < 0.01, "{m:?}");
    }

    #[test]
    fn snapping_can_be_switched_off() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        t.app.settings.snap_objects = false;
        t.app.settings.snap_similar = false;
        let (a, b) = (t.at(35.0, 10.0), t.at(35.0 + 192.0, 10.0 + 87.0));
        t.drag(a, b, NONE);
        let m = rect_of(&t, 0);
        assert!((m.x0 - 202.0).abs() < 0.01 && (m.y0 - 97.0).abs() < 0.01, "{m:?}");
        assert!(t.app.ed.snap_guides.is_empty());
    }

    #[test]
    fn the_pointer_leaves_a_snap_without_drifting() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        t.app.settings.snap_objects = true;
        let (q0, q1, q2) = (p(35.0, 10.0), p(35.0 + 192.0, 10.0 + 87.0), p(35.0 + 222.0, 10.0 + 87.0));
        t.app.snap_move_object(0, q1, q0, true);
        let m = rect_of(&t, 0);
        assert_eq!((m.x0, m.y0), (200.0, 100.0));
        t.app.snap_move_object(0, q2, q1, true);
        let m = rect_of(&t, 0);
        assert_eq!((m.x0, m.y0), (232.0, 100.0), "x follows the pointer, y is still held by the other box: {m:?}");
        assert_eq!(t.app.ed.snap_carry.0, 0.0, "no leftover pull on x");
    }

    #[test]
    fn drawing_a_rectangle_snaps_its_corner_to_the_other_ones() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        t.app.settings.snap_objects = true;
        t.app.ed.tool = Tool::Rectangle;
        let (a, b) = (t.at(300.0, 200.0), t.at(253.0, 143.0));
        t.drag(a, b, NONE);
        let n = t.app.doc.as_ref().unwrap().objs.len();
        assert_eq!(n, 3);
        let m = rect_of(&t, 2);
        assert_eq!((m.x0, m.y0, m.x1, m.y1), (250.0, 140.0, 300.0, 200.0), "{m:?}");
    }

    #[test]
    fn dragging_a_handle_snaps_and_shift_does_not() {
        let mut t = Rig::new();
        two_boxes(&mut t);
        t.app.settings.snap_objects = true;
        t.app.ed.selected = Some(0);
        let (a, b) = (t.at(60.0, 50.0), t.at(198.0, 98.0));
        t.drag(a, b, NONE);
        let m = rect_of(&t, 0);
        assert_eq!((m.x1, m.y1), (200.0, 100.0), "{m:?}");
        let (a, b) = (t.at(200.0, 100.0), t.at(194.0, 94.0));
        t.app.ed.selected = Some(0);
        t.drag(a, b, shift());
        let m = rect_of(&t, 0);
        assert!(m.x1 < 196.0 && (m.w() - m.h()).abs() < 0.01, "shift leaves the corner under the pointer: {m:?}");
    }

    #[test]
    fn undoing_an_add_capture_drops_the_area_selected_on_the_wider_canvas() {
        let mut t = Rig::new();
        t.app.doc.as_mut().unwrap().append_capture(&rgba(300, 200, |_, _| [9, 9, 9, 255]));
        t.app.ed.selection = Some(r(450.0, 20.0, 650.0, 150.0));
        t.app.ed.crop_draft = Some(r(450.0, 20.0, 650.0, 150.0));
        t.key(Key::Z, cmd());
        assert_eq!(t.app.doc.as_ref().unwrap().base.w, 400);
        assert_eq!(t.sel(), None, "the old selection lies outside the picture now");
        assert_eq!(t.app.ed.crop_draft, None);
        // Redo brings the wider canvas back, and nothing is selected there either.
        t.key(Key::Z, cmd_shift());
        assert_eq!(t.app.doc.as_ref().unwrap().base.w, 700);
        assert_eq!(t.sel(), None);
    }

    #[test]
    fn undo_that_changes_no_size_keeps_the_selection() {
        let mut t = Rig::new();
        t.app.doc.as_mut().unwrap().checkpoint();
        t.app.ed.selection = Some(r(50.0, 50.0, 110.0, 100.0));
        t.key(Key::Z, cmd());
        assert_eq!(t.sel(), Some(r(50.0, 50.0, 110.0, 100.0)));
    }

    #[test]
    fn cmd_v_pastes_once_even_when_egui_reports_it_as_an_event_and_as_a_key() {
        let mut t = Rig::new();
        t.app.queue.lock().unwrap().clear();
        let key = Event::Key { key: Key::V, physical_key: Some(Key::V), pressed: true, repeat: false, modifiers: cmd() };
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(SIZE.0, SIZE.1)));
        input.time = Some(t.t + 0.05);
        input.events = vec![Event::ModifiersChanged(cmd()), Event::Paste("text".into()), key];
        let app = &mut t.app;
        let _ = t.ctx.run_ui(input, |ui| app.editor_ui(ui));
        let pastes = t.app.queue.lock().unwrap().iter().filter(|a| matches!(a, Action::Paste)).count();
        assert_eq!(pastes, 1);
    }

    #[test]
    fn arrows_nudge_the_selection_and_shift_makes_it_ten_pixels() {
        let mut t = Rig::new();
        t.app.ed.selection = Some(r(50.0, 50.0, 110.0, 100.0));
        t.key(Key::ArrowRight, NONE);
        assert_eq!(t.sel(), Some(r(51.0, 50.0, 111.0, 100.0)));
        t.key(Key::ArrowDown, shift());
        assert_eq!(t.sel(), Some(r(51.0, 60.0, 111.0, 110.0)));
        t.key(Key::ArrowLeft, NONE);
        t.key(Key::ArrowUp, NONE);
        assert_eq!(t.sel(), Some(r(50.0, 59.0, 110.0, 109.0)));
    }

    #[test]
    fn command_arrows_resize_the_selection() {
        let mut t = Rig::new();
        t.app.ed.selection = Some(r(50.0, 50.0, 110.0, 100.0));
        t.key(Key::ArrowRight, cmd());
        assert_eq!(t.sel(), Some(r(50.0, 50.0, 111.0, 100.0)));
        t.key(Key::ArrowDown, cmd_shift());
        assert_eq!(t.sel(), Some(r(50.0, 50.0, 111.0, 110.0)));
        t.key(Key::ArrowLeft, cmd_shift());
        t.key(Key::ArrowUp, cmd());
        assert_eq!(t.sel(), Some(r(50.0, 50.0, 101.0, 109.0)));
    }

    #[test]
    fn arrows_still_move_a_selected_object_and_leave_the_selection_alone() {
        let mut t = Rig::new();
        t.app.ed.selection = Some(r(50.0, 50.0, 110.0, 100.0));
        let d = t.app.doc.as_mut().unwrap();
        d.objs.push(Obj { kind: Kind::Rect { r: r(10.0, 10.0, 30.0, 30.0), seed: 1 }, style: Style::default() });
        t.app.ed.selected = Some(0);
        t.key(Key::ArrowRight, shift());
        assert_eq!(t.sel(), Some(r(50.0, 50.0, 110.0, 100.0)));
        let Kind::Rect { r: moved, .. } = t.app.doc.as_ref().unwrap().objs[0].kind else { panic!() };
        assert_eq!(moved, r(20.0, 10.0, 40.0, 30.0));
    }

    #[test]
    fn brackets_grow_and_shrink_the_selection_and_no_longer_open_resize() {
        let mut t = Rig::new();
        t.app.ed.selection = Some(r(50.0, 50.0, 110.0, 100.0));
        t.key(Key::CloseBracket, NONE);
        assert_eq!(t.sel(), Some(r(49.0, 49.0, 111.0, 101.0)));
        t.key(Key::OpenBracket, NONE);
        t.key(Key::OpenBracket, NONE);
        assert_eq!(t.sel(), Some(r(51.0, 51.0, 109.0, 99.0)));
        t.key(Key::CloseBracket, shift());
        assert_eq!(t.sel(), Some(r(41.0, 41.0, 119.0, 109.0)));
        assert!(matches!(t.app.ed.popover, Popover::None), "Shift+] is not the Resize shortcut any more");
        t.key(Key::OpenCurlyBracket, shift());
        assert_eq!(t.sel(), Some(r(51.0, 51.0, 109.0, 99.0)));
        t.key(Key::CloseCurlyBracket, NONE);
        assert_eq!(t.sel(), Some(r(41.0, 41.0, 119.0, 109.0)));
    }

    #[test]
    fn selection_keys_do_nothing_without_a_selection() {
        let mut t = Rig::new();
        let zoom = t.app.ed.zoom;
        for (k, m) in [(Key::ArrowRight, NONE), (Key::CloseBracket, NONE), (Key::Q, NONE), (Key::W, NONE), (Key::Num2, cmd()), (Key::Enter, NONE)] {
            t.key(k, m);
        }
        assert_eq!(t.sel(), None);
        assert_eq!(t.app.ed.zoom, zoom);
        assert!(t.app.doc.as_ref().unwrap().crop.is_none());
        assert_eq!(t.app.ed.tool, Tool::Select);
    }

    #[test]
    fn command_2_zooms_to_the_selection() {
        let mut t = Rig::new();
        let sel = r(100.0, 80.0, 220.0, 160.0);
        t.app.ed.selection = Some(sel);
        t.key(Key::Num2, cmd());
        let canvas = (t.app.ed.canvas.width(), t.app.ed.canvas.height());
        let (zoom, pan) = selection::zoom_to_rect(canvas, sel, PPP);
        assert!((t.app.ed.zoom - zoom).abs() < 1e-4);
        assert!((t.app.ed.pan.x - pan.0).abs() < 1e-3 && (t.app.ed.pan.y - pan.1).abs() < 1e-3);
        assert!(!t.app.ed.fit);
        let c = t.app.ed.canvas.center();
        let mid = t.at(160.0, 120.0);
        assert!((mid - c).length() < 0.01, "the selection is centred");
        let s = t.app.ed.rect_screen(&sel);
        assert!(t.app.ed.canvas.contains_rect(s), "the whole selection is visible");
    }

    #[test]
    fn q_and_w_zoom_on_the_corners_of_the_selection() {
        let mut t = Rig::new();
        t.app.ed.selection = Some(r(100.0, 80.0, 220.0, 160.0));
        t.key(Key::Q, NONE);
        assert!((t.app.ed.zoom * PPP - selection::CORNER_ZOOM).abs() < 1e-3, "{}", t.app.ed.zoom);
        assert!((t.at(100.0, 80.0) - t.app.ed.canvas.center()).length() < 0.01);
        t.key(Key::W, NONE);
        assert!((t.at(220.0, 160.0) - t.app.ed.canvas.center()).length() < 0.01);
        assert_eq!(t.sel(), Some(r(100.0, 80.0, 220.0, 160.0)), "zooming leaves the selection");
    }

    #[test]
    fn the_zoom_menu_actions_work_like_the_keys() {
        let mut t = Rig::new();
        t.app.ed.selection = Some(r(100.0, 80.0, 220.0, 160.0));
        t.app.editor_action(Action::ZoomSelection);
        let zoom = t.app.ed.zoom;
        t.app.editor_action(Action::ZoomFit);
        t.frame(vec![], NONE);
        t.app.editor_action(Action::SelectionTopLeft);
        assert!((t.at(100.0, 80.0) - t.app.ed.canvas.center()).length() < 0.01);
        assert!(zoom > 0.0);
        t.app.editor_action(Action::SelectionBottomRight);
        assert!((t.at(220.0, 160.0) - t.app.ed.canvas.center()).length() < 0.01);
    }

    #[test]
    fn a_smart_selects_the_content_and_b_blurs_and_enter_crops() {
        let mut t = Rig::new();
        t.app.ed.selection = Some(r(60.0, 40.0, 300.0, 250.0));
        t.key(Key::A, NONE);
        assert_eq!(t.sel(), Some(r(100.0, 80.0, 220.0, 160.0)));
        assert_eq!(t.app.ed.tool, Tool::Select);
        t.key(Key::B, NONE);
        assert_eq!(t.sel(), None);
        let d = t.app.doc.as_ref().unwrap();
        assert!(matches!(d.objs.last().map(|o| &o.kind), Some(Kind::Blur { r: b, .. }) if *b == r(100.0, 80.0, 220.0, 160.0)));
        t.app.ed.selection = Some(r(100.0, 80.0, 220.0, 160.0));
        t.key(Key::Enter, NONE);
        assert_eq!(t.app.doc.as_ref().unwrap().crop, Some(r(100.0, 80.0, 220.0, 160.0)));
        assert_eq!(t.sel(), None);
        assert_eq!(t.app.ed.tool, Tool::Select);
    }

    #[test]
    fn a_and_b_are_still_tool_letters_without_a_selection() {
        let mut t = Rig::new();
        t.key(Key::A, NONE);
        assert_eq!(t.app.ed.tool, Tool::Arrow);
        t.key(Key::B, NONE);
        assert_eq!(t.app.ed.tool, Tool::Blur);
    }

    #[test]
    fn dragging_a_selection_with_shift_makes_it_square() {
        let mut t = Rig::new();
        let (a, b) = (t.at(30.0, 30.0), t.at(130.0, 70.0));
        t.drag(a, b, shift());
        let s = t.sel().expect("a selection");
        assert!((s.w() - s.h()).abs() < 0.01, "{s:?}");
        assert!((s.w() - 100.0).abs() < 1.5, "the longer side wins: {s:?}");
    }

    #[test]
    fn holding_option_while_selecting_shows_the_snapped_selection() {
        let mut t = Rig::new();
        let (a, b) = (t.at(60.0, 40.0), t.at(300.0, 250.0));
        t.drag(a, b, alt());
        let s = t.sel().expect("a selection");
        assert_eq!(s, r(100.0, 80.0, 220.0, 160.0));
        t.drag(a, b, NONE);
        let s = t.sel().expect("a selection");
        assert!(s.w() > 200.0 && s.h() > 150.0, "without ⌥ it is the dragged area: {s:?}");
    }

    #[test]
    fn command_click_selects_the_flat_area_under_the_pointer() {
        let mut t = Rig::new();
        let to = t.at(150.0, 100.0);
        t.click(to, cmd());
        assert_eq!(t.sel(), Some(r(100.0, 80.0, 220.0, 160.0)));
        let to = t.at(10.0, 10.0);
        t.click(to, cmd());
        assert_eq!(t.sel(), Some(r(0.0, 0.0, 400.0, 300.0)));
    }

    #[test]
    fn z_click_zooms_in_on_the_spot_and_z_drag_on_the_area() {
        let mut t = Rig::new();
        t.app.ed.zoom = 0.5;
        t.app.ed.fit = false;
        t.frame(vec![], NONE);
        let before = t.app.ed.zoom;
        t.hold(Key::Z);
        let to = t.at(300.0, 200.0);
        t.click(to, NONE);
        t.release(Key::Z);
        assert!((t.app.ed.zoom * PPP - zoom_key_target(before * PPP, true)).abs() < 1e-3, "one zoom-in step");
        assert!((t.at(300.0, 200.0) - t.app.ed.canvas.center()).length() < 0.01, "the spot is centred");
        assert!(t.app.doc.as_ref().unwrap().objs.is_empty() && t.sel().is_none(), "Z does not start a selection or a drawing");

        let mut t = Rig::new();
        t.hold(Key::Z);
        let (a, b) = (t.at(100.0, 80.0), t.at(160.0, 120.0));
        t.drag(a, b, NONE);
        t.release(Key::Z);
        let area = r(100.0, 80.0, 160.0, 120.0);
        let canvas = (t.app.ed.canvas.width(), t.app.ed.canvas.height());
        let (zoom, _) = selection::zoom_to_rect(canvas, area, PPP);
        assert!((t.app.ed.zoom - zoom).abs() / zoom < 0.05, "the area fills the canvas: {} vs {zoom}", t.app.ed.zoom);
        assert!(t.sel().is_none());
    }

    #[test]
    fn freehand_keys_change_variability_and_smoothness() {
        let mut t = Rig::new();
        t.key(Key::D, NONE);
        assert_eq!(t.app.ed.tool, Tool::Freehand);
        t.key(Key::Enter, cmd());
        assert_eq!(t.app.ed.style_for(Tool::Freehand).vary, 1);
        t.key(Key::Enter, cmd());
        t.key(Key::Enter, cmd());
        assert_eq!(t.app.ed.style_for(Tool::Freehand).vary, 0, "wraps round");
        t.key(Key::ArrowUp, alt());
        t.key(Key::ArrowUp, alt());
        t.key(Key::ArrowDown, alt());
        assert_eq!(t.app.ed.style_for(Tool::Freehand).smooth, 1);
        t.key(Key::ArrowDown, alt());
        t.key(Key::ArrowDown, alt());
        assert_eq!(t.app.ed.style_for(Tool::Freehand).smooth, 0);
        assert_eq!(t.sel(), None, "⌥↑ is not a selection nudge");
    }

    #[test]
    fn the_keys_change_the_selected_stroke_too() {
        let mut t = Rig::new();
        t.app.doc.as_mut().unwrap().objs.push(Obj { kind: Kind::Freehand { pts: vec![p(10.0, 10.0), p(20.0, 30.0), p(40.0, 20.0)] }, style: Style::default() });
        t.app.ed.selected = Some(0);
        t.key(Key::ArrowUp, alt());
        t.key(Key::Enter, cmd());
        let st = t.app.doc.as_ref().unwrap().objs[0].style;
        assert_eq!((st.smooth, st.vary), (1, 1));
        let Kind::Freehand { pts } = &t.app.doc.as_ref().unwrap().objs[0].kind else { panic!() };
        assert_eq!(pts[0], p(10.0, 10.0), "the points are not moved by ⌥↑");
    }

    #[test]
    fn highlighter_command_enter_switches_the_cap() {
        let mut t = Rig::new();
        t.key(Key::H, NONE);
        assert!(!t.app.ed.style_for(Tool::Highlighter).round_cap);
        t.key(Key::Enter, cmd());
        assert!(t.app.ed.style_for(Tool::Highlighter).round_cap);
        t.key(Key::Enter, cmd());
        assert!(!t.app.ed.style_for(Tool::Highlighter).round_cap);
    }

    #[test]
    fn spotlight_digits_set_the_darkness_and_other_tools_ignore_them() {
        let mut t = Rig::new();
        t.key(Key::Num5, NONE);
        assert_eq!(t.app.ed.style_for(Tool::Spotlight).dim, SPOTLIGHT_DIM, "not in the spotlight tool");
        t.key(Key::S, NONE);
        assert_eq!(t.app.ed.tool, Tool::Spotlight);
        t.key(Key::Num3, NONE);
        assert_eq!(t.app.ed.style_for(Tool::Spotlight).dim, 0.3);
        t.app.doc.as_mut().unwrap().objs.push(Obj { kind: Kind::Spotlight { r: r(10.0, 10.0, 90.0, 60.0) }, style: t.app.ed.style_for(Tool::Spotlight) });
        t.key(Key::Num8, NONE);
        assert_eq!(t.app.doc.as_ref().unwrap().objs[0].style.dim, 0.8);
        t.key(Key::Num0, NONE);
        t.key(Key::Num1, cmd());
        assert_eq!(t.app.doc.as_ref().unwrap().objs[0].style.dim, 0.8, "0 and ⌘1 are not levels");
    }

    #[test]
    fn a_click_with_freehand_draws_a_point() {
        let mut t = Rig::new();
        t.key(Key::D, NONE);
        let to = t.at(150.0, 100.0);
        t.click(to, NONE);
        let d = t.app.doc.as_ref().unwrap();
        assert_eq!(d.objs.len(), 1);
        let Kind::Freehand { pts } = &d.objs[0].kind else { panic!("{:?}", d.objs.len()) };
        assert_eq!(pts.len(), 1);
        assert!((pts[0].x - 150.0).abs() < 0.6 && (pts[0].y - 100.0).abs() < 0.6);
        assert!(hit(&d.objs[0], pts[0], 1.0, &t.app.fonts), "the point can be picked");
    }
}

#[cfg(test)]
mod style_memory_tests {
    use super::*;
    use crate::settings::{Settings, ToolStyle};

    fn all_tools() -> impl Iterator<Item = Tool> {
        Tool::TOOLBAR.into_iter()
    }

    #[test]
    fn a_fresh_install_keeps_the_factory_styles() {
        let fresh = Editor::from_settings(&Settings::default());
        let factory = Editor::default();
        for t in all_tools() {
            assert_eq!(fresh.style_for(t), factory.style_for(t), "{t:?}");
        }
        assert_eq!(fresh.default_tool, None);
        assert_eq!(fresh.stored_styles().len(), Tool::TOOLBAR.len().min(fresh.styles.len()));
    }

    #[test]
    fn every_tools_style_survives_a_save_and_load() {
        let mut ed = Editor::default();
        for (n, t) in all_tools().enumerate() {
            let mut st = ed.style_for(t);
            st.color = [(n * 17) as u8, 200, (255 - n * 9) as u8, 255];
            st.size = (n % 4) as u8;
            st.dashed = n % 2 == 0;
            st.arrow = ARROW_STYLES[n % 4];
            st.fill = FILLS[n % 4];
            st.shape = SHAPES[n % 2];
            st.font = FONTS[n % 2];
            st.blur = BlurMode::ALL[n % BlurMode::ALL.len()];
            st.smooth = (n as u8) % (FREEHAND_SMOOTH_MAX + 1);
            st.vary = (n % FREEHAND_VARY.len()) as u8;
            st.round_cap = n % 3 == 0;
            st.dim = 0.1 * (n % 9) as f32;
            ed.styles.insert(t, st);
        }
        let mut settings = Settings::default();
        settings.tool_styles = ed.stored_styles();
        // Through the file and back, as a restart does.
        let settings: Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        let back = Editor::from_settings(&settings);
        for t in all_tools() {
            let (a, b) = (ed.style_for(t), back.style_for(t));
            assert_eq!((a.color, a.size, a.dashed, a.arrow, a.fill, a.shape, a.font, a.blur), (b.color, b.size, b.dashed, b.arrow, b.fill, b.shape, b.font, b.blur), "{t:?}");
            assert_eq!((a.smooth, a.vary, a.round_cap), (b.smooth, b.vary, b.round_cap), "{t:?}");
            assert!((a.dim - b.dim).abs() < 1e-6, "{t:?}");
        }
    }

    #[test]
    fn a_hand_edited_style_cannot_break_the_editor() {
        let base = Style::default();
        let junk = ToolStyle { color: "red".into(), size: 250, arrow: 99, fill: 99, shape: 99, font: 99, blur: 99, smooth: 99, vary: 99, dim: f32::NAN, ..ToolStyle::default() };
        let st = style_from_stored(&junk, &base);
        assert_eq!(st.color, base.color);
        assert_eq!((st.size, st.arrow, st.fill, st.shape, st.font, st.blur), (base.size, base.arrow, base.fill, base.shape, base.font, base.blur));
        assert!(st.smooth <= FREEHAND_SMOOTH_MAX && (st.vary as usize) < FREEHAND_VARY.len());
        assert_eq!(st.dim, base.dim);
        let loud = style_from_stored(&ToolStyle { dim: 7.0, color: "#00ff7f".into(), ..ToolStyle::default() }, &base);
        assert_eq!((loud.dim, loud.color), (1.0, [0, 255, 127, 255]));
    }

    #[test]
    fn a_stored_style_for_one_tool_leaves_the_others_alone() {
        let mut settings = Settings::default();
        settings.tool_styles.insert("Oval".into(), ToolStyle { size: 3, color: "#123456".into(), ..ToolStyle::default() });
        settings.tool_styles.insert("Gone Tool".into(), ToolStyle::default());
        let ed = Editor::from_settings(&settings);
        let factory = Editor::default();
        assert_eq!(ed.style_for(Tool::Oval).size, 3);
        assert_eq!(ed.style_for(Tool::Oval).color, [0x12, 0x34, 0x56, 255]);
        assert_eq!(ed.style_for(Tool::Rectangle), factory.style_for(Tool::Rectangle));
    }

    #[test]
    fn the_default_tool_is_picked_by_name() {
        let mut settings = Settings::default();
        settings.default_tool = "Arrow".into();
        assert_eq!(Editor::from_settings(&settings).default_tool, Some(Tool::Arrow));
        settings.default_tool = "Backdrop".into();
        assert_eq!(Editor::from_settings(&settings).default_tool, None, "Backdrop is not a tool to start with");
        settings.default_tool = "no such tool".into();
        assert_eq!(Editor::from_settings(&settings).default_tool, None);
    }

    #[test]
    fn tool_names_are_unique_keys() {
        let mut names: Vec<&str> = all_tools().map(|t| t.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Tool::TOOLBAR.len(), "{names:?}");
    }

    #[test]
    fn a_style_the_user_changes_lands_in_the_settings() {
        crate::headless::with_rig(|rig| {
            let mut app = rig.app();
            app.doc = Some(crate::model::Doc::new(Image { w: 40, h: 30, px: std::sync::Arc::new(vec![255; 40 * 30 * 4]) }, 1.0));
            app.ed.tool = Tool::Rectangle;
            app.update_style(|st| st.size = 3);
            app.remember_styles();
            assert_eq!(app.settings.tool_styles["Rectangle"].size, 3);
            assert_eq!(app.settings.tool_styles["Oval"], Editor::default().stored_styles()["Oval"]);
            // And a new launch reads it back.
            assert_eq!(Editor::from_settings(&app.settings).style_for(Tool::Rectangle).size, 3);
        });
    }
}

#[cfg(test)]
mod audit_integration {
    use super::*;
    use egui::{Event, Modifiers};
    use std::sync::Arc;

    fn rgba(w: u32, h: u32) -> Image {
        Image { w, h, px: Arc::new(vec![255; (w * h * 4) as usize]) }
    }

    /// One frame; returns what the editor queued (not drained) and the ruler axis it sees.
    fn frame(app: &mut App, ctx: &egui::Context, t: &mut f64, events: Vec<Event>, mods: Modifiers) -> (Vec<Action>, Option<bool>) {
        *t += 0.05;
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 760.0)));
        input.time = Some(*t);
        input.events = vec![Event::ModifiersChanged(mods)];
        input.events.extend(events);
        let mut axis = None;
        let mut out = ctx.run_ui(input, |ui| {
            app.editor_ui(ui);
            axis = app.ruler_axis(ui);
        });
        out.textures_delta.clear();
        (std::mem::take(&mut *app.queue.lock().unwrap()), axis)
    }

    fn rig() -> (App, egui::Context, f64) {
        let ctx = egui::Context::default();
        crate::setup_fonts(&ctx);
        ctx.set_pixels_per_point(2.0);
        let mut app = App::headless(ctx.clone());
        app.doc = Some(Doc::new(rgba(400, 300), 2.0));
        let mut t = 0.0;
        for _ in 0..3 {
            frame(&mut app, &ctx, &mut t, vec![], Modifiers::NONE);
        }
        (app, ctx, t)
    }

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers }
    }

    /// AUDIT: egui-winit 0.36 turns Cmd+C into Event::Copy and drops the Key event, so `Key::C if cmd` is never reached on a Mac.
    #[test]
    fn audit_cmd_c_arrives_as_event_copy_and_copies_the_image() {
        let (mut app, ctx, mut t) = rig();
        let (queued, _) = frame(&mut app, &ctx, &mut t, vec![Event::Copy], Modifiers::COMMAND);
        assert!(queued.iter().any(|a| matches!(a, Action::Copy)), "Event::Copy (what Cmd+C becomes under egui-winit) copies nothing");
    }

    /// AUDIT: the editor's own fallback maps Cmd+Shift+P to Pin although the menu says Print.
    #[test]
    fn audit_cmd_shift_p_is_print_not_pin() {
        let (mut app, ctx, mut t) = rig();
        let m = Modifiers { command: true, shift: true, mac_cmd: true, ..Modifiers::NONE };
        let (queued, _) = frame(&mut app, &ctx, &mut t, vec![key(Key::P, true, m)], m);
        assert!(!queued.iter().any(|a| matches!(a, Action::Pin)), "Cmd+Shift+P queued Pin");
    }

    /// AUDIT: Cmd+Opt+W (Close All) reaches the Close handler of the editor.
    #[test]
    fn audit_cmd_alt_w_is_not_plain_close() {
        let (mut app, ctx, mut t) = rig();
        let m = Modifiers { command: true, alt: true, mac_cmd: true, ..Modifiers::NONE };
        let (queued, _) = frame(&mut app, &ctx, &mut t, vec![key(Key::W, true, m)], m);
        assert!(!queued.iter().any(|a| matches!(a, Action::Close)), "Cmd+Opt+W queued Close");
    }

    /// AUDIT: arrows move the selection (#16) and also hold the ruler (#14-era code).
    #[test]
    fn audit_arrow_on_selection_does_not_also_measure() {
        let (mut app, ctx, mut t) = rig();
        app.ed.selection = Some(R { x0: 50.0, y0: 50.0, x1: 150.0, y1: 150.0 });
        let (_, axis) = frame(&mut app, &ctx, &mut t, vec![key(Key::ArrowRight, true, Modifiers::NONE)], Modifiers::NONE);
        assert_eq!(app.ed.selection.unwrap().x0, 51.0, "the arrow moved the selection");
        assert_eq!(axis, None, "and the same key shows the horizontal ruler");
    }

    /// AUDIT: Cmd+1 (zoom to fit) / Cmd+2 (zoom to selection) also hold the ruler.
    #[test]
    fn audit_cmd_digits_do_not_hold_the_ruler() {
        let (mut app, ctx, mut t) = rig();
        let m = Modifiers { command: true, mac_cmd: true, ..Modifiers::NONE };
        let (_, axis) = frame(&mut app, &ctx, &mut t, vec![key(Key::Num1, true, m)], m);
        assert_eq!(axis, None, "Cmd+1 held shows the vertical ruler");
    }

    /// AUDIT: Spotlight's 1-9 keys (#16) are also the ruler keys.
    #[test]
    fn audit_spotlight_digits_do_not_hold_the_ruler() {
        let (mut app, ctx, mut t) = rig();
        app.set_tool(Tool::Spotlight);
        let (_, axis) = frame(&mut app, &ctx, &mut t, vec![key(Key::Num2, true, Modifiers::NONE)], Modifiers::NONE);
        assert_eq!(axis, None, "'2' sets the spotlight darkness and also shows the horizontal ruler");
    }
}

#[cfg(test)]
mod shortcuts_window_tests {
    /// The Keyboard Shortcuts window (every menu shortcut is in it) is drawn; with SHOTCRAFT_SHOTS set
    /// the picture is written there to look at.
    #[test]
    fn the_shortcuts_window_lists_the_menu_shortcuts_and_fits_a_small_editor() {
        let size = (1100.0, 480.0);
        let img = crate::headless::with_rig(|rig| {
            let mut app = rig.app();
            app.shortcuts_open = true;
            rig.render_editor(&mut app, size, 2.0, true)
        });
        if let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") {
            crate::headless::save_png(&img, &std::path::Path::new(&dir).join("shortcuts-window.png"));
        }
        assert_eq!((img.w, img.h), (2200, 960));
    }
}

/// The toolbar, options bar and status bar at several window widths, in both appearances. With
/// SHOTCRAFT_SHOTS set, the pictures are written there (prefix: SHOTCRAFT_SHOT_PREFIX) to look at.
#[cfg(test)]
mod toolbar_look_tests {
    use super::*;
    use std::sync::Arc;

    fn picture() -> Image {
        let (w, h) = (640u32, 400u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let c = if y < 28 { [60, 63, 70] } else if (x / 40 + y / 40) % 2 == 0 { [248, 248, 250] } else { [226, 232, 242] };
                px.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        Image { w, h, px: Arc::new(px) }
    }

    fn shot(size: (f32, f32), dark: bool, tool: Tool) -> Image {
        crate::headless::with_rig(|rig| {
            let mut app = rig.app();
            app.doc = Some(Doc::new(picture(), 2.0));
            if tool == Tool::Backdrop {
                app.doc.as_mut().unwrap().backdrop = Some(Backdrop::default());
            }
            app.ed.tool = tool;
            rig.render_editor(&mut app, size, 2.0, dark)
        })
    }

    /// Clicks the "More" button of a narrow toolbar and returns what is on screen.
    fn overflow_menu_shot(size: (f32, f32), at: egui::Pos2) -> Image {
        use egui::{Event, Modifiers, PointerButton};
        crate::headless::with_rig(|rig| {
            let mut app = rig.app();
            app.doc = Some(Doc::new(picture(), 2.0));
            let events = vec![
                Event::PointerMoved(at),
                Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
                Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
            ];
            rig.render_window(size, 2.0, events, |ui| app.editor_ui(ui));
            // The menu fades in: let it finish.
            rig.render_window(size, 2.0, vec![], |ui| app.editor_ui(ui))
        })
    }

    #[test]
    fn overflow_menu_picture_for_the_eyes() {
        let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") else { return };
        let size = (480.0, 270.0);
        // The More button ends the centred groups.
        let fit = toolbar_fit(size.0);
        let at = egui::pos2(toolbar_cluster_left(size.0, fit) + fit.cluster_width() - seg_group_w(1) / 2.0, 19.0);
        let img = overflow_menu_shot(size, at);
        crate::headless::save_png(&img, &std::path::Path::new(&dir).join("after-480-more-menu.png"));
    }

    /// Where the drag handle sits in a toolbar of this width.
    fn drag_handle_center(width: f32) -> egui::Pos2 {
        let left = toolbar_cluster_left(width, toolbar_fit(width)) + seg_group_w(crate::sizing::TOOLBAR_COPY_ITEMS) + GROUP_GAP;
        egui::pos2(left + TOOLBAR_DRAG_W / 2.0, 19.0)
    }

    fn pull(app: &mut App, rig: &mut crate::headless::Rig, size: (f32, f32), from: egui::Pos2) {
        use egui::{Event, Modifiers, PointerButton};
        let events = vec![Event::PointerMoved(from), Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }];
        rig.render_window(size, 2.0, events, |ui| app.editor_ui(ui));
        let to = from + egui::vec2(30.0, 25.0);
        rig.render_window(size, 2.0, vec![Event::PointerMoved(to)], |ui| app.editor_ui(ui));
        // The rig's context is shared by every test: leave no button held.
        let up = vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }, Event::PointerGone];
        rig.render_window(size, 2.0, up, |ui| app.editor_ui(ui));
    }

    #[test]
    fn pulling_the_drag_handle_starts_a_drag_and_clicking_it_does_not() {
        use egui::{Event, Modifiers, PointerButton};
        let size = (760.0, 420.0);
        let at = drag_handle_center(size.0);
        crate::headless::with_rig(|rig| {
            // Pulled: the drag is attempted. There is no native drag session off macOS, so it
            // reports that, which proves the toolbar reached the drag code.
            let mut app = rig.app();
            app.doc = Some(Doc::new(picture(), 2.0));
            pull(&mut app, rig, size, at);
            let msg = app.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
            assert!(cfg!(target_os = "macos") || msg.contains("Can't drag the image"), "toast: {msg:?}");
            // A plain click does nothing.
            let mut app = rig.app();
            app.doc = Some(Doc::new(picture(), 2.0));
            let down = vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }];
            rig.render_window(size, 2.0, down, |ui| app.editor_ui(ui));
            let up = vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }];
            rig.render_window(size, 2.0, up, |ui| app.editor_ui(ui));
            assert!(app.toast.is_none());
            rig.render_window(size, 2.0, vec![Event::PointerGone], |ui| app.editor_ui(ui));
            // Without a picture there is nothing to drag.
            let mut app = rig.app();
            pull(&mut app, rig, size, at);
            assert!(app.toast.is_none());
        });
    }

    #[test]
    fn drag_handle_picture_for_the_eyes() {
        let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") else { return };
        for (w, h) in [(1180.0, 640.0), (760.0, 420.0), (480.0, 270.0)] {
            for dark in [false, true] {
                let img = shot((w, h), dark, Tool::Select);
                let name = format!("after-handle-{}-{}.png", w as u32, if dark { "dark" } else { "light" });
                crate::headless::save_png(&img, &std::path::Path::new(&dir).join(name));
            }
        }
        // Hovered: tooltip and highlight.
        let size = (1180.0, 640.0);
        let at = drag_handle_center(size.0);
        let img = crate::headless::with_rig(|rig| {
            let mut app = rig.app();
            app.doc = Some(Doc::new(picture(), 2.0));
            rig.render_window(size, 2.0, vec![egui::Event::PointerMoved(at)], |ui| app.editor_ui(ui));
            let img = rig.render_window(size, 2.0, vec![], |ui| app.editor_ui(ui));
            rig.render_window(size, 2.0, vec![egui::Event::PointerGone], |ui| app.editor_ui(ui));
            img
        });
        crate::headless::save_png(&img, &std::path::Path::new(&dir).join("after-handle-hover.png"));
    }

    #[test]
    fn toolbar_pictures_for_the_eyes() {
        let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") else { return };
        let prefix = std::env::var("SHOTCRAFT_SHOT_PREFIX").unwrap_or_else(|_| "after".into());
        for (w, h) in [(1180.0, 640.0), (760.0, 420.0), (480.0, 270.0)] {
            for dark in [false, true] {
                for tool in [Tool::Select, Tool::Arrow, Tool::Text, Tool::Backdrop, Tool::Rectangle] {
                    let img = shot((w, h), dark, tool);
                    let name = format!("{prefix}-{}-{}-{}.png", w as u32, if dark { "dark" } else { "light" }, tool.name().to_lowercase());
                    crate::headless::save_png(&img, &std::path::Path::new(&dir).join(name));
                }
            }
        }
    }
}
