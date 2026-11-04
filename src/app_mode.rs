//! ShotCraft lives in the menu bar: no Dock icon, and no window until the user asks for one.
//!
//! The Dock icon is removed twice: `LSUIElement` in `packaging/Info.plist`, and the accessory
//! activation policy set before the first window exists. An accessory app has no menu bar of its
//! own, so every command is also in the status item menu (`shell.rs`), and each window that opens
//! has to activate the app itself, which nobody else will do for it.
use std::path::PathBuf;

/// What the command line asks for.
#[derive(Debug, Default, PartialEq)]
pub struct Launch {
    /// `--screenshot <file>`: render one frame to a file and quit (development).
    pub screenshot: Option<PathBuf>,
    /// An image file to open in the editor.
    pub open: Option<PathBuf>,
}

impl Launch {
    pub fn parse(args: &[String]) -> Launch {
        let shot_flag = args.iter().position(|a| a == "--screenshot");
        let screenshot = shot_flag.and_then(|i| args.get(i + 1)).map(PathBuf::from);
        let open = args
            .iter()
            .enumerate()
            .skip(1)
            .find(|(i, a)| !a.starts_with('-') && shot_flag.map_or(true, |f| *i != f + 1))
            .map(|(_, a)| PathBuf::from(a));
        Launch { screenshot, open }
    }

    /// Starting the app shows a window only when it was asked to render or open something.
    /// Launching from Finder, `open`, a login item or `--background` shows the menu bar item alone.
    pub fn shows_window(&self) -> bool {
        self.screenshot.is_some() || self.open.is_some()
    }
}

/// The accessory policy, applied by winit when the app finishes launching (before any window is
/// shown). Without a window to show, the app also stays out of the way of whatever is in front.
#[cfg(target_os = "macos")]
pub fn event_loop_hook(shows_window: bool) -> eframe::EventLoopBuilderHook {
    use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
    Box::new(move |builder| {
        builder.with_activation_policy(ActivationPolicy::Accessory);
        builder.with_activate_ignoring_other_apps(shows_window);
    })
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindow};

    use crate::EDITOR_TITLE;

    fn windows(mtm: MainThreadMarker) -> Vec<Retained<NSWindow>> {
        NSApplication::sharedApplication(mtm).windows().iter().collect()
    }

    /// The editor window is hidden by making it transparent for the frames eframe still draws.
    pub fn set_editor_alpha(alpha: f64) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        for w in windows(mtm).iter().filter(|w| w.title().to_string() == EDITOR_TITLE) {
            w.setAlphaValue(alpha);
            // A transparent window must not swallow clicks meant for what is behind it.
            w.setIgnoresMouseEvents(alpha <= 0.0);
        }
    }

    /// Activates the app and puts its windows in front, the one titled `key` in focus. Needed for
    /// every window, since an accessory app is never brought forward by the system. Returns whether
    /// the window titled `key` is on screen and the app is active; windows are created a few
    /// frames after they are asked for, so the caller keeps trying until this is true. Without `focus`
    /// the window is only ordered in front: the app stays inactive and the key window stays where it is
    /// (a HUD over a browser must not take the keyboard from it).
    pub fn bring_to_front(key: &str, focus: bool) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        if focus {
            activate();
        }
        let shown: Vec<_> = windows(mtm).into_iter().filter(|w| w.isVisible() && w.alphaValue() > 0.0 && w.canBecomeKeyWindow()).collect();
        let target = shown.iter().find(|w| w.title().to_string() == key);
        if let Some(w) = target {
            w.orderFrontRegardless();
            if focus {
                w.makeKeyWindow();
            }
        }
        target.is_some() && (!focus || NSApplication::sharedApplication(mtm).isActive())
    }

    /// Activates the app, so that its windows, and the ones Sparkle opens, can come forward.
    pub fn activate() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        // macOS 14 adds `activate`, and no longer honours `activateIgnoringOtherApps` for all callers.
        let cooperative: bool = unsafe { objc2::msg_send![&*app, respondsToSelector: objc2::sel!(activate)] };
        if cooperative {
            app.activate();
        }
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

#[cfg(target_os = "macos")]
pub use mac::{activate, bring_to_front, set_editor_alpha};

#[cfg(not(target_os = "macos"))]
pub fn set_editor_alpha(_alpha: f64) {}
#[cfg(not(target_os = "macos"))]
pub fn bring_to_front(_key: &str, _focus: bool) -> bool {
    true
}
#[cfg(not(target_os = "macos"))]
pub fn activate() {}

#[cfg(test)]
mod tests {
    use super::Launch;
    use std::path::PathBuf;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_launch_shows_no_window() {
        let l = Launch::parse(&args(&["shotcraft"]));
        assert_eq!(l, Launch::default());
        assert!(!l.shows_window());
    }

    #[test]
    fn login_item_and_finder_flags_show_no_window() {
        assert!(!Launch::parse(&args(&["shotcraft", "--background"])).shows_window());
        // Old launchers add a process serial number.
        assert!(!Launch::parse(&args(&["shotcraft", "-psn_0_12345"])).shows_window());
    }

    #[test]
    fn an_image_argument_opens_the_editor() {
        let l = Launch::parse(&args(&["shotcraft", "/tmp/a.png"]));
        assert_eq!(l.open, Some(PathBuf::from("/tmp/a.png")));
        assert!(l.shows_window());
        let l = Launch::parse(&args(&["shotcraft", "--background", "/tmp/a.png"]));
        assert_eq!(l.open, Some(PathBuf::from("/tmp/a.png")));
    }

    #[test]
    fn the_screenshot_target_is_not_an_image_to_open() {
        let l = Launch::parse(&args(&["shotcraft", "--screenshot", "/tmp/out.png"]));
        assert_eq!(l.screenshot, Some(PathBuf::from("/tmp/out.png")));
        assert_eq!(l.open, None);
        assert!(l.shows_window());
    }
}
