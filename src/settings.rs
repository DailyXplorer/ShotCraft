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
    Transparent,
    Solid,
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
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 8] = [
        HotkeyAction::Fullscreen,
        HotkeyAction::AllDisplays,
        HotkeyAction::Area,
        HotkeyAction::Window,
        HotkeyAction::ActiveWindow,
        HotkeyAction::RepeatArea,
        HotkeyAction::Ocr,
        HotkeyAction::Show,
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
    pub trim_shadow: bool,
    pub always_on_top: bool,
    pub reverse_zoom: bool,
    pub prefer_100: bool,
    pub esc_copy: bool,
    pub esc_save: bool,
    pub confirmation: Confirm,
    pub color_format: usize,
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
            trim_shadow: false,
            always_on_top: false,
            reverse_zoom: false,
            prefer_100: false,
            esc_copy: true,
            esc_save: false,
            confirmation: Confirm::Custom,
            color_format: 0,
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
        std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
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
