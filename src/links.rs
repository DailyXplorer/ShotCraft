//! Windows and Linux: how `shotcraft://` links reach the app (macOS: Apple events, `url_scheme`).
//!
//! The system hands a link to a program by starting it with the link as its argument, so the link
//! type has to be registered for this user (not yet: the Windows and Linux backends do it). macOS
//! registers it in the app bundle;
//! a portable exe has no installer to do it, so ShotCraft registers it itself, and only while
//! Preferences ▸ Advanced ▸ Automation allows links. Switching that off removes the registration.
//!
//! What a link may do is `url_scheme`'s to decide, the same on every system. A copy of ShotCraft
//! that is already running gets the link from the new launch (`single`).

/// A link starts with this, in any case (`ShotCraft://` works too, as on macOS).
const PREFIX: &str = "shotcraft:";

/// The launch's arguments without the link, and the link.
///
/// A launch with a link is only that: every other argument is dropped. Windows builds the command
/// line from `"%1"`, so a link holding a quote would otherwise add options of its own
/// (`--screenshot`, a file to open).
pub fn split_args(args: Vec<String>) -> (Vec<String>, Option<String>) {
    let is_link = |a: &String| a.get(..PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(PREFIX));
    match args.iter().skip(1).find(|a| is_link(a)).cloned() {
        Some(link) => (args.into_iter().take(1).collect(), Some(link)),
        None => (args, None),
    }
}

/// Keeps the registration on this copy of ShotCraft while links are allowed, wherever it was
/// switched on from. A copy run from cargo's target folder (a development build) leaves it alone.
pub fn refresh(allowed: bool) {
    let dev_build = std::env::current_exe().is_ok_and(|e| e.components().any(|c| c.as_os_str() == "target"));
    if allowed && !dev_build {
        set_registered(true);
    }
}

/// A placeholder until the Windows and Linux backends arrive: links aren't registered yet.
#[cfg(not(target_os = "macos"))]
pub fn set_registered(_on: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_launch_without_a_link_keeps_its_arguments() {
        let a = args(&["shotcraft", "--background"]);
        assert_eq!(split_args(a.clone()), (a, None));
        let a = args(&["shotcraft", "C:\\shots\\a.png"]);
        assert_eq!(split_args(a.clone()), (a, None));
        assert_eq!(split_args(args(&["shotcraft"])), (args(&["shotcraft"]), None));
    }

    #[test]
    fn a_link_launch_is_only_the_link() {
        let (rest, link) = split_args(args(&["shotcraft", "shotcraft://grab/area"]));
        assert_eq!((rest, link.as_deref()), (args(&["shotcraft"]), Some("shotcraft://grab/area")));
        // Any case, as the system passes it on.
        assert_eq!(split_args(args(&["shotcraft", "ShotCraft://show"])).1.as_deref(), Some("ShotCraft://show"));
        // What a link with a quote turns into on Windows: the extra arguments it smuggled in are dropped.
        let (rest, link) = split_args(args(&["shotcraft", "shotcraft://show", "--screenshot", "C:\\x.png", "C:\\y.png"]));
        assert_eq!((rest, link.as_deref()), (args(&["shotcraft"]), Some("shotcraft://show")));
        // The program's own name is never taken for a link.
        assert_eq!(split_args(args(&["shotcraft:"])).1, None);
    }
}
