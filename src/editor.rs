use crate::icons::{self, Icon};
use crate::model::*;
use crate::render;
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
    tex: egui::TextureHandle,
}

struct Overlay {
    obj: Obj,
    version: u64,
    rect: R,
    tex: egui::TextureHandle,
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
    preview: Option<(u64, (u32, u32), egui::TextureHandle)>,
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

    fn rect_screen(&self, r: &R) -> Rect {
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

fn hit(obj: &Obj, q: P, tol: f32) -> bool {
    let seg_dist = |a: P, b: P| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let l2 = (dx * dx + dy * dy).max(1e-6);
        let t = (((q.x - a.x) * dx + (q.y - a.y) * dy) / l2).clamp(0.0, 1.0);
        q.dist(p(a.x + dx * t, a.y + dy * t))
    };
    let w = obj.style.width();
    match &obj.kind {
        Kind::Arrow { a, b } | Kind::Line { a, b } | Kind::Ruler { a, b } => seg_dist(*a, *b) <= w * 1.5 + tol,
        Kind::Highlighter { a, b } => seg_dist(*a, *b) <= obj.style.highlight_size() / 2.0 + tol,
        Kind::Freehand { pts } => pts.windows(2).any(|s| seg_dist(s[0], s[1]) <= w + tol),
        Kind::Rect { r, .. } | Kind::Oval { r, .. } => {
            if matches!(obj.style.fill, Fill::Translucent | Fill::Opaque) {
                r.expand(tol).contains(q)
            } else {
                r.expand(w + tol).contains(q) && !r.expand(-(w + tol)).contains(q)
            }
        }
        _ => obj.bounds().expand(tol).contains(q),
    }
}

fn handles(obj: &Obj) -> Vec<(Handle, P)> {
    match &obj.kind {
        Kind::Arrow { a, b } | Kind::Line { a, b } | Kind::Ruler { a, b } | Kind::Highlighter { a, b } => vec![(Handle::A, *a), (Handle::B, *b)],
        Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } => vec![
            (Handle::Corner(0), p(r.x0, r.y0)),
            (Handle::Corner(1), p(r.x1, r.y0)),
            (Handle::Corner(2), p(r.x1, r.y1)),
            (Handle::Corner(3), p(r.x0, r.y1)),
        ],
        _ => vec![],
    }
}

fn snap45(a: P, b: P) -> P {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    let ang = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
    p(a.x + len * ang.cos(), a.y + len * ang.sin())
}

fn square(a: P, b: P) -> P {
    let s = (b.x - a.x).abs().max((b.y - a.y).abs());
    p(a.x + s * (b.x - a.x).signum(), a.y + s * (b.y - a.y).signum())
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
        Tool::Arrow => Kind::Arrow { a: q, b: q },
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

fn color_image(pm: &Pixmap) -> egui::ColorImage {
    egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data())
}

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
            let (cmd, shift) = (modifiers.command, modifiers.shift);
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
                Key::Enter if self.ed.tool == Tool::Crop => self.apply_crop(),
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
                Key::CloseBracket if shift => {
                    self.ed.popover = Popover::Resize(Rect::from_min_size(self.ed.canvas.right_top() + vec2(-300.0, 4.0), vec2(1.0, 1.0)))
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
                        (Icon::Resize, "Resize ( ⇧+] )", matches!(self.ed.popover, Popover::Resize(_))),
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
            let img = color_image(&pm);
            match &mut self.ed.comp {
                Some(c) if c.pm.width() == pm.width() && c.pm.height() == pm.height() => {
                    c.tex.set(img, TEX);
                    c.pm = pm;
                    c.key = key;
                }
                _ => {
                    let tex = ctx.load_texture("composite", img, TEX);
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
                    let b = obj.bounds().expand(2.0).clamp_to(w, h);
                    let b = R { x0: b.x0.floor(), y0: b.y0.floor(), x1: b.x1.ceil(), y1: b.y1.ceil() };
                    if b.w() >= 1.0 && b.h() >= 1.0 {
                        let mut region = render::crop_pixmap(&comp.pm, &b);
                        let mut local = obj.clone();
                        local.translate(-b.x0, -b.y0);
                        render::draw_obj(&mut region, &local, &self.fonts, d.scale_factor);
                        let img = color_image(&region);
                        let tex = match self.ed.overlay.take() {
                            Some(mut o) => {
                                o.tex.set(img, TEX);
                                o.tex
                            }
                            None => ctx.load_texture("overlay", img, TEX),
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
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.px);
                let tex = ctx.load_texture("backdrop-preview", ci, egui::TextureOptions::LINEAR);
                self.ed.preview = Some((d.version, (img.w, img.h), tex));
            }
        }
    }

    fn canvas_ui(&mut self, ui: &mut Ui, t: &Th) {
        let rect = ui.max_rect();
        self.ed.canvas = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        self.paint_checker(ui, rect);
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
            let content = vec2(fit_target.w(), fit_target.h()) * self.ed.zoom;
            self.ed.pan = (rect.size() - content) / 2.0 - vec2(fit_target.x0, fit_target.y0) * self.ed.zoom;
        }

        let painter = ui.painter_at(rect);
        if backdrop_mode {
            if let Some((_, (w, h), tex)) = &self.ed.preview {
                let r = self.ed.rect_screen(&R { x0: 0.0, y0: 0.0, x1: *w as f32, y1: *h as f32 });
                painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            self.canvas_zoom_pan(ui, &resp);
            return;
        }

        let view_screen = self.ed.rect_screen(&view);
        if let Some(c) = &self.ed.comp {
            let uv = Rect::from_min_max(pos2(view.x0 / iw, view.y0 / ih), pos2(view.x1 / iw, view.y1 / ih));
            painter.image(c.tex.id(), view_screen, uv, Color32::WHITE);
        }
        painter.rect_stroke(view_screen, 0.0, Stroke::new(1.0, t.line), StrokeKind::Outside);
        let clip = painter.with_clip_rect(view_screen.intersect(rect));
        if let Some(o) = &self.ed.overlay {
            clip.image(o.tex.id(), self.ed.rect_screen(&o.rect), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
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

    /// Checkerboard layer (0x10007b654): a repeating tile of two greys behind the picture.
    fn paint_checker(&mut self, ui: &mut Ui, rect: Rect) {
        let dark = ui.visuals().dark_mode;
        if self.ed.checker.as_ref().map_or(true, |c| c.0 != dark) {
            let (base, square) = if dark { CHECKER_DARK } else { CHECKER_LIGHT };
            let g = |v: f32| Color32::from_gray((v * 255.0).round() as u8);
            let img = egui::ColorImage::new([2, 2], vec![g(square), g(base), g(base), g(square)]);
            let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::NEAREST };
            self.ed.checker = Some((dark, ui.ctx().load_texture("checker", img, options)));
        }
        if let Some((_, tex)) = &self.ed.checker {
            let uv = Rect::from_min_size(pos2(0.0, 0.0), rect.size() / (CHECKER_SQUARE * 2.0));
            ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
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
        let tol = 6.0 / self.ed.zoom;
        let ruler_hold = self.ruler_axis(ui);

        if resp.drag_started() {
            self.ed.popover = Popover::None;
            let start_s = press.unwrap_or_default();
            let q = self.ed.to_img(start_s);
            if space || ui.input(|i| i.pointer.middle_down()) {
                self.ed.drag = Drag::Pan { last: start_s };
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
                    Drag::Select { start } => self.ed.selection = Some(R::from_points(start, q)),
                    Drag::Crop { start } => self.ed.crop_draft = Some(R::from_points(start, q)),
                    Drag::None => {}
                }
            }
        }
        if resp.drag_stopped() {
            if matches!(self.ed.drag, Drag::Create { .. } | Drag::Handle { .. }) {
                self.detect_blur_text();
                self.keep_lens_inside();
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
            self.click(q, tol, resp.double_clicked());
        }
    }

    /// Ruler keys (0x10009e964): 2 and ←→ measure across, 1 and ↑↓ measure down.
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

    /// Blur Text and Erase Text only touch the text found inside their area (0x100107cfc).
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
        (0..d.objs.len()).rev().find(|i| hit(&d.objs[*i], q, tol))
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
            if self.doc.as_ref().map_or(false, |d| i < d.objs.len() && hit(&d.objs[i], q, tol)) {
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
            Tool::Counter | Tool::Text => {
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
                    _ => o.bounds(),
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
                        ("Space + drag", "Move around the picture"),
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
