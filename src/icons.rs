use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, FontId, Painter, Rect, Stroke, StrokeKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Icon {
    Copy,
    Save,
    Pin,
    Arrow,
    Text,
    Ruler,
    Rect,
    Backdrop,
    Pen,
    Lens,
    Blur,
    Highlighter,
    Spotlight,
    Counter,
    Oval,
    Line,
    Resize,
    Ocr,
    SelSize,
    Erase,
    Scissors,
    Reset,
    Gear,
    Back,
    Solid,
    Dash,
    ArrowShape,
    ArrowHand,
    ArrowLine,
    ArrowDouble,
    FillHand,
    FillNone,
    FillHalf,
    FillFull,
    Randomize,
    Revert,
    Close,
    Edit,
    Crop,
    ChevronDown,
    Trash,
    Keyboard,
    Sliders,
    DragFile,
}

impl Icon {
    pub fn for_tool(t: crate::model::Tool) -> Icon {
        use crate::model::Tool as T;
        match t {
            T::Arrow => Icon::Arrow,
            T::Text => Icon::Text,
            T::Ruler => Icon::Ruler,
            T::Rectangle => Icon::Rect,
            T::Backdrop => Icon::Backdrop,
            T::Freehand => Icon::Pen,
            T::Magnifier => Icon::Lens,
            T::Blur => Icon::Blur,
            T::Highlighter => Icon::Highlighter,
            T::Spotlight => Icon::Spotlight,
            T::Counter => Icon::Counter,
            T::Oval => Icon::Oval,
            T::Line => Icon::Line,
            T::Crop => Icon::Crop,
            T::Cut => Icon::Scissors,
            T::Ocr => Icon::Ocr,
            T::Select => Icon::Back,
        }
    }
}

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Hugeicons (MIT) stroke icons, see assets/icons/LICENSE-hugeicons.md.
fn svg(icon: Icon) -> Option<&'static str> {
    match icon {
        Icon::Copy => Some(include_str!("../assets/icons/copy.svg")),
        Icon::Save => Some(include_str!("../assets/icons/save.svg")),
        Icon::Pin => Some(include_str!("../assets/icons/pin.svg")),
        Icon::Arrow => Some(include_str!("../assets/icons/arrow.svg")),
        Icon::Text => Some(include_str!("../assets/icons/text.svg")),
        Icon::Ruler => Some(include_str!("../assets/icons/ruler.svg")),
        Icon::Rect => Some(include_str!("../assets/icons/rect.svg")),
        Icon::Backdrop => Some(include_str!("../assets/icons/backdrop.svg")),
        Icon::Pen => Some(include_str!("../assets/icons/pen.svg")),
        Icon::Lens => Some(include_str!("../assets/icons/lens.svg")),
        Icon::Blur => Some(include_str!("../assets/icons/blur.svg")),
        Icon::Highlighter => Some(include_str!("../assets/icons/highlighter.svg")),
        Icon::Spotlight => Some(include_str!("../assets/icons/spotlight.svg")),
        Icon::Counter => Some(include_str!("../assets/icons/counter.svg")),
        Icon::Oval => Some(include_str!("../assets/icons/oval.svg")),
        Icon::Line => Some(include_str!("../assets/icons/line.svg")),
        Icon::Resize => Some(include_str!("../assets/icons/resize.svg")),
        Icon::Ocr => Some(include_str!("../assets/icons/ocr.svg")),
        Icon::SelSize => Some(include_str!("../assets/icons/selsize.svg")),
        Icon::Erase => Some(include_str!("../assets/icons/erase.svg")),
        Icon::Scissors => Some(include_str!("../assets/icons/scissors.svg")),
        Icon::Reset => Some(include_str!("../assets/icons/reset.svg")),
        Icon::Gear => Some(include_str!("../assets/icons/gear.svg")),
        Icon::Back => Some(include_str!("../assets/icons/back.svg")),
        Icon::Solid => Some(include_str!("../assets/icons/solid.svg")),
        Icon::Dash => Some(include_str!("../assets/icons/dash.svg")),
        Icon::ArrowShape => Some(include_str!("../assets/icons/arrow_shape.svg")),
        Icon::ArrowHand => Some(include_str!("../assets/icons/arrow_hand.svg")),
        Icon::ArrowLine => Some(include_str!("../assets/icons/arrow_line.svg")),
        Icon::ArrowDouble => Some(include_str!("../assets/icons/arrow_double.svg")),
        Icon::FillNone => Some(include_str!("../assets/icons/rect.svg")),
        Icon::Randomize => Some(include_str!("../assets/icons/randomize.svg")),
        Icon::Revert => Some(include_str!("../assets/icons/revert.svg")),
        Icon::Close => Some(include_str!("../assets/icons/close.svg")),
        Icon::Edit => Some(include_str!("../assets/icons/edit.svg")),
        Icon::Crop => Some(include_str!("../assets/icons/crop.svg")),
        Icon::ChevronDown => Some(include_str!("../assets/icons/chevron_down.svg")),
        Icon::Trash => Some(include_str!("../assets/icons/trash.svg")),
        Icon::Keyboard => Some(include_str!("../assets/icons/keyboard.svg")),
        Icon::Sliders => Some(include_str!("../assets/icons/sliders.svg")),
        Icon::DragFile => Some(include_str!("../assets/icons/drag_file.svg")),
        _ => None,
    }
}

const ICON_PX: u32 = 48;

/// Drop-down menu whose arrow is the Hugeicons chevron.
pub fn combo(id: impl egui::AsIdSalt) -> egui::ComboBox {
    egui::ComboBox::from_id_salt(id).icon(|ui, rect, visuals, _open| {
        draw(ui.painter(), rect.expand(3.0), Icon::ChevronDown, visuals.fg_stroke.color);
    })
}

fn texture(ctx: &egui::Context, icon: Icon) -> Option<egui::TextureHandle> {
    static CACHE: OnceLock<Mutex<HashMap<Icon, Option<egui::TextureHandle>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache
        .entry(icon)
        .or_insert_with(|| {
            let tree = resvg::usvg::Tree::from_str(svg(icon)?, &resvg::usvg::Options::default()).ok()?;
            let mut pm = resvg::tiny_skia::Pixmap::new(ICON_PX, ICON_PX)?;
            let s = ICON_PX as f32 / 24.0;
            resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
            let img = egui::ColorImage::from_rgba_premultiplied([ICON_PX as usize, ICON_PX as usize], pm.data());
            Some(ctx.load_texture(format!("icon-{icon:?}"), img, egui::TextureOptions::LINEAR))
        })
        .clone()
}

pub fn draw(p: &Painter, r: Rect, icon: Icon, c: Color32) {
    let side = r.width().min(r.height());
    let sq = Rect::from_center_size(r.center(), vec2(side, side));
    if let Some(tex) = texture(p.ctx(), icon) {
        p.image(tex.id(), sq, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), c);
        return;
    }
    let k = side / 24.0;
    let at = |x: f32, y: f32| pos2(sq.min.x + x * k, sq.min.y + y * k);
    let s = Stroke::new(1.5, c);
    let bx = Rect::from_min_max(at(3.0, 5.0), at(21.0, 19.0));
    match icon {
        Icon::FillHand => {
            p.line(vec![at(3.5, 5.5), at(20.5, 4.5)], s);
            p.line(vec![at(20.0, 4.0), at(20.8, 19.5)], s);
            p.line(vec![at(21.0, 19.0), at(3.0, 19.8)], s);
            p.line(vec![at(3.6, 20.2), at(3.2, 5.0)], s);
        }
        Icon::FillHalf => {
            p.rect_filled(bx, CornerRadius::same(3), c.gamma_multiply(0.35));
            p.rect_stroke(bx, CornerRadius::same(3), s, StrokeKind::Middle);
        }
        Icon::FillFull => {
            p.rect_filled(bx, CornerRadius::same(3), c);
        }
        _ => {
            p.text(r.center(), egui::Align2::CENTER_CENTER, "?", FontId::proportional(side * 0.6), c);
        }
    }
}
