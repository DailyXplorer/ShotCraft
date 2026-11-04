//! The floating control of a scrolling capture: a small dark HUD with the live height and frame
//! count, a one-line hint, and Done / Cancel.
use crate::scroll::{Note, CONTROL_SIZE};
use eframe::egui::{self, text::LayoutJob, Align2, Color32, CursorIcon, FontFamily, FontId, Rect, Sense, Stroke, StrokeKind, TextFormat, Ui};

const PAD: f32 = 14.0;
const RADIUS: f32 = 12.0;
const BUTTON_H: f32 = 30.0;
const BUTTON_GAP: f32 = 8.0;

const PANEL: Color32 = Color32::from_rgba_premultiplied(26, 27, 31, 240);
const BORDER: Color32 = Color32::from_rgba_premultiplied(40, 40, 40, 40);
const TEXT: Color32 = Color32::WHITE;
/// White at 70%: secondary text.
const TEXT_DIM: Color32 = Color32::from_rgba_premultiplied(179, 179, 179, 179);
/// White at 55%: key hints.
const TEXT_HINT: Color32 = Color32::from_rgba_premultiplied(140, 140, 140, 140);
const WARNING: Color32 = Color32::from_rgb(255, 204, 102);
const ACCENT: Color32 = Color32::from_rgb(10, 98, 222);
const ACCENT_HOVER: Color32 = Color32::from_rgb(30, 116, 240);
const SUBTLE: Color32 = Color32::from_rgba_premultiplied(31, 31, 31, 31);
const SUBTLE_HOVER: Color32 = Color32::from_rgba_premultiplied(56, 56, 56, 56);

/// What the control shows.
pub struct View {
    pub height: u32,
    pub frames: u32,
    pub note: Note,
    /// ShotCraft scrolls the page (otherwise the user does).
    pub auto: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Click {
    None,
    Done,
    Cancel,
}

/// "1 frame", "2 frames".
pub fn frames_text(n: u32) -> String {
    if n == 1 {
        "1 frame".to_string()
    } else {
        format!("{n} frames")
    }
}

/// "984 px", "12,480 px".
pub fn height_text(px: u32) -> String {
    let s = px.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out} px")
}

fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// A button: a rounded fill, the label and the key that does the same, dimmer.
fn button(ui: &mut Ui, rect: Rect, label: &str, key: &str, fill: Color32, hover: Color32, id: &str) -> bool {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    ui.painter().rect_filled(rect, 8.0, if resp.hovered() { hover } else { fill });
    let mut job = LayoutJob::default();
    job.append(label, 0.0, TextFormat { font_id: medium(13.0), color: TEXT, ..Default::default() });
    job.append(key, 7.0, TextFormat { font_id: regular(10.5), color: Color32::from_rgba_premultiplied(190, 190, 190, 190), ..Default::default() });
    let galley = ui.painter().layout_job(job);
    let pos = Align2::CENTER_CENTER.align_size_within_rect(galley.size(), rect).min;
    ui.painter().galley(pos, galley, TEXT);
    resp.clicked()
}

/// Draws the control filling the window and says what was clicked.
pub fn draw(ui: &mut Ui, v: &View) -> Click {
    let full = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(CONTROL_SIZE[0], CONTROL_SIZE[1]));
    let rect = Rect::from_min_size(ui.max_rect().min, full.size());
    let p = ui.painter().clone();
    p.rect_filled(rect, RADIUS, PANEL);
    p.rect_stroke(rect.shrink(0.5), RADIUS, Stroke::new(1.0, BORDER), StrokeKind::Inside);

    let inner = rect.shrink2(egui::vec2(PAD, PAD - 2.0));
    let mut y = inner.top();

    // Title and who scrolls.
    let title = p.text(egui::pos2(inner.left(), y), Align2::LEFT_TOP, "Scrolling capture", medium(13.5), TEXT);
    p.text(egui::pos2(inner.right(), y + 1.5), Align2::RIGHT_TOP, if v.auto { "Auto" } else { "Manual" }, regular(11.0), TEXT_DIM);
    y = title.bottom() + 3.0;

    // Live numbers.
    let stats = format!("{} · {}", height_text(v.height), frames_text(v.frames));
    let r = p.text(egui::pos2(inner.left(), y), Align2::LEFT_TOP, stats, regular(12.5), TEXT);
    y = r.bottom() + 2.0;

    // What to do, or what went wrong.
    let colour = if v.note.is_warning() { WARNING } else { TEXT_DIM };
    p.text(egui::pos2(inner.left(), y), Align2::LEFT_TOP, v.note.text(), regular(12.0), colour);

    // Buttons, along the bottom.
    let top = inner.bottom() - BUTTON_H;
    let cancel_w = 104.0;
    let done_rect = Rect::from_min_max(egui::pos2(inner.left(), top), egui::pos2(inner.right() - cancel_w - BUTTON_GAP, inner.bottom()));
    let cancel_rect = Rect::from_min_max(egui::pos2(inner.right() - cancel_w, top), inner.right_bottom());
    let mut click = Click::None;
    if button(ui, done_rect, "Done", "Return", ACCENT, ACCENT_HOVER, "done") {
        click = Click::Done;
    }
    if button(ui, cancel_rect, "Cancel", "Esc", SUBTLE, SUBTLE_HOVER, "cancel") {
        click = Click::Cancel;
    }
    let _ = TEXT_HINT;
    click
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_frame_is_singular() {
        assert_eq!(frames_text(0), "0 frames");
        assert_eq!(frames_text(1), "1 frame");
        assert_eq!(frames_text(2), "2 frames");
    }

    #[test]
    fn heights_get_thousands_separators() {
        assert_eq!(height_text(0), "0 px");
        assert_eq!(height_text(984), "984 px");
        assert_eq!(height_text(1000), "1,000 px");
        assert_eq!(height_text(12480), "12,480 px");
        assert_eq!(height_text(1234567), "1,234,567 px");
    }

    #[test]
    fn every_note_fits_the_control() {
        // At 12 pt a character is about 6.2 points wide; the text area is the control minus its padding.
        let room = CONTROL_SIZE[0] - 2.0 * PAD;
        for n in [
            Note::ScrollingForYou,
            Note::ScrollYourself,
            Note::Capturing,
            Note::AboveStart,
            Note::ScrollSlower,
            Note::LostTrack,
            Note::SizeChanged,
            Note::HeightLimit,
            Note::EndOfPage,
            Note::NeedsAccessibility,
            Note::PageDidNotMove,
            Note::CheckScreenRecording,
            Note::EscAgain,
        ] {
            assert!(n.text().chars().count() as f32 * 6.2 <= room, "{:?} is too long: {}", n, n.text());
        }
    }

    #[test]
    fn the_scroll_slower_hint_is_a_warning() {
        assert!(Note::ScrollSlower.is_warning());
        assert!(!Note::Capturing.is_warning());
    }
}
