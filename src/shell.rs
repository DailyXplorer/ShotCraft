//! Menu bar icon, the app's native menus and the global hotkeys.
use crate::settings::{HotkeyAction, Settings};
use crate::{Action, Queue};
use crate::model::Tool;
use crate::system::CaptureMode;
use crate::updater::Updater;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, ContextMenu, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIcon, TrayIconBuilder};

pub struct Shell {
    tray: Option<TrayIcon>,
    _menubar: Menu,
    autostart_item: CheckMenuItem,
    auto_updates_item: CheckMenuItem,
    updater: Option<Updater>,
    hotkeys: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    hotkey_map: Arc<Mutex<HashMap<u32, HotkeyAction>>>,
}

fn action_for(id: &str) -> Option<Action> {
    Some(match id {
        "fullscreen" => Action::Capture(CaptureMode::Fullscreen),
        "area" => Action::Capture(CaptureMode::Area),
        "repeat" => Action::Capture(CaptureMode::RepeatArea),
        "window" => Action::Capture(CaptureMode::Window),
        "delayed" => Action::Capture(CaptureMode::Delayed),
        "ocr" => Action::InstantOcr,
        "clipboard" => Action::OpenClipboard,
        "open" => Action::OpenFile,
        "show" => Action::Show,
        "prefs" => Action::Preferences,
        "autostart" => Action::ToggleAutostart,
        "updates" => Action::CheckForUpdates,
        "autoupdates" => Action::ToggleAutoUpdates,
        "quit" => Action::Quit,
        "copy" => Action::Copy,
        "save" => Action::Save,
        "saveas" => Action::SaveAs,
        "pin" => Action::Pin,
        "close" => Action::Close,
        "undo" => Action::Undo,
        "redo" => Action::Redo,
        "resetcrop" => Action::ResetCrop,
        "rasterize" => Action::Rasterize,
        "zoomin" => Action::ZoomIn,
        "zoomout" => Action::ZoomOut,
        "zoomfit" => Action::ZoomFit,
        "zoom100" => Action::Zoom100,
        "about" => Action::About,
        "shortcuts" => Action::Shortcuts,
        t if t.starts_with("tool:") => {
            let name = &t[5..];
            Action::Tool(*Tool::TOOLBAR.iter().find(|x| x.name() == name)?)
        }
        _ => return None,
    })
}

fn acc(m: Modifiers, c: Code) -> Option<Accelerator> {
    Some(Accelerator::new(m, c))
}

/// SF Symbol shown before a menu item, as in Apple's own menus. Matched on the start of the title.
const MENU_SYMBOLS: &[(&str, &str)] = &[
    ("Preferences", "gearshape"),
    ("About", "info.circle"),
    ("Check for Updates", "arrow.triangle.2.circlepath"),
    ("Quit", "rectangle.portrait.and.arrow.right"),
    ("Show ShotCraft", "eye"),
    ("Show All", "eye"),
    ("Hide ShotCraft", "eye.slash"),
    ("Hide Others", "eye.slash"),
    ("Services", "gearshape.2"),
    ("Launch at Startup", "power"),
    ("Area Capture", "rectangle.dashed"),
    ("Fullscreen Screenshot", "display"),
    ("Repeat Area Capture", "arrow.clockwise"),
    ("Capture Any Window", "macwindow"),
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

fn add_menu_symbols(menu: &dyn ContextMenu) {
    use objc2_app_kit::{NSImage, NSMenu};
    use objc2_foundation::NSString;
    fn walk(menu: &NSMenu) {
        for item in menu.itemArray().iter() {
            let title = item.title().to_string();
            if let Some((_, name)) = MENU_SYMBOLS.iter().find(|(prefix, _)| title.starts_with(prefix)) {
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

fn tray_icon_image() -> tray_icon::Icon {
    // A small viewfinder: four corners and a dot, drawn as a template image.
    let n = 36u32;
    let mut px = vec![0u8; (n * n * 4) as usize];
    let mut set = |x: u32, y: u32| {
        let i = ((y * n + x) * 4) as usize;
        px[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
    };
    let (a, b, l, t) = (4u32, 31u32, 10u32, 3u32);
    for k in 0..l {
        for s in 0..t {
            for (cx, cy, dx, dy) in [(a, a, 1i32, 1i32), (b, a, -1, 1), (a, b, 1, -1), (b, b, -1, -1)] {
                let hx = (cx as i32 + dx * k as i32) as u32;
                let hy = (cy as i32 + dy * s as i32) as u32;
                set(hx, hy);
                let vx = (cx as i32 + dx * s as i32) as u32;
                let vy = (cy as i32 + dy * k as i32) as u32;
                set(vx, vy);
            }
        }
    }
    for y in 13..23u32 {
        for x in 13..23u32 {
            let (dx, dy) = (x as f32 - 17.5, y as f32 - 17.5);
            if dx * dx + dy * dy <= 22.0 {
                set(x, y);
            }
        }
    }
    tray_icon::Icon::from_rgba(px, n, n).expect("icon")
}

impl Shell {
    pub fn start(queue: Queue, ctx: eframe::egui::Context, settings: &Settings) -> Shell {
        let cmd = Modifiers::META;
        let cmd_shift = Modifiers::META | Modifiers::SHIFT;
        let updater = Updater::start();
        let updates_item = MenuItem::with_id("updates", "Check for Updates…", updater.is_some(), None);
        let automatic = updater.as_ref().is_some_and(Updater::checks_automatically);
        let auto_updates_item = CheckMenuItem::with_id("autoupdates", "Check for Updates Automatically", updater.is_some(), automatic, None);

        let app_menu = Submenu::with_items(
            "ShotCraft",
            true,
            &[
                &MenuItem::with_id("about", "About ShotCraft", true, None),
                &updates_item,
                &auto_updates_item,
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id("prefs", "Preferences…", true, acc(cmd, Code::Comma)),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(Some("Hide ShotCraft")),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id("quit", "Quit ShotCraft", true, acc(cmd, Code::KeyQ)),
            ],
        )
        .expect("menu");
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &MenuItem::with_id("copy", "Copy Image", true, acc(cmd_shift, Code::KeyC)),
                &MenuItem::with_id("save", "Save Image", true, acc(cmd, Code::KeyS)),
                &MenuItem::with_id("saveas", "Save Image As", true, acc(cmd_shift, Code::KeyS)),
                &MenuItem::with_id("pin", "Pin to Screen", true, acc(cmd, Code::KeyP)),
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id("open", "Open File", true, acc(cmd_shift, Code::KeyO)),
                &MenuItem::with_id("clipboard", "Open From Clipboard", true, acc(cmd_shift, Code::KeyV)),
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id("close", "Close", true, acc(cmd, Code::KeyW)),
            ],
        )
        .expect("menu");
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &MenuItem::with_id("undo", "Undo", true, acc(cmd, Code::KeyZ)),
                &MenuItem::with_id("redo", "Redo", true, acc(cmd_shift, Code::KeyZ)),
                &MenuItem::with_id("resetcrop", "Reset Crop", true, None),
                &MenuItem::with_id("rasterize", "Rasterize (Merge) Image", true, None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::cut(None),
                &PredefinedMenuItem::copy(None),
                &PredefinedMenuItem::paste(None),
                &PredefinedMenuItem::select_all(None),
            ],
        )
        .expect("menu");
        let draw = Submenu::new("Draw", true);
        for t in [
            Tool::Arrow,
            Tool::Text,
            Tool::Rectangle,
            Tool::Freehand,
            Tool::Magnifier,
            Tool::Highlighter,
            Tool::Blur,
            Tool::Spotlight,
            Tool::Counter,
            Tool::Oval,
            Tool::Line,
            Tool::Ruler,
            Tool::Backdrop,
        ] {
            let _ = draw.append(&MenuItem::with_id(format!("tool:{}", t.name()), t.tooltip(), true, None));
        }
        let zoom = Submenu::with_items(
            "Zoom",
            true,
            &[
                &MenuItem::with_id("zoomin", "Zoom In", true, acc(cmd, Code::Equal)),
                &MenuItem::with_id("zoomout", "Zoom Out", true, acc(cmd, Code::Minus)),
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id("zoomfit", "Zoom To Fit", true, acc(cmd, Code::Digit1)),
                &MenuItem::with_id("zoom100", "Zoom To 100%", true, acc(cmd, Code::Digit0)),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::fullscreen(Some("Enter Full Screen")),
            ],
        )
        .expect("menu");
        let help = Submenu::with_items("Help", true, &[&MenuItem::with_id("shortcuts", "Keyboard Shortcuts", true, None)]).expect("menu");
        let menubar = Menu::with_items(&[&app_menu, &file, &edit, &draw, &zoom, &help]).expect("menu");
        add_menu_symbols(&menubar);
        menubar.init_for_nsapp();
        help.set_as_help_menu_for_nsapp();

        let autostart_item = CheckMenuItem::with_id("autostart", "Launch at Startup", true, settings.autostart, None);
        let tray_menu = Menu::with_items(&[
            &MenuItem::with_id("area", "Area Capture", true, None),
            &MenuItem::with_id("fullscreen", "Fullscreen Screenshot", true, None),
            &MenuItem::with_id("repeat", "Repeat Area Capture", true, None),
            &MenuItem::with_id("window", "Capture Any Window", true, None),
            &MenuItem::with_id("delayed", "Delayed Screenshot (3s)", true, None),
            &MenuItem::with_id("ocr", "Recognize Text/QR", true, None),
            &MenuItem::with_id("clipboard", "Load From Clipboard", true, None),
            &MenuItem::with_id("open", "Open File…", true, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("show", "Show ShotCraft", true, None),
            &MenuItem::with_id("prefs", "Preferences…", true, None),
            &autostart_item,
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("quit", "Quit ShotCraft", true, None),
        ])
        .expect("menu");
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
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            if e.state != HotKeyState::Pressed {
                return;
            }
            let Some(a) = map.lock().unwrap().get(&e.id).copied() else { return };
            let action = match a {
                HotkeyAction::Fullscreen => Action::Capture(CaptureMode::Fullscreen),
                HotkeyAction::Area => Action::Capture(CaptureMode::Area),
                HotkeyAction::Window => Action::Capture(CaptureMode::Window),
                HotkeyAction::RepeatArea => Action::Capture(CaptureMode::RepeatArea),
                HotkeyAction::Ocr => Action::InstantOcr,
                HotkeyAction::Show => Action::Show,
            };
            queue.lock().unwrap().push(action);
            ctx.request_repaint();
        }));

        let mut shell = Shell { tray, _menubar: menubar, autostart_item, auto_updates_item, updater, hotkeys: GlobalHotKeyManager::new().ok(), registered: vec![], hotkey_map };
        shell.apply_hotkeys(settings);
        shell
    }

    /// Re-register global hotkeys from the settings; returns the ones macOS refused.
    pub fn apply_hotkeys(&mut self, settings: &Settings) -> Vec<HotkeyAction> {
        let Some(mgr) = &self.hotkeys else { return vec![] };
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
        failed
    }

    pub fn check_for_updates(&self) {
        if let Some(updater) = &self.updater {
            updater.check_now();
        }
    }

    /// The menu has already flipped its check mark, so it holds what the user asked for.
    pub fn apply_auto_updates(&self) {
        if let Some(updater) = &self.updater {
            updater.set_checks_automatically(self.auto_updates_item.is_checked());
        }
    }

    pub fn sync(&self, settings: &Settings) {
        self.autostart_item.set_checked(settings.autostart);
        if let Some(t) = &self.tray {
            let _ = t.set_visible(!settings.hide_menubar_icon);
        }
    }
}
