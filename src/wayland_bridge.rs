//! Linux, Wayland: ShotCraft's own requests on the connection winit opened (the same `wl_display`),
//! for what winit 0.30 doesn't offer. For now: bringing the editor back with xdg-activation.
//!
//! Why: GNOME sends a minimized window no frame callbacks, winit waits for one before every redraw,
//! and ShotCraft's actions (hotkeys, links, the tray) only run in a UI pass. So a minimized editor
//! stopped ShotCraft after one more pass. A Wayland app can't un-minimize itself, but it may
//! activate its window with a token the desktop gave for a user action: GNOME's GlobalShortcuts
//! portal sends one with every hotkey press. When a hotkey's action hasn't run soon after, the
//! editor is activated with that token, GNOME brings it back, and the pass runs.

use std::sync::{Mutex, OnceLock};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;

struct Bridge {
    conn: Connection,
    activation: XdgActivationV1,
    editor: WlSurface,
    /// Our own queue on winit's connection; kept so its objects stay valid.
    _queue: Mutex<EventQueue<State>>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<XdgActivationV1, ()> for State {
    fn event(_: &mut Self, _: &XdgActivationV1, _: <XdgActivationV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// Attaches to winit's Wayland connection and finds the editor's surface. Call once, with the
/// editor's creation context; does nothing (an error) outside Wayland.
pub fn attach(cc: &eframe::CreationContext<'_>) -> Result<(), String> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    let display = cc.display_handle().map_err(|e| e.to_string())?.as_raw();
    let window = cc.window_handle().map_err(|e| e.to_string())?.as_raw();
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) = (display, window) else {
        return Err("not a Wayland window".into());
    };
    // SAFETY: winit's `wl_display`, which lives until the event loop ends, and with it ShotCraft.
    let backend = unsafe { wayland_client::backend::Backend::from_foreign_display(display.display.as_ptr().cast()) };
    let conn = Connection::from_backend(backend);
    let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let activation: XdgActivationV1 = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("no xdg-activation ({e})"))?;
    // SAFETY: the editor's `wl_surface`, created by winit for the root window, which lives as long
    // as the app.
    let id = unsafe { wayland_client::backend::ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr().cast()) }.map_err(|e| e.to_string())?;
    let editor = WlSurface::from_id(&conn, id).map_err(|e| e.to_string())?;
    queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
    BRIDGE.set(Bridge { conn, activation, editor, _queue: Mutex::new(queue) }).map_err(|_| "attached twice".to_string())
}

/// Activates the editor with `token` (from the desktop, for a user action). False when there is
/// no bridge (not Wayland, or attaching failed) or the request couldn't be sent.
pub fn activate_editor(token: &str) -> bool {
    let Some(b) = BRIDGE.get() else { return false };
    b.activation.activate(token.to_string(), &b.editor);
    b.conn.flush().is_ok()
}

/// When the editor's last UI pass started (`now_ms`).
static LAST_PASS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A UI pass starts (`App::ui`).
pub fn note_pass() {
    LAST_PASS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

pub fn last_pass_ms() -> u64 {
    LAST_PASS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Milliseconds on a monotonic clock, for `stalled`.
pub fn now_ms() -> u64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// Whether the UI is stalled: an action was queued at `queued_ms`, `waited_ms` have passed, and
/// no UI pass has started since it was queued (`last_pass_ms`).
pub fn stalled(queued_ms: u64, last_pass_ms: u64, waited_ms: u64, limit_ms: u64) -> bool {
    waited_ms >= limit_ms && last_pass_ms < queued_ms
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_queued_action_with_no_pass_since_is_a_stall_once_the_wait_is_over() {
        assert!(super::stalled(1_000, 900, 400, 400), "no pass since it was queued");
        assert!(!super::stalled(1_000, 1_050, 400, 400), "a pass ran after it was queued");
        assert!(!super::stalled(1_000, 900, 100, 400), "too soon to tell");
    }
}
