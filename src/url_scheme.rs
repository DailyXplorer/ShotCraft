//! The `shotcraft://` URL scheme: other apps (Shortcuts, Raycast, Alfred, a shell with `open`)
//! start a capture or open a picture with a link.
//!
//! This file holds the part that decides what a link means. It is pure: a link goes in, a `Route`
//! (or the reason it is refused) comes out, so every rule is tested without a window. The
//! operating system hands the links over in `mac` (an Apple event handler), and the app runs the
//! routes in `App::handle_url` after the user has allowed links.
//!
//! Rules that keep it safe:
//! * Only the routes below exist. Anything else is dropped, never guessed.
//! * Nothing a link carries is executed or used as a command line. The only data a link can
//!   carry is the path of a picture, and that path must be absolute, free of `..`, and name an
//!   image file by its extension; the app checks the file itself before it reads it.
//! * Links are off until the user allows them (a preference, asked for on the first link).
//!
//! Routes (the path is case-insensitive, a trailing `/` is ignored):
//!
//! | Link | Does |
//! |---|---|
//! | `show` | opens the editor window |
//! | `grab/area` | Area Capture |
//! | `grab/fullscreen` | Fullscreen Screenshot |
//! | `grab/window` | Any Window Screenshot |
//! | `grab/window/active` | Active Window Screenshot |
//! | `grab/repeat` | Repeat Area Capture |
//! | `grab/scrolling` | Scrolling Capture |
//! | `grab/scrolling/reverse` | Scrolling Capture (Up) |
//! | `grab/delayed` | Delayed Screenshot, waiting as long as the preference says |
//! | `grab/delayed=N` | Delayed Screenshot waiting N seconds (1 to 60) |
//! | `grab/alldisplays` | All Displays Screenshot |
//! | `grab/append` | Add Capture to the open picture |
//! | `ocr` | Instant text recognition of an area |
//! | `load/clipboard` | opens the picture on the clipboard |
//! | `load/file?path=/path/to/image.png` | opens an image file (percent-encode the path) |
//! | `settings` | opens Preferences |
use crate::Action;
use crate::system::{CaptureMode, OPEN_EXTENSIONS};
use eframe::egui;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The scheme, as registered in `packaging/Info.plist`.
pub const SCHEME: &str = "shotcraft";

/// Longest link taken into account, in bytes: nobody types more, and a bigger one is not ours.
const MAX_URL: usize = 4096;
/// Longest picture path accepted, in bytes (macOS limits a path to 1024).
const MAX_PATH: usize = 1024;
/// A picture bigger than this is not opened through a link.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Longest wait `grab/delayed=N` accepts, in seconds (the same limit as the preference).
const MAX_DELAY: u32 = 60;
/// Links waiting for the app, at most: a loop in another app can't pile up work.
const MAX_PENDING: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Show,
    Settings,
    Ocr,
    Area,
    Fullscreen,
    Window,
    ActiveWindow,
    Repeat,
    Scrolling,
    ScrollingUp,
    /// Delayed Screenshot; `None` waits as long as the preference says.
    Delayed(Option<u32>),
    AllDisplays,
    Append,
    LoadClipboard,
    LoadFile(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    /// Not a `shotcraft:` link.
    WrongScheme,
    /// Empty, too long, or carrying control characters.
    Malformed,
    /// A well-formed link whose route does not exist.
    UnknownRoute(String),
    /// `load/file` without a usable picture path.
    BadPath(&'static str),
}

impl std::fmt::Display for UrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UrlError::WrongScheme => write!(f, "not a {SCHEME}:// link"),
            UrlError::Malformed => write!(f, "malformed link"),
            UrlError::UnknownRoute(r) => write!(f, "unknown command \"{r}\""),
            UrlError::BadPath(why) => write!(f, "{why}"),
        }
    }
}

/// What a link asks for, or why it is refused.
pub fn parse(url: &str) -> Result<Route, UrlError> {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_URL || url.chars().any(char::is_control) {
        return Err(UrlError::Malformed);
    }
    let (scheme, rest) = url.split_once(':').ok_or(UrlError::WrongScheme)?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return Err(UrlError::WrongScheme);
    }
    // Links are written `shotcraft://grab/area`: the first word is the "host" for the system and
    // the first word of the route for us. A missing `//` and extra slashes mean the same.
    let rest = rest.split('#').next().unwrap_or("");
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let path = path.trim_matches('/').to_ascii_lowercase();
    // The route names have no escapes: a `%` in the path is never a route.
    let segments: Vec<&str> = if path.is_empty() { vec![] } else { path.split('/').collect() };
    let unknown = || UrlError::UnknownRoute(path.chars().take(60).collect());
    Ok(match segments.as_slice() {
        ["show"] => Route::Show,
        ["settings"] => Route::Settings,
        ["ocr"] => Route::Ocr,
        ["load", "clipboard"] => Route::LoadClipboard,
        ["load", "file"] => Route::LoadFile(image_path(&query_value(query, "path").ok_or(UrlError::BadPath("load/file needs ?path="))?)?),
        ["grab", what @ ..] => match what {
            ["area"] => Route::Area,
            ["fullscreen"] => Route::Fullscreen,
            ["window"] => Route::Window,
            ["window", "active"] => Route::ActiveWindow,
            ["repeat"] => Route::Repeat,
            ["scrolling"] => Route::Scrolling,
            ["scrolling", "reverse"] => Route::ScrollingUp,
            ["delayed"] => Route::Delayed(None),
            [d] if d.starts_with("delayed=") => match d["delayed=".len()..].parse::<u32>() {
                Ok(n) if (1..=MAX_DELAY).contains(&n) => Route::Delayed(Some(n)),
                _ => return Err(unknown()),
            },
            ["alldisplays"] => Route::AllDisplays,
            ["append"] => Route::Append,
            _ => return Err(unknown()),
        },
        _ => return Err(unknown()),
    })
}

/// The decoded value of the first `key=` in a query string.
fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k.eq_ignore_ascii_case(key)).then(|| percent_decode(v)).flatten()
    })
}

/// `%XX` escapes to bytes; `None` when an escape is broken or the result isn't text. (`+` stays
/// a plus: it is a legal character of a file name.)
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The path of a picture, if it is acceptable: absolute, no `..`, no control characters, and the
/// name of an image file.
fn image_path(text: &str) -> Result<PathBuf, UrlError> {
    if text.is_empty() || text.len() > MAX_PATH || text.chars().any(char::is_control) {
        return Err(UrlError::BadPath("the path is empty, too long or not plain text"));
    }
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err(UrlError::BadPath("the path must be absolute"));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(UrlError::BadPath("the path must not contain .."));
    }
    if !is_image_name(&path) {
        return Err(UrlError::BadPath("only image files can be opened"));
    }
    Ok(path)
}

fn is_image_name(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| OPEN_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Checks the file a link names before the app reads it: after links and symlinks are resolved it
/// must still be an image by name, a regular file (never a device or a pipe) and not huge.
/// Returns the resolved path to open.
pub fn check_file(path: &Path) -> Result<PathBuf, String> {
    let real = path.canonicalize().map_err(|_| "The file does not exist".to_string())?;
    if !is_image_name(&real) {
        return Err("Only image files can be opened".into());
    }
    let meta = std::fs::metadata(&real).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("That is not a file".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("The file is too big".into());
    }
    Ok(real)
}

impl Route {
    /// The app command of the routes that are exactly one: `None` for the two that carry data
    /// (a wait in seconds, a file).
    pub fn simple_action(&self) -> Option<Action> {
        Some(match self {
            Route::Show => Action::Show,
            Route::Settings => Action::Preferences,
            Route::Ocr => Action::InstantOcr,
            Route::Area => Action::Capture(CaptureMode::Area),
            Route::Fullscreen => Action::Capture(CaptureMode::Fullscreen),
            Route::Window => Action::Capture(CaptureMode::Window),
            Route::ActiveWindow => Action::Capture(CaptureMode::ActiveWindow),
            Route::Repeat => Action::Capture(CaptureMode::RepeatArea),
            Route::Scrolling => Action::ScrollingCapture,
            Route::ScrollingUp => Action::ScrollingCaptureUp,
            Route::Delayed(None) => Action::Capture(CaptureMode::Delayed),
            Route::AllDisplays => Action::Capture(CaptureMode::AllDisplays),
            Route::Append => Action::AddCapture,
            Route::LoadClipboard => Action::OpenClipboard,
            Route::Delayed(Some(_)) | Route::LoadFile(_) => return None,
        })
    }
}

// ---- hand-over from the operating system to the app ----

static INBOX: Mutex<Vec<Route>> = Mutex::new(Vec::new());
static WAKE: OnceLock<egui::Context> = OnceLock::new();

/// A link reached the process (from any thread). Refused links are logged and dropped.
pub fn submit(url: &str) {
    match parse(url) {
        Ok(route) => {
            {
                let mut inbox = INBOX.lock().unwrap();
                if inbox.len() >= MAX_PENDING {
                    return;
                }
                inbox.push(route);
            }
            if let Some(ctx) = WAKE.get() {
                ctx.request_repaint();
            }
        }
        Err(e) => eprintln!("{SCHEME}:// link ignored: {e}"),
    }
}

/// The links that arrived since the last call, oldest first.
pub fn take() -> Vec<Route> {
    std::mem::take(&mut *INBOX.lock().unwrap())
}

/// Lets a link that arrives while the app sleeps wake it up. Links that came earlier (the one
/// that launched the app) are already waiting in the inbox.
pub fn attach(ctx: &egui::Context) {
    let _ = WAKE.set(ctx.clone());
    if !INBOX.lock().unwrap().is_empty() {
        ctx.request_repaint();
    }
}

/// Starts listening for links. Call on the main thread, before the event loop runs so the link
/// that launches the app is not lost, and again once the app is up in case the system's own
/// start-up replaced the handler. Calling it twice is harmless.
pub fn install() {
    #[cfg(target_os = "macos")]
    mac::install();
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

    /// 'GURL': the class and the id of the "open this URL" Apple event.
    const GURL: u32 = 0x4755_524C;
    /// '----': the keyword of the event's main parameter, here the link.
    const KEY_DIRECT_OBJECT: u32 = 0x2D2D_2D2D;

    define_class!(
        /// Receives the "open URL" Apple events the system sends for `shotcraft://` links.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "ShotCraftUrlHandler"]
        struct UrlHandler;

        unsafe impl NSObjectProtocol for UrlHandler {}

        impl UrlHandler {
            #[unsafe(method(handleUrlEvent:withReplyEvent:))]
            fn handle_url_event(&self, event: &AnyObject, _reply: &AnyObject) {
                let descriptor: Option<Retained<AnyObject>> = unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
                let text: Option<Retained<NSString>> = descriptor.and_then(|d| unsafe { msg_send![&*d, stringValue] });
                if let Some(text) = text {
                    super::submit(&text.to_string());
                }
            }
        }
    );

    thread_local! {
        /// The manager doesn't retain its handler: this one lives as long as the process.
        static HANDLER: std::cell::OnceCell<Retained<UrlHandler>> = const { std::cell::OnceCell::new() };
    }

    impl UrlHandler {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = mtm.alloc::<Self>().set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let handler = HANDLER.with(|h| h.get_or_init(|| UrlHandler::new(mtm)).clone());
        let manager: Retained<AnyObject> = unsafe { msg_send![objc2::class!(NSAppleEventManager), sharedAppleEventManager] };
        unsafe {
            let _: () = msg_send![&*manager, setEventHandler: &*handler, andSelector: sel!(handleUrlEvent:withReplyEvent:), forEventClass: GURL, andEventID: GURL];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(url: &str) -> Route {
        parse(url).unwrap_or_else(|e| panic!("{url}: {e}"))
    }

    #[test]
    // Its file route uses a Unix path, which is not absolute on Windows.
    #[cfg(unix)]
    fn every_route_has_its_link() {
        let table = [
            ("shotcraft://show", Route::Show),
            ("shotcraft://settings", Route::Settings),
            ("shotcraft://ocr", Route::Ocr),
            ("shotcraft://grab/area", Route::Area),
            ("shotcraft://grab/fullscreen", Route::Fullscreen),
            ("shotcraft://grab/window", Route::Window),
            ("shotcraft://grab/window/active", Route::ActiveWindow),
            ("shotcraft://grab/repeat", Route::Repeat),
            ("shotcraft://grab/scrolling", Route::Scrolling),
            ("shotcraft://grab/scrolling/reverse", Route::ScrollingUp),
            ("shotcraft://grab/delayed", Route::Delayed(None)),
            ("shotcraft://grab/delayed=5", Route::Delayed(Some(5))),
            ("shotcraft://grab/alldisplays", Route::AllDisplays),
            ("shotcraft://grab/append", Route::Append),
            ("shotcraft://load/clipboard", Route::LoadClipboard),
            ("shotcraft://load/file?path=/tmp/a.png", Route::LoadFile("/tmp/a.png".into())),
        ];
        for (url, want) in table {
            assert_eq!(route(url), want, "{url}");
        }
    }

    #[test]
    fn spelling_variants_mean_the_same() {
        for url in ["shotcraft://grab/area", "SHOTCRAFT://Grab/Area", "shotcraft:grab/area", "shotcraft:///grab/area", "shotcraft://grab/area/", "  shotcraft://grab/area\n", "shotcraft://grab/area#top", "shotcraft://grab/area?x=1"] {
            assert_eq!(route(url), Route::Area, "{url}");
        }
    }

    #[test]
    fn the_wait_of_a_delayed_screenshot_is_one_to_sixty_seconds() {
        assert_eq!(route("shotcraft://grab/delayed=1"), Route::Delayed(Some(1)));
        assert_eq!(route("shotcraft://grab/delayed=60"), Route::Delayed(Some(60)));
        for bad in ["delayed=0", "delayed=61", "delayed=-3", "delayed=", "delayed=abc", "delayed=3.5", "delayed=99999999999"] {
            assert!(matches!(parse(&format!("shotcraft://grab/{bad}")), Err(UrlError::UnknownRoute(_))), "{bad}");
        }
    }

    #[test]
    fn unknown_routes_are_refused_not_guessed() {
        for url in [
            "shotcraft://",
            "shotcraft://grab",
            "shotcraft://grab/",
            "shotcraft://grab/area/extra",
            "shotcraft://grab/scrolling/up",
            "shotcraft://quit",
            "shotcraft://run?cmd=ls",
            "shotcraft://settings/library/enable",
            "shotcraft://load",
            "shotcraft://load/url",
            "shotcraft://grab%2Farea",
            "shotcraft://gra%62/area",
            "shotcraft://../grab/area",
            "shotcraft://grab/area/../ocr",
            "shotcraft://shotcraft://grab/area",
        ] {
            assert!(matches!(parse(url), Err(UrlError::UnknownRoute(_))), "{url} -> {:?}", parse(url));
        }
    }

    #[test]
    fn other_schemes_and_junk_are_refused() {
        assert_eq!(parse("https://grab/area"), Err(UrlError::WrongScheme));
        assert_eq!(parse("file:///tmp/a.png"), Err(UrlError::WrongScheme));
        assert_eq!(parse("grab/area"), Err(UrlError::WrongScheme));
        assert_eq!(parse("shotcraftx://grab/area"), Err(UrlError::WrongScheme));
        assert_eq!(parse(""), Err(UrlError::Malformed));
        assert_eq!(parse("   "), Err(UrlError::Malformed));
        assert_eq!(parse("shotcraft://grab/ar\0ea"), Err(UrlError::Malformed));
        assert_eq!(parse("shotcraft://grab/\u{1b}area"), Err(UrlError::Malformed));
        let long = format!("shotcraft://load/file?path=/tmp/{}.png", "a".repeat(5000));
        assert_eq!(parse(&long), Err(UrlError::Malformed));
    }

    #[test]
    // Unix paths (/home/...): a Windows path starts with a drive, and Windows hands no links to the app yet.
    #[cfg(unix)]
    fn a_picture_path_is_decoded_and_must_name_an_image() {
        assert_eq!(route("shotcraft://load/file?path=/home/me/My%20Shots/a%2Bb+c.PNG"), Route::LoadFile("/home/me/My Shots/a+b+c.PNG".into()));
        assert_eq!(route("shotcraft://load/file?x=1&path=%2Ftmp%2Fé.jpeg"), Route::LoadFile("/tmp/é.jpeg".into()));
        assert_eq!(route("shotcraft://load/file?PATH=/tmp/a.webp&path=/tmp/b.png"), Route::LoadFile("/tmp/a.webp".into()));
        for ext in OPEN_EXTENSIONS {
            assert!(parse(&format!("shotcraft://load/file?path=/tmp/a.{ext}")).is_ok(), "{ext}");
        }
    }

    #[test]
    fn a_path_that_is_not_a_plain_absolute_image_is_refused() {
        let refused = |url: &str| matches!(parse(url), Err(UrlError::BadPath(_)));
        assert!(refused("shotcraft://load/file"));
        assert!(refused("shotcraft://load/file?path="));
        assert!(refused("shotcraft://load/file?other=/tmp/a.png"));
        // Not absolute.
        assert!(refused("shotcraft://load/file?path=a.png"));
        assert!(refused("shotcraft://load/file?path=~/a.png"));
        assert!(refused("shotcraft://load/file?path=file:///tmp/a.png"));
        // Climbing out.
        assert!(refused("shotcraft://load/file?path=/tmp/../etc/a.png"));
        assert!(refused("shotcraft://load/file?path=/tmp/%2e%2e/a.png"));
        // Not an image, or not by name.
        assert!(refused("shotcraft://load/file?path=/etc/passwd"));
        assert!(refused("shotcraft://load/file?path=/tmp/a.png.sh"));
        assert!(refused("shotcraft://load/file?path=/tmp/script.command"));
        assert!(refused("shotcraft://load/file?path=/tmp/png"));
        assert!(refused("shotcraft://load/file?path=/tmp/.png"));
        // Broken escapes, bytes that are not text, control characters.
        assert!(refused("shotcraft://load/file?path=/tmp/a%zz.png"));
        assert!(refused("shotcraft://load/file?path=/tmp/a%.png"));
        assert!(refused("shotcraft://load/file?path=/tmp/%ff.png"));
        assert!(refused("shotcraft://load/file?path=/tmp/a%00.png"));
        assert!(refused("shotcraft://load/file?path=/tmp/a%0a.png"));
        let long = format!("shotcraft://load/file?path=/{}.png", "a".repeat(1100));
        assert!(refused(&long));
    }

    #[test]
    fn a_route_is_an_app_command_unless_it_carries_data() {
        assert!(matches!(Route::Show.simple_action(), Some(Action::Show)));
        assert!(matches!(Route::Settings.simple_action(), Some(Action::Preferences)));
        assert!(matches!(Route::Ocr.simple_action(), Some(Action::InstantOcr)));
        assert!(matches!(Route::Area.simple_action(), Some(Action::Capture(CaptureMode::Area))));
        assert!(matches!(Route::Fullscreen.simple_action(), Some(Action::Capture(CaptureMode::Fullscreen))));
        assert!(matches!(Route::Window.simple_action(), Some(Action::Capture(CaptureMode::Window))));
        assert!(matches!(Route::ActiveWindow.simple_action(), Some(Action::Capture(CaptureMode::ActiveWindow))));
        assert!(matches!(Route::Repeat.simple_action(), Some(Action::Capture(CaptureMode::RepeatArea))));
        assert!(matches!(Route::Scrolling.simple_action(), Some(Action::ScrollingCapture)));
        assert!(matches!(Route::ScrollingUp.simple_action(), Some(Action::ScrollingCaptureUp)));
        assert!(matches!(Route::Delayed(None).simple_action(), Some(Action::Capture(CaptureMode::Delayed))));
        assert!(matches!(Route::AllDisplays.simple_action(), Some(Action::Capture(CaptureMode::AllDisplays))));
        assert!(matches!(Route::Append.simple_action(), Some(Action::AddCapture)));
        assert!(matches!(Route::LoadClipboard.simple_action(), Some(Action::OpenClipboard)));
        assert!(Route::Delayed(Some(5)).simple_action().is_none());
        assert!(Route::LoadFile("/tmp/a.png".into()).simple_action().is_none());
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shotcraft-url-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_file_is_checked_again_before_it_is_read() {
        let dir = scratch("check");
        let png = dir.join("a.png");
        std::fs::write(&png, b"x").unwrap();
        assert_eq!(check_file(&png).unwrap(), png.canonicalize().unwrap());
        // Missing, a folder with an image name, not an image.
        assert!(check_file(&dir.join("missing.png")).is_err());
        std::fs::create_dir(dir.join("d.png")).unwrap();
        assert!(check_file(&dir.join("d.png")).is_err());
        let txt = dir.join("secret.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(check_file(&txt).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_non_image_through_a_symlink_is_refused() {
        let dir = scratch("symlink");
        let secret = dir.join("secret.txt");
        std::fs::write(&secret, b"x").unwrap();
        let disguised = dir.join("looks-like.png");
        std::os::unix::fs::symlink(&secret, &disguised).unwrap();
        assert!(check_file(&disguised).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_wait_in_the_inbox_in_order_and_the_inbox_is_bounded() {
        let _ = take();
        submit("shotcraft://grab/area");
        submit("shotcraft://nonsense");
        submit("https://example.com");
        submit("shotcraft://ocr");
        assert_eq!(take(), vec![Route::Area, Route::Ocr]);
        assert!(take().is_empty());
        for _ in 0..50 {
            submit("shotcraft://grab/fullscreen");
        }
        assert_eq!(take().len(), MAX_PENDING);
    }
}
