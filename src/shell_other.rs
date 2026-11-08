//! Menu bar icon and native menus (macOS), tray icon (Windows, Linux) and the global hotkeys. The
//! menus' items come from the command table (`commands`).
use crate::commands;
#[cfg(not(target_os = "linux"))]
use crate::commands::Item;
use crate::settings::{HotkeyAction, Settings};
use crate::Queue;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(not(target_os = "linux"))]
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(not(target_os = "linux"))]
use tray_icon::{TrayIcon, TrayIconBuilder};

/// An on/off item of the tray menu and how to read its state from the settings.
#[cfg(not(target_os = "linux"))]
type CheckItem = (fn(&Settings) -> bool, CheckMenuItem);

pub struct Shell {
    #[cfg(not(target_os = "linux"))]
    tray: Option<TrayIcon>,
    #[cfg(not(target_os = "linux"))]
    checks: Vec<CheckItem>,
    #[cfg(target_os = "linux")]
    tray: Option<ksni::blocking::Handle<linux::Tray>>,
    #[cfg(target_os = "linux")]
    events: (Queue, eframe::egui::Context),
    /// Wayland: the hotkeys go through the desktop's GlobalShortcuts portal (`portal_hotkeys`),
    /// and `hotkeys` stays empty: an X11 grab there only saw keys while an X11 window had focus.
    #[cfg(target_os = "linux")]
    portal_keys: Option<crate::portal_hotkeys::PortalHotkeys>,
    hotkeys: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    /// Whether the Show ShotCraft hotkey works: the tray icon can be hidden only then.
    show_hotkey_works: bool,
    hotkey_map: Arc<Mutex<HashMap<u32, HotkeyAction>>>,
    /// Esc, registered only while a scrolling capture runs so that it can be finished from any app.
    escape: Option<HotKey>,
    escape_id: Arc<AtomicU32>,
    /// The check mark the tray icon shows for a moment after a copy or a save (upstream's timing).
    flash: crate::tray_flash::TrayFlash,
    clock: std::time::Instant,
}

/// How long a hotkey's action may wait for a UI pass before the editor is brought back.
#[cfg(target_os = "linux")]
const STALL_MS: u64 = 400;

/// A Wayland session, where hotkeys go through the portal instead (`Shell::portal_keys`).
fn wayland_session() -> bool {
    #[cfg(target_os = "linux")]
    let w = crate::capture::wayland();
    #[cfg(not(target_os = "linux"))]
    let w = false;
    w
}

fn push(queue: &Queue, ctx: &eframe::egui::Context, a: crate::Action) {
    queue.lock().unwrap_or_else(PoisonError::into_inner).push(a);
    ctx.request_repaint();
}

/// A menu item's id was clicked.
fn run_id(queue: &Queue, ctx: &eframe::egui::Context, id: &str) {
    if let Some(a) = commands::action_for_id(id) {
        push(queue, ctx, a);
    }
}

/// A small viewfinder: four corners and a dot, or (`done`, for a moment after a copy or a save) a
/// check mark in place of the dot, as the macOS menu bar icon does. White with a dark rim, so it shows
/// on light and dark taskbars.
fn tray_icon_rgba(done: bool) -> (Vec<u8>, u32) {
    let n = 36u32;
    let mut on = vec![false; (n * n) as usize];
    let mut set = |x: u32, y: u32| on[(y * n + x) as usize] = true;
    let (a, b, l, t) = (4u32, 31u32, 10u32, 3u32);
    for k in 0..l {
        for s in 0..t {
            for (cx, cy, dx, dy) in [(a, a, 1i32, 1i32), (b, a, -1, 1), (a, b, 1, -1), (b, b, -1, -1)] {
                set((cx as i32 + dx * k as i32) as u32, (cy as i32 + dy * s as i32) as u32);
                set((cx as i32 + dx * s as i32) as u32, (cy as i32 + dy * k as i32) as u32);
            }
        }
    }
    if done {
        // Two strokes, 3 pixels thick: down to the bottom of the tick, then up to the right.
        for (from, to) in [((13.5f32, 18.0f32), (16.5f32, 21.0f32)), ((16.5, 21.0), (22.5, 15.0))] {
            for step in 0..=40 {
                let f = step as f32 / 40.0;
                let (cx, cy) = (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f);
                for y in 10..27u32 {
                    for x in 9..28u32 {
                        let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                        if dx * dx + dy * dy <= 2.4 {
                            set(x, y);
                        }
                    }
                }
            }
        }
    } else {
        for y in 13..23u32 {
            for x in 13..23u32 {
                let (dx, dy) = (x as f32 - 17.5, y as f32 - 17.5);
                if dx * dx + dy * dy <= 22.0 {
                    set(x, y);
                }
            }
        }
    }
    let mut px = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let i = (y * n + x) as usize;
            let rgba = if on[i] {
                Some([255, 255, 255, 255])
            } else {
                let near = (-2i32..=2).any(|dy| {
                    (-2i32..=2).any(|dx| {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        nx >= 0 && ny >= 0 && nx < n as i32 && ny < n as i32 && on[(ny as u32 * n + nx as u32) as usize]
                    })
                });
                near.then_some([30, 30, 34, 230])
            };
            if let Some(c) = rgba {
                px[i * 4..i * 4 + 4].copy_from_slice(&c);
            }
        }
    }
    (px, n)
}

/// Turns command-table items into native menu items: the tray's, and the macOS menu bar's (with
/// the editor's shortcuts shown next to them). Collects the on/off items to keep their check marks
/// current, and Sparkle's check item on macOS.
#[cfg(not(target_os = "linux"))]
struct Native<'a> {
    settings: &'a Settings,
    accelerators: bool,
    checks: Vec<CheckItem>,
}

#[cfg(not(target_os = "linux"))]
impl Native<'_> {
    fn items(&mut self, items: &[Item]) -> Vec<Box<dyn IsMenuItem>> {
        let mut out: Vec<Box<dyn IsMenuItem>> = vec![];
        for item in items {
            match item {
                Item::Cmd(c) => match c.checked {
                    Some(on) => {
                        let check = CheckMenuItem::with_id(c.id, c.label, true, on(self.settings), None);
                        self.checks.push((on, check.clone()));
                        out.push(Box::new(check));
                    }
                    None => {
                        let keys = None;
                        out.push(Box::new(MenuItem::with_id(c.id, c.label, true, keys)));
                    }
                },
                Item::Separator => out.push(Box::new(PredefinedMenuItem::separator())),
                Item::Tools => {
                    for t in commands::DRAW {
                        out.push(Box::new(MenuItem::with_id(format!("tool:{}", t.name()), t.tooltip(), true, None)));
                    }
                }
                Item::Sub(name, sub) => {
                    let menu = Submenu::new(*name, true);
                    for i in self.items(sub) {
                        let _ = menu.append(i.as_ref());
                    }
                    out.push(Box::new(menu));
                } // Sparkle's two items work only when it loaded (packaged releases).
            }
        }
        out
    }

    fn menu(&mut self, items: &[Item]) -> Menu {
        let menu = Menu::new();
        for i in self.items(items) {
            let _ = menu.append(i.as_ref());
        }
        menu
    }
}

#[cfg(not(target_os = "linux"))]
fn build_tray(menu: Menu) -> Option<TrayIcon> {
    // macOS: the author's black menu bar glyph, tinted by the system. Elsewhere the viewfinder.
    let icon = {
        let (px, n) = tray_icon_rgba(false);
        tray_icon::Icon::from_rgba(px, n, n).ok()?
    };
    TrayIconBuilder::new().with_menu(Box::new(menu)).with_icon(icon).with_tooltip("ShotCraft").build().ok()
}

impl Shell {
    pub fn start(queue: Queue, ctx: eframe::egui::Context, settings: &Settings) -> Shell {
        #[cfg(not(target_os = "linux"))]
        let mut native = Native { settings, accelerators: true, checks: vec![] };
        #[cfg(not(target_os = "linux"))]
        let tray = {
            native.accelerators = false;
            let menu = native.menu(commands::TRAY);
            let (q, c) = (queue.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| run_id(&q, &c, e.id.0.as_str())));
            build_tray(menu)
        };
        #[cfg(not(target_os = "linux"))]
        let checks = std::mem::take(&mut native.checks);

        let hotkey_map: Arc<Mutex<HashMap<u32, HotkeyAction>>> = Arc::default();
        let map = hotkey_map.clone();
        let (q, c) = (queue.clone(), ctx.clone());
        let escape_id: Arc<AtomicU32> = Arc::default();
        let esc = escape_id.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            if e.state != HotKeyState::Pressed {
                return;
            }
            if e.id == esc.load(Ordering::SeqCst) {
                push(&q, &c, crate::Action::ScrollingFinish);
                return;
            }
            let Some(a) = map.lock().unwrap_or_else(PoisonError::into_inner).get(&e.id).copied() else { return };
            if let Some(cmd) = commands::for_hotkey(a) {
                push(&q, &c, (cmd.action)());
            }
        }));

        let mut shell = Shell {
            #[cfg(not(target_os = "linux"))]
            tray,
            #[cfg(not(target_os = "linux"))]
            checks,
            #[cfg(target_os = "linux")]
            tray: None,
            #[cfg(target_os = "linux")]
            events: (queue.clone(), ctx.clone()),
            #[cfg(target_os = "linux")]
            portal_keys: crate::capture::wayland().then(crate::portal_hotkeys::PortalHotkeys::new),
            hotkeys: if wayland_session() { None } else { GlobalHotKeyManager::new().ok() },
            registered: vec![],
            show_hotkey_works: false,
            escape: None,
            escape_id,
            flash: crate::tray_flash::TrayFlash::default(),
            clock: std::time::Instant::now(),
            hotkey_map,
        };
        shell.sync(settings);
        shell.apply_hotkeys(settings);
        shell
    }

    /// Re-register global hotkeys from the settings; returns the ones the system refused. Whether
    /// Show ShotCraft works decides whether the tray icon may be hidden (`sync`).
    pub fn apply_hotkeys(&mut self, settings: &Settings) -> Vec<HotkeyAction> {
        #[cfg(target_os = "linux")]
        if let Some(portal) = &self.portal_keys {
            let set: Vec<(HotkeyAction, String)> = HotkeyAction::ALL.iter().filter_map(|a| Some((*a, settings.hotkey(*a)?.to_string()))).collect();
            let label = |a: HotkeyAction| commands::for_hotkey(a).map_or_else(|| format!("{a:?}"), |c| c.label.to_string());
            let (bindings, failed) = crate::portal_hotkeys::bindings(&set, label);
            let (q, c) = self.events.clone();
            portal.apply(bindings, move |a, token| {
                let Some(cmd) = commands::for_hotkey(a) else { return };
                let queued = crate::wayland_bridge::now_ms();
                push(&q, &c, (cmd.action)());
                // A minimized editor gets no frame callbacks, so the action would wait for good:
                // when no UI pass has run soon after, bring the editor back with this press's token.
                if let Some(token) = token {
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(STALL_MS));
                        let waited = crate::wayland_bridge::now_ms().saturating_sub(queued);
                        if crate::wayland_bridge::stalled(queued, crate::wayland_bridge::last_pass_ms(), waited, STALL_MS) {
                            crate::wayland_bridge::activate_editor(&token);
                        }
                    });
                }
            });
            self.show_hotkey_works = settings.hotkey(HotkeyAction::Show).is_some() && !failed.contains(&HotkeyAction::Show);
            self.sync(settings);
            return failed;
        }
        let failed = match &self.hotkeys {
            None => HotkeyAction::ALL.iter().copied().filter(|a| settings.hotkey(*a).is_some()).collect(),
            Some(mgr) => {
                let _ = mgr.unregister_all(&self.registered);
                self.registered.clear();
                let mut map = self.hotkey_map.lock().unwrap_or_else(PoisonError::into_inner);
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
        };
        self.show_hotkey_works = settings.hotkey(HotkeyAction::Show).is_some() && !failed.contains(&HotkeyAction::Show);
        self.sync(settings);
        failed
    }

    /// A copy or a save succeeded: the tray icon shows a check mark for a moment.
    pub fn flash_done(&mut self) {
        self.flash.start(self.clock.elapsed().as_secs_f64());
        self.tick_flash();
    }

    /// Swaps the tray icon when the confirmation starts or ends. Returns how long until it has to be
    /// called again, while a confirmation is on.
    pub fn tick_flash(&mut self) -> Option<std::time::Duration> {
        let now = self.clock.elapsed().as_secs_f64();
        if let Some(image) = self.flash.tick(now) {
            let done = image == crate::tray_flash::TrayImage::Done;
            #[cfg(not(target_os = "linux"))]
            if let Some(t) = &self.tray {
                let (px, n) = tray_icon_rgba(done);
                if let Ok(icon) = tray_icon::Icon::from_rgba(px, n, n) {
                    let _ = t.set_icon(Some(icon));
                }
            }
            #[cfg(target_os = "linux")]
            if let Some(t) = &self.tray {
                t.update(|tray| tray.done = done);
            }
        }
        self.flash.next_check(now).map(|s| std::time::Duration::from_secs_f64(s + 0.02))
    }

    /// Takes Esc for the length of a scrolling capture, or gives it back.
    pub fn grab_escape(&mut self, on: bool) {
        let Some(mgr) = &self.hotkeys else { return };
        if let Some(hk) = self.escape.take() {
            self.escape_id.store(0, Ordering::SeqCst);
            release_hotkey(mgr, hk);
        }
        if on {
            let Ok(hk) = "Escape".parse::<HotKey>() else { return };
            if mgr.register(hk).is_ok() {
                self.escape_id.store(hk.id(), Ordering::SeqCst);
                self.escape = Some(hk);
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn sync(&mut self, settings: &Settings) {
        for (on, item) in &self.checks {
            item.set_checked(on(settings));
        }
        if let Some(t) = &self.tray {
            let _ = t.set_visible(settings.menubar_icon_visible(self.show_hotkey_works));
        }
    }

    /// A StatusNotifierItem can't be hidden, so hiding stops it and showing starts a new one.
    #[cfg(target_os = "linux")]
    pub fn sync(&mut self, settings: &Settings) {
        if !settings.menubar_icon_visible(self.show_hotkey_works) {
            if let Some(t) = self.tray.take() {
                let _ = t.shutdown();
            }
            return;
        }
        let checked = linux::checked(settings);
        match &self.tray {
            Some(t) if !t.is_closed() => {
                t.update(|tray| tray.checked = checked);
            }
            _ => {
                let (queue, ctx) = self.events.clone();
                self.tray = linux::start(linux::Tray { queue, ctx, checked, done: false });
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{run_id, tray_icon_rgba};
    use crate::commands::{Item, TRAY};
    use crate::settings::Settings;
    use crate::Queue;
    use ksni::blocking::TrayMethods;
    use ksni::menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu};

    pub struct Tray {
        pub queue: Queue,
        pub ctx: eframe::egui::Context,
        /// Ids of the on/off items that are on.
        pub checked: Vec<&'static str>,
        /// The check mark of a copy or a save is showing (`Shell::tick_flash`).
        pub done: bool,
    }

    pub fn checked(settings: &Settings) -> Vec<&'static str> {
        fn walk(items: &[Item], settings: &Settings, out: &mut Vec<&'static str>) {
            for i in items {
                match i {
                    Item::Cmd(c) if c.checked.is_some_and(|on| on(settings)) => out.push(c.id),
                    Item::Sub(_, sub) => walk(sub, settings, out),
                    _ => {}
                }
            }
        }
        let mut out = vec![];
        walk(TRAY, settings, &mut out);
        out
    }

    pub fn start(tray: Tray) -> Option<ksni::blocking::Handle<Tray>> {
        tray.spawn().ok()
    }

    impl Tray {
        fn items(&self, items: &[Item]) -> Vec<MenuItem<Self>> {
            let mut out = vec![];
            for item in items {
                match item {
                    Item::Cmd(c) => {
                        let id = c.id;
                        let activate = Box::new(move |t: &mut Tray| run_id(&t.queue, &t.ctx, id));
                        out.push(match c.checked {
                            Some(_) => CheckmarkItem { label: c.label.into(), checked: self.checked.contains(&id), activate, ..Default::default() }.into(),
                            None => StandardItem {
                                label: crate::commands::session_label(c).into_owned(),
                                enabled: crate::commands::session_allows(id),
                                activate,
                                ..Default::default()
                            }
                            .into(),
                        });
                    }
                    Item::Sub(name, sub) => out.push(SubMenu { label: (*name).into(), submenu: self.items(sub), ..Default::default() }.into()),
                    Item::Separator => out.push(MenuItem::Separator),
                    // The drawing tools are the editor's menu bar's only.
                    Item::Tools => {}
                }
            }
            out
        }
    }

    impl ksni::Tray for Tray {
        // A click opens the menu, as the menu bar icon does on macOS.
        const MENU_ON_ACTIVATE: bool = true;

        fn id(&self) -> String {
            "shotcraft".into()
        }

        fn title(&self) -> String {
            "ShotCraft".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let (rgba, n) = tray_icon_rgba(self.done);
            // ARGB32 in network byte order.
            let data = rgba.chunks_exact(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
            vec![ksni::Icon { width: n as i32, height: n as i32, data }]
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip { title: "ShotCraft".into(), ..Default::default() }
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            self.items(TRAY)
        }
    }
}

/// Gives a hotkey back to the other programs.
fn release_hotkey(mgr: &GlobalHotKeyManager, hk: HotKey) {
    let _ = mgr.unregister(hk);
    #[cfg(target_os = "linux")]
    flush_ungrab(mgr);
}

/// Linux (X11): `global-hotkey` (0.8 and 0.9) sends the ungrab of an unregistered key but never
/// flushes its connection, so the key stays grabbed, and a program asking for it gets BadAccess
/// (ShotCraft's own capture picker could no longer be cancelled with Esc after a scrolling
/// capture). Registering a key waits for the server's reply, which sends everything queued before
/// it. So register a key combination nobody types, then release that one too; its own ungrab
/// goes out with the next call. Drop this once the crate flushes.
#[cfg(target_os = "linux")]
fn flush_ungrab(mgr: &GlobalHotKeyManager) {
    use global_hotkey::hotkey::{Code, Modifiers};
    let nobody_types_this = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::SUPER), Code::F24);
    if mgr.register(nobody_types_this).is_ok() {
        let _ = mgr.unregister(nobody_types_this);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs an X server (the suite runs under `xvfb-run`); without one there is nothing to grab.
    #[cfg(target_os = "linux")]
    #[test]
    fn escape_is_free_for_other_programs_right_after_it_is_released() {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
        let Ok((conn, screen)) = x11rb::connect(None) else { return };
        let setup = conn.setup();
        let root = setup.roots[screen].root;
        let (first, last) = (setup.min_keycode, setup.max_keycode);
        let map = conn.get_keyboard_mapping(first, last - first + 1).expect("ask").reply().expect("keyboard map");
        let per = usize::from(map.keysyms_per_keycode).max(1);
        let index = map.keysyms.chunks(per).position(|syms| syms.contains(&0xff1b)).expect("a key for Escape");
        let escape = first + index as u8;

        let mgr = GlobalHotKeyManager::new().expect("hotkey manager");
        let hk: HotKey = "Escape".parse().expect("Escape parses");
        mgr.register(hk).expect("ShotCraft takes Esc");
        let held = conn.grab_key(false, root, ModMask::ANY, escape, GrabMode::ASYNC, GrabMode::ASYNC).expect("ask").check();
        assert!(held.is_err(), "while ShotCraft holds Esc, another program can't have it");

        release_hotkey(&mgr, hk);
        let freed = conn.grab_key(false, root, ModMask::ANY, escape, GrabMode::ASYNC, GrabMode::ASYNC).expect("ask").check();
        assert!(freed.is_ok(), "Esc must be free the moment it is released: {freed:?}");
    }

    #[test]
    fn every_tray_item_has_an_action() {
        for item in commands::TRAY {
            if let commands::Item::Cmd(c) = item {
                assert!(commands::action_for_id(c.id).is_some(), "{}", c.id);
            }
        }
    }

    #[test]
    fn tray_icon_is_36_pixels_and_not_empty() {
        let (px, n) = tray_icon_rgba(false);
        assert_eq!((px.len(), n), (36 * 36 * 4, 36));
        assert!(px.chunks_exact(4).any(|p| p[3] == 255));
    }

    #[test]
    fn the_done_icon_is_the_same_viewfinder_with_a_check_mark() {
        let ((normal, n), (done, _)) = (tray_icon_rgba(false), tray_icon_rgba(true));
        assert_eq!(normal.len(), done.len());
        let at = |px: &[u8], x: usize, y: usize| px[(y * n as usize + x) * 4..(y * n as usize + x) * 4 + 4].to_vec();
        // The corners are the same, the middle is not.
        for (x, y) in [(5, 5), (30, 5), (5, 30), (30, 30)] {
            assert_eq!(at(&normal, x, y), at(&done, x, y), "corner at {x},{y}");
        }
        assert_ne!(normal, done);
        // The tick: its bottom is white, the middle of the old dot is not part of it.
        assert_eq!(at(&done, 16, 20), vec![255, 255, 255, 255]);
        assert_eq!(at(&normal, 17, 17), vec![255, 255, 255, 255]);
        assert_ne!(at(&done, 17, 16)[..3], [255, 255, 255]);
        // To look at them: SHOTCRAFT_SHOTS=<dir> saves both, eight times their size, on grey.
        if let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") {
            for (name, px) in [("tray-normal", &normal), ("tray-done", &done)] {
                let mut on_grey = image::RgbaImage::from_pixel(n, n, image::Rgba([120, 120, 120, 255]));
                for (o, s) in on_grey.pixels_mut().zip(px.chunks_exact(4)) {
                    let a = s[3] as u32;
                    for c in 0..3 {
                        o.0[c] = ((s[c] as u32 * a + o.0[c] as u32 * (255 - a)) / 255) as u8;
                    }
                }
                let big = image::imageops::resize(&on_grey, n * 8, n * 8, image::imageops::FilterType::Nearest);
                let _ = big.save(std::path::Path::new(&dir).join(format!("{name}.png")));
            }
        }
    }
}
