//! Linux, Wayland: the global hotkeys through the desktop's GlobalShortcuts portal. On Wayland an
//! app can't grab keys; `global-hotkey`'s X11 grab only saw keys while an X11 window had focus, so
//! the hotkeys did nothing in practice (tested on GNOME 50: no capture from Text Editor or from
//! ShotCraft's own window, a capture only from `xev`).
//!
//! GNOME asks the user once to allow new shortcuts and then remembers them by id, keeping the key
//! it stored even when a later request suggests another. So the id carries the key
//! (`shortcut_id`): a hotkey changed in Preferences is a new shortcut, which GNOME asks about once,
//! and Preferences stays what decides the keys, as on Windows and macOS. Every change opens a new
//! session with exactly the current hotkeys; the old one is closed, so a removed hotkey stops.

use crate::portal;
use crate::settings::HotkeyAction;
use ashpd::zbus;
use futures_lite::StreamExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

/// A hotkey as ShotCraft stores it (`global-hotkey`'s format, e.g. `ctrl+shift+Digit1`) in the
/// portal's trigger format (the XDG shortcuts spec: modifiers and an XKB key name, `CTRL+SHIFT+1`).
/// `None` when the key has no name the portal would understand.
pub fn trigger(hotkey: &str) -> Option<String> {
    use global_hotkey::hotkey::{HotKey, Modifiers};
    let hk: HotKey = hotkey.parse().ok()?;
    let mut parts = vec![];
    for (m, name) in [(Modifiers::CONTROL, "CTRL"), (Modifiers::ALT, "ALT"), (Modifiers::SHIFT, "SHIFT"), (Modifiers::SUPER, "LOGO")] {
        if hk.mods.contains(m) {
            parts.push(name.to_string());
        }
    }
    parts.push(key_name(&format!("{:?}", hk.key))?);
    Some(parts.join("+"))
}

/// An XKB key name for a `Code` (by its name: `Digit1`, `KeyA`, `F5`, `ArrowUp`...).
fn key_name(code: &str) -> Option<String> {
    if let Some(d) = code.strip_prefix("Digit").filter(|d| d.len() == 1) {
        return Some(d.to_string());
    }
    if let Some(l) = code.strip_prefix("Key").filter(|l| l.len() == 1) {
        return Some(l.to_ascii_lowercase());
    }
    if code.len() >= 2 && code.starts_with('F') && code[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) {
        return Some(code.to_string());
    }
    if let Some(d) = code.strip_prefix("Numpad").filter(|d| d.len() == 1 && d.as_bytes()[0].is_ascii_digit()) {
        return Some(format!("KP_{d}"));
    }
    let name = match code {
        "Space" => "space",
        "Enter" => "Return",
        "Escape" => "Escape",
        "Tab" => "Tab",
        "Backspace" => "BackSpace",
        "Delete" => "Delete",
        "Insert" => "Insert",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "Page_Up",
        "PageDown" => "Page_Down",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "PrintScreen" => "Print",
        "Minus" => "minus",
        "Equal" => "equal",
        "BracketLeft" => "bracketleft",
        "BracketRight" => "bracketright",
        "Backslash" => "backslash",
        "Semicolon" => "semicolon",
        "Quote" => "apostrophe",
        "Comma" => "comma",
        "Period" => "period",
        "Slash" => "slash",
        "Backquote" => "grave",
        _ => return None,
    };
    Some(name.to_string())
}

/// The shortcut's id: the action and its key (letters, digits, `_` and `-`), e.g.
/// `fullscreen_ctrl-shift-1`. See the module comment for why the key is part of it.
pub fn shortcut_id(action: HotkeyAction, trigger: &str) -> String {
    let key: String = trigger.chars().map(|c| if c == '+' { '-' } else { c.to_ascii_lowercase() }).filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    format!("{}_{key}", format!("{action:?}").to_ascii_lowercase())
}

/// One hotkey as the portal gets it.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub id: String,
    pub action: HotkeyAction,
    pub trigger: String,
    pub description: String,
}

/// The bindings for `hotkeys` (action, stored hotkey), and the actions whose key the portal
/// can't take.
pub fn bindings(hotkeys: &[(HotkeyAction, String)], label: impl Fn(HotkeyAction) -> String) -> (Vec<Binding>, Vec<HotkeyAction>) {
    let mut out = vec![];
    let mut failed = vec![];
    for (action, hotkey) in hotkeys {
        match trigger(hotkey) {
            Some(t) => out.push(Binding { id: shortcut_id(*action, &t), action: *action, description: label(*action), trigger: t }),
            None => failed.push(*action),
        }
    }
    (out, failed)
}

/// The running portal session. Each `apply` starts a new one and retires the last: its thread
/// closes its session and ends at the next check.
pub struct PortalHotkeys {
    generation: Arc<AtomicU64>,
}

impl PortalHotkeys {
    pub fn new() -> Self {
        PortalHotkeys { generation: Arc::default() }
    }

    /// Binds `bindings` (none: just retires the current session). `hit` runs on the portal's
    /// thread for each press.
    pub fn apply(&self, bindings: Vec<Binding>, hit: impl Fn(HotkeyAction) + Send + 'static) {
        let mine = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if bindings.is_empty() {
            return;
        }
        let generation = self.generation.clone();
        std::thread::spawn(move || {
            if let Err(e) = async_io::block_on(run(bindings, generation, mine, hit)) {
                eprintln!("hotkeys: {e}");
            }
        });
    }
}

async fn run(bindings: Vec<Binding>, generation: Arc<AtomicU64>, mine: u64, hit: impl Fn(HotkeyAction)) -> Result<(), String> {
    let err = |e: zbus::Error| e.to_string();
    let conn = zbus::Connection::session().await.map_err(err)?;
    let token = portal::token("keys");
    let session_token = portal::token("keysession");
    let created = portal::request(&conn, INTERFACE, &token, "CreateSession", &(std::collections::HashMap::from([("handle_token", portal::value(token.clone())), ("session_handle_token", portal::value(session_token))]),)).await?;
    let session: String = created.as_ref().and_then(|r| r.get("session_handle")).and_then(|v| v.downcast_ref::<String>().ok()).ok_or("the desktop gave no hotkey session")?;
    let session_path = zbus::zvariant::ObjectPath::try_from(session.as_str()).map_err(|e| e.to_string())?;
    let shortcuts: Vec<(String, std::collections::HashMap<&str, zbus::zvariant::Value>)> = bindings
        .iter()
        .map(|b| (b.id.clone(), std::collections::HashMap::from([("description", portal::value(b.description.clone())), ("preferred_trigger", portal::value(b.trigger.clone()))])))
        .collect();
    let portal_proxy = zbus::Proxy::new(&conn, portal::DESTINATION, portal::PATH, INTERFACE).await.map_err(err)?;
    let mut presses = portal_proxy.receive_signal("Activated").await.map_err(err)?;
    let token = portal::token("bind");
    let bound = portal::request(&conn, INTERFACE, &token, "BindShortcuts", &(&session_path, shortcuts, "", std::collections::HashMap::from([("handle_token", portal::value(token.clone()))]))).await;
    let close = || async {
        if let Ok(s) = zbus::Proxy::new(&conn, portal::DESTINATION, session.as_str(), "org.freedesktop.portal.Session").await {
            let _ = s.call_method("Close", &()).await;
        }
    };
    if bound.as_ref().map_or(true, |b| b.is_none()) {
        close().await;
        return bound.map(|_| ());
    }
    loop {
        let next = futures_lite::future::or(async { presses.next().await.map(Some) }, async {
            async_io::Timer::after(std::time::Duration::from_secs(1)).await;
            Some(None)
        })
        .await;
        if generation.load(Ordering::SeqCst) != mine {
            close().await;
            return Ok(());
        }
        let msg = match next {
            Some(Some(msg)) => msg,
            Some(None) => continue,
            None => return Err("the desktop stopped sending hotkeys".into()),
        };
        let Ok((from, id, _, _)) = msg.body().deserialize::<(zbus::zvariant::OwnedObjectPath, String, u64, std::collections::HashMap<String, zbus::zvariant::OwnedValue>)>() else { continue };
        if from.as_str() != session {
            continue;
        }
        if let Some(b) = bindings.iter().find(|b| b.id == id) {
            hit(b.action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_hotkeys_become_portal_triggers() {
        assert_eq!(trigger("ctrl+shift+Digit1").as_deref(), Some("CTRL+SHIFT+1"));
        assert_eq!(trigger("alt+KeyS").as_deref(), Some("ALT+s"));
        assert_eq!(trigger("super+shift+F12").as_deref(), Some("SHIFT+LOGO+F12"));
        assert_eq!(trigger("ctrl+PrintScreen").as_deref(), Some("CTRL+Print"));
        assert_eq!(trigger("shift+ctrl+ArrowUp").as_deref(), Some("CTRL+SHIFT+Up"), "modifiers in one order");
        assert_eq!(trigger("not a key"), None);
    }

    #[test]
    fn the_id_carries_the_action_and_the_key() {
        assert_eq!(shortcut_id(HotkeyAction::Fullscreen, "CTRL+SHIFT+1"), "fullscreen_ctrl-shift-1");
        assert_eq!(shortcut_id(HotkeyAction::ActiveWindow, "ALT+s"), "activewindow_alt-s");
        assert_ne!(shortcut_id(HotkeyAction::Area, "CTRL+SHIFT+2"), shortcut_id(HotkeyAction::Area, "CTRL+SHIFT+3"), "a changed key is a new shortcut");
    }

    #[test]
    fn keys_the_portal_cant_name_are_reported_not_bound() {
        let hotkeys = vec![(HotkeyAction::Fullscreen, "ctrl+shift+Digit1".to_string()), (HotkeyAction::Ocr, "ctrl+shift+IntlRo".to_string())];
        let (b, failed) = bindings(&hotkeys, |a| format!("{a:?}"));
        assert_eq!(b.len(), 1);
        assert_eq!(b[0], Binding { id: "fullscreen_ctrl-shift-1".into(), action: HotkeyAction::Fullscreen, trigger: "CTRL+SHIFT+1".into(), description: "Fullscreen".into() });
        assert_eq!(failed, vec![HotkeyAction::Ocr]);
    }
}
