//! Linux: requests to the desktop portals (org.freedesktop.portal.*) over D-Bus, the freedesktop
//! way: listen on the request's object path, call, and wait for its Response. Used by printing
//! (`system::print_image`) and the Wayland hotkeys (`portal_hotkeys`); ashpd's typed answers
//! refused some of GNOME's, so only what's needed is read, by the caller.

use ashpd::zbus::zvariant::{DynamicType, OwnedValue, Value};
use ashpd::zbus::{Connection, Proxy};
use futures_lite::StreamExt;
use std::collections::HashMap;

pub const DESTINATION: &str = "org.freedesktop.portal.Desktop";
pub const PATH: &str = "/org/freedesktop/portal/desktop";

pub fn value<'a>(v: impl Into<Value<'a>>) -> Value<'a> {
    v.into()
}

/// A handle token: letters, digits and `_` only, unique in this process.
pub fn token(what: &str) -> String {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    format!("shotcraft_{what}_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// The request object path the portal will use for `token` (the sender's unique name without
/// its colon, dots as underscores).
pub fn request_path(unique_name: &str, token: &str) -> String {
    format!("/org/freedesktop/portal/desktop/request/{}/{token}", unique_name.trim_start_matches(':').replace('.', "_"))
}

/// Calls `method` of the portal `interface` (e.g. "org.freedesktop.portal.Print") with `body`,
/// whose options carry `handle_token: token`, and waits for the answer. `Ok(None)`: the user
/// cancelled.
pub async fn request<B>(conn: &Connection, interface: &str, token: &str, method: &str, body: &B) -> Result<Option<HashMap<String, OwnedValue>>, String>
where
    B: serde::Serialize + DynamicType,
{
    let err = |e: ashpd::zbus::Error| e.to_string();
    let name = conn.unique_name().ok_or("no name on the session bus")?.to_string();
    let path = request_path(&name, token);
    let request = Proxy::new(conn, DESTINATION, path.as_str(), "org.freedesktop.portal.Request").await.map_err(err)?;
    let mut answers = request.receive_signal("Response").await.map_err(err)?;
    let portal = Proxy::new(conn, DESTINATION, PATH, interface).await.map_err(err)?;
    portal.call_method(method, body).await.map_err(|e| format!("the desktop doesn't offer {method} ({e})"))?;
    let msg = answers.next().await.ok_or("the desktop closed the request without an answer")?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(err)?;
    match code {
        0 => Ok(Some(results)),
        1 => Ok(None),
        _ => Err(format!("the desktop refused {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_path_is_the_one_the_portal_uses_and_tokens_differ() {
        assert_eq!(request_path(":1.183", "shotcraft_prep_9_0"), "/org/freedesktop/portal/desktop/request/1_183/shotcraft_prep_9_0");
        let (a, b) = (token("x"), token("x"));
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "{a}");
    }
}
