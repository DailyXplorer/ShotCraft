//! Linux (X11): bringing the editor to the front after something the user just did (a capture that
//! ends, a tray click). The polite request winit sends (`_NET_ACTIVE_WINDOW` from an application)
//! is turned down by a window manager's focus-stealing protection when another app has the focus,
//! and the editor then stayed behind it. A taskbar asks the same way as a "pager", which window
//! managers always honour; this is only used for what the user's own action caused.
//!
//! The focus can still move for a moment after the editor is shown (the crosshair windows and the
//! helper window close, and the window manager hands the focus back to the app that had it), so
//! the request is repeated, for a short time, while the editor is not the active window.

use std::sync::OnceLock;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask};
use x11rb::rust_connection::RustConnection;

/// The editor's X window, remembered at start-up.
static EDITOR: OnceLock<u32> = OnceLock::new();

/// Source indication 2 of `_NET_ACTIVE_WINDOW`: a pager.
const SOURCE_PAGER: u32 = 2;

/// How long after the editor is shown the request is repeated, and how often it is checked.
const KEEP_TRYING_MS: u64 = 1500;
const CHECK_EVERY_MS: u64 = 150;
/// The window has to be mapped first (it was hidden for the capture).
const FIRST_AFTER_MS: u64 = 450;

/// Remembers the editor's window. Call once, with the editor's creation context; does nothing
/// outside X11.
pub fn remember(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = cc.window_handle() else { return };
    let id = match handle.as_raw() {
        RawWindowHandle::Xlib(x) => x.window as u32,
        RawWindowHandle::Xcb(x) => x.window.get(),
        _ => return,
    };
    let _ = EDITOR.set(id);
}

/// The `data` of the client message that asks for `window` to be activated while `active` is.
pub fn activate_data(active: u32) -> [u32; 5] {
    [SOURCE_PAGER, 0, active, 0, 0]
}

/// Whether to ask again: the editor is not the active window, and the time is not up.
pub fn should_ask(editor: u32, active: Option<u32>, waited_ms: u64) -> bool {
    waited_ms < KEEP_TRYING_MS && active != Some(editor)
}

/// Brings the editor to the front and keeps it there for a moment.
pub fn activate_editor_soon() {
    let Some(&editor) = EDITOR.get() else { return };
    std::thread::spawn(move || {
        let Ok((conn, n)) = x11rb::connect(None) else { return };
        let Some(root) = conn.setup().roots.get(n).map(|s| s.root) else { return };
        let (Some(request), Some(property)) = (atom(&conn, b"_NET_ACTIVE_WINDOW"), atom(&conn, b"_NET_ACTIVE_WINDOW")) else { return };
        std::thread::sleep(std::time::Duration::from_millis(FIRST_AFTER_MS));
        let mut waited = FIRST_AFTER_MS;
        while waited < KEEP_TRYING_MS {
            let active = active_window(&conn, root, property);
            if should_ask(editor, active, waited) {
                let event = ClientMessageEvent::new(32, editor, request, activate_data(active.unwrap_or(0)));
                let sent = conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event);
                if sent.is_err() || conn.flush().is_err() {
                    return;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(CHECK_EVERY_MS));
            waited += CHECK_EVERY_MS;
        }
    });
}

fn atom(conn: &RustConnection, name: &[u8]) -> Option<u32> {
    Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
}

/// The window the window manager says is active.
fn active_window(conn: &RustConnection, root: u32, property: u32) -> Option<u32> {
    let reply = conn.get_property(false, root, property, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
    let first = reply.value32()?.next();
    first.filter(|&w| w != 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_request_comes_from_a_pager_and_names_the_active_window() {
        assert_eq!(super::activate_data(77), [2, 0, 77, 0, 0]);
    }

    #[test]
    fn it_asks_again_only_while_the_editor_is_not_active_and_the_time_is_not_up() {
        assert!(super::should_ask(5, Some(9), 100), "another window is active");
        assert!(super::should_ask(5, None, 100), "none is");
        assert!(!super::should_ask(5, Some(5), 100), "the editor is");
        assert!(!super::should_ask(5, Some(9), super::KEEP_TRYING_MS), "time is up");
    }
}
