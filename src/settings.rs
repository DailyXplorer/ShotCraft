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
    /// The order of the rows in Preferences → Hotkeys: the original's order, with All displays
    /// next to Fullscreen.
    pub const ALL: [HotkeyAction; 9] = [
        HotkeyAction::Fullscreen,
        HotkeyAction::AllDisplays,
        HotkeyAction::Area,
        HotkeyAction::ActiveWindow,
        HotkeyAction::Scrolling,
        HotkeyAction::Ocr,
        HotkeyAction::RepeatArea,
        HotkeyAction::Window,
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
            HotkeyAction::Scrolling => "Scrolling screenshot",
        }
    }
}

/// Scrolling screenshot max height: pixels of the stitched picture. A hand-edited file can't ask for
/// less than one screenful or for a picture that cannot fit in memory.
pub const SCROLL_MAX_HEIGHT: u32 = 20_000;
pub const SCROLL_MAX_HEIGHT_RANGE: (u32, u32) = (1_000, 400_000);
/// Scrolling speed levels, 0 (slowest) to `SCROLL_SPEED_MAX`.
pub const SCROLL_SPEED_DEFAULT: u8 = 2;
pub const SCROLL_SPEED_MAX: u8 = 4;
pub const SCROLL_SPEED_LABELS: [&str; 5] = ["Slowest", "Slow", "Normal", "Fast", "Fastest"];

/// What a saved screenshot is called, without the extension. The tokens are in `file_stem`.
pub const FILE_NAME_TEMPLATE: &str = "ShotCraft-%Y%m%d-%t";

/// The shape of an annotation tool's style as it is kept between launches. Mirrors the editor's
/// style; the editor converts both ways, so this file stays free of the editor's types. Choices are
/// small numbers (the position in the editor's list) and the editor clamps what it reads back.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ToolStyle {
    pub color: String,
    pub size: u8,
    pub dashed: bool,
    pub arrow: u8,
    pub fill: u8,
    pub shape: u8,
    pub font: u8,
    pub blur: u8,
    pub smooth: u8,
    pub vary: u8,
    pub round_cap: bool,
    pub dim: f32,
}

impl Default for ToolStyle {
    fn default() -> Self {
        ToolStyle { color: crate::model::DEFAULT_COLOR.into(), size: crate::model::SIZE_DEFAULT, dashed: false, arrow: 0, fill: 1, shape: 0, font: 0, blur: 0, smooth: 0, vary: 0, round_cap: false, dim: crate::model::SPOTLIGHT_DIM }
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
    /// Area Capture freezes the screen and draws its own selection on the frozen picture, instead
    /// of the system selector. Off by default, as in the original (where it is experimental).
    pub frozen_area: bool,
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
    /// Scrolling screenshot max height, in pixels of the stitched picture. Read it with `scroll_max_height`.
    pub scroll_max_height: u32,
    /// Scrolling screenshot speed, 0 to `SCROLL_SPEED_MAX`. Read it with `scroll_speed_level`.
    pub scroll_speed: u8,
    /// For apps that turn the wheel around: scroll the other way.
    pub scroll_reverse: bool,
    /// Scroll by hand even when ShotCraft could do it.
    pub scroll_manual: bool,
    /// Name of saved screenshots; see `file_stem` for the tokens.
    pub file_name_template: String,
    /// Language the text recognition tries first, as a Vision language code; empty for automatic.
    pub ocr_language: String,
    /// Tool that is selected when a screenshot opens, by its name; empty keeps the one in use.
    pub default_tool: String,
    /// Last style of each annotation tool, by tool name.
    pub tool_styles: std::collections::BTreeMap<String, ToolStyle>,
    pub hotkeys: Vec<(HotkeyAction, String)>,
    pub last_area: Option<[f32; 4]>,
    /// Draw menu: Snap to Objects (edges and centres line up with other annotations).
    pub snap_objects: bool,
    /// Draw menu: Snap to Similar Objects (same sizes and equal spacing among annotations of one kind).
    pub snap_similar: bool,
    /// The backdrop configuration used last; new backdrops start from it.
    pub backdrop: Option<crate::model::Backdrop>,
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
            frozen_area: false,
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
            scroll_max_height: SCROLL_MAX_HEIGHT,
            scroll_speed: SCROLL_SPEED_DEFAULT,
            scroll_reverse: false,
            scroll_manual: false,
            file_name_template: FILE_NAME_TEMPLATE.into(),
            ocr_language: String::new(),
            default_tool: String::new(),
            tool_styles: Default::default(),
            hotkeys: vec![
                (HotkeyAction::Fullscreen, "super+shift+Digit1".into()),
                (HotkeyAction::Area, "super+shift+Digit2".into()),
                (HotkeyAction::ActiveWindow, "control+alt+super+KeyW".into()),
                (HotkeyAction::Ocr, "control+alt+super+KeyO".into()),
            ],
            last_area: None,
            snap_objects: true,
            snap_similar: false,
            backdrop: None,
        }
    }
}

/// Where an unreadable settings file is kept: `settings.json.bad` next to it.
fn bad_copy_path(p: &std::path::Path) -> PathBuf {
    let mut name = p.file_name().unwrap_or_default().to_os_string();
    name.push(".bad");
    p.with_file_name(name)
}

fn path() -> PathBuf {
    crate::system::home().join("Library/Application Support/ShotCraft/settings.json")
}

impl Settings {
    pub fn load() -> Settings {
        Settings::load_from(&path())
    }

    /// Reads the settings file. A missing file gives the defaults silently. A file that exists but
    /// can't be read as settings (damaged, or written by another version with a value this one
    /// doesn't know) is copied to `settings.json.bad` first, and the reason is logged: the next save
    /// would overwrite it with the defaults, and the user's choices would be gone for good.
    fn load_from(p: &std::path::Path) -> Settings {
        let bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("settings: can't read {}: {e}; using the defaults", p.display());
                }
                return Settings::default();
            }
        };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(s) => s.migrated(),
            Err(e) => {
                let bad = bad_copy_path(p);
                match std::fs::write(&bad, &bytes) {
                    Ok(()) => eprintln!("settings: {} is unreadable ({e}); kept a copy in {} and using the defaults", p.display(), bad.display()),
                    Err(w) => eprintln!("settings: {} is unreadable ({e}) and the copy to {} failed ({w}); using the defaults", p.display(), bad.display()),
                }
                Settings::default()
            }
        }
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

    /// The height limit of a scrolling screenshot, kept in range.
    pub fn scroll_max_height(&self) -> u32 {
        self.scroll_max_height.clamp(SCROLL_MAX_HEIGHT_RANGE.0, SCROLL_MAX_HEIGHT_RANGE.1)
    }

    pub fn scroll_speed_level(&self) -> u8 {
        self.scroll_speed.min(SCROLL_SPEED_MAX)
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

/// A date and time on the clock, for names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stamp {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

const NAME_MAX: usize = 120;
/// File names are limited in bytes too (255 on macOS, and the extension and a `-N` suffix come after).
const NAME_MAX_BYTES: usize = 200;

/// The name of a saved screenshot (no extension) from `template` and the time. Tokens: `%Y` year,
/// `%y` two-digit year, `%m` month, `%d` day, `%H` hour, `%M` minute, `%S` second, `%t` the time as
/// `%H%M%S`, `%%` a percent sign. Other text, including unknown tokens, stays as typed. Characters a
/// file name can't hold become `-`; an empty result falls back to the default template.
pub fn file_stem(template: &str, now: &Stamp) -> String {
    let render = |t: &str| {
        let mut out = String::new();
        let mut chars = t.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('Y') => out.push_str(&format!("{:04}", now.year)),
                Some('y') => out.push_str(&format!("{:02}", now.year.rem_euclid(100))),
                Some('m') => out.push_str(&format!("{:02}", now.month)),
                Some('d') => out.push_str(&format!("{:02}", now.day)),
                Some('H') => out.push_str(&format!("{:02}", now.hour)),
                Some('M') => out.push_str(&format!("{:02}", now.minute)),
                Some('S') => out.push_str(&format!("{:02}", now.second)),
                Some('t') => out.push_str(&format!("{:02}{:02}{:02}", now.hour, now.minute, now.second)),
                Some('%') => out.push('%'),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            }
        }
        let clean: String = out.chars().map(|c| if matches!(c, '/' | '\\' | ':') || c.is_control() { '-' } else { c }).collect();
        clean.trim().trim_start_matches('.').chars().scan(0usize, |bytes, c| { *bytes += c.len_utf8(); (*bytes <= NAME_MAX_BYTES).then_some(c) }).take(NAME_MAX).collect::<String>().trim_end().to_string()
    };
    let name = render(template);
    if name.is_empty() {
        render(FILE_NAME_TEMPLATE)
    } else {
        name
    }
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
    fn snapping_defaults_to_objects_only_and_survives_a_save() {
        let s = load("{}");
        assert!(s.snap_objects && !s.snap_similar);
        let mut s = Settings::default();
        s.snap_objects = false;
        s.snap_similar = true;
        let back = load(&serde_json::to_string(&s).unwrap());
        assert!(!back.snap_objects && back.snap_similar);
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
mod preference_tests {
    use super::*;

    const NOW: Stamp = Stamp { year: 2026, month: 11, day: 5, hour: 9, minute: 7, second: 3 };

    #[test]
    fn old_settings_files_get_the_new_defaults() {
        let old: Settings = serde_json::from_str(r#"{"after_show":"Thumbnail","folder":"/tmp/x","hotkeys":[["Area","super+shift+Digit2"]]}"#).unwrap();
        assert_eq!(old.after_show, AfterShow::Thumbnail);
        assert_eq!(old.folder, "/tmp/x");
        assert_eq!(old.hotkey(HotkeyAction::Area), Some("super+shift+Digit2"));
        assert_eq!((old.scroll_max_height(), old.scroll_speed_level(), old.scroll_reverse, old.scroll_manual), (20_000, 2, false, false));
        assert_eq!(old.file_name_template, "ShotCraft-%Y%m%d-%t");
        assert_eq!((old.ocr_language.as_str(), old.default_tool.as_str()), ("", ""));
        assert!(old.tool_styles.is_empty());
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.scroll_max_height, SCROLL_MAX_HEIGHT);
    }

    #[test]
    fn the_new_fields_survive_a_round_trip() {
        let mut s = Settings::default();
        s.scroll_max_height = 50_000;
        s.scroll_speed = 4;
        s.scroll_reverse = true;
        s.scroll_manual = true;
        s.file_name_template = "SCR-%Y%m%d-%t".into();
        s.ocr_language = "fr-FR".into();
        s.default_tool = "Arrow".into();
        s.tool_styles.insert("Rectangle".into(), ToolStyle { size: 1, color: "#00A490".into(), ..ToolStyle::default() });
        let back: Settings = serde_json::from_str(&serde_json::to_string_pretty(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn a_stored_style_missing_fields_fills_them_in() {
        let s: Settings = serde_json::from_str(r##"{"tool_styles":{"Oval":{"size":4}}}"##).unwrap();
        let oval = &s.tool_styles["Oval"];
        assert_eq!((oval.size, oval.color.as_str()), (4, "#FF0C01"));
    }

    #[test]
    fn the_scrolling_limits_stay_in_range() {
        let mut s = Settings::default();
        s.scroll_max_height = 0;
        assert_eq!(s.scroll_max_height(), 1_000);
        s.scroll_max_height = 4_000_000_000;
        assert_eq!(s.scroll_max_height(), 400_000);
        s.scroll_max_height = 123_456;
        assert_eq!(s.scroll_max_height(), 123_456);
        s.scroll_speed = 200;
        assert_eq!(s.scroll_speed_level(), SCROLL_SPEED_MAX);
        assert_eq!(SCROLL_SPEED_LABELS.len(), SCROLL_SPEED_MAX as usize + 1);
    }

    #[test]
    fn hotkey_rows_follow_the_original_order() {
        use HotkeyAction::*;
        assert_eq!(HotkeyAction::ALL, [Fullscreen, AllDisplays, Area, ActiveWindow, Scrolling, Ocr, RepeatArea, Window, Show]);
        let mut seen = HotkeyAction::ALL.to_vec();
        seen.dedup();
        assert_eq!(seen.len(), 9);
    }

    #[test]
    fn the_default_name_is_the_old_name() {
        assert_eq!(file_stem(FILE_NAME_TEMPLATE, &NOW), "ShotCraft-20261105-090703");
    }

    #[test]
    fn name_tokens_turn_into_the_date_and_time() {
        assert_eq!(file_stem("SCR-%Y%m%d-%t", &NOW), "SCR-20261105-090703");
        assert_eq!(file_stem("%y-%m-%d %H.%M.%S", &NOW), "26-11-05 09.07.03");
        assert_eq!(file_stem("100%% %Q", &NOW), "100% %Q");
        assert_eq!(file_stem("end%", &NOW), "end%");
    }

    #[test]
    fn names_are_made_safe_and_never_empty() {
        assert_eq!(file_stem("a/b:c\\d", &NOW), "a-b-c-d");
        assert_eq!(file_stem("..hidden", &NOW), "hidden");
        assert_eq!(file_stem("", &NOW), "ShotCraft-20261105-090703");
        assert_eq!(file_stem("   ", &NOW), "ShotCraft-20261105-090703");
        assert_eq!(file_stem(&"x".repeat(500), &NOW).len(), 120);
        let wide = file_stem(&"名".repeat(500), &NOW);
        assert!(wide.len() <= 200 && wide.chars().all(|c| c == '名'), "{}", wide.len());
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
    fn the_frozen_grabber_is_off_until_chosen_and_old_files_keep_it_off() {
        assert!(!Settings::default().frozen_area);
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.frozen_area);
        let on: Settings = serde_json::from_str(r#"{"frozen_area":true}"#).unwrap();
        assert!(on.frozen_area);
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

#[cfg(test)]
mod load_tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shotcraft-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_missing_file_gives_the_defaults_and_no_backup() {
        let d = dir("missing");
        let s = Settings::load_from(&d.join("settings.json"));
        assert_eq!(s.delay_secs, Settings::default().delay_secs);
        assert!(!d.join("settings.json.bad").exists());
    }

    #[test]
    fn an_unreadable_file_is_kept_as_bad_before_the_defaults_are_used() {
        let d = dir("bad");
        let file = d.join("settings.json");
        // A value this version doesn't know (what a downgrade leaves behind) next to settings that matter.
        let text = r#"{"after_show": "FromTheFuture", "folder": "/precious"}"#;
        std::fs::write(&file, text).unwrap();
        let s = Settings::load_from(&file);
        assert_eq!(s.folder, Settings::default().folder, "falls back to the defaults");
        assert_eq!(std::fs::read_to_string(d.join("settings.json.bad")).unwrap(), text, "the original is kept");
    }

    #[test]
    fn a_good_file_is_loaded_and_leaves_no_backup() {
        let d = dir("good");
        let file = d.join("settings.json");
        std::fs::write(&file, r#"{"folder": "/pics"}"#).unwrap();
        assert_eq!(Settings::load_from(&file).folder, "/pics");
        assert!(!d.join("settings.json.bad").exists());
    }
}
