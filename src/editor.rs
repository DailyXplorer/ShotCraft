use crate::icons::{self, Icon};
use crate::model::*;
use crate::render;
use crate::selection::{self, snap45, square, Corner};
use crate::tiled::{Alpha, TiledTexture};
use crate::{Action, App};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, FontFamily, FontId, Key, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use std::collections::HashMap;
use tiny_skia::Pixmap;

pub const TOOLBAR_H: f32 = 38.0;
pub const OPTIONS_H: f32 = 44.0;
pub const STATUS_H: f32 = 34.0;
pub const MIN_W: f32 = 940.0;
const TRAFFIC_LIGHTS_W: f32 = 78.0;
const SEG_W: f32 = 30.0;
const GROUP_GAP: f32 = 10.0;
const TOOLBAR_EDGE: f32 = 12.0;

fn seg_group_w(items: usize) -> f32 {
    SEG_W * items as f32 + 4.0
}

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
    hover_color: Rgba,
    comp: Option<Comp>,
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
            hover: None,
            hover_color: [255, 255, 255, 255],
            comp: None,
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
    }

    fn style_for(&self, t: Tool) -> Style {
        Style { scale: self.scale, ..self.styles.get(&t).copied().unwrap_or_default() }
    }

    fn to_screen(&self, q: P) -> Pos2 {
        self.canvas.min + self.pan + vec2(q.x, q.y) * self.zoom
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
        Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } => vec![
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
        Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } => {
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
            Action::Undo => {
                if let Some(d) = &mut self.doc {
                    d.undo();
                    self.ed.selected = None;
                    self.ed.editing_text = false;
                }
            }
            Action::Redo => {
                if let Some(d) = &mut self.doc {
                    d.redo();
                    self.ed.selected = None;
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

    fn set_tool(&mut self, t: Tool) {
        self.finish_text();
        self.ed.selected = None;
        self.ed.popover = Popover::None;
        if t == Tool::Backdrop {
            if let Some(d) = &mut self.doc {
                if d.backdrop.is_none() {
                    d.checkpoint();
                    d.backdrop = Some(Backdrop::default());
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

    fn delete_selected(&mut self) {
        if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
            if i < d.objs.len() {
                d.checkpoint();
                d.objs.remove(i);
            }
            self.ed.selected = None;
            self.ed.editing_text = false;
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
                        _ => {}
                    }
                }
            }
            if stop {
                self.finish_text();
            }
            return;
        }
        for e in &events {
            let egui::Event::Key { key, pressed: true, modifiers, repeat, .. } = e else { continue };
            let (cmd, shift, alt) = (modifiers.command, modifiers.shift, modifiers.alt);
            let plain = !cmd && !modifiers.ctrl && !alt;
            let sel_keys = self.ed.tool == Tool::Select && self.ed.selected.is_none() && self.ed.selection.is_some();
            let tool_now = self.context_tool();
            match key {
                Key::Z if cmd && shift => self.push(Action::Redo),
                Key::Z if cmd => self.push(Action::Undo),
                Key::C if cmd => self.push(Action::Copy),
                Key::S if cmd && shift => self.push(Action::SaveAs),
                Key::S if cmd => self.push(Action::Save),
                Key::P if cmd => self.push(Action::Pin),
                Key::O if cmd && !shift => self.set_tool(Tool::Ocr),
                Key::Equals | Key::Plus if cmd => self.push(Action::ZoomIn),
                Key::Minus if cmd => self.push(Action::ZoomOut),
                Key::Num0 if cmd => self.push(Action::Zoom100),
                Key::Num1 if cmd => self.push(Action::ZoomFit),
                Key::Num2 if cmd => self.push(Action::ZoomSelection),
                Key::Q if plain && self.ed.selection.is_some() => self.push(Action::SelectionTopLeft),
                Key::W if plain && self.ed.selection.is_some() => self.push(Action::SelectionBottomRight),
                Key::A if plain && sel_keys => self.smart_select(),
                Key::B if plain && sel_keys => self.blur_selection(),
                Key::W if cmd => self.push(Action::Close),
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
        if self.doc.is_some() && !matches!(self.context_tool(), Tool::Select | Tool::Cut | Tool::Ocr) {
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
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(0.0, 5.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = GROUP_GAP;
                let groups_w = seg_group_w(3) + seg_group_w(Tool::TOOLBAR.len()) + seg_group_w(7) + GROUP_GAP * 2.0;
                let free = rect.width() - TRAFFIC_LIGHTS_W - seg_group_w(1) - TOOLBAR_EDGE;
                ui.add_space(TRAFFIC_LIGHTS_W + ((free - groups_w) / 2.0).max(0.0));
                if let Some(i) = segments(
                    ui,
                    &[(Icon::Copy, "Copy (Esc or ⌘C)", false), (Icon::Save, "Save (⌘S)", false), (Icon::Pin, "Pin to Screen (⌘P)", false)],
                    t,
                    SEG_W,
                ) {
                    self.push([Action::Copy, Action::Save, Action::Pin].into_iter().nth(i).unwrap());
                }
                let current = self.ed.tool;
                let tools: Vec<(Icon, &str, bool)> = Tool::TOOLBAR.iter().map(|x| (Icon::for_tool(*x), x.tooltip(), current == *x)).collect();
                if let (Some(i), true) = (segments(ui, &tools, t, SEG_W), has_doc) {
                    self.set_tool(Tool::TOOLBAR[i]);
                }
                let picked = segments(
                    ui,
                    &[
                        (Icon::Crop, "Crop", current == Tool::Crop),
                        (Icon::Resize, "Resize", matches!(self.ed.popover, Popover::Resize(_))),
                        (Icon::Ocr, "Recognize Text or QR (⌘O)", current == Tool::Ocr),
                        (Icon::SelSize, "Imprint Selection Size", false),
                        (Icon::Erase, "Erase (Del)", false),
                        (Icon::Scissors, "Cut Out", current == Tool::Cut),
                        (Icon::Reset, "Reset Selection (Esc)", false),
                    ],
                    t,
                    SEG_W,
                );
                if let (Some(i), true) = (picked, has_doc) {
                    let anchor = Rect::from_min_size(ui.min_rect().right_bottom() + vec2(-180.0, 4.0), vec2(1.0, 1.0));
                    match i {
                        0 if current == Tool::Crop => self.apply_crop(),
                        0 => self.set_tool(Tool::Crop),
                        1 => {
                            if let Some(d) = &self.doc {
                                let v = d.view_rect();
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
    }

    fn options_content(&mut self, ui: &mut Ui, t: &Th) {
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

    fn backdrop_options(&mut self, ui: &mut Ui, t: &Th) {
        let Some(d) = &mut self.doc else { return };
        let Some(mut bd) = d.backdrop else {
            if ui.button("Add Gradient Background").clicked() {
                d.checkpoint();
                d.backdrop = Some(Backdrop::default());
            }
            return;
        };
        let before = bd;
        crate::icons::combo("gradkind").width(150.0).selected_text(bd.kind.label()).show_ui(ui, |ui| {
            for k in GradientKind::ALL {
                ui.selectable_value(&mut bd.kind, k, k.label());
            }
        });
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
        if icon_btn(ui, Icon::Revert, false, "Reverse colors", t, 26.0).clicked() {
            bd.reverse = !bd.reverse;
        }
        let slider = |ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>| {
            ui.label(egui::RichText::new(label).color(t.dim).size(12.0));
            ui.add_sized([78.0, 20.0], egui::Slider::new(v, range).show_value(false));
        };
        slider(ui, "Backdrop", &mut bd.padding, 0.0..=BACKDROP_SLIDER_MAX);
        slider(ui, "Shadow", &mut bd.shadow, 0.0..=BACKDROP_SLIDER_MAX);
        slider(ui, "Inset", &mut bd.inset, 0.0..=BACKDROP_SLIDER_MAX);
        slider(ui, "Outer Radius", &mut bd.outer_radius, 0.0..=BACKDROP_SLIDER_MAX);
        slider(ui, "Inner Radius", &mut bd.inner_radius, 0.0..=BACKDROP_SLIDER_MAX);
        let ratio_label = RATIOS.iter().find(|(_, r)| *r == bd.ratio && r.is_some()).map(|x| x.0).unwrap_or("•••");
        crate::icons::combo("ratio").width(70.0).selected_text(ratio_label).show_ui(ui, |ui| {
            for (name, r) in RATIOS {
                if ui.selectable_label(bd.ratio == r && r.is_some(), name).clicked() {
                    bd.ratio = r;
                }
            }
        });
        let remove = ui.button("Remove").clicked();
        let ok = ui.button("Ok").clicked();
        if remove {
            d.checkpoint();
            d.backdrop = None;
            self.ed.tool = Tool::Select;
            return;
        }
        if bd != before {
            if self.ed.drag == Drag::None && !ui.ctx().egui_is_using_pointer() {
                d.checkpoint();
            } else {
                d.touch();
            }
            d.backdrop = Some(bd);
        }
        if ok {
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
        let (r, resp) = cell(ui, &mut x, 140.0, "st-color");
        let sw = Rect::from_min_size(pos2(r.left() + 6.0, r.center().y - 9.0), vec2(18.0, 18.0));
        ui.painter().rect_filled(sw, 4.0, c32(self.ed.hover_color));
        ui.painter().rect_stroke(sw, 4.0, Stroke::new(1.0, t.line), StrokeKind::Inside);
        let txt = render::format_color(self.ed.hover_color, self.settings.color_format);
        ui.painter().text(pos2(r.left() + 30.0, r.top() + 4.0), Align2::LEFT_TOP, txt, value.clone(), t.text);
        ui.painter().text(pos2(r.left() + 30.0, r.bottom() - 4.0), Align2::LEFT_BOTTOM, "Tab to copy", small.clone(), t.dim);
        if resp.clicked() {
            self.ed.popover = if matches!(self.ed.popover, Popover::Color(_)) { Popover::None } else { Popover::Color(r) };
        }
        sep(ui, &mut x);
        let sel = self.ed.selection.or(self.ed.crop_draft).or_else(|| self.doc.as_ref().map(|d| d.view_rect()));
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
        let ppp = ui.ctx().pixels_per_point();
        let (r, resp) = cell(ui, &mut x, 64.0, "st-zoom");
        let zt = if self.doc.is_some() { format!("{}%", (self.ed.zoom * ppp * 100.0).round()) } else { "—".into() };
        ui.painter().text(pos2(r.left() + 8.0, r.top() + 4.0), Align2::LEFT_TOP, zt, value, t.text);
        ui.painter().text(pos2(r.left() + 8.0, r.bottom() - 4.0), Align2::LEFT_BOTTOM, "Zoom", small.clone(), t.dim);
        if resp.clicked() {
            self.ed.popover = if matches!(self.ed.popover, Popover::Zoom(_)) { Popover::None } else { Popover::Zoom(r) };
        }
        if let Some(d) = &self.doc {
            let info = format!("{} × {} px", d.base.w, d.base.h);
            ui.painter().text(pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, info, small, t.dim);
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
            let pm = render::compose(d, &self.fonts, skip);
            let size = (pm.width(), pm.height());
            match &mut self.ed.comp {
                Some(c) if c.tex.fits(ctx, size) => {
                    c.tex.update(Some(c.pm.data()), pm.data(), Alpha::Premultiplied, TEX);
                    c.pm = pm;
                    c.key = key;
                }
                _ => {
                    let tex = TiledTexture::new(ctx, "composite", size, pm.data(), Alpha::Premultiplied, TEX);
                    self.ed.comp = Some(Comp { key, pm, tex });
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
                    let b = render::bounds(obj, &self.fonts).expand(2.0).clamp_to(w, h);
                    let b = R { x0: b.x0.floor(), y0: b.y0.floor(), x1: b.x1.ceil(), y1: b.y1.ceil() };
                    if b.w() >= 1.0 && b.h() >= 1.0 {
                        let mut region = render::crop_pixmap(&comp.pm, &b);
                        let mut local = obj.clone();
                        local.translate(-b.x0, -b.y0);
                        render::draw_obj(&mut region, &local, &self.fonts, d.scale_factor);
                        let size = (region.width(), region.height());
                        let tex = match self.ed.overlay.take() {
                            Some(mut o) if o.tex.fits(ctx, size) => {
                                o.tex.update(None, region.data(), Alpha::Premultiplied, TEX);
                                o.tex
                            }
                            _ => TiledTexture::new(ctx, "overlay", size, region.data(), Alpha::Premultiplied, TEX),
                        };
                        self.ed.overlay = Some(Overlay { obj: obj.clone(), version: comp.key.0, rect: b, tex });
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
            let v = if crop_mode { full } else { d.view_rect() };
            (d.base.w as f32, d.base.h as f32, v)
        };
        let fit_target = if backdrop_mode {
            let (w, h) = self.ed.preview.as_ref().map(|p| (p.1 .0 as f32, p.1 .1 as f32)).unwrap_or((iw, ih));
            R { x0: 0.0, y0: 0.0, x1: w, y1: h }
        } else {
            view
        };
        if self.ed.fit {
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
            return;
        }

        let view_screen = self.ed.rect_screen(&view);
        // The canvas is a plain colour. A checkerboard shows only behind the picture itself, and
        // only when the picture has see-through pixels (a window with its shadow, a logo): there it
        // is what tells transparent from white. An opaque picture covers it and gets an outline.
        let see_through = self.picture_has_alpha();
        if see_through {
            self.paint_checker(ui, view_screen.intersect(rect), view_screen.min);
        }
        if let Some(c) = &self.ed.comp {
            c.tex.paint(&painter, &view, |r| self.ed.rect_screen(r), Color32::WHITE);
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
            let (x, y) = (q.x.floor() as i32, q.y.floor() as i32);
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
                self.begin_drag(q, tol);
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
                        if let (Some(d), Some(i)) = (&mut self.doc, self.ed.selected) {
                            set_end(&mut d.objs[i].kind, start, q, shift);
                            d.touch();
                        }
                    }
                    Drag::Move { idx, last } => {
                        if let Some(d) = &mut self.doc {
                            d.objs[idx].translate(q.x - last.x, q.y - last.y);
                            d.touch();
                        }
                        self.ed.drag = Drag::Move { idx, last: q };
                    }
                    Drag::Handle { idx, h, anchor } => {
                        if let Some(d) = &mut self.doc {
                            match (&mut d.objs[idx].kind, h) {
                                (Kind::Arrow { a, .. } | Kind::Line { a, .. } | Kind::Ruler { a, .. } | Kind::Highlighter { a, .. }, Handle::A) => *a = q,
                                (Kind::Arrow { b, .. } | Kind::Line { b, .. } | Kind::Ruler { b, .. } | Kind::Highlighter { b, .. }, Handle::B) => *b = q,
                                (Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. }, Handle::Corner(_)) => {
                                    *r = R::from_points(anchor, if shift { square(anchor, q) } else { q })
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
    /// `Some(true)` is horizontal. Arrows keep moving a selected object.
    fn ruler_axis(&self, ui: &Ui) -> Option<bool> {
        if self.ed.editing_text {
            return None;
        }
        let arrows = self.ed.selected.is_none();
        ui.input(|i| {
            if i.key_down(Key::Num2) || (arrows && (i.key_down(Key::ArrowLeft) || i.key_down(Key::ArrowRight))) {
                Some(true)
            } else if i.key_down(Key::Num1) || (arrows && (i.key_down(Key::ArrowUp) || i.key_down(Key::ArrowDown))) {
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
        if let Some(i) = self.top_hit(q, tol) {
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

    fn handle_at(&self, i: usize, q: P, tol: f32) -> Option<Handle> {
        let o = self.doc.as_ref()?.objs.get(i)?;
        handles(o).into_iter().find(|(_, hp)| hp.dist(q) <= tol * 1.4).map(|(h, _)| h)
    }

    fn start_handle(&mut self, i: usize, h: Handle) {
        let d = self.doc.as_mut().unwrap();
        let anchor = match (&d.objs[i].kind, h) {
            (Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. }, Handle::Corner(c)) => match c {
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
        let hit_i = self.top_hit(q, tol);
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
            crate::icons::combo("colorfmt").width(170.0).selected_text(render::COLOR_FORMATS[cur]).show_ui(ui, |ui| {
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
        let Some((w0, h0)) = self.doc.as_ref().map(|d| (d.view_rect().w(), d.view_rect().h())) else { return };
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
            egui::Window::new("About ShotCraft").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                ui.heading("ShotCraft");
                ui.label("Version 0.1");
                ui.label(egui::RichText::new("A screenshot tool with an annotation editor. Everything stays on this computer.").color(t.dim));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Font: DM Sans (SIL Open Font License). Icons: Hugeicons free set (MIT).").size(11.0).color(t.dim));
            });
            self.about_open = open;
        }
        if self.shortcuts_open {
            let mut open = true;
            egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                    for (k, v) in [
                        ("A T R O L", "Arrow, Text, Rectangle, Oval, Line"),
                        ("D H B S C M K", "Freehand, Highlighter, Blur, Spotlight, Counter, Magnifier, Backdrop"),
                        ("Hold 2 or ←→ / 1 or ↑↓", "Ruler: horizontal / vertical span under the cursor"),
                        ("Tab / ⇧Tab", "Copy pixel colour / darkest nearby colour"),
                        ("C (with a selection)", "Copy the average colour"),
                        ("Del", "Delete object or erase the selection"),
                        ("Esc", "Deselect, or copy and close"),
                        ("⌘C  ⌘S  ⌘P", "Copy, Save, Pin to Screen"),
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
                });
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
