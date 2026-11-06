//! Menu bar icon and native menus (macOS), tray icon (Windows, Linux) and the global hotkeys. The
//! menus' items come from the command table (`commands`).
use crate::commands;
use crate::settings::{HotkeyAction, Settings};
use crate::Queue;
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

pub struct Shell {
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

impl Shell {
    pub fn start(queue: Queue, ctx: eframe::egui::Context, settings: &Settings) -> Shell {

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
            hotkeys: GlobalHotKeyManager::new().ok(),
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
        // No tray icon to swap yet: the Windows and Linux trays do it.
        let _ = self.flash.tick(now);
        self.flash.next_check(now).map(|s| std::time::Duration::from_secs_f64(s + 0.02))
    }

    /// Takes Esc for the length of a scrolling capture, or gives it back.
    pub fn grab_escape(&mut self, on: bool) {
        let Some(mgr) = &self.hotkeys else { return };
        if let Some(hk) = self.escape.take() {
            self.escape_id.store(0, Ordering::SeqCst);
            let _ = mgr.unregister(hk);
        }
        if on {
            let Ok(hk) = "Escape".parse::<HotKey>() else { return };
            if mgr.register(hk).is_ok() {
                self.escape_id.store(hk.id(), Ordering::SeqCst);
                self.escape = Some(hk);
            }
        }
    }

    /// A placeholder until the Windows and Linux trays arrive.
    pub fn sync(&mut self, _settings: &Settings) {}
}

#[cfg(test)]
mod tests {
    use super::*;

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
