use crate::editor::th;
use crate::settings::{pretty_hotkey, AfterShow, Confirm, HotkeyAction, Settings, WindowBg};
use crate::{Action, App, Queue};
use eframe::egui::{self, vec2, Align2, Color32, FontFamily, Key, Ui, ViewportBuilder, ViewportId};
use std::sync::{Arc, Mutex};

const LABEL_W: f32 = 200.0;
pub const WINDOW: [f32; 2] = [520.0, 440.0];

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
}

impl PrefsState {
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
        }))
    }
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
        let (edited, open, focus) = {
            let mut p = self.prefs.lock().unwrap();
            let edited = std::mem::take(&mut p.edited).then(|| p.settings.clone());
            if edited.is_none() {
                p.settings = self.settings.clone();
            }
            (edited, p.open, std::mem::take(&mut p.focus))
        };
        if let Some(new) = edited {
            self.apply_settings(new);
        }
        if !open {
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
            if ui.input(|i| i.viewport().close_requested()) {
                p.open = false;
                p.recording = None;
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
            p.body(ui);
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
                    }
                }
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.bar2).inner_margin(20)).show(ui, |ui| {
            let before = self.settings.clone();
            match self.tab {
                0 => self.prefs_general(ui),
                1 => self.prefs_hotkeys(ui, t.dim),
                _ => self.prefs_advanced(ui, t.dim),
            }
            if self.settings != before {
                self.edited = true;
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
        });
    }

    fn prefs_general(&mut self, ui: &mut Ui) {
        let queue = self.queue.clone();
        let s = &mut self.settings;
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
            let label = if s.thumbnail_auto_close { "Automatically" } else { "Manually (close button)" };
            crate::icons::combo("thumbclose").width(200.0).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.thumbnail_auto_close, true, "Automatically");
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
        row(ui, "Save format", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.save_auto_format, false, "PNG");
                ui.radio_value(&mut s.save_auto_format, true, "Auto (PNG/JPEG)");
            });
        });
        row(ui, "Resize retina screenshots", |ui| {
            ui.checkbox(&mut s.downscale_retina, "Downscale to 1× when saving");
        });
        row(ui, "Window Screenshot Background", |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.window_bg, WindowBg::Transparent, "Transparent");
                ui.radio_value(&mut s.window_bg, WindowBg::Solid, "Solid Color");
                if s.window_bg == WindowBg::Solid {
                    let c = crate::model::hex(&s.window_solid);
                    let mut col = Color32::from_rgb(c[0], c[1], c[2]);
                    if ui.color_edit_button_srgba(&mut col).changed() {
                        s.window_solid = format!("#{:02X}{:02X}{:02X}", col.r(), col.g(), col.b());
                    }
                }
            });
            ui.checkbox(&mut s.trim_shadow, "Trim shadow");
        });
    }

    fn prefs_hotkeys(&mut self, ui: &mut Ui, dim: Color32) {
        if let Some(a) = self.recording {
            let events = ui.input(|i| i.events.clone());
            for e in events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                    if key == Key::Escape {
                        self.recording = None;
                    } else if key == Key::Backspace || key == Key::Delete {
                        self.settings.set_hotkey(a, None);
                        self.recording = None;
                    } else if let Some(code) = key_code(key) {
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
                            let s = format!("{}+{}", parts.join("+"), code);
                            self.settings.set_hotkey(a, Some(s));
                            self.recording = None;
                        }
                    }
                }
            }
        }
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
        let s = &mut self.settings;
        row(ui, "OCR line breaks", |ui| {
            ui.checkbox(&mut s.ocr_remove_breaks, "Remove line breaks");
        });
        row(ui, "Menubar icon", |ui| {
            ui.checkbox(&mut s.hide_menubar_icon, "Hide menubar icon");
        });
        row(ui, "Main window", |ui| {
            ui.checkbox(&mut s.always_on_top, "Always on top");
        });
        row(ui, "Zoom with mouse wheel", |ui| {
            ui.checkbox(&mut s.reverse_zoom, "Reverse scroll direction");
        });
        row(ui, "Default zoom level", |ui| {
            ui.checkbox(&mut s.prefer_100, "Prefer 100%");
            ui.set_max_width(236.0);
            ui.label(egui::RichText::new("When selected, Area Capture will open the editor at 100% zoom when possible, and Fullscreen Capture will open the editor at 80% zoom.").color(dim).size(11.0));
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
    }
}
