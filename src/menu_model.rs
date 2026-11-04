//! The content of the menu bar and of the status item menu, as plain data.
//!
//! `shell.rs` turns these trees into native menus. Keeping them here, free of any platform type,
//! lets the tests check titles, order, separators and key equivalents on every platform.
use crate::model::Tool;
use crate::settings::HotkeyAction;

pub const CMD: u8 = 1;
pub const SHIFT: u8 = 2;
pub const ALT: u8 = 4;

/// A key equivalent: modifier bits and a W3C key code name ("KeyA", "Comma", "Digit1").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub mods: u8,
    pub code: &'static str,
}

const fn key(mods: u8, code: &'static str) -> Option<Key> {
    Some(Key { mods, code })
}

/// Items AppKit provides itself: they act on the first responder or on the app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Native {
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Cut,
    Copy,
    Paste,
    PasteAndMatchStyle,
    Delete,
    SelectAll,
    FullScreen,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Entry {
    Item { id: String, title: &'static str, key: Option<Key>, enabled: bool },
    /// A check mark item; whether it is checked is set when the menu is built or synced.
    Check { id: String, title: &'static str, enabled: bool },
    Sep,
    Sub { title: &'static str, items: Vec<Entry> },
    Native { kind: Native, title: Option<&'static str> },
}

fn item(id: &str, title: &'static str, key: Option<Key>) -> Entry {
    Entry::Item { id: id.to_string(), title, key, enabled: true }
}

fn native(kind: Native) -> Entry {
    Entry::Native { kind, title: None }
}

/// The Draw menu: title, tool and the key that picks it. These are the editor's own tool keys,
/// without a modifier. Ruler and Backdrop are not listed: they have their own keys and buttons.
pub const DRAW: [(&str, Tool, &str); 11] = [
    ("Arrow", Tool::Arrow, "KeyA"),
    ("Text", Tool::Text, "KeyT"),
    ("Rectangle", Tool::Rectangle, "KeyR"),
    ("Freehand Drawing", Tool::Freehand, "KeyD"),
    ("Magnifier", Tool::Magnifier, "KeyM"),
    ("Highlighter", Tool::Highlighter, "KeyH"),
    ("Blur", Tool::Blur, "KeyB"),
    ("Spotlight", Tool::Spotlight, "KeyS"),
    ("Counter", Tool::Counter, "KeyC"),
    ("Oval", Tool::Oval, "KeyO"),
    ("Line", Tool::Line, "KeyL"),
];

/// Id prefix of the Draw menu items; the rest is the tool's name.
pub const TOOL_PREFIX: &str = "tool:";

pub fn is_draw_item(id: &str) -> bool {
    id.starts_with(TOOL_PREFIX)
}

/// The selection corner items carry plain letters too (Q and W) and follow the same rule as the Draw items.
fn is_plain_letter_item(id: &str) -> bool {
    is_draw_item(id) || matches!(id, "seltl" | "selbr")
}

fn app_menu() -> Entry {
    Entry::Sub {
        title: "ShotCraft",
        items: vec![
            item("about", "About ShotCraft", None),
            Entry::Sep,
            item("prefs", "Preferences…", key(CMD, "Comma")),
            Entry::Sep,
            native(Native::Services),
            Entry::Sep,
            Entry::Native { kind: Native::Hide, title: Some("Hide ShotCraft") },
            native(Native::HideOthers),
            native(Native::ShowAll),
            Entry::Sep,
            item("quit", "Quit ShotCraft", key(CMD, "KeyQ")),
        ],
    }
}

fn file_menu() -> Entry {
    Entry::Sub {
        title: "File",
        items: vec![
            item("copy", "Copy Image", key(CMD | SHIFT, "KeyC")),
            item("save", "Save Image", key(CMD, "KeyS")),
            item("saveas", "Save Image As", key(CMD | SHIFT, "KeyS")),
            item("pin", "Pin to Screen", key(CMD, "KeyP")),
            item("print", "Print…", key(CMD | SHIFT, "KeyP")),
            Entry::Sep,
            item("open", "Open File", key(CMD | SHIFT, "KeyO")),
            item("clipboard", "Open From Clipboard", key(CMD | SHIFT, "KeyV")),
            Entry::Sep,
            item("close", "Close", key(CMD, "KeyW")),
            item("closeall", "Close All", key(CMD | ALT, "KeyW")),
        ],
    }
}

fn edit_menu() -> Entry {
    Entry::Sub {
        title: "Edit",
        items: vec![
            item("undo", "Undo", key(CMD, "KeyZ")),
            item("redo", "Redo", key(CMD | SHIFT, "KeyZ")),
            item("resetcrop", "Reset Crop", None),
            item("rasterize", "Rasterize (Merge) Image", None),
            Entry::Sep,
            native(Native::Cut),
            native(Native::Copy),
            native(Native::Paste),
            native(Native::PasteAndMatchStyle),
            native(Native::Delete),
            native(Native::SelectAll),
        ],
    }
}

fn draw_menu() -> Entry {
    let mut items: Vec<Entry> = DRAW.iter().map(|(title, tool, code)| item(&format!("{TOOL_PREFIX}{}", tool.name()), title, key(0, code))).collect();
    // Add Capture is listed as in the original; it stays off until the feature behind it exists.
    items.push(Entry::Item { id: "addcapture".into(), title: "Add Capture", key: None, enabled: false });
    items.push(Entry::Sep);
    items.push(Entry::Check { id: "snap".into(), title: "Snap to Objects", enabled: true });
    items.push(Entry::Check { id: "snapsimilar".into(), title: "Snap to Similar Objects", enabled: true });
    Entry::Sub { title: "Draw", items }
}

fn zoom_menu() -> Entry {
    Entry::Sub {
        title: "Zoom",
        items: vec![
            item("zoomin", "Zoom In", key(CMD, "Equal")),
            item("zoomout", "Zoom Out", key(CMD, "Minus")),
            Entry::Sep,
            item("zoomfit", "Zoom To Fit", key(CMD, "Digit1")),
            item("zoom100", "Zoom To 100%", key(CMD, "Digit0")),
            Entry::Sep,
            item("zoomsel", "Zoom To Selection", key(CMD, "Digit2")),
            item("seltl", "Selection Top Left", key(0, "KeyQ")),
            item("selbr", "Selection Bottom Right", key(0, "KeyW")),
            Entry::Sep,
            Entry::Native { kind: Native::FullScreen, title: Some("Enter Full Screen") },
        ],
    }
}

fn help_menu() -> Entry {
    Entry::Sub {
        title: "Help",
        items: vec![item("shortcuts", "Keyboard Shortcuts", None)],
    }
}

/// The main menu. A menu-bar-only app never shows it, but its key equivalents work while the
/// editor has the keyboard.
pub fn menu_bar() -> Vec<Entry> {
    vec![app_menu(), file_menu(), edit_menu(), draw_menu(), zoom_menu(), help_menu()]
}

/// The menu of the status item: the same grouping as the original, with the entries this app adds
/// (every capture mode, updates, About, Keyboard Shortcuts, commands for the current image that the
/// hidden main menu would otherwise keep out of reach).
pub fn status_menu() -> Vec<Entry> {
    vec![
        item("show", "Show ShotCraft", None),
        Entry::Sep,
        item("fullscreen", "Capture Screen", None),
        item("area", "Capture Area", None),
        item("scrolling", "Scrolling Capture", None),
        item("ocr", "Recognize Text/QR", None),
        Entry::Sub {
            title: "More",
            items: vec![
                item("repeat", "Repeat Area Capture", None),
                item("activewindow", "Capture Active Window", None),
                item("window", "Capture Any Window", None),
                item("alldisplays", "All Displays Screenshot", None),
                item("delayed", "Delayed Screenshot (3s)", None),
                item("scrollingup", "Scrolling Capture (Up)", None),
                Entry::Sep,
                item("open", "Open File…", None),
                item("clipboard", "Load From Clipboard", None),
                Entry::Sep,
                item("closeall", "Close All", None),
            ],
        },
        Entry::Sep,
        Entry::Sub {
            title: "Current Image",
            items: vec![
                item("undo", "Undo", None),
                item("redo", "Redo", None),
                item("resetcrop", "Reset Crop", None),
                item("rasterize", "Rasterize (Merge) Image", None),
                item("saveas", "Save Image As", None),
                item("print", "Print…", None),
            ],
        },
        Entry::Sep,
        Entry::Check { id: "autostart".into(), title: "Launch at Startup", enabled: true },
        item("prefs", "Preferences…", key(CMD, "Comma")),
        Entry::Sep,
        item("updates", "Check for Updates…", None),
        Entry::Check { id: "autoupdates".into(), title: "Check for Updates Automatically", enabled: true },
        item("about", "About ShotCraft", None),
        item("shortcuts", "Keyboard Shortcuts", None),
        Entry::Sep,
        item("quit", "Quit ShotCraft", key(CMD, "KeyQ")),
    ]
}

/// The hotkey shown next to a status menu item, if it has one.
pub fn hotkey_of_item(id: &str) -> Option<HotkeyAction> {
    Some(match id {
        "fullscreen" => HotkeyAction::Fullscreen,
        "area" => HotkeyAction::Area,
        "scrolling" => HotkeyAction::Scrolling,
        "ocr" => HotkeyAction::Ocr,
        "repeat" => HotkeyAction::RepeatArea,
        "activewindow" => HotkeyAction::ActiveWindow,
        "window" => HotkeyAction::Window,
        "alldisplays" => HotkeyAction::AllDisplays,
        "show" => HotkeyAction::Show,
        _ => return None,
    })
}

/// Whether the state of the editor decides if an item is enabled (see `item_enabled`).
pub fn is_stateful(id: &str) -> bool {
    is_plain_letter_item(id) || matches!(id, "zoomsel" | "copy" | "save" | "saveas" | "pin" | "print" | "undo" | "redo" | "resetcrop" | "rasterize" | "zoomin" | "zoomout" | "zoomfit" | "zoom100" | "close")
}

/// What decides which menu items can act.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MenuState {
    pub has_image: bool,
    /// The editor window is on screen (not hidden to the menu bar).
    pub editor_visible: bool,
    /// The editor window has the keyboard.
    pub editor_focused: bool,
    /// Something takes typed text: the text tool, or a field in the editor.
    pub typing: bool,
    /// With an area selected, plain C copies its average colour instead of picking the counter.
    pub c_copies_colour: bool,
    /// With an area selected and nothing else, plain A (smart select) and B (blur the area) act on
    /// the area instead of picking a tool.
    pub area_keys: bool,
    /// An area is selected: Q and W go to its corners.
    pub has_area: bool,
}

/// Whether an item is enabled. Commands on the current image need one; the Close command needs the
/// editor on screen. The Draw items carry plain letters and AppKit runs a key equivalent before the
/// window sees the key, so they are switched off unless the editor has the keyboard and nothing is
/// taking text, and the letters reach whoever is typing. A disabled item lets the key through.
pub fn item_enabled(id: &str, st: &MenuState) -> bool {
    match id {
        "copy" | "save" | "saveas" | "pin" | "print" | "undo" | "redo" | "resetcrop" | "rasterize" | "zoomin" | "zoomout" | "zoomfit" | "zoom100" | "zoomsel" => st.has_image,
        "close" => st.has_image && st.editor_visible,
        _ if is_plain_letter_item(id) => {
            let tool = |t: Tool| id == format!("{TOOL_PREFIX}{}", t.name());
            let taken = (st.c_copies_colour && tool(Tool::Counter)) || (st.area_keys && (tool(Tool::Arrow) || tool(Tool::Blur))) || (!st.has_area && matches!(id, "seltl" | "selbr"));
            st.has_image && st.editor_visible && st.editor_focused && !st.typing && !taken
        }
        _ => true,
    }
}

/// Lays an image out on a sheet of paper, in points.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PrintLayout {
    pub landscape: bool,
    pub width: f64,
    pub height: f64,
}

/// Wider than tall prints in landscape (the default orientation is automatic, as in the original).
pub fn print_landscape(px: (u32, u32)) -> bool {
    px.0 > px.1
}

/// The picture at its on-screen size (`px / scale` points), shrunk to fit the printable area of the
/// page and never enlarged. `page` is the paper in portrait, `margin` is on every side.
pub fn print_layout(px: (u32, u32), scale: f32, page: (f64, f64), margin: f64) -> PrintLayout {
    let landscape = print_landscape(px);
    let (short, long) = (page.0.min(page.1), page.0.max(page.1));
    let (pw, ph) = if landscape { (long, short) } else { (short, long) };
    let (avail_w, avail_h) = ((pw - 2.0 * margin).max(1.0), (ph - 2.0 * margin).max(1.0));
    let s = f64::from(scale.max(1.0));
    let (w, h) = (f64::from(px.0.max(1)) / s, f64::from(px.1.max(1)) / s);
    let fit = (avail_w / w).min(avail_h / h).min(1.0);
    PrintLayout { landscape, width: w * fit, height: h * fit }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(items: &[Entry]) -> Vec<String> {
        items
            .iter()
            .map(|e| match e {
                Entry::Item { title, .. } | Entry::Check { title, .. } => title.to_string(),
                Entry::Sep => "-".into(),
                Entry::Sub { title, items } => format!("{title} [{}]", titles(items).join(" | ")),
                Entry::Native { kind, title } => title.map(str::to_string).unwrap_or_else(|| format!("{kind:?}")),
            })
            .collect()
    }

    fn sub<'a>(menu: &'a [Entry], name: &str) -> &'a [Entry] {
        for e in menu {
            if let Entry::Sub { title, items } = e {
                if *title == name {
                    return items;
                }
            }
        }
        panic!("no menu {name}");
    }

    fn find<'a>(items: &'a [Entry], id: &str) -> &'a Entry {
        items.iter().find(|e| matches!(e, Entry::Item { id: i, .. } | Entry::Check { id: i, .. } if i == id)).unwrap_or_else(|| panic!("no item {id}"))
    }

    #[test]
    fn menu_bar_titles_in_order() {
        let bar = menu_bar();
        let names: Vec<_> = bar.iter().map(|e| titles(std::slice::from_ref(e)).remove(0)).collect();
        let expected = [
            "ShotCraft [About ShotCraft | - | Preferences… | - | Services | - | Hide ShotCraft | HideOthers | ShowAll | - | Quit ShotCraft]",
            "File [Copy Image | Save Image | Save Image As | Pin to Screen | Print… | - | Open File | Open From Clipboard | - | Close | Close All]",
            "Edit [Undo | Redo | Reset Crop | Rasterize (Merge) Image | - | Cut | Copy | Paste | PasteAndMatchStyle | Delete | SelectAll]",
            "Draw [Arrow | Text | Rectangle | Freehand Drawing | Magnifier | Highlighter | Blur | Spotlight | Counter | Oval | Line | Add Capture | - | Snap to Objects | Snap to Similar Objects]",
            "Zoom [Zoom In | Zoom Out | - | Zoom To Fit | Zoom To 100% | - | Zoom To Selection | Selection Top Left | Selection Bottom Right | - | Enter Full Screen]",
            "Help [Keyboard Shortcuts]",
        ];
        assert_eq!(names, expected);
    }

    #[test]
    fn status_menu_follows_the_original_grouping() {
        let expected = [
            "Show ShotCraft",
            "-",
            "Capture Screen",
            "Capture Area",
            "Scrolling Capture",
            "Recognize Text/QR",
            "More [Repeat Area Capture | Capture Active Window | Capture Any Window | All Displays Screenshot | Delayed Screenshot (3s) | Scrolling Capture (Up) | - | Open File… | Load From Clipboard | - | Close All]",
            "-",
            "Current Image [Undo | Redo | Reset Crop | Rasterize (Merge) Image | Save Image As | Print…]",
            "-",
            "Launch at Startup",
            "Preferences…",
            "-",
            "Check for Updates…",
            "Check for Updates Automatically",
            "About ShotCraft",
            "Keyboard Shortcuts",
            "-",
            "Quit ShotCraft",
        ];
        assert_eq!(titles(&status_menu()), expected);
    }

    #[test]
    fn file_menu_key_equivalents() {
        let bar = menu_bar();
        let file = sub(&bar, "File");
        let key_of = |id: &str| match find(file, id) {
            Entry::Item { key, .. } => *key,
            _ => None,
        };
        assert_eq!(key_of("print"), Some(Key { mods: CMD | SHIFT, code: "KeyP" }));
        assert_eq!(key_of("closeall"), Some(Key { mods: CMD | ALT, code: "KeyW" }));
        assert_eq!(key_of("close"), Some(Key { mods: CMD, code: "KeyW" }));
        assert_eq!(key_of("pin"), Some(Key { mods: CMD, code: "KeyP" }));
    }

    #[test]
    fn draw_items_use_the_tool_keys_without_modifier() {
        let bar = menu_bar();
        let draw = sub(&bar, "Draw");
        let mut seen = 0;
        for e in draw {
            let Entry::Item { id, key: Some(k), .. } = e else { continue };
            assert!(is_draw_item(id));
            assert_eq!(k.mods, 0, "{id}");
            let letter = k.code.strip_prefix("Key").unwrap().to_lowercase().chars().next().unwrap();
            let tool = Tool::from_key(letter).unwrap_or_else(|| panic!("{letter} is no tool key"));
            assert_eq!(format!("{TOOL_PREFIX}{}", tool.name()), *id);
            seen += 1;
        }
        assert_eq!(seen, DRAW.len());
    }

    #[test]
    fn only_listed_items_are_disabled_placeholders() {
        let bar = menu_bar();
        let mut disabled = vec![];
        fn walk(items: &[Entry], out: &mut Vec<String>) {
            for e in items {
                match e {
                    Entry::Item { title, enabled: false, .. } | Entry::Check { title, enabled: false, .. } => out.push(title.to_string()),
                    Entry::Sub { items, .. } => walk(items, out),
                    _ => {}
                }
            }
        }
        walk(&bar, &mut disabled);
        assert_eq!(disabled, ["Add Capture"]);
    }

    #[test]
    fn ids_are_unique_within_a_menu() {
        for menu in [menu_bar(), status_menu()] {
            fn collect(items: &[Entry], ids: &mut Vec<String>) {
                for e in items {
                    match e {
                        Entry::Item { id, .. } | Entry::Check { id, .. } => ids.push(id.clone()),
                        Entry::Sub { items, .. } => collect(items, ids),
                        _ => {}
                    }
                }
            }
            let mut ids = vec![];
            collect(&menu, &mut ids);
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), ids.len(), "duplicate id in {ids:?}");
        }
    }

    #[test]
    fn status_menu_items_with_a_hotkey() {
        let mut found = vec![];
        fn walk(items: &[Entry], out: &mut Vec<HotkeyAction>) {
            for e in items {
                match e {
                    Entry::Item { id, .. } => out.extend(hotkey_of_item(id)),
                    Entry::Sub { items, .. } => walk(items, out),
                    _ => {}
                }
            }
        }
        walk(&status_menu(), &mut found);
        // Every hotkey the app has appears once in the status menu.
        for a in HotkeyAction::ALL {
            assert_eq!(found.iter().filter(|f| **f == a).count(), 1, "{a:?}");
        }
    }

    #[test]
    fn current_image_commands_need_an_image() {
        let none = MenuState::default();
        let img = MenuState { has_image: true, ..Default::default() };
        for id in ["copy", "save", "saveas", "pin", "print", "undo", "redo", "resetcrop", "rasterize", "zoomin", "zoomfit"] {
            assert!(!item_enabled(id, &none), "{id}");
            assert!(item_enabled(id, &img), "{id}");
        }
        for id in ["quit", "open", "clipboard", "prefs", "closeall", "area", "shortcuts"] {
            assert!(item_enabled(id, &none), "{id}");
        }
    }

    #[test]
    fn close_needs_the_editor_on_screen() {
        let hidden = MenuState { has_image: true, ..Default::default() };
        assert!(!item_enabled("close", &hidden));
        assert!(item_enabled("close", &MenuState { editor_visible: true, ..hidden }));
        assert!(!item_enabled("close", &MenuState { editor_visible: true, has_image: false, ..hidden }));
    }

    #[test]
    fn draw_keys_need_a_visible_focused_editor_with_an_image_and_no_typing() {
        let ok = MenuState { has_image: true, editor_visible: true, editor_focused: true, typing: false, ..Default::default() };
        let id = "tool:Arrow";
        assert!(item_enabled(id, &ok));
        assert!(!item_enabled(id, &MenuState { has_image: false, ..ok }));
        assert!(!item_enabled(id, &MenuState { editor_visible: false, ..ok }));
        assert!(!item_enabled(id, &MenuState { editor_focused: false, ..ok }));
        assert!(!item_enabled(id, &MenuState { typing: true, ..ok }));
    }

    #[test]
    fn with_an_area_selected_c_keeps_copying_the_colour() {
        let ok = MenuState { has_image: true, editor_visible: true, editor_focused: true, typing: false, c_copies_colour: true, ..Default::default() };
        assert!(!item_enabled("tool:Counter", &ok));
        assert!(item_enabled("tool:Oval", &ok));
    }

    #[test]
    fn selection_corner_letters_need_an_area_and_follow_the_draw_rule() {
        let ok = MenuState { has_image: true, editor_visible: true, editor_focused: true, has_area: true, ..Default::default() };
        for id in ["seltl", "selbr"] {
            assert!(item_enabled(id, &ok));
            assert!(!item_enabled(id, &MenuState { typing: true, ..ok }));
            assert!(!item_enabled(id, &MenuState { has_area: false, ..ok }));
        }
    }

    #[test]
    fn with_only_an_area_selected_a_and_b_act_on_it() {
        let ok = MenuState { has_image: true, editor_visible: true, editor_focused: true, area_keys: true, has_area: true, ..Default::default() };
        assert!(!item_enabled("tool:Arrow", &ok));
        assert!(!item_enabled("tool:Blur/Erase", &ok));
        assert!(item_enabled("tool:Rectangle", &ok));
        assert!(item_enabled("tool:Counter", &ok));
        assert!(item_enabled("tool:Arrow", &MenuState { area_keys: false, ..ok }));
    }

    #[test]
    fn landscape_images_print_in_landscape() {
        assert!(print_landscape((1600, 900)));
        assert!(!print_landscape((900, 1600)));
        assert!(!print_landscape((500, 500)));
    }

    const A4: (f64, f64) = (595.0, 842.0);

    #[test]
    fn a_big_picture_is_scaled_down_to_the_printable_area() {
        // 2400x1200 px at 2x = 1200x600 pt, on A4 landscape (842x595) with 36 pt margins: 770 wide.
        let l = print_layout((2400, 1200), 2.0, A4, 36.0);
        assert!(l.landscape);
        assert!((l.width - 770.0).abs() < 1e-9, "{l:?}");
        assert!((l.height - 385.0).abs() < 1e-9, "{l:?}");
    }

    #[test]
    fn a_tall_picture_is_limited_by_the_page_height() {
        let l = print_layout((1000, 4000), 1.0, A4, 36.0);
        assert!(!l.landscape);
        assert!((l.height - 770.0).abs() < 1e-9, "{l:?}");
        assert!((l.width - 192.5).abs() < 1e-9, "{l:?}");
    }

    #[test]
    fn a_small_picture_keeps_its_size() {
        let l = print_layout((300, 200), 2.0, A4, 36.0);
        assert_eq!((l.width, l.height), (150.0, 100.0));
    }

    #[test]
    fn page_orientation_in_the_paper_size_does_not_matter() {
        assert_eq!(print_layout((2400, 1200), 2.0, (842.0, 595.0), 36.0), print_layout((2400, 1200), 2.0, A4, 36.0));
    }
}
