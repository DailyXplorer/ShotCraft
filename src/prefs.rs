use crate::editor::th;
use crate::settings::{pretty_hotkey, AfterShow, Confirm, HotkeyAction, Settings, Stamp, WindowBg};
use crate::{Action, App, Queue};
use eframe::egui::{self, vec2, Align2, Color32, FontFamily, Key, Ui, ViewportBuilder, ViewportId};
use std::sync::{Arc, Mutex};

const LABEL_W: f32 = 200.0;
pub const WINDOW: [f32; 2] = [560.0, 720.0];
/// The date the file name example is shown with.
const SAMPLE: Stamp = Stamp { year: 2026, month: 11, day: 5, hour: 9, minute: 7, second: 3 };
/// How small the scrolling speed slider's steps are: one per level.
const SPEED_LEVELS: std::ops::RangeInclusive<i32> = 0..=crate::settings::SCROLL_SPEED_MAX as i32;

/// What the Preferences window shares with the app. It runs as its own deferred window, so it can
/// open while the editor is hidden; the app picks up `settings` whenever `edited` is set.
pub struct PrefsState {
    pub open: bool,
    pub focus: bool,
    pub tab: usize,
    pub settings: Settings,
    edited: bool,
    queue: Queue,
    recording: Option<HotkeyAction>,
    refused: Vec<HotkeyAction>,
    /// What is typed in the max height field while it has the keyboard.
    height_text: Option<String>,
    /// "Reset All Settings" was clicked and waits for its confirmation.
    confirm_reset: bool,
}

impl PrefsState {
    /// The hotkeys macOS refused the last time they were registered.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn set_refused(&mut self, refused: Vec<HotkeyAction>) {
        self.refused = refused;
    }

    pub fn shared(settings: Settings, queue: Queue) -> Arc<Mutex<PrefsState>> {
        Arc::new(Mutex::new(PrefsState {
            open: false,
            focus: false,
            tab: 0,
            settings,
            edited: false,
            queue,
            recording: None,
            refused: vec![],
            height_text: None,
            confirm_reset: false,
        }))
    }
}

/// What a frame of input means while a shortcut is being recorded.
#[derive(Debug, PartialEq)]
enum Recorded {
    Cancel,
    Clear,
    Set(String),
}

/// The first thing in `events` that ends the recording. `modifiers` are the ones held now: the
/// copy, cut and paste events carry none of their own. On a Mac egui-winit turns Cmd+C, Cmd+X and
/// Cmd+V into those events and sends no key event, so they are read back as C, X and V.
fn recorded(events: &[egui::Event], modifiers: egui::Modifiers) -> Option<Recorded> {
    for e in events {
        let (key, modifiers) = match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } => (*key, *modifiers),
            egui::Event::Copy => (Key::C, modifiers),
            egui::Event::Cut => (Key::X, modifiers),
            egui::Event::Paste(_) => (Key::V, modifiers),
            _ => continue,
        };
        if key == Key::Escape {
            return Some(Recorded::Cancel);
        }
        if key == Key::Backspace || key == Key::Delete {
            return Some(Recorded::Clear);
        }
        let Some(code) = key_code(key) else { continue };
        let mut parts = vec![];
        if modifiers.ctrl {
            parts.push("ctrl");
        }
        if modifiers.alt {
            parts.push("alt");
        }
        if modifiers.shift {
            parts.push("shift");
        }
        if modifiers.mac_cmd || modifiers.command {
            parts.push("super");
        }
        if !parts.is_empty() {
            return Some(Recorded::Set(format!("{}+{}", parts.join("+"), code)));
        }
    }
    None
}

fn key_code(k: Key) -> Option<String> {
    let name = format!("{k:?}");
    if let Some(d) = name.strip_prefix("Num") {
        return Some(format!("Digit{d}"));
    }
    if name.len() == 1 {
        return Some(format!("Key{name}"));
    }
    if name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit()) {
        return Some(name);
    }
    match k {
        Key::Space => Some("Space".into()),
        Key::Minus => Some("Minus".into()),
        Key::Equals => Some("Equal".into()),
        Key::Comma => Some("Comma".into()),
        Key::Period => Some("Period".into()),
        Key::Slash => Some("Slash".into()),
        _ => None,
    }
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(LABEL_W, 22.0), egui::Sense::hover());
        ui.painter().text(r.right_center(), Align2::RIGHT_CENTER, label, egui::FontId::proportional(13.0), ui.visuals().text_color());
        ui.add_space(6.0);
        ui.vertical(add);
    });
    ui.add_space(6.0);
}

impl App {
    pub fn prefs_ui(&mut self, ctx: &egui::Context) {
        let (edited, open, focus, recording) = {
            let mut p = self.prefs.lock().unwrap();
            let edited = std::mem::take(&mut p.edited).then(|| p.settings.clone());
            if edited.is_none() {
                p.settings = self.settings.clone();
            }
            (edited, p.open, std::mem::take(&mut p.focus), p.open && p.recording.is_some())
        };
        // While a shortcut is recorded, the menus' key equivalents and the global hotkeys would eat it.
        self.set_keys_suspended(recording);
        if let Some(new) = edited {
            self.apply_settings(new);
        }
        if !open {
            self.prefs.lock().unwrap().height_text = None;
            return;
        }
        let vid = ViewportId::from_hash_of("prefs");
        if focus {
            ctx.send_viewport_cmd_to(vid, egui::ViewportCommand::Focus);
        }
        let builder = ViewportBuilder::default().with_title("Preferences").with_inner_size(WINDOW).with_resizable(false);
        let shared = self.prefs.clone();
        crate::show_window(ctx, vid, builder, move |ui| {
            let mut p = shared.lock().unwrap();
            let was_recording = p.recording.is_some();
            if ui.input(|i| i.viewport().close_requested()) {
                p.open = false;
                p.recording = None;
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
            // Recording ends when the window loses the keyboard: the keys would go somewhere else.
            if ui.input(|i| i.viewport().focused == Some(false)) {
                p.recording = None;
            }
            if ui.input(|i| i.modifiers.command && !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(Key::W)) && !was_recording {
                p.open = false;
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
            p.body(ui);
            if p.recording.is_some() != was_recording {
                // The app switches its key equivalents and hotkeys from its own update.
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
        });
    }

    fn apply_settings(&mut self, new: Settings) {
        let old = std::mem::replace(&mut self.settings, new);
        if old.autostart != self.settings.autostart {
            crate::system::set_autostart(self.settings.autostart);
        }
        if old.always_on_top != self.settings.always_on_top {
            self.apply_window_level();
        }
        if old.hotkeys != self.settings.hotkeys {
            let refused = self.apply_hotkeys();
            self.prefs.lock().unwrap().refused = refused;
        }
        if old.tool_styles != self.settings.tool_styles || old.default_tool != self.settings.default_tool {
            // Reset All Settings: the editor goes back to the factory styles too.
            let settings = self.settings.clone();
            self.ed.load_styles(&settings);
        }
        self.settings.save();
        self.sync_shell();
    }
}

impl PrefsState {
    pub fn body(&mut self, ui: &mut Ui) {
        let t = th(ui.visuals().dark_mode);
        egui::Panel::top("prefs-tabs").frame(egui::Frame::new().fill(t.bar2).inner_margin(10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let w = ui.available_width();
                ui.add_space((w - 3.0 * 92.0) / 2.0);
                for (i, (icon, name)) in [(crate::icons::Icon::Gear, "General"), (crate::icons::Icon::Keyboard, "Hotkeys"), (crate::icons::Icon::Sliders, "Advanced")].iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(88.0, 50.0), egui::Sense::click());
                    let sel = self.tab == i;
                    if sel || resp.hovered() {
                        ui.painter().rect_filled(r, 8.0, if sel { t.hover } else { t.hover.gamma_multiply(0.6) });
                    }
                    let c = if sel { t.accent } else { t.text };
                    crate::icons::draw(ui.painter(), egui::Rect::from_center_size(r.center_top() + vec2(0.0, 17.0), vec2(22.0, 22.0)), *icon, c);
                    ui.painter().text(r.center_bottom() - vec2(0.0, 9.0), Align2::CENTER_CENTER, *name, egui::FontId::new(12.0, FontFamily::Proportional), c);
                    if resp.clicked() {
                        self.tab = i;
                        self.recording = None;
                        self.confirm_reset = false;
                    }
                }
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.bar2).inner_margin(20)).show(ui, |ui| {
            let before = self.settings.clone();
            match self.tab {
                0 => self.prefs_general(ui, t.dim),
                1 => self.prefs_hotkeys(ui, t.dim),
                _ => self.prefs_advanced(ui, t.dim),
            }
            if self.settings != before {
                self.edited = true;
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
        });
    }

    fn prefs_general(&mut self, ui: &mut Ui, dim: Color32) {
        let queue = self.queue.clone();
        let PrefsState { settings: s, height_text, .. } = self;
        row(ui, "After screenshot", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.after_show, AfterShow::Editor, "Editor");
                ui.radio_value(&mut s.after_show, AfterShow::Thumbnail, "Thumbnail");
                ui.radio_value(&mut s.after_show, AfterShow::Nothing, "Nothing");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.after_copy, "Copy");
                ui.checkbox(&mut s.after_save, "Save");
            });
        });
        row(ui, "Hide preview thumbnail", |ui| {
            let label = if s.thumbnail_auto_close { "Automatically (click outside)" } else { "Manually (close button)" };
            crate::icons::combo("thumbclose").width(200.0).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.thumbnail_auto_close, true, "Automatically (click outside)");
                ui.selectable_value(&mut s.thumbnail_auto_close, false, "Manually (close button)");
            });
        });
        row(ui, "Autostart", |ui| {
            ui.checkbox(&mut s.autostart, "Launch at startup");
        });
        row(ui, "Screenshots folder", |ui| {
            ui.horizontal(|ui| {
                let shown = s.folder.replace(&crate::system::home().display().to_string(), "~");
                ui.label(shown);
                if ui.button("Select Folder").clicked() {
                    queue.lock().unwrap().push(Action::PickFolder);
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }
            });
        });
        row(ui, "File name", |ui| {
            ui.add(egui::TextEdit::singleline(&mut s.file_name_template).desired_width(200.0).hint_text(crate::settings::FILE_NAME_TEMPLATE));
            ui.label(egui::RichText::new(format!("Like {}", crate::settings::file_stem(&s.file_name_template, &SAMPLE))).color(dim).size(11.0));
            ui.label(egui::RichText::new("%Y year  %m month  %d day  %H hour  %M minute  %S second  %t time").color(dim).size(11.0));
        });
        row(ui, "Save format", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.save_auto_format, false, "PNG");
                ui.radio_value(&mut s.save_auto_format, true, "Auto (PNG/JPEG)").on_hover_text("Photos, gradients and noisy pictures are saved as JPEG; flat screenshots with text as PNG");
            });
        });
        row(ui, "Resize retina screenshots", |ui| {
            ui.checkbox(&mut s.downscale_retina, "Downscale to 1× when saving");
        });
        row(ui, "Window Screenshot Background", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.window_bg, WindowBg::Wallpaper, "Wallpaper").on_hover_text("The window and its shadow on your desktop picture");
                ui.radio_value(&mut s.window_bg, WindowBg::Transparent, "Transparent").on_hover_text("The window and its shadow on a see-through background");
                ui.radio_value(&mut s.window_bg, WindowBg::Solid, "Solid Color").on_hover_text("The window and its shadow on a colour");
                if matches!(s.window_bg, WindowBg::Solid | WindowBg::Wallpaper) {
                    let c = crate::model::hex(&s.window_solid);
                    let mut col = Color32::from_rgb(c[0], c[1], c[2]);
                    if ui.color_edit_button_srgba(&mut col).changed() {
                        s.window_solid = format!("#{:02X}{:02X}{:02X}", col.r(), col.g(), col.b());
                    }
                }
            });
            ui.radio_value(&mut s.window_bg, WindowBg::Trimmed, "Trim shadow").on_hover_text("The window cut out without its shadow");
            ui.label(egui::RichText::new("Applies to Active Window and Any Window screenshots, not to Area or Fullscreen.").color(dim).size(11.0));
        });
        row(ui, "Delayed screenshot", |ui| {
            ui.horizontal(|ui| {
                for secs in crate::settings::DELAYS {
                    ui.radio_value(&mut s.delay_secs, secs, format!("{secs} s"));
                }
            });
        });
        row(ui, "Area capture", |ui| {
            ui.checkbox(&mut s.frozen_area, "Freeze the screen while selecting");
            ui.label(egui::RichText::new("Draws the selection on a still picture of the screen, with guide lines. Space does not switch to a window.").color(dim).size(11.0));
        });
        row(ui, "Mouse pointer", |ui| {
            ui.checkbox(&mut s.capture_cursor, "Include it in screenshots");
            ui.label(egui::RichText::new("Fullscreen, Active Window, Delayed, Repeat Area and frozen Area. Not with the system selector.").color(dim).size(11.0));
        });
        row(ui, "Scrolling screenshot max height", |ui| {
            ui.horizontal(|ui| {
                let (lo, hi) = crate::settings::SCROLL_MAX_HEIGHT_RANGE;
                let mut text = height_text.clone().unwrap_or_else(|| s.scroll_max_height().to_string());
                let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0));
                if field.changed() {
                    text.retain(|c| c.is_ascii_digit());
                    text.truncate(9);
                    if let Ok(v) = text.parse::<u32>() {
                        s.scroll_max_height = v.clamp(lo, hi);
                    }
                    *height_text = Some(text);
                }
                if field.lost_focus() {
                    *height_text = None;
                }
                ui.label("px");
            });
            let (lo, hi) = crate::settings::SCROLL_MAX_HEIGHT_RANGE;
            ui.label(egui::RichText::new(format!("From {} to {}. The picture stops there.", thousands(lo), thousands(hi))).color(dim).size(11.0));
        });
        row(ui, "Scrolling screenshot speed", |ui| {
            let mut level = s.scroll_speed_level() as i32;
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut level, SPEED_LEVELS).show_value(false));
                ui.label(crate::settings::SCROLL_SPEED_LABELS[level as usize]);
            });
            s.scroll_speed = level as u8;
            ui.label(egui::RichText::new("Slower: use it when the capture loses track.").color(dim).size(11.0));
        });
    }

    fn prefs_hotkeys(&mut self, ui: &mut Ui, dim: Color32) {
        if let Some(a) = self.recording {
            let (events, modifiers) = ui.input(|i| (i.events.clone(), i.modifiers));
            match recorded(&events, modifiers) {
                Some(Recorded::Cancel) => self.recording = None,
                Some(Recorded::Clear) => {
                    self.settings.set_hotkey(a, None);
                    self.recording = None;
                }
                Some(Recorded::Set(keys)) => {
                    self.settings.set_hotkey(a, Some(keys));
                    self.recording = None;
                }
                None => {}
            }
        }
        ui.label(egui::RichText::new("Reassign default shortcuts").strong().size(12.0));
        ui.add_space(2.0);
        for step in ["macOS keeps Cmd+Shift+3 and Cmd+Shift+4 for its own screenshots until you free them.", "1. In System Settings > Keyboard > Keyboard Shortcuts, pick Screenshots.", "2. Untick the system screenshot shortcuts.", "3. Put Cmd+Shift+3 into Fullscreen and Cmd+Shift+4 into Area below."] {
            ui.label(egui::RichText::new(step).color(dim).size(11.0));
        }
        ui.add_space(4.0);
        if ui.button("Open System Settings").clicked() {
            crate::system::open_keyboard_settings();
        }
        ui.add_space(12.0);
        for a in crate::settings::HotkeyAction::ALL {
            let current = self.settings.hotkey(a).map(pretty_hotkey);
            row(ui, a.label(), |ui| {
                ui.horizontal(|ui| {
                    let recording = self.recording == Some(a);
                    let text = if recording { "Press the keys…".to_string() } else { current.clone().unwrap_or_else(|| "Click to set".into()) };
                    if ui.add_sized([170.0, 24.0], egui::Button::new(text).selected(recording)).clicked() {
                        self.recording = if recording { None } else { Some(a) };
                    }
                    if current.is_some() {
                        let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
                        }
                        crate::icons::draw(ui.painter(), r.shrink(4.0), crate::icons::Icon::Trash, dim);
                        if resp.on_hover_text("Remove this shortcut").clicked() {
                            self.settings.set_hotkey(a, None);
                        }
                    }
                    if self.refused.contains(&a) {
                        ui.label(egui::RichText::new("used by another app").color(Color32::from_rgb(0xFF, 0x6B, 0x5B)).size(11.0));
                    }
                });
            });
        }
    }

    fn prefs_advanced(&mut self, ui: &mut Ui, dim: Color32) {
        let PrefsState { settings: s, confirm_reset, .. } = self;
        row(ui, "Primary OCR language", |ui| {
            let languages = crate::system::ocr_languages();
            let name = |code: &str| languages.iter().find(|(c, _)| c == code).map_or(code.to_string(), |(_, n)| n.clone());
            let label = if s.ocr_language.is_empty() { "Automatic".to_string() } else { name(&s.ocr_language) };
            crate::icons::combo("ocrlang").width(200.0).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.ocr_language, String::new(), "Automatic");
                for (code, name) in languages {
                    ui.selectable_value(&mut s.ocr_language, code.clone(), name);
                }
            });
        });
        row(ui, "OCR line breaks", |ui| {
            ui.checkbox(&mut s.ocr_remove_breaks, "Remove line breaks");
        });
        row(ui, "Menubar icon", |ui| {
            ui.checkbox(&mut s.hide_menubar_icon, "Hide menubar icon");
            ui.label(egui::RichText::new("Needs a working Show ShotCraft hotkey").color(dim).size(11.0));
        });
        row(ui, "Main window", |ui| {
            ui.checkbox(&mut s.always_on_top, "Always on top");
        });
        row(ui, "Zoom with mouse wheel", |ui| {
            ui.checkbox(&mut s.reverse_zoom, "Reverse scroll direction");
        });
        row(ui, "Scrolling capture", |ui| {
            ui.checkbox(&mut s.scroll_reverse, "Reverse scroll direction").on_hover_text("For apps that reverse the mouse wheel. The macOS natural scrolling setting does not need this.");
            ui.checkbox(&mut s.scroll_manual, "Scroll by hand").on_hover_text("ShotCraft grabs the area while you scroll it, instead of scrolling for you");
        });
        row(ui, "Default zoom level", |ui| {
            ui.checkbox(&mut s.prefer_100, "Prefer 100%");
            ui.set_max_width(236.0);
            ui.label(egui::RichText::new("When selected, Area Capture will open the editor at 100% zoom when possible, and Fullscreen Capture will open the editor at 80% zoom.").color(dim).size(11.0));
        });
        row(ui, "Default tool", |ui| {
            let label = if s.default_tool.is_empty() { "Keep the current tool" } else { s.default_tool.as_str() };
            crate::icons::combo("deftool").width(200.0).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.default_tool, String::new(), "Keep the current tool");
                for t in crate::model::Tool::TOOLBAR.into_iter().filter(|t| *t != crate::model::Tool::Backdrop) {
                    ui.selectable_value(&mut s.default_tool, t.name().to_string(), t.name());
                }
            });
        });
        row(ui, "Action when hiding with Esc", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.esc_copy, "Copy Image");
                ui.checkbox(&mut s.esc_save, "Save Image");
            });
        });
        row(ui, "Confirmation style", |ui| {
            let label = match s.confirmation {
                Confirm::Custom => "Custom Notification",
                Confirm::System => "System Notification",
                Confirm::None => "None",
            };
            crate::icons::combo("confirm").width(200.0).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.confirmation, Confirm::Custom, "Custom Notification");
                ui.selectable_value(&mut s.confirmation, Confirm::System, "System Notification");
                ui.selectable_value(&mut s.confirmation, Confirm::None, "None");
            });
        });
        row(ui, "Reset", |ui| {
            if *confirm_reset {
                ui.label(egui::RichText::new("Put every preference back to its default?").size(12.0));
                ui.horizontal(|ui| {
                    if ui.button("Reset All Settings").clicked() {
                        *s = Settings::default();
                        *confirm_reset = false;
                    }
                    if ui.button("Cancel").clicked() {
                        *confirm_reset = false;
                    }
                });
            } else if ui.button("Reset All Settings…").clicked() {
                *confirm_reset = true;
            }
        });
    }
}

/// 400000 → "400,000"
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::{save_png, with_rig};

    fn state() -> PrefsState {
        PrefsState { open: true, focus: false, tab: 0, settings: Settings::default(), edited: false, queue: Arc::new(Mutex::new(vec![])), recording: None, refused: vec![], height_text: None, confirm_reset: false }
    }

    /// Renders each tab to `$SHOTCRAFT_SHOTS/<name>-<tab>.png` when that variable is set, and returns the
    /// pictures so a test can check they are not blank.
    fn render_tabs(name: &str, mut st: PrefsState) -> Vec<crate::model::Image> {
        let mut out = vec![];
        for tab in 0..3 {
            st.tab = tab;
            let img = with_rig(|rig| rig.render_window((WINDOW[0], WINDOW[1]), 2.0, vec![], |ui| st.body(ui)));
            if let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") {
                save_png(&img, &std::path::Path::new(&dir).join(format!("{name}-{}.png", ["general", "hotkeys", "advanced"][tab])));
            }
            out.push(img);
        }
        out
    }

    #[test]
    fn every_tab_draws_and_the_tabs_differ() {
        let tabs = render_tabs("prefs", state());
        assert!(tabs.iter().all(|t| t.px.iter().any(|&b| b != tabs[0].px[0])));
        assert!(tabs[0].px != tabs[1].px && tabs[1].px != tabs[2].px);
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }
    }

    fn cmd() -> egui::Modifiers {
        egui::Modifiers { command: true, mac_cmd: true, ..egui::Modifiers::NONE }
    }

    #[test]
    fn a_key_with_modifiers_is_recorded() {
        let m = egui::Modifiers { shift: true, ..cmd() };
        assert_eq!(recorded(&[key(Key::Num2, m)], m), Some(Recorded::Set("shift+super+Digit2".into())));
        assert_eq!(recorded(&[key(Key::A, egui::Modifiers::NONE)], egui::Modifiers::NONE), None, "a bare letter is no hotkey");
        assert_eq!(recorded(&[key(Key::Escape, egui::Modifiers::NONE)], egui::Modifiers::NONE), Some(Recorded::Cancel));
        assert_eq!(recorded(&[key(Key::Backspace, egui::Modifiers::NONE)], egui::Modifiers::NONE), Some(Recorded::Clear));
    }

    /// egui-winit sends Cmd+C / Cmd+X / Cmd+V as copy, cut and paste events and no key event.
    #[test]
    fn cmd_c_x_v_arrive_as_clipboard_events_and_are_still_recorded() {
        let shift = egui::Modifiers { shift: true, ..cmd() };
        assert_eq!(recorded(&[egui::Event::Copy], cmd()), Some(Recorded::Set("super+KeyC".into())));
        assert_eq!(recorded(&[egui::Event::Cut], shift), Some(Recorded::Set("shift+super+KeyX".into())));
        assert_eq!(recorded(&[egui::Event::Paste("x".into())], cmd()), Some(Recorded::Set("super+KeyV".into())));
    }

    #[test]
    fn thousands_are_grouped() {
        assert_eq!((thousands(0), thousands(999), thousands(1_000), thousands(400_000), thousands(1_234_567)), ("0".into(), "999".into(), "1,000".into(), "400,000".into(), "1,234,567".into()));
    }
}

#[cfg(test)]
mod capture_rows_tests {
    use super::*;

    /// Draws the General tab. With SHOTCRAFT_TEST_OUT set, the picture is written there.
    #[test]
    fn the_general_tab_with_the_capture_rows_fits_the_window() {
        let queue: crate::Queue = Default::default();
        let mut state = PrefsState::shared(Settings::default(), queue);
        let ppp = 2.0;
        let img = crate::headless::with_rig(|rig| {
            rig.render_ui((WINDOW[0], WINDOW[1]), ppp, Color32::from_rgb(40, 40, 44), |ui| {
                state.lock().unwrap().body(ui);
            })
        });
        if let Some(dir) = std::env::var_os("SHOTCRAFT_TEST_OUT") {
            let name = std::env::var("SHOTCRAFT_TEST_TAG").unwrap_or_else(|_| "general".into());
            crate::headless::save_png(&img, &std::path::PathBuf::from(dir).join(format!("prefs-{name}.png")));
        }
        assert_eq!((img.w, img.h), ((WINDOW[0] * ppp) as u32, (WINDOW[1] * ppp) as u32));
        let _ = &mut state;
    }
}
