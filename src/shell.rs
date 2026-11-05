//! Menu bar icon, the app's menus and the global hotkeys.
//!
//! The app has no Dock icon, so macOS never shows its main menu: the status item menu is the one
//! users see and has to carry every command that is not on screen in the editor. The main menu is
//! still installed because its key equivalents keep working while the editor has focus.
use crate::menu_model::{self, Entry, MenuState, Native};
use crate::model::Tool;
use crate::settings::{HotkeyAction, Settings};
use crate::{Action, Queue};
use crate::system::CaptureMode;
use crate::updater::Updater;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, ContextMenu, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIcon, TrayIconBuilder};

pub struct Shell {
    tray: Option<TrayIcon>,
    menubar: Menu,
    autostart_item: CheckMenuItem,
    /// "Delayed Screenshot (Ns)": the wait is a setting.
    delayed_item: Option<MenuItem>,
    auto_updates_item: CheckMenuItem,
    /// Draw ▸ Snap to Objects / Snap to Similar Objects.
    snap_items: Vec<(String, CheckMenuItem)>,
    /// Menu bar items whose enabled state follows the editor (see `menu_model::item_enabled`), by id.
    stateful: Vec<(String, MenuItem)>,
    /// The same for the status menu (see `menu_model::tray_item_enabled`).
    tray_stateful: Vec<(String, MenuItem)>,
    /// Status menu items that show a global hotkey, kept to update when it changes.
    hotkey_items: Vec<(HotkeyAction, MenuItem)>,
    state: Option<MenuState>,
    updater: Option<Updater>,
    hotkeys: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    show_hotkey_works: bool,
    hotkey_map: Arc<Mutex<HashMap<u32, HotkeyAction>>>,
    /// Esc, registered only while a scrolling capture runs so that it can be cancelled from any app.
    escape: Option<HotKey>,
    escape_id: Arc<AtomicU32>,
    enter: Option<HotKey>,
    enter_id: Arc<AtomicU32>,
    /// A shortcut is being recorded: the menu bar and the global hotkeys are off, so every key
    /// combination reaches the window.
    keys_suspended: bool,
}

fn action_for(id: &str) -> Option<Action> {
    Some(match id {
        "fullscreen" => Action::Capture(CaptureMode::Fullscreen),
        "area" => Action::Capture(CaptureMode::Area),
        "repeat" => Action::Capture(CaptureMode::RepeatArea),
        "window" => Action::Capture(CaptureMode::Window),
        "scrolling" => Action::ScrollingCapture,
        "scrollingup" => Action::ScrollingCaptureUp,
        "activewindow" => Action::Capture(CaptureMode::ActiveWindow),
        "alldisplays" => Action::Capture(CaptureMode::AllDisplays),
        "delayed" => Action::Capture(CaptureMode::Delayed),
        "ocr" => Action::InstantOcr,
        "clipboard" => Action::OpenClipboard,
        "open" => Action::OpenFile,
        "show" => Action::Show,
        "prefs" => Action::Preferences,
        "autostart" => Action::ToggleAutostart,
        "updates" => Action::CheckForUpdates,
        "autoupdates" => Action::ToggleAutoUpdates,
        "snap" => Action::ToggleSnap(false),
        "snapsimilar" => Action::ToggleSnap(true),
        "quit" => Action::Quit,
        "copy" | "editcopy" => Action::Copy,
        "save" => Action::Save,
        "saveas" => Action::SaveAs,
        "pin" => Action::Pin,
        "close" => Action::Close,
        "closeall" => Action::CloseAll,
        "print" => Action::Print,
        "undo" => Action::Undo,
        "redo" => Action::Redo,
        "resetcrop" => Action::ResetCrop,
        "rasterize" => Action::Rasterize,
        "zoomin" => Action::ZoomIn,
        "zoomout" => Action::ZoomOut,
        "zoomfit" => Action::ZoomFit,
        "zoom100" => Action::Zoom100,
        "zoomsel" => Action::ZoomSelection,
        "seltl" => Action::SelectionTopLeft,
        "selbr" => Action::SelectionBottomRight,
        "about" => Action::About,
        "shortcuts" => Action::Shortcuts,
        t if t.starts_with("tool:") => {
            let name = &t[menu_model::TOOL_PREFIX.len()..];
            Action::Tool(*Tool::TOOLBAR.iter().find(|x| x.name() == name)?)
        }
        _ => return None,
    })
}

fn key_equivalent(k: menu_model::Key) -> Option<Accelerator> {
    let mut m = Modifiers::empty();
    for (bit, flag) in [(menu_model::CMD, Modifiers::META), (menu_model::SHIFT, Modifiers::SHIFT), (menu_model::ALT, Modifiers::ALT)] {
        if k.mods & bit != 0 {
            m |= flag;
        }
    }
    Some(Accelerator::new(m, k.code.parse::<Code>().ok()?))
}

/// What building the native menus from `menu_model` leaves behind: the items that are changed later.
#[derive(Default)]
struct Parts {
    updates_available: bool,
    autostart: bool,
    snap: (bool, bool),
    snap_items: Vec<(String, CheckMenuItem)>,
    stateful: Vec<(String, MenuItem)>,
    hotkey_items: Vec<(HotkeyAction, MenuItem)>,
    delayed_item: Option<MenuItem>,
    autostart_item: Option<CheckMenuItem>,
    auto_updates_item: Option<CheckMenuItem>,
    help: Option<Submenu>,
}

impl Parts {
    fn build(&mut self, e: &Entry) -> Box<dyn IsMenuItem> {
        match e {
            Entry::Sep => Box::new(PredefinedMenuItem::separator()),
            Entry::Item { id, title, key, enabled } => {
                let on = *enabled && (id != "updates" || self.updates_available);
                let item = MenuItem::with_id(id.as_str(), *title, on, key.and_then(key_equivalent));
                if *enabled && menu_model::is_stateful(id) {
                    self.stateful.push((id.clone(), item.clone()));
                }
                if id == "delayed" {
                    self.delayed_item = Some(item.clone());
                }
                if let Some(a) = menu_model::hotkey_of_item(id) {
                    self.hotkey_items.push((a, item.clone()));
                }
                Box::new(item)
            }
            Entry::Check { id, title, enabled } => {
                let checked = (id == "autostart" && self.autostart) || (id == "snap" && self.snap.0) || (id == "snapsimilar" && self.snap.1);
                let item = CheckMenuItem::with_id(id.as_str(), *title, *enabled && (id != "autoupdates" || self.updates_available), checked, None);
                match id.as_str() {
                    "autostart" => self.autostart_item = Some(item.clone()),
                    "autoupdates" => self.auto_updates_item = Some(item.clone()),
                    "snap" | "snapsimilar" => self.snap_items.push((id.clone(), item.clone())),
                    _ => {}
                }
                Box::new(item)
            }
            Entry::Sub { title, items } => {
                let children: Vec<Box<dyn IsMenuItem>> = items.iter().map(|c| self.build(c)).collect();
                let sub = Submenu::with_items(*title, true, &children.iter().map(|b| &**b).collect::<Vec<_>>()).expect("menu");
                if *title == "Help" {
                    self.help = Some(sub.clone());
                }
                Box::new(sub)
            }
            Entry::Native { kind, title } => Box::new(match kind {
                Native::Services => PredefinedMenuItem::services(*title),
                Native::Hide => PredefinedMenuItem::hide(*title),
                Native::HideOthers => PredefinedMenuItem::hide_others(*title),
                Native::ShowAll => PredefinedMenuItem::show_all(*title),
                Native::FullScreen => PredefinedMenuItem::fullscreen(*title),
            }),
        }
    }
}

/// SF Symbol shown before a menu item, as in Apple's own menus. Matched on the start of the title.
const MENU_SYMBOLS: &[(&str, &str)] = &[
    ("Preferences", "gearshape"),
    ("About", "info.circle"),
    ("Check for Updates…", "arrow.triangle.2.circlepath"),
    ("Quit", "rectangle.portrait.and.arrow.right"),
    ("Show ShotCraft", "eye"),
    ("Show All", "eye"),
    ("Hide ShotCraft", "eye.slash"),
    ("Hide Others", "eye.slash"),
    ("Services", "gearshape.2"),
    ("Launch at Startup", "power"),
    ("Area Capture", "rectangle.dashed"),
    ("Capture Area", "rectangle.dashed"),
    ("Capture Screen", "display"),
    ("More", "ellipsis.circle"),
    ("Print", "printer"),
    ("Delete", "trash"),
    ("Fullscreen Screenshot", "display"),
    ("All Displays Screenshot", "display.2"),
    ("Repeat Area Capture", "arrow.clockwise"),
    ("Capture Any Window", "macwindow"),
    ("Capture Active Window", "macwindow.on.rectangle"),
    ("Scrolling Capture (Up)", "arrow.up.square"),
    ("Scrolling Capture", "arrow.up.and.down.square"),
    ("Delayed Screenshot", "timer"),
    ("Recognize Text", "text.viewfinder"),
    ("Load From Clipboard", "doc.on.clipboard"),
    ("Open From Clipboard", "doc.on.clipboard"),
    ("Open File", "folder"),
    ("Copy", "doc.on.doc"),
    ("Save Image", "square.and.arrow.down"),
    ("Pin to Screen", "pin"),
    ("Close", "xmark"),
    ("Undo", "arrow.uturn.backward"),
    ("Redo", "arrow.uturn.forward"),
    ("Reset Crop", "crop"),
    ("Current Image", "photo"),
    ("Cut", "scissors"),
    ("Paste", "doc.on.clipboard"),
    ("Zoom In", "plus.magnifyingglass"),
    ("Zoom Out", "minus.magnifyingglass"),
    ("Zoom To", "magnifyingglass"),
    ("Enter Full Screen", "arrow.up.left.and.arrow.down.right"),
    ("Keyboard Shortcuts", "keyboard"),
    ("Arrow", "arrow.up.right"),
    ("Text", "textformat"),
    ("Rectangle", "rectangle"),
    ("Freehand", "scribble"),
    ("Magnifier", "magnifyingglass"),
    ("Highlighter", "highlighter"),
    ("Counter", "1.circle"),
    ("Oval", "oval"),
    ("Line", "line.diagonal"),
    ("Ruler", "ruler"),
    ("Blur", "square.grid.3x3"),
    ("Spotlight", "flashlight.on.fill"),
    ("Backdrop", "photo"),
    ("Rasterize", "square.stack.3d.down.right"),
    ("Select All", "selection.pin.in.out"),
];

/// The symbol for a menu title. Titles may continue after the listed text ("Zoom To 100%"), so the
/// entries are prefixes; "Check for Updates…" ends in its ellipsis to leave out "Check for Updates Automatically".
fn menu_symbol(title: &str) -> Option<&'static str> {
    MENU_SYMBOLS.iter().find(|(prefix, _)| title.starts_with(prefix)).map(|(_, name)| *name)
}

fn add_menu_symbols(menu: &dyn ContextMenu) {
    use objc2_app_kit::{NSImage, NSMenu};
    use objc2_foundation::NSString;
    fn walk(menu: &NSMenu) {
        for item in menu.itemArray().iter() {
            let title = item.title().to_string();
            if let Some(name) = menu_symbol(&title) {
                item.setImage(NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None).as_deref());
            }
            if let Some(sub) = item.submenu() {
                walk(&sub);
            }
        }
    }
    let ptr = menu.ns_menu() as *const NSMenu;
    if !ptr.is_null() {
        walk(unsafe { &*ptr });
    }
}

/// The menu bar glyph: black on transparent, 36 px for an 18 pt status item. It is marked as a
/// template image where the tray icon is built, so macOS tints it for light and dark menu bars.
fn tray_icon_image() -> tray_icon::Icon {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/menubar.png")).expect("menu bar icon");
    tray_icon::Icon::from_rgba(icon.rgba, icon.width, icon.height).expect("menu bar icon")
}

impl Shell {
    pub fn start(queue: Queue, ctx: eframe::egui::Context, settings: &Settings) -> Shell {
        let updater = Updater::start();
        let mut parts = Parts { updates_available: updater.is_some(), autostart: settings.autostart, snap: (settings.snap_objects, settings.snap_similar), ..Parts::default() };
        let bar_items: Vec<Box<dyn IsMenuItem>> = menu_model::menu_bar().iter().map(|e| parts.build(e)).collect();
        let menubar = Menu::with_items(&bar_items.iter().map(|b| &**b).collect::<Vec<_>>()).expect("menu");
        add_menu_symbols(&menubar);
        menubar.init_for_nsapp();
        if let Some(help) = &parts.help {
            help.set_as_help_menu_for_nsapp();
        }
        let stateful = std::mem::take(&mut parts.stateful);
        let tray_items: Vec<Box<dyn IsMenuItem>> = menu_model::status_menu().iter().map(|e| parts.build(e)).collect();
        let tray_menu = Menu::with_items(&tray_items.iter().map(|b| &**b).collect::<Vec<_>>()).expect("menu");
        let tray_stateful = std::mem::take(&mut parts.stateful);
        let delayed_item = parts.delayed_item.take();
        let snap_items = std::mem::take(&mut parts.snap_items);
        let (autostart_item, auto_updates_item, hotkey_items) = (parts.autostart_item.expect("autostart item"), parts.auto_updates_item.expect("updates item"), parts.hotkey_items);
        if let Some(updater) = &updater {
            auto_updates_item.set_checked(updater.checks_automatically());
        }
        add_menu_symbols(&tray_menu);
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_icon(tray_icon_image())
            .with_icon_as_template(true)
            .with_tooltip("ShotCraft")
            .build()
            .ok();
        if let Some(t) = &tray {
            let _ = t.set_visible(!settings.hide_menubar_icon);
        }

        let q = queue.clone();
        let c = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Some(a) = action_for(e.id.0.as_str()) {
                q.lock().unwrap().push(a);
                c.request_repaint();
            }
        }));

        let hotkey_map: Arc<Mutex<HashMap<u32, HotkeyAction>>> = Arc::default();
        let map = hotkey_map.clone();
        let escape_id: Arc<AtomicU32> = Arc::default();
        let esc = escape_id.clone();
        let enter_id: Arc<AtomicU32> = Arc::default();
        let ent = enter_id.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            if e.state != HotKeyState::Pressed {
                return;
            }
            if e.id == esc.load(Ordering::SeqCst) {
                queue.lock().unwrap().push(Action::ScrollingCancel);
                ctx.request_repaint();
                return;
            }
            if e.id == ent.load(Ordering::SeqCst) {
                // Return only ends a capture that runs; it never starts one.
                queue.lock().unwrap().push(Action::ScrollingFinish);
                ctx.request_repaint();
                return;
            }
            let Some(a) = map.lock().unwrap().get(&e.id).copied() else { return };
            let action = match a {
                HotkeyAction::Fullscreen => Action::Capture(CaptureMode::Fullscreen),
                HotkeyAction::Area => Action::Capture(CaptureMode::Area),
                HotkeyAction::Window => Action::Capture(CaptureMode::Window),
                HotkeyAction::ActiveWindow => Action::Capture(CaptureMode::ActiveWindow),
                HotkeyAction::AllDisplays => Action::Capture(CaptureMode::AllDisplays),
                HotkeyAction::RepeatArea => Action::Capture(CaptureMode::RepeatArea),
                HotkeyAction::Ocr => Action::InstantOcr,
                HotkeyAction::Show => Action::Show,
                HotkeyAction::Scrolling => Action::ScrollingCapture,
            };
            queue.lock().unwrap().push(action);
            ctx.request_repaint();
        }));

        let mut shell = Shell { tray, menubar, autostart_item, auto_updates_item, snap_items, delayed_item, stateful, tray_stateful, hotkey_items, state: None, updater, hotkeys: GlobalHotKeyManager::new().ok(), registered: vec![], show_hotkey_works: false, hotkey_map, escape: None, escape_id, enter: None, enter_id, keys_suspended: false };
        shell.apply_hotkeys(settings);
        shell
    }

    /// Re-register global hotkeys from the settings; returns the ones macOS refused.
    pub fn apply_hotkeys(&mut self, settings: &Settings) -> Vec<HotkeyAction> {
        let Some(mgr) = &self.hotkeys else { return vec![] };
        if self.keys_suspended {
            // Registered again when the recording ends, from the settings of that moment.
            return vec![];
        }
        let _ = mgr.unregister_all(&self.registered);
        self.registered.clear();
        let mut map = self.hotkey_map.lock().unwrap();
        map.clear();
        let mut failed = vec![];
        for a in HotkeyAction::ALL {
            let Some(s) = settings.hotkey(a) else { continue };
            match s.parse::<HotKey>() {
                Ok(hk) if mgr.register(hk).is_ok() => {
                    map.insert(hk.id(), a);
                    self.registered.push(hk);
                }
                _ => failed.push(a),
            }
        }
        self.show_hotkey_works = settings.hotkey(HotkeyAction::Show).is_some() && !failed.contains(&HotkeyAction::Show);
        self.sync(settings);
        failed
    }

    /// Switches the menu bar's key equivalents and the global hotkeys off while a shortcut is being
    /// recorded in Preferences, and back on afterwards. A key equivalent is run by AppKit before the
    /// window sees the key (⌘Q would quit in the middle of recording, ⌘S would save) and a registered
    /// global hotkey never reaches the app as a key at all, so neither could be recorded. Removing the
    /// main menu takes all of its items out, the predefined ones (Hide, Full Screen...) included.
    /// Returns the hotkeys macOS refused when they are registered again.
    pub fn suspend_keys(&mut self, on: bool, settings: &Settings) -> Vec<HotkeyAction> {
        if self.keys_suspended == on {
            return vec![];
        }
        if on {
            self.menubar.remove_for_nsapp();
            if let Some(mgr) = &self.hotkeys {
                let _ = mgr.unregister_all(&self.registered);
            }
            self.registered.clear();
            self.hotkey_map.lock().unwrap().clear();
            self.keys_suspended = true;
            vec![]
        } else {
            self.keys_suspended = false;
            self.menubar.init_for_nsapp();
            self.apply_hotkeys(settings)
        }
    }

    /// Gives Return back as soon as it has ended the capture; Esc stays until the capture is over.
    pub fn release_enter(&mut self) {
        let Some(mgr) = &self.hotkeys else { return };
        if let Some(hk) = self.enter.take() {
            self.enter_id.store(0, Ordering::SeqCst);
            let _ = mgr.unregister(hk);
        }
    }

    /// Takes Esc and Return for the length of a scrolling capture, or gives them back. The control
    /// never has the keyboard (the page being captured keeps it), so these keys are global.
    pub fn grab_escape(&mut self, on: bool) {
        let Some(mgr) = &self.hotkeys else { return };
        for (slot, id, name) in [(&mut self.escape, &self.escape_id, "Escape"), (&mut self.enter, &self.enter_id, "Enter")] {
            if let Some(hk) = slot.take() {
                id.store(0, Ordering::SeqCst);
                let _ = mgr.unregister(hk);
            }
            if on {
                let Ok(hk) = name.parse::<HotKey>() else { continue };
                if mgr.register(hk).is_ok() {
                    id.store(hk.id(), Ordering::SeqCst);
                    *slot = Some(hk);
                }
            }
        }
    }

    pub fn check_for_updates(&self) {
        if let Some(updater) = &self.updater {
            updater.check_now();
        }
    }

    /// Flips Sparkle's own setting, which also changes from its first-launch question, and shows the
    /// result. The check mark is not trusted here: it may have been stale when muda flipped it.
    pub fn toggle_auto_updates(&self) {
        if let Some(updater) = &self.updater {
            updater.set_checks_automatically(!updater.checks_automatically());
        }
        self.refresh_updates();
    }

    /// Shows what Sparkle holds, so the check mark follows changes made outside this menu.
    pub fn refresh_updates(&self) {
        if let Some(updater) = &self.updater {
            self.auto_updates_item.set_checked(updater.checks_automatically());
        }
    }

    /// Enables and disables the items that depend on the editor. Only talks to AppKit on a change.
    pub fn set_state(&mut self, st: MenuState) {
        if self.state == Some(st) {
            return;
        }
        self.state = Some(st);
        for (id, item) in &self.stateful {
            item.set_enabled(menu_model::item_enabled(id, &st));
        }
        for (id, item) in &self.tray_stateful {
            item.set_enabled(menu_model::tray_item_enabled(id, &st));
        }
    }

    pub fn sync(&self, settings: &Settings) {
        // The status menu shows each global hotkey next to its item.
        for (a, item) in &self.hotkey_items {
            let accel = settings.hotkey(*a).and_then(|s| s.parse::<Accelerator>().ok());
            let _ = item.set_accelerator(accel);
        }
        self.autostart_item.set_checked(settings.autostart);
        for (id, item) in &self.snap_items {
            item.set_checked(if id == "snap" { settings.snap_objects } else { settings.snap_similar });
        }
        if let Some(item) = &self.delayed_item {
            item.set_text(crate::settings::delayed_label(settings.delay()));
        }
        self.refresh_updates();
        if let Some(t) = &self.tray {
            let _ = t.set_visible(settings.menubar_icon_visible(self.show_hotkey_works));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::menu_symbol;

    #[test]
    fn menu_bar_icon_is_a_36_px_black_template() {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/menubar.png")).unwrap();
        assert_eq!((icon.width, icon.height), (36, 36));
        assert!(icon.rgba.chunks(4).all(|p| p[..3] == [0, 0, 0]), "a template image only uses alpha");
        assert!(icon.rgba.chunks(4).any(|p| p[3] == 255) && icon.rgba.chunks(4).any(|p| p[3] == 0));
    }

    #[test]
    fn updates_symbol_is_only_for_the_check_item() {
        assert!(menu_symbol("Check for Updates…").is_some());
        assert_eq!(menu_symbol("Check for Updates Automatically"), None);
        assert!(menu_symbol("Zoom To 100%").is_some());
    }
}
