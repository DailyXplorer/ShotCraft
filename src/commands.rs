//! Every command ShotCraft takes from a menu, the tray, a key or a toolbar button, in one table.
//! The macOS menu bar, the editor's menu bar on Windows and Linux, the tray menu, the editor's
//! keys, the toolbar's tooltips and the Keyboard Shortcuts window are all built from it, so a
//! label, icon or shortcut is written once and the places that show it can't disagree.
use crate::icons::Icon;
use crate::model::Tool;
use crate::settings::{HotkeyAction, Settings};
use crate::system::CaptureMode;
use crate::{Action, App};
#[cfg(not(target_os = "macos"))]
use eframe::egui::{Event, InputState};
use eframe::egui::{Key, KeyboardShortcut, Modifiers};

// macOS reads only the labels and keys (the Keyboard Shortcuts window).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub struct Command {
    /// The item's id in the native menus (tray, macOS menu bar).
    pub id: &'static str,
    pub label: &'static str,
    /// Drawn next to the label in the editor's menus (Windows, Linux).
    pub icon: Option<Icon>,
    /// Its shortcuts in the editor (⌘ on macOS, Ctrl elsewhere); menus show the first.
    pub keys: &'static [KeyboardShortcut],
    /// The system-wide shortcut Preferences sets for it, if it has one.
    pub hotkey: Option<HotkeyAction>,
    pub action: fn() -> Action,
    /// Whether it can run now: menus grey it out and its keys do nothing when it can't.
    pub enabled: fn(&App) -> bool,
    /// An on/off setting: the menus show a check mark while it's on.
    pub checked: Option<fn(&Settings) -> bool>,
}

fn always(_: &App) -> bool {
    true
}

/// Whether this session can run the command at all. On Linux, scrolling capture can't run on
/// Wayland (`capture::scrolling_unavailable`), so its items are greyed out there and say why; the
/// hotkey and `shotcraft://` links still explain it in the window.
pub fn session_allows(id: &str) -> bool {
    #[cfg(target_os = "linux")]
    let ok = allowed_in(id, crate::capture::wayland());
    #[cfg(not(target_os = "linux"))]
    let ok = {
        let _ = id;
        true
    };
    ok
}

#[cfg(target_os = "linux")]
fn allowed_in(id: &str, wayland: bool) -> bool {
    let scrolling = id == SCROLLING.id || id == SCROLLING_UP.id;
    !scrolling || crate::capture::scrolling_unavailable(wayland).is_none()
}

/// The label a menu shows for `c`: with the reason when this session can't run it.
pub fn session_label(c: &Command) -> std::borrow::Cow<'static, str> {
    if session_allows(c.id) {
        c.label.into()
    } else {
        format!("{} (needs X11)", c.label).into()
    }
}

fn scrolling_possible(_: &App) -> bool {
    session_allows(SCROLLING.id)
}

fn picture(app: &App) -> bool {
    app.doc.is_some()
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const CMD_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);

const fn key(m: Modifiers, k: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(m, k)
}

const BASE: Command = Command { id: "", label: "", icon: None, keys: &[], hotkey: None, action: || Action::Show, enabled: always, checked: None };

// Capture (tray, and the Capture menu on Windows and Linux).
pub const AREA: Command = Command { id: "area", label: "Area Capture", icon: Some(Icon::SelSize), hotkey: Some(HotkeyAction::Area), action: || Action::Capture(CaptureMode::Area), ..BASE };
pub const FULLSCREEN: Command =
    Command { id: "fullscreen", label: "Fullscreen Screenshot", icon: Some(Icon::Display), hotkey: Some(HotkeyAction::Fullscreen), action: || Action::Capture(CaptureMode::Fullscreen), ..BASE };
pub const REPEAT_AREA: Command =
    Command { id: "repeat", label: "Repeat Area Capture", icon: Some(Icon::Reset), hotkey: Some(HotkeyAction::RepeatArea), action: || Action::Capture(CaptureMode::RepeatArea), ..BASE };
pub const WINDOW: Command =
    Command { id: "window", label: "Capture Any Window", icon: Some(Icon::Window), hotkey: Some(HotkeyAction::Window), action: || Action::Capture(CaptureMode::Window), ..BASE };
pub const DELAYED: Command = Command { id: "delayed", label: "Delayed Screenshot (3s)", icon: Some(Icon::Timer), action: || Action::Capture(CaptureMode::Delayed), ..BASE };
pub const ALL_DISPLAYS: Command =
    Command { id: "alldisplays", label: "All Displays Screenshot", icon: Some(Icon::Display), hotkey: Some(HotkeyAction::AllDisplays), action: || Action::Capture(CaptureMode::AllDisplays), ..BASE };
pub const SCROLLING: Command = Command { id: "scrolling", label: "Scrolling Capture", hotkey: Some(HotkeyAction::Scrolling), action: || Action::ScrollingCapture, enabled: scrolling_possible, ..BASE };
/// Starts at the bottom of the page and goes up.
pub const SCROLLING_UP: Command = Command { id: "scrollingup", label: "Scrolling Capture (Up)", action: || Action::ScrollingCaptureUp, enabled: scrolling_possible, ..BASE };
pub const ACTIVE_WINDOW: Command =
    Command { id: "activewindow", label: "Capture Active Window", icon: Some(Icon::Window), hotkey: Some(HotkeyAction::ActiveWindow), action: || Action::Capture(CaptureMode::ActiveWindow), ..BASE };
pub const INSTANT_OCR: Command = Command { id: "ocr", label: "Recognize Text/QR", icon: Some(Icon::Ocr), hotkey: Some(HotkeyAction::Ocr), action: || Action::InstantOcr, ..BASE };

// The app.
pub const SHOW: Command = Command { id: "show", label: "Show ShotCraft", hotkey: Some(HotkeyAction::Show), action: || Action::Show, ..BASE };
pub const PREFERENCES: Command = Command { id: "prefs", label: "Preferences…", icon: Some(Icon::Gear), keys: &[key(CMD, Key::Comma)], action: || Action::Preferences, ..BASE };
pub const AUTOSTART: Command = Command { id: "autostart", label: "Launch at Startup", action: || Action::ToggleAutostart, checked: Some(|s| s.autostart), ..BASE };
pub const QUIT: Command = Command { id: "quit", label: "Quit ShotCraft", icon: Some(Icon::Close), keys: &[key(CMD, Key::Q)], action: || Action::Quit, ..BASE };
pub const SHORTCUTS: Command = Command { id: "shortcuts", label: "Keyboard Shortcuts", icon: Some(Icon::Keyboard), action: || Action::Shortcuts, ..BASE };
pub const ABOUT: Command = Command { id: "about", label: "About ShotCraft", icon: Some(Icon::Info), action: || Action::About, ..BASE };

// File.
/// macOS keeps ⌘C for the Edit menu's Copy, so the image is ⌘⇧C there; elsewhere Ctrl+C copies the
/// image (Ctrl+Shift+C still works).
#[cfg(target_os = "macos")]
const COPY_KEYS: &[KeyboardShortcut] = &[key(CMD_SHIFT, Key::C), key(CMD, Key::C)];
#[cfg(not(target_os = "macos"))]
const COPY_KEYS: &[KeyboardShortcut] = &[key(CMD, Key::C), key(CMD_SHIFT, Key::C)];
pub const COPY: Command = Command { id: "copy", label: "Copy Image", icon: Some(Icon::Copy), keys: COPY_KEYS, action: || Action::Copy, enabled: picture, ..BASE };
pub const SAVE: Command = Command { id: "save", label: "Save Image", icon: Some(Icon::Save), keys: &[key(CMD, Key::S)], action: || Action::Save, enabled: picture, ..BASE };
pub const SAVE_AS: Command = Command { id: "saveas", label: "Save Image As…", keys: &[key(CMD_SHIFT, Key::S)], action: || Action::SaveAs, enabled: picture, ..BASE };
pub const PIN: Command = Command { id: "pin", label: "Pin to Screen", icon: Some(Icon::Pin), keys: &[key(CMD, Key::P)], action: || Action::Pin, enabled: picture, ..BASE };
pub const OPEN_FILE: Command = Command { id: "open", label: "Open File…", icon: Some(Icon::Folder), keys: &[key(CMD_SHIFT, Key::O)], action: || Action::OpenFile, ..BASE };
pub const OPEN_CLIPBOARD: Command = Command { id: "clipboard", label: "Open From Clipboard", keys: &[key(CMD_SHIFT, Key::V)], action: || Action::OpenClipboard, ..BASE };
pub const CLOSE: Command = Command { id: "close", label: "Close", keys: &[key(CMD, Key::W)], action: || Action::Close, ..BASE };
pub const CLOSE_ALL: Command = Command { id: "closeall", label: "Close All", keys: &[key(CMD_ALT, Key::W)], action: || Action::CloseAll, ..BASE };
/// The system's print dialog (on Linux, the desktop's print portal).
pub const PRINT: Command = Command { id: "print", label: "Print…", keys: &[key(CMD_SHIFT, Key::P)], action: || Action::Print, enabled: picture, ..BASE };

// Edit.
/// The clipboard picture goes over the screenshot as a movable object.
pub const PASTE: Command = Command { id: "paste", label: "Paste", keys: &[key(CMD, Key::V)], action: || Action::Paste, enabled: picture, ..BASE };
/// Windows and Linux also take Ctrl+Y, their usual Redo.
#[cfg(target_os = "macos")]
const REDO_KEYS: &[KeyboardShortcut] = &[key(CMD_SHIFT, Key::Z)];
#[cfg(not(target_os = "macos"))]
const REDO_KEYS: &[KeyboardShortcut] = &[key(CMD_SHIFT, Key::Z), key(CMD, Key::Y)];
pub const UNDO: Command =
    Command { id: "undo", label: "Undo", icon: Some(Icon::Undo), keys: &[key(CMD, Key::Z)], action: || Action::Undo, enabled: |app| app.doc.as_ref().is_some_and(|d| !d.undo.is_empty()), ..BASE };
pub const REDO: Command =
    Command { id: "redo", label: "Redo", icon: Some(Icon::Redo), keys: REDO_KEYS, action: || Action::Redo, enabled: |app| app.doc.as_ref().is_some_and(|d| !d.redo.is_empty()), ..BASE };
pub const RESET_CROP: Command =
    Command { id: "resetcrop", label: "Reset Crop", icon: Some(Icon::Crop), action: || Action::ResetCrop, enabled: |app| app.doc.as_ref().is_some_and(|d| d.crop.is_some()), ..BASE };
pub const RASTERIZE: Command = Command { id: "rasterize", label: "Rasterize (Merge) Image", action: || Action::Rasterize, enabled: |app| app.doc.as_ref().is_some_and(|d| !d.objs.is_empty()), ..BASE };
/// The editor's text-recognition tool (a toolbar button; in no menu, as on macOS).
pub const RECOGNIZE: Command =
    Command { id: "recognize", label: "Recognize Text or QR", icon: Some(Icon::Ocr), keys: &[key(CMD, Key::O)], action: || Action::Tool(Tool::Ocr), enabled: picture, ..BASE };

// Zoom.
pub const ZOOM_IN: Command =
    Command { id: "zoomin", label: "Zoom In", icon: Some(Icon::ZoomIn), keys: &[key(CMD, Key::Equals), key(CMD, Key::Plus)], action: || Action::ZoomIn, enabled: picture, ..BASE };
pub const ZOOM_OUT: Command = Command { id: "zoomout", label: "Zoom Out", icon: Some(Icon::ZoomOut), keys: &[key(CMD, Key::Minus)], action: || Action::ZoomOut, enabled: picture, ..BASE };
pub const ZOOM_FIT: Command = Command { id: "zoomfit", label: "Zoom To Fit", keys: &[key(CMD, Key::Num1)], action: || Action::ZoomFit, enabled: picture, ..BASE };
pub const ZOOM_100: Command = Command { id: "zoom100", label: "Zoom To 100%", keys: &[key(CMD, Key::Num0)], action: || Action::Zoom100, enabled: picture, ..BASE };
pub const ZOOM_SELECTION: Command =
    Command { id: "zoomsel", label: "Zoom To Selection", keys: &[key(CMD, Key::Num2)], action: || Action::ZoomSelection, enabled: |app| app.ed.selection.is_some(), ..BASE };
/// Q and W move the selection's corners; the editor takes the plain keys itself, so they have no keys here.
pub const SELECTION_TOP_LEFT: Command = Command { id: "seltl", label: "Selection Top Left", action: || Action::SelectionTopLeft, enabled: |app| app.ed.selection.is_some(), ..BASE };
pub const SELECTION_BOTTOM_RIGHT: Command = Command { id: "selbr", label: "Selection Bottom Right", action: || Action::SelectionBottomRight, enabled: |app| app.ed.selection.is_some(), ..BASE };
// Draw.
pub const ADD_CAPTURE: Command = Command { id: "addcapture", label: "Add Capture", icon: Some(Icon::SelSize), action: || Action::AddCapture, enabled: picture, ..BASE };
pub const SNAP_OBJECTS: Command = Command { id: "snap", label: "Snap to Objects", action: || Action::ToggleSnap(false), checked: Some(|s| s.snap_objects), ..BASE };
pub const SNAP_SIMILAR: Command = Command { id: "snapsimilar", label: "Snap to Similar Objects", action: || Action::ToggleSnap(true), checked: Some(|s| s.snap_similar), ..BASE };
/// macOS uses its own Enter Full Screen item instead.
#[cfg(not(target_os = "macos"))]
pub const FULL_SCREEN: Command =
    Command { id: "fullscreen-window", label: "Enter Full Screen", icon: Some(Icon::Resize), keys: &[key(Modifiers::NONE, Key::F11)], action: || Action::ToggleFullscreen, ..BASE };

/// Every command, in the order the Keyboard Shortcuts window lists them.
pub const ALL: &[&Command] = &[
    &AREA,
    &FULLSCREEN,
    &ALL_DISPLAYS,
    &REPEAT_AREA,
    &WINDOW,
    &ACTIVE_WINDOW,
    &SCROLLING,
    &SCROLLING_UP,
    &DELAYED,
    &INSTANT_OCR,
    &SHOW,
    &COPY,
    &SAVE,
    &SAVE_AS,
    &PIN,
    &OPEN_FILE,
    &OPEN_CLIPBOARD,
    &CLOSE,
    &CLOSE_ALL,
    &PRINT,
    &PASTE,
    &UNDO,
    &REDO,
    &RESET_CROP,
    &RASTERIZE,
    &RECOGNIZE,
    &ZOOM_IN,
    &ZOOM_OUT,
    &ZOOM_FIT,
    &ZOOM_100,
    &ZOOM_SELECTION,
    &SELECTION_TOP_LEFT,
    &SELECTION_BOTTOM_RIGHT,
    &ADD_CAPTURE,
    &SNAP_OBJECTS,
    &SNAP_SIMILAR,
    #[cfg(not(target_os = "macos"))]
    &FULL_SCREEN,
    &PREFERENCES,
    &AUTOSTART,
    &QUIT,
    &SHORTCUTS,
    &ABOUT,
];

/// The drawing tools, in the Draw menu's order.
#[cfg(not(target_os = "macos"))]
pub const DRAW: [Tool; 13] =
    [Tool::Arrow, Tool::Text, Tool::Rectangle, Tool::Freehand, Tool::Magnifier, Tool::Highlighter, Tool::Blur, Tool::Spotlight, Tool::Counter, Tool::Oval, Tool::Line, Tool::Ruler, Tool::Backdrop];

/// One entry of a menu.
#[cfg(not(target_os = "macos"))]
pub enum Item {
    Cmd(&'static Command),
    Separator,
    /// The drawing tools ([`DRAW`]), each with its key.
    Tools,
    /// A submenu (the tray's "Current Image").
    Sub(&'static str, &'static [Item]),
}

#[cfg(not(target_os = "macos"))]
use Item::{Cmd as C, Separator as Sep};

/// About sits under Help on Windows and Linux.
#[cfg(not(target_os = "macos"))]
const APP_MENU: &[Item] = &[C(&PREFERENCES), C(&AUTOSTART), Sep, C(&QUIT)];

#[cfg(not(target_os = "macos"))]
const FILE_MENU: &[Item] = &[
    C(&COPY),
    C(&SAVE),
    C(&SAVE_AS),
    C(&PIN),
    C(&PRINT),
    Sep,
    C(&OPEN_FILE),
    C(&OPEN_CLIPBOARD),
    Sep,
    C(&CLOSE),
    C(&CLOSE_ALL),
];

#[cfg(not(target_os = "macos"))]
const EDIT_MENU: &[Item] = &[C(&UNDO), C(&REDO), Sep, C(&RESET_CROP), C(&RASTERIZE), Sep, C(&PASTE)];

#[cfg(not(target_os = "macos"))]
const ZOOM_MENU: &[Item] = &[C(&ZOOM_IN), C(&ZOOM_OUT), Sep, C(&ZOOM_FIT), C(&ZOOM_100), Sep, C(&ZOOM_SELECTION), C(&SELECTION_TOP_LEFT), C(&SELECTION_BOTTOM_RIGHT), Sep, C(&FULL_SCREEN)];

#[cfg(not(target_os = "macos"))]
const HELP_MENU: &[Item] = &[C(&SHORTCUTS), C(&ABOUT)];

/// Captures live in the menu bar icon on macOS; Windows and Linux also give them a menu.
#[cfg(not(target_os = "macos"))]
const CAPTURE_MENU: &[Item] = &[C(&AREA), C(&FULLSCREEN), C(&ALL_DISPLAYS), C(&REPEAT_AREA), C(&WINDOW), C(&ACTIVE_WINDOW), C(&DELAYED), Sep, C(&SCROLLING), C(&SCROLLING_UP), Sep, C(&INSTANT_OCR)];

#[cfg(not(target_os = "macos"))]
const DRAW_MENU: &[Item] = &[Item::Tools, C(&ADD_CAPTURE), Sep, C(&SNAP_OBJECTS), C(&SNAP_SIMILAR)];

/// The menu bar: macOS's own at the top of the screen, the editor's on Windows and Linux.
#[cfg(not(target_os = "macos"))]
pub const MENU_BAR: &[(&str, &[Item])] = &[
    ("ShotCraft", APP_MENU),
    #[cfg(not(target_os = "macos"))]
    ("Capture", CAPTURE_MENU),
    ("File", FILE_MENU),
    ("Edit", EDIT_MENU),
    ("Draw", DRAW_MENU),
    ("Zoom", ZOOM_MENU),
    ("Help", HELP_MENU),
];

/// Commands on the current image, from the tray too (the editor shows itself to make the change
/// visible).
#[cfg(not(target_os = "macos"))]
const CURRENT_IMAGE: &[Item] = &[C(&UNDO), C(&REDO), C(&RESET_CROP), C(&RASTERIZE), C(&SAVE_AS)];

/// The menu of the tray icon (the menu bar icon on macOS). macOS has no Dock icon, so this is the
/// menu people see there and it carries every command that isn't on screen in the editor.
#[cfg(not(target_os = "macos"))]
pub const TRAY: &[Item] = &[
    C(&AREA),
    C(&FULLSCREEN),
    C(&ALL_DISPLAYS),
    C(&REPEAT_AREA),
    C(&WINDOW),
    C(&ACTIVE_WINDOW),
    C(&SCROLLING),
    C(&SCROLLING_UP),
    C(&DELAYED),
    C(&INSTANT_OCR),
    C(&OPEN_CLIPBOARD),
    C(&OPEN_FILE),
    Sep,
    C(&SHOW),
    Item::Sub("Current Image", CURRENT_IMAGE),
    Sep,
    C(&PREFERENCES),
    C(&AUTOSTART),
    Sep,
    C(&ABOUT),
    C(&SHORTCUTS),
    Sep,
    C(&QUIT),
];

/// Keys the editor handles itself, in the picture: for the Keyboard Shortcuts window, written the
/// macOS way (`settings::keys` rewrites them for Windows and Linux).
pub const EDITOR_KEYS: &[(&str, &str)] = &[
    ("Hold 2 or ←→ / 1 or ↑↓", "Ruler: horizontal / vertical span under the cursor"),
    ("Tab / ⇧Tab", "Copy pixel colour / darkest nearby colour"),
    ("C (with a selection)", "Copy the average colour"),
    ("Del", "Delete object or erase the selection"),
    ("←↑→↓ / ⇧←↑→↓", "Move the object 1 / 10 pixels"),
    ("Enter", "Apply the crop"),
    ("⇧]", "Resize the picture"),
    ("P", "Sizes in points or pixels"),
    ("Esc", "Deselect, or close the editor"),
    ("Esc (scrolling capture)", "Finish it and keep what was captured, from any app"),
    ("Space + drag", "Move around the picture"),
    #[cfg(not(target_os = "macos"))]
    ("Wheel  ⇧Wheel  ⌘Wheel", "Scroll the picture up and down, sideways, zoom"),
    #[cfg(not(target_os = "macos"))]
    ("PgUp  PgDn  Home  End", "Move through a long picture, to its top or bottom"),
];

impl Command {
    /// The menu's shortcut text: the first key ("⌘⇧S" on macOS, "Ctrl+Shift+S" elsewhere), or "".
    pub fn keys_text(&self) -> String {
        self.keys.first().map(shortcut_text).unwrap_or_default()
    }

    /// What a menu shows on the right: the hotkey set in Preferences, else the editor's key.
    #[cfg(not(target_os = "macos"))]
    pub fn shown_keys(&self, settings: &Settings) -> String {
        match self.hotkey.and_then(|h| settings.hotkey(h)) {
            Some(h) => crate::settings::pretty_hotkey(h),
            None => self.keys_text(),
        }
    }

    /// "Save Image (Ctrl+S)", for a tooltip.
    #[cfg(not(target_os = "macos"))]
    pub fn tooltip(&self) -> String {
        match self.keys_text() {
            k if k.is_empty() => self.label.to_string(),
            k => format!("{} ({k})", self.label.trim_end_matches('…')),
        }
    }
}

/// A shortcut as the system writes it: "⌘⇧S" on macOS, "Ctrl+Shift+S" on Windows and Linux.
pub fn shortcut_text(s: &KeyboardShortcut) -> String {
    let m = s.modifiers;
    let key = match s.logical_key {
        Key::Equals | Key::Plus => "+",
        Key::Minus => "-",
        Key::Comma => ",",
        k => k.symbol_or_name(),
    };
    let mut out = String::new();
    if cfg!(target_os = "macos") {
        for (on, sym) in [(m.command || m.mac_cmd, "⌘"), (m.shift, "⇧"), (m.alt, "⌥"), (m.ctrl && !m.command, "⌃")] {
            if on {
                out.push_str(sym);
            }
        }
    } else {
        for (on, name) in [(m.command || m.ctrl, "Ctrl+"), (m.shift, "Shift+"), (m.alt, "Alt+")] {
            if on {
                out.push_str(name);
            }
        }
    }
    out.push_str(key);
    out
}

/// The command a key and modifiers give in the editor. Shortcuts with Shift are tried first, so
/// ⌘⇧Z finds Redo before ⌘Z finds Undo.
#[cfg(not(target_os = "macos"))]
pub fn for_key(key: Key, m: Modifiers) -> Option<&'static Command> {
    for shift in [true, false] {
        for c in ALL {
            if c.keys.iter().any(|s| {
                s.modifiers.shift == shift && s.logical_key == key && m.alt == s.modifiers.alt && (shift || !m.shift || matches!(key, Key::Plus | Key::Equals)) && m.matches_logically(s.modifiers)
            }) {
                return Some(c);
            }
        }
    }
    None
}

/// The commands this frame's keys ask for.
#[cfg(not(target_os = "macos"))]
pub fn pressed(i: &InputState) -> Vec<&'static Command> {
    let mut out = vec![];
    for e in &i.events {
        let (key, m) = match e {
            Event::Key { key, pressed: true, modifiers, .. } => (*key, *modifiers),
            // egui turns ⌘C / Ctrl+C (with or without Shift) into Copy, with no key event.
            Event::Copy => (Key::C, i.modifiers),
            // ⌘V / Ctrl+V becomes Paste, or nothing at all when the clipboard holds a picture;
            // only the key's release comes through, so V shortcuts run when V comes up. (macOS's
            // menu bar takes them before egui sees them.)
            #[cfg(not(target_os = "macos"))]
            Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command => (Key::V, *modifiers),
            _ => continue,
        };
        if let Some(c) = for_key(key, m) {
            out.push(c);
        }
    }
    out
}

/// The command a global hotkey runs.
#[cfg(not(target_os = "macos"))]
pub fn for_hotkey(h: HotkeyAction) -> Option<&'static Command> {
    ALL.iter().copied().find(|c| c.hotkey == Some(h))
}

/// The action of a native menu item: a command's id, "tool:<name>" for a Draw menu item, or one of
/// the updater's items.
#[cfg(not(target_os = "macos"))]
pub fn action_for_id(id: &str) -> Option<Action> {
    if let Some(name) = id.strip_prefix("tool:") {
        return DRAW.iter().find(|t| t.name() == name).map(|t| Action::Tool(*t));
    }
    ALL.iter().find(|c| c.id == id).map(|c| (c.action)())
}

/// The Draw menu's tools as (name, key): "Arrow (A)" → ("Arrow", "A").
#[cfg(not(target_os = "macos"))]
pub fn tool_name_and_key(t: Tool) -> (&'static str, &'static str) {
    let tip = t.tooltip();
    match tip.strip_suffix(')').and_then(|s| s.rsplit_once(" (")) {
        Some((name, key)) => (name, key),
        None => (tip, ""),
    }
}

/// The rows of the Keyboard Shortcuts window: (keys, what they do) for the menus, the picture and
/// the system-wide hotkeys set in Preferences.
pub struct ShortcutRows {
    pub menus: Vec<(String, &'static str)>,
    pub picture: Vec<(String, String)>,
    pub anywhere: Vec<(String, &'static str)>,
}

pub fn shortcut_rows(settings: &Settings) -> ShortcutRows {
    let menus = ALL.iter().filter(|c| !c.keys.is_empty()).map(|c| (c.keys_text(), c.label.trim_end_matches('…'))).collect();
    // The tool keys, four to a row: "A T R K" — "Arrow, Text, Rectangle, Backdrop".
    let tools: Vec<(char, &str)> = Tool::TOOLBAR.iter().filter_map(|t| Some((t.key()?.to_ascii_uppercase(), t.name()))).collect();
    let mut picture: Vec<(String, String)> =
        tools.chunks(4).map(|row| (row.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>().join(" "), row.iter().map(|(_, n)| *n).collect::<Vec<_>>().join(", "))).collect();
    picture.extend(EDITOR_KEYS.iter().map(|(k, v)| (crate::settings::keys(k).into_owned(), v.to_string())));
    let anywhere = ALL.iter().filter_map(|c| Some((crate::settings::pretty_hotkey(settings.hotkey(c.hotkey?)?), c.label))).collect();
    ShortcutRows { menus, picture, anywhere }
}

// The menus and the key lookup are Windows' and Linux's (macOS builds its menus from `menu_model`).
#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn wayland_greys_out_scrolling_capture_and_nothing_else() {
        for c in [&SCROLLING, &SCROLLING_UP] {
            assert!(!allowed_in(c.id, true), "{} can't run on Wayland", c.id);
            assert!(allowed_in(c.id, false), "{} runs on X11", c.id);
        }
        for c in ALL.iter().filter(|c| c.id != SCROLLING.id && c.id != SCROLLING_UP.id) {
            assert!(allowed_in(c.id, true), "{} isn't about scrolling", c.id);
        }
    }

    fn menu_commands() -> impl Iterator<Item = &'static Command> {
        let items = MENU_BAR.iter().flat_map(|(_, items)| items.iter()).chain(TRAY);
        items
            .flat_map(|i| match i {
                Item::Sub(_, sub) => sub.iter().collect::<Vec<_>>(),
                other => vec![other],
            })
            .filter_map(|i| match i {
                Item::Cmd(c) => Some(*c),
                _ => None,
            })
    }

    /// The editor's menu bar draws one level of items; submenus are the tray's.
    #[test]
    fn only_the_tray_has_submenus() {
        assert!(MENU_BAR.iter().all(|(_, items)| items.iter().all(|i| !matches!(i, Item::Sub(..)))));
        assert!(TRAY.iter().any(|i| matches!(i, Item::Sub("Current Image", _))));
    }

    #[test]
    fn ids_are_unique_and_every_menu_command_is_listed() {
        for (i, a) in ALL.iter().enumerate() {
            assert!(ALL[i + 1..].iter().all(|b| b.id != a.id), "two commands are called {}", a.id);
        }
        for c in menu_commands() {
            assert!(ALL.iter().any(|a| a.id == c.id), "{} is in a menu but not in ALL", c.id);
            assert!(action_for_id(c.id).is_some());
        }
    }

    #[test]
    fn no_two_commands_share_a_shortcut() {
        let all: Vec<(&str, &KeyboardShortcut)> = ALL.iter().flat_map(|c| c.keys.iter().map(move |k| (c.id, k))).collect();
        for (i, (a, ka)) in all.iter().enumerate() {
            for (b, kb) in &all[i + 1..] {
                assert!(ka != kb || a == b, "{a} and {b} both use {}", shortcut_text(ka));
            }
        }
    }

    #[test]
    fn each_shortcut_finds_its_own_command() {
        for c in ALL {
            for k in c.keys {
                assert_eq!(for_key(k.logical_key, k.modifiers).map(|f| f.id), Some(c.id), "{}", shortcut_text(k));
            }
        }
    }

    #[test]
    fn shift_shortcuts_win_over_plain_ones() {
        assert_eq!(for_key(Key::Z, CMD).map(|c| c.id), Some("undo"));
        assert_eq!(for_key(Key::Z, CMD_SHIFT).map(|c| c.id), Some("redo"));
        assert_eq!(for_key(Key::S, CMD_SHIFT).map(|c| c.id), Some("saveas"));
        assert_eq!(for_key(Key::S, Modifiers::NONE).map(|c| c.id), None);
    }

    #[test]
    fn every_hotkey_and_tool_has_a_command() {
        for h in HotkeyAction::ALL {
            assert!(for_hotkey(h).is_some(), "{h:?}");
        }
        for t in DRAW {
            assert!(matches!(action_for_id(&format!("tool:{}", t.name())), Some(Action::Tool(x)) if x == t));
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn copy_and_paste_shortcuts_survive_eguis_clipboard_events() {
        let mut i = InputState::default();
        i.modifiers = Modifiers::CTRL.plus(Modifiers::COMMAND);
        i.events = vec![Event::Copy];
        assert_eq!(pressed(&i).iter().map(|c| c.id).collect::<Vec<_>>(), ["copy"]);
        let ctrl_shift = Modifiers::CTRL.plus(Modifiers::COMMAND).plus(Modifiers::SHIFT);
        // Ctrl+Shift+V: the press never arrives (the clipboard held a picture), the release does.
        i.events = vec![Event::Key { key: Key::V, physical_key: None, pressed: false, repeat: false, modifiers: ctrl_shift }];
        assert_eq!(pressed(&i).iter().map(|c| c.id).collect::<Vec<_>>(), ["clipboard"]);
        // A plain V coming up does nothing.
        i.events = vec![Event::Key { key: Key::V, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }];
        assert!(pressed(&i).is_empty());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn shortcuts_read_the_windows_way() {
        assert_eq!(SAVE_AS.keys_text(), "Ctrl+Shift+S");
        assert_eq!(ZOOM_IN.keys_text(), "Ctrl++");
        assert_eq!(FULL_SCREEN.keys_text(), "F11");
        assert_eq!(PREFERENCES.keys_text(), "Ctrl+,");
        assert_eq!(SAVE.tooltip(), "Save Image (Ctrl+S)");
        assert_eq!(OPEN_FILE.tooltip(), "Open File (Ctrl+Shift+O)");
        assert_eq!(DELAYED.tooltip(), "Delayed Screenshot (3s)");
    }

    #[test]
    fn tools_split_into_name_and_key() {
        assert_eq!(tool_name_and_key(Tool::Arrow), ("Arrow", "A"));
        assert_eq!(tool_name_and_key(Tool::Ruler), ("Ruler", "1 or 2"));
        assert_eq!(tool_name_and_key(Tool::Blur), ("Blur/Erase", "B"));
        assert_eq!(tool_name_and_key(Tool::Crop), ("Crop", ""));
    }

    #[test]
    fn shortcuts_window_lists_menus_tools_and_hotkeys() {
        let rows = shortcut_rows(&Settings::default());
        assert!(rows.menus.iter().any(|(_, v)| *v == "Copy Image"));
        let tool_rows: Vec<&str> = rows.picture.iter().take(3).map(|(k, _)| k.as_str()).collect();
        assert_eq!(tool_rows, ["A T R K", "D M B H", "S C O L"]);
        assert_eq!(rows.picture[0].1, "Arrow, Text, Rectangle, Backdrop");
        assert_eq!(rows.anywhere.len(), 4, "the default hotkeys: fullscreen, area, active window and text recognition");
    }
}
