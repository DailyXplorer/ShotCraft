use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfterShow {
    Editor,
    Thumbnail,
    Nothing,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowBg {
    /// The window with its shadow on a see-through background.
    Transparent,
    /// The window cut tight, without its shadow; the corners stay rounded and see-through.
    Trimmed,
    /// The window and its shadow on a solid colour: an opaque picture.
    Solid,
    /// The window and its shadow on the desktop wallpaper: an opaque picture.
    Wallpaper,
}

/// The waits Delayed Screenshot offers, in seconds. The first one is the default.
pub const DELAYS: [u32; 3] = [3, 5, 10];
pub const DEFAULT_DELAY: u32 = DELAYS[0];

/// The menu entry of Delayed Screenshot, which tells how long it waits.
pub fn delayed_label(secs: u32) -> String {
    format!("Delayed Screenshot ({secs}s)")
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confirm {
    Custom,
    System,
    None,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum HotkeyAction {
    Fullscreen,
    Area,
    Window,
    ActiveWindow,
    AllDisplays,
    RepeatArea,
    Ocr,
    Show,
    Scrolling,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 9] = [
        HotkeyAction::Fullscreen,
        HotkeyAction::AllDisplays,
        HotkeyAction::Area,
        HotkeyAction::Window,
        HotkeyAction::ActiveWindow,
        HotkeyAction::RepeatArea,
        HotkeyAction::Ocr,
        HotkeyAction::Show,
        HotkeyAction::Scrolling,
    ];
    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::Fullscreen => "Fullscreen screenshot",
            HotkeyAction::AllDisplays => "All displays screenshot",
            HotkeyAction::Area => "Area screenshot",
            HotkeyAction::Window => "Any window screenshot",
            HotkeyAction::ActiveWindow => "Active window screenshot",
            HotkeyAction::RepeatArea => "Repeat area screenshot",
            HotkeyAction::Ocr => "Instant Text/QR Recognition",
            HotkeyAction::Show => "Show ShotCraft",
            HotkeyAction::Scrolling => "Scrolling screenshot",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub after_show: AfterShow,
    pub after_copy: bool,
    pub after_save: bool,
    pub thumbnail_auto_close: bool,
    pub autostart: bool,
    pub folder: String,
    pub save_auto_format: bool,
    pub downscale_retina: bool,
    pub window_bg: WindowBg,
    pub window_solid: String,
    /// Whether the mouse pointer is in the pictures of the captures that take no selection.
    pub capture_cursor: bool,
    /// Seconds Delayed Screenshot waits before it takes the picture.
    pub delay_secs: u32,
    /// Settings files written before `WindowBg::Trimmed` kept "Trim shadow" as its own checkbox.
    /// Only read, to carry an old choice over; never written.
    #[serde(skip_serializing)]
    pub trim_shadow: bool,
    pub always_on_top: bool,
    pub reverse_zoom: bool,
    pub prefer_100: bool,
    pub esc_copy: bool,
    pub esc_save: bool,
    pub confirmation: Confirm,
    pub color_format: usize,
    pub contrast_type: crate::color::ContrastType,
    pub logical_points: bool,
    pub ocr_remove_breaks: bool,
    pub hide_menubar_icon: bool,
    pub hotkeys: Vec<(HotkeyAction, String)>,
    pub last_area: Option<[f32; 4]>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            after_show: AfterShow::Editor,
            after_copy: false,
            after_save: false,
            thumbnail_auto_close: false,
            autostart: false,
            folder: crate::system::home().join("Desktop").display().to_string(),
            save_auto_format: false,
            downscale_retina: false,
            window_bg: WindowBg::Transparent,
            window_solid: "#C4C6C8".into(),
            capture_cursor: false,
            delay_secs: DEFAULT_DELAY,
            trim_shadow: false,
            always_on_top: false,
            reverse_zoom: false,
            prefer_100: false,
            esc_copy: true,
            esc_save: false,
            confirmation: Confirm::Custom,
            color_format: 0,
            contrast_type: crate::color::ContrastType::Wcag2,
            logical_points: false,
            ocr_remove_breaks: false,
            hide_menubar_icon: false,
            hotkeys: vec![
                (HotkeyAction::Fullscreen, "super+shift+Digit1".into()),
                (HotkeyAction::Area, "super+shift+Digit2".into()),
                (HotkeyAction::ActiveWindow, "control+alt+super+KeyW".into()),
                (HotkeyAction::Ocr, "control+alt+super+KeyO".into()),
            ],
            last_area: None,
        }
    }
}

fn path() -> PathBuf {
    crate::system::home().join("Library/Application Support/ShotCraft/settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read(path()).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default().migrated()
    }

    /// Carries the old "Trim shadow" checkbox over to the window background mode.
    fn migrated(mut self) -> Settings {
        if std::mem::take(&mut self.trim_shadow) && self.window_bg == WindowBg::Transparent {
            self.window_bg = WindowBg::Trimmed;
        }
        self
    }

    pub fn save(&self) {
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(p, b);
        }
    }

    /// The menu bar icon is the only visible part of the app, so the "hide" setting only takes
    /// effect while the Show ShotCraft hotkey works: without either, nothing could open it again.
    pub fn menubar_icon_visible(&self, show_hotkey_works: bool) -> bool {
        !self.hide_menubar_icon || !show_hotkey_works
    }

    /// The wait of Delayed Screenshot: a hand-edited settings file can't make it zero or endless.
    pub fn delay(&self) -> u32 {
        self.delay_secs.clamp(1, 60)
    }

    pub fn hotkey(&self, a: HotkeyAction) -> Option<&str> {
        self.hotkeys.iter().find(|(k, _)| *k == a).map(|(_, v)| v.as_str()).filter(|s| !s.is_empty())
    }

    pub fn set_hotkey(&mut self, a: HotkeyAction, v: Option<String>) {
        self.hotkeys.retain(|(k, _)| *k != a);
        if let Some(v) = v {
            self.hotkeys.push((a, v));
        }
    }
}

/// "super+shift+Digit1" → "⌘⇧1"
pub fn pretty_hotkey(s: &str) -> String {
    let mut out = String::new();
    for part in s.split('+') {
        out.push_str(match part {
            "ctrl" | "control" => "⌃",
            "alt" | "option" => "⌥",
            "shift" => "⇧",
            "super" | "cmd" | "command" => "⌘",
            k => k.strip_prefix("Digit").or_else(|| k.strip_prefix("Key")).unwrap_or(k),
        });
    }
    out
}

#[cfg(test)]
mod window_bg_tests {
    use super::*;

    fn load(json: &str) -> Settings {
        serde_json::from_slice::<Settings>(json.as_bytes()).unwrap().migrated()
    }

    #[test]
    fn the_factory_default_keeps_the_shadow_on_a_transparent_background() {
        let s = Settings::default();
        assert_eq!(s.window_bg, WindowBg::Transparent);
        assert_eq!(s.window_solid, "#C4C6C8");
    }

    #[test]
    fn old_settings_files_still_load() {
        assert_eq!(load("{}").window_bg, WindowBg::Transparent);
        assert_eq!(load(r#"{"window_bg":"Solid"}"#).window_bg, WindowBg::Solid);
        assert_eq!(load(r#"{"window_bg":"Wallpaper"}"#).window_bg, WindowBg::Wallpaper);
    }

    #[test]
    fn the_old_trim_shadow_checkbox_becomes_a_mode() {
        assert_eq!(load(r#"{"window_bg":"Transparent","trim_shadow":true}"#).window_bg, WindowBg::Trimmed);
        // With a solid colour the shadow was composited, not trimmed: the colour wins.
        assert_eq!(load(r#"{"window_bg":"Solid","trim_shadow":true}"#).window_bg, WindowBg::Solid);
        assert_eq!(load(r#"{"trim_shadow":false}"#).window_bg, WindowBg::Transparent);
    }

    #[test]
    fn the_old_checkbox_is_not_written_back() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.contains("trim_shadow"));
        let again: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(again, Settings::default());
    }
}

#[cfg(test)]
mod capture_option_tests {
    use super::*;

    #[test]
    fn the_pointer_stays_out_and_the_delay_is_three_seconds_by_default() {
        let s = Settings::default();
        assert!(!s.capture_cursor);
        assert_eq!(s.delay(), 3);
    }

    #[test]
    fn old_settings_files_get_the_defaults_and_odd_delays_are_kept_in_range() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!((old.capture_cursor, old.delay_secs), (false, 3));
        let s: Settings = serde_json::from_str(r#"{"capture_cursor":true,"delay_secs":0}"#).unwrap();
        assert!(s.capture_cursor);
        assert_eq!(s.delay(), 1);
        let s: Settings = serde_json::from_str(r#"{"delay_secs":100000}"#).unwrap();
        assert_eq!(s.delay(), 60);
    }

    #[test]
    fn the_menu_label_tells_the_wait() {
        assert_eq!(delayed_label(3), "Delayed Screenshot (3s)");
        assert_eq!(delayed_label(10), "Delayed Screenshot (10s)");
        assert!(DELAYS.contains(&DEFAULT_DELAY));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn hidden_menubar_icon_needs_the_show_hotkey() {
        let mut s = Settings::default();
        s.hide_menubar_icon = true;
        assert!(s.menubar_icon_visible(false), "no working Show hotkey: the icon stays");
        assert!(!s.menubar_icon_visible(true));
        s.hide_menubar_icon = false;
        assert!(s.menubar_icon_visible(true));
    }

    #[test]
    fn default_hotkeys_parse() {
        let settings = Settings::default();
        for (_, keys) in &settings.hotkeys {
            assert!(keys.parse::<global_hotkey::hotkey::HotKey>().is_ok(), "{keys}");
        }
        assert_eq!(pretty_hotkey(settings.hotkey(HotkeyAction::Ocr).unwrap()), "⌃⌥⌘O");
    }
}
