use crate::model::{Image, R};
use crate::settings::WindowBg;
use crate::targets::{self, Snapshot, Target};
use crate::windowbg;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureMode {
    Fullscreen,
    Area,
    RepeatArea,
    Window,
    ActiveWindow,
    AllDisplays,
    Delayed,
}

pub struct Captured {
    pub img: Image,
    pub scale: f32,
    pub area: Option<R>,
    pub mode: CaptureMode,
}

pub fn temp_png(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("shotcraft");
    let _ = std::fs::create_dir_all(&dir);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    dir.join(format!("{tag}-{t}.png"))
}

/// File extensions the Open dialog offers. Each one needs its decoder enabled on the `image` crate in `Cargo.toml`.
pub const OPEN_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp"];

pub fn load_image_file(path: &Path) -> Result<Image, String> {
    let img = image::open(path).map_err(|e| format!("Something went wrong. Is the image file okay? ({e})"))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(Image { w, h, px: Arc::new(rgba.into_raw()) })
}

/// Read the capture rectangle macOS stores on the file (kMDItemScreenCaptureGlobalRect).
pub(crate) fn capture_rect(path: &Path) -> Option<R> {
    let out = Command::new("xattr").args(["-px", "com.apple.metadata:kMDItemScreenCaptureGlobalRect"]).arg(path).output().ok()?;
    let hexs: String = String::from_utf8_lossy(&out.stdout).split_whitespace().collect();
    let bytes: Vec<u8> = (0..hexs.len() / 2).filter_map(|i| u8::from_str_radix(&hexs[i * 2..i * 2 + 2], 16).ok()).collect();
    let tmp = temp_png("rect").with_extension("plist");
    std::fs::write(&tmp, &bytes).ok()?;
    let json = Command::new("plutil").args(["-convert", "json", "-o", "-"]).arg(&tmp).output().ok()?;
    let _ = std::fs::remove_file(&tmp);
    let v: Vec<f32> = serde_json::from_slice(&json.stdout).ok()?;
    if v.len() == 4 {
        Some(R { x0: v[0], y0: v[1], x1: v[0] + v[2], y1: v[1] + v[3] })
    } else {
        None
    }
}

pub struct CaptureOptions {
    pub last_area: Option<R>,
    /// The look of Active Window and Any Window captures.
    pub window_bg: WindowBg,
    /// The colour of `WindowBg::Solid`, and what Wallpaper falls back to.
    pub solid_color: [u8; 3],
    pub scale: f32,
    /// What the screen looked like when the capture was asked for.
    pub snapshot: Snapshot,
}

/// Runs `screencapture` with `args` and reads the picture back. `Ok(None)` means the user cancelled.
/// `from_file` runs while the file still exists, for what macOS stores on it.
fn shoot<T>(args: &[String], from_file: impl FnOnce(&Path) -> T) -> Result<Option<(Image, T)>, String> {
    let path = temp_png("capture");
    let status = Command::new("/usr/sbin/screencapture").args(args).arg(&path).status().map_err(|e| format!("Can't take a screenshot: {e}"))?;
    if !path.exists() {
        if status.success() {
            return Ok(None);
        }
        return Err("Can't take a screenshot. ShotCraft needs the Screen Recording permission: System Settings → Privacy & Security → Screen Recording.".into());
    }
    let extra = from_file(&path);
    let img = load_image_file(&path);
    let _ = std::fs::remove_file(&path);
    Ok(Some((img?, extra)))
}

/// Arguments for the modes that are not resolved to a target: the user selects, or the area is known.
fn selection_args(mode: CaptureMode, opt: &CaptureOptions) -> Result<Vec<String>, String> {
    let mut a = vec!["-x".to_string()];
    match mode {
        CaptureMode::Area => a.extend(["-i".into(), "-s".into()]),
        CaptureMode::RepeatArea => {
            let Some(r) = opt.last_area else {
                return Err("No area to repeat. Repeat Area Capture takes a screenshot of the previously captured area. You need to perform the area capture first.".into());
            };
            a.push(format!("-R{},{},{},{}", r.x0, r.y0, r.w(), r.h()));
        }
        CaptureMode::Window => return Ok(pick_window_args(opt)),
        _ => return Err("This capture needs no selection.".into()),
    }
    Ok(a)
}

/// The interactive window picker.
fn pick_window_args(opt: &CaptureOptions) -> Vec<String> {
    let mut a = vec!["-x".to_string(), "-i".into(), "-w".into()];
    if opt.window_bg == WindowBg::Trimmed {
        a.push("-o".into());
    }
    a
}

/// Where a window sits on screen: x, y, width and height in global points.
type Frame = (f64, f64, f64, f64);

/// Non-interactive capture of a resolved target, with the picture's pixels per point and, for a
/// window, where it was on screen when that is known and needed.
fn capture_target(target: Target, opt: &CaptureOptions) -> Result<Option<(Image, f32, Option<Frame>)>, String> {
    let scale = opt.scale;
    let trim = opt.window_bg == WindowBg::Trimmed;
    Ok(match target {
        Target::Window(id, px_per_point) => {
            let frame = opt.snapshot.windows.iter().find(|w| w.id == id).map(|w| (w.bounds.x, w.bounds.y, w.bounds.w, w.bounds.h));
            shoot(&targets::window_args(id, trim), |_| ())?.map(|(img, _)| (img, px_per_point as f32, frame))
        }
        Target::PickWindow => {
            let want_frame = opt.window_bg == WindowBg::Wallpaper;
            shoot(&pick_window_args(opt), |p| if want_frame { capture_rect(p) } else { None })?.map(|(img, r)| (img, scale, r.map(frame_of)))
        }
        Target::MainDisplay => shoot(&["-x".to_string(), "-m".to_string()], |_| ())?.map(|(img, _)| (img, scale, None)),
        Target::Displays(displays) => {
            let mut shots = Vec::new();
            for d in &displays {
                let Some((img, _)) = shoot(&targets::display_args(*d), |_| ())? else { return Ok(None) };
                shots.push(img);
            }
            let snap = &opt.snapshot;
            let px_per_point = displays.iter().map(|d| targets::scale_at(&snap.displays, &snap.scales, *d) as f32).fold(1.0, f32::max);
            if shots.len() == 1 {
                return Ok(Some((shots.remove(0), px_per_point, None)));
            }
            let canvas = targets::compose(&displays, &shots).ok_or("Can't put the displays together in one picture.")?;
            Some((canvas, px_per_point, None))
        }
    })
}

fn frame_of(r: R) -> Frame {
    (r.x0 as f64, r.y0 as f64, r.w() as f64, r.h() as f64)
}

pub fn capture(mode: CaptureMode, opt: &CaptureOptions) -> Result<Option<Captured>, String> {
    let is_window = matches!(mode, CaptureMode::Window | CaptureMode::ActiveWindow);
    let (img, scale, area, frame) = match targets::resolve(mode, &opt.snapshot) {
        Some(target) => {
            if mode == CaptureMode::Delayed {
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
            let Some((img, scale, frame)) = capture_target(target, opt)? else { return Ok(None) };
            (img, scale, None, frame)
        }
        None => {
            let args = selection_args(mode, opt)?;
            let want_frame = mode == CaptureMode::Window && opt.window_bg == WindowBg::Wallpaper;
            let Some((img, rect)) = shoot(&args, |p| match mode {
                CaptureMode::Area => capture_rect(p),
                CaptureMode::RepeatArea => opt.last_area,
                _ if want_frame => capture_rect(p),
                _ => None,
            })?
            else {
                return Ok(None);
            };
            // The rectangle of a picked window places it on the wallpaper; it is not an area to repeat.
            if is_window {
                (img, opt.scale, None, rect.map(frame_of))
            } else {
                (img, opt.scale, rect, None)
            }
        }
    };
    let img = if is_window { apply_window_bg(img, scale, frame, opt) } else { img };
    Ok(Some(Captured { img, scale, area, mode }))
}

/// The final look of a window capture for the chosen background.
fn apply_window_bg(img: Image, scale: f32, frame: Option<Frame>, opt: &CaptureOptions) -> Image {
    match opt.window_bg {
        WindowBg::Transparent | WindowBg::Trimmed => windowbg::trim_transparent(&img),
        WindowBg::Solid => windowbg::on_color(&img, opt.solid_color),
        WindowBg::Wallpaper => match wallpaper_backdrop(&img, scale, frame, opt) {
            Ok(wall) => windowbg::on_picture(&img, &wall),
            Err(why) => {
                notify("ShotCraft", &format!("Can't read the desktop wallpaper ({why}). The window is on the solid colour instead."));
                windowbg::on_color(&img, opt.solid_color)
            }
        },
    }
}

/// The wallpaper of the window's display, cut to the canvas of `img`.
fn wallpaper_backdrop(img: &Image, scale: f32, frame: Option<Frame>, opt: &CaptureOptions) -> Result<Image, String> {
    let snap = &opt.snapshot;
    let center = frame.map(|f| (f.0 + f.2 / 2.0, f.1 + f.3 / 2.0)).or(snap.mouse);
    let index = center.and_then(|c| targets::display_at(&snap.displays, c)).unwrap_or(0);
    let path = snap.wallpapers.get(index).cloned().flatten().ok_or("the system gave no picture for this display")?;
    let wall = load_wallpaper(&path)?;
    let lay = windowbg::layout(img);
    let display = snap.displays.get(index).zip(snap.scales.get(index)).map(|(d, s)| windowbg::DisplayPlace { frame: (d.x, d.y, d.w, d.h), px_per_point: *s });
    let origin = frame.and_then(|f| windowbg::canvas_origin(f, scale as f64, img, &lay));
    Ok(windowbg::wallpaper_canvas(&wall, &lay, origin, display))
}

/// Reads a desktop picture. Formats the `image` crate lacks (HEIC, the usual one on macOS) go
/// through `sips`, which ships with macOS.
fn load_wallpaper(path: &Path) -> Result<Image, String> {
    if path.is_dir() {
        return Err("the desktop shows a folder of pictures".into());
    }
    if let Ok(img) = load_image_file(path) {
        return Ok(img);
    }
    let tmp = temp_png("wallpaper");
    let ok = Command::new("/usr/bin/sips").args(["-s", "format", "png"]).arg(path).arg("--out").arg(&tmp).output().map(|o| o.status.success()).unwrap_or(false);
    let img = if ok { load_image_file(&tmp).ok() } else { None };
    let _ = std::fs::remove_file(&tmp);
    img.ok_or_else(|| format!("{} can't be opened", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
}

/// Reads the displays, the mouse and the frontmost app. Call it on the main thread.
#[cfg(target_os = "macos")]
pub fn snapshot(mode: CaptureMode) -> Snapshot {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSWorkspace};
    use targets::Rect;
    let own_pid = std::process::id() as i32;
    let frontmost_pid = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier());
    let windows = if mode == CaptureMode::ActiveWindow { window_infos() } else { Vec::new() };
    let Some(mtm) = MainThreadMarker::new() else { return Snapshot { own_pid, frontmost_pid, windows, ..Snapshot::default() } };
    let screens = NSScreen::screens(mtm);
    let rect = |f: objc2_foundation::NSRect| Rect::new(f.origin.x, f.origin.y, f.size.width, f.size.height);
    // The first screen is the one with the menu bar; every frame is relative to its bottom left corner.
    let primary_h = screens.iter().next().map_or(0.0, |s| s.frame().size.height);
    let displays = screens.iter().map(|s| targets::flip_frame(rect(s.frame()), primary_h)).collect();
    let m = NSEvent::mouseLocation();
    let scales = screens.iter().map(|s| s.backingScaleFactor()).collect();
    // Only a window capture on the wallpaper needs the desktop pictures.
    let wallpapers = if matches!(mode, CaptureMode::Window | CaptureMode::ActiveWindow) {
        let ws = NSWorkspace::sharedWorkspace();
        screens.iter().map(|s| ws.desktopImageURLForScreen(&s).and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()))).collect()
    } else {
        Vec::new()
    };
    Snapshot { own_pid, frontmost_pid, displays, scales, mouse: Some(targets::flip_point((m.x, m.y), primary_h)), windows, wallpapers }
}

#[cfg(not(target_os = "macos"))]
pub fn snapshot(_mode: CaptureMode) -> Snapshot {
    Snapshot { own_pid: std::process::id() as i32, ..Snapshot::default() }
}

/// On-screen windows, front to back.
#[cfg(target_os = "macos")]
fn window_infos() -> Vec<targets::WindowInfo> {
    use targets::{Rect, WindowInfo};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_core_foundation::{CFArray, CFString};
    use objc2_core_graphics::{kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID, CGWindowListCopyWindowInfo, CGWindowListOption};
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
    type Dict = NSDictionary<NSString, AnyObject>;
    let opts = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(opts, 0) else { return Vec::new() };
    // Core Foundation arrays and dictionaries are the same objects as their Foundation counterparts.
    let list: &NSArray<Dict> = unsafe { &*(&*list as *const CFArray as *const NSArray<Dict>) };
    let key = |k: &CFString| -> &NSString { unsafe { &*(k as *const CFString as *const NSString) } };
    let number = |d: &Dict, k: &NSString| -> Option<f64> {
        let v: Retained<AnyObject> = d.objectForKey(k)?;
        v.downcast_ref::<NSNumber>().map(|n| n.as_f64())
    };
    unsafe {
        list.iter()
            .filter_map(|d| {
                let bounds = d.objectForKey(key(kCGWindowBounds))?;
                let is_dict: bool = objc2::msg_send![&*bounds, isKindOfClass: <NSDictionary as objc2::ClassType>::class()];
                if !is_dict {
                    return None;
                }
                let b: &Dict = &*(&*bounds as *const AnyObject as *const Dict);
                let side = |name: &str| number(b, &NSString::from_str(name));
                Some(WindowInfo {
                    id: number(&d, key(kCGWindowNumber))? as u32,
                    pid: number(&d, key(kCGWindowOwnerPID))? as i32,
                    layer: number(&d, key(kCGWindowLayer))? as i32,
                    alpha: number(&d, key(kCGWindowAlpha)).unwrap_or(1.0),
                    bounds: Rect::new(side("X")?, side("Y")?, side("Width")?, side("Height")?),
                })
            })
            .collect()
    }
}

pub fn copy_image(img: &Image) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_image(arboard::ImageData { width: img.w as usize, height: img.h as usize, bytes: img.px.as_slice().into() })
        .map_err(|e| e.to_string())
}

pub fn copy_text(s: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(s.to_string());
    }
}

pub fn clipboard_image() -> Result<Image, String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let data = cb.get_image().map_err(|_| "Your clipboard doesn't seem to contain an image at this moment.".to_string())?;
    Ok(Image { w: data.width as u32, h: data.height as u32, px: Arc::new(data.bytes.into_owned()) })
}

pub fn encode_png(img: &Image) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let rgba = image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).ok_or("bad image")?;
    rgba.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// "Auto (PNG/JPEG)" keeps PNG for flat screenshots and switches to JPEG for photo-like images.
/// A picture with see-through pixels (a window with its shadow) stays PNG: JPEG has no alpha.
pub fn looks_like_photo(img: &Image) -> bool {
    if img.has_alpha() {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    for s in img.px.chunks_exact(4).step_by(7) {
        seen.insert([s[0] & 0xF8, s[1] & 0xF8, s[2] & 0xF8]);
        if seen.len() > 6000 {
            return true;
        }
    }
    false
}

pub fn save_image(img: &Image, path: &Path) -> Result<(), String> {
    let is_jpeg = path.extension().map(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")).unwrap_or(false);
    if is_jpeg {
        // JPEG has no alpha: lay the picture on white, or transparent pixels (stored as black) come out black.
        let flat = windowbg::flatten(img, [255, 255, 255]);
        let flat = image::RgbaImage::from_raw(flat.w, flat.h, flat.px.to_vec()).ok_or("bad image")?;
        let rgb = image::DynamicImage::ImageRgba8(flat).to_rgb8();
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 90);
        enc.encode_image(&rgb).map_err(|e| e.to_string())
    } else {
        let rgba = image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).ok_or("bad image")?;
        rgba.save_with_format(path, image::ImageFormat::Png).map_err(|e| e.to_string())
    }
}

/// The local date and time, for the names of saved files.
pub fn now_stamp() -> crate::settings::Stamp {
    let out = Command::new("date").arg("+%Y %m %d %H %M %S").output().ok();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    parse_stamp(&text).unwrap_or(crate::settings::Stamp { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 })
}

fn parse_stamp(text: &str) -> Option<crate::settings::Stamp> {
    let n: Vec<i32> = text.split_whitespace().map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [year, month, day, hour, minute, second] = n[..] else { return None };
    Some(crate::settings::Stamp { year, month: month as u32, day: day as u32, hour: hour as u32, minute: minute as u32, second: second as u32 })
}

/// `stem.ext` in `dir`, or `stem-2.ext`, `stem-3.ext`... when that name is taken: a save never
/// replaces a file.
pub fn unique_path(dir: &std::path::Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2u32..).map(|n| dir.join(format!("{stem}-{n}.{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

pub fn notify(title: &str, body: &str) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", esc(body), esc(title));
    let _ = Command::new("osascript").args(["-e", &script]).spawn();
}

/// Sizes the editor for the screen under the mouse and centres it there. `size` gets that screen's
/// usable width and height (menu bar and Dock excluded) and returns the window size.
#[cfg(target_os = "macos")]
pub fn center_main_window(_ctx: &eframe::egui::Context, size: impl Fn(f32, f32) -> (f32, f32)) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEvent, NSScreen};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let Some(mtm) = MainThreadMarker::new() else { return };
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let contains = |f: NSRect| mouse.x >= f.origin.x && mouse.x < f.origin.x + f.size.width && mouse.y >= f.origin.y && mouse.y < f.origin.y + f.size.height;
    let Some(screen) = screens.iter().find(|s| contains(s.frame())).or_else(|| screens.iter().next()) else { return };
    let area = screen.visibleFrame();
    let (w, h) = size(area.size.width as f32, area.size.height as f32);
    let (w, h) = (w as f64, h as f64);
    let origin = NSPoint::new(area.origin.x + (area.size.width - w) / 2.0, area.origin.y + (area.size.height - h) / 2.0);
    let frame = NSRect::new(origin, NSSize::new(w, h));
    let windows = NSApplication::sharedApplication(mtm).windows();
    // The editor is the only large window named ShotCraft; the helper window is 1×1.
    let editor = windows.iter().find(|win| win.title().to_string() == "ShotCraft" && win.frame().size.width > 100.0);
    if let Some(win) = editor {
        win.setFrame_display(frame, true);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn center_main_window(ctx: &eframe::egui::Context, size: impl Fn(f32, f32) -> (f32, f32)) {
    let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(eframe::egui::vec2(1440.0, 900.0));
    let (w, h) = size(monitor.x, monitor.y);
    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::InnerSize(eframe::egui::vec2(w, h)));
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

const AGENT: &str = "app.shotcraft.launcher";

pub fn set_autostart(on: bool) {
    let path = home().join("Library/LaunchAgents").join(format!("{AGENT}.plist"));
    if !on {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{AGENT}</string><key>ProgramArguments</key><array><string>{}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>\n",
        exe.display()
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, plist);
}

/// Keeps the login item on the installed app, wherever it was first switched on from.
pub fn refresh_autostart(on: bool) {
    let bundled = std::env::current_exe().is_ok_and(|e| e.to_string_lossy().contains(".app/Contents/MacOS/"));
    if on && bundled {
        set_autostart(true);
    }
}

pub struct OcrResult {
    pub lines: Vec<String>,
    pub codes: Vec<String>,
}

/// Text and codes in an image, with the text read in the default languages.
pub fn recognize(img: &Image) -> Result<OcrResult, String> {
    recognize_in(img, "")
}

/// Like `recognize`, trying `language` (a Vision language code from `ocr_languages`) first; empty or
/// unknown keeps Vision's own choice.
#[cfg(target_os = "macos")]
pub fn recognize_in(img: &Image, language: &str) -> Result<OcrResult, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
    use objc2_vision::{VNDetectBarcodesRequest, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
    let png = encode_png(img)?;
    objc2::rc::autoreleasepool(|_| unsafe {
        let data = NSData::with_bytes(&png);
        let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &NSDictionary::new());
        let text = VNRecognizeTextRequest::new();
        text.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        text.setUsesLanguageCorrection(true);
        if ocr_languages().iter().any(|(code, _)| code == language) {
            // The chosen language first, the others after it, so mixed text is still read.
            let mut order = vec![NSString::from_str(language)];
            order.extend(ocr_languages().iter().filter(|(code, _)| code != language).map(|(code, _)| NSString::from_str(code)));
            text.setRecognitionLanguages(&NSArray::from_retained_slice(&order));
        }
        let codes = VNDetectBarcodesRequest::new();
        let a: &VNRequest = &text;
        let b: &VNRequest = &codes;
        handler.performRequests_error(&NSArray::from_slice(&[a, b])).map_err(|e| format!("Unable to perform the OCR requests: {}", e.localizedDescription()))?;
        let mut lines = vec![];
        if let Some(res) = text.results() {
            for obs in res.iter() {
                let top = obs.topCandidates(1);
                if let Some(t) = top.iter().next() {
                    lines.push(t.string().to_string());
                }
            }
        }
        let mut found = vec![];
        if let Some(res) = codes.results() {
            for obs in res.iter() {
                if let Some(s) = obs.payloadStringValue() {
                    found.push(s.to_string());
                }
            }
        }
        Ok(OcrResult { lines, codes: found })
    })
}

/// Text areas in an image, in pixels from the top left: what Blur Text and Erase Text act on.
#[cfg(target_os = "macos")]
pub fn text_boxes(img: &Image) -> Vec<R> {
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
    let Ok(png) = encode_png(img) else { return vec![] };
    let (w, h) = (img.w as f32, img.h as f32);
    objc2::rc::autoreleasepool(|_| unsafe {
        let data = NSData::with_bytes(&png);
        let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &NSDictionary::new());
        let text = VNRecognizeTextRequest::new();
        text.setRecognitionLevel(VNRequestTextRecognitionLevel::Fast);
        let a: &VNRequest = &text;
        if handler.performRequests_error(&NSArray::from_slice(&[a])).is_err() {
            return vec![];
        }
        text.results().map_or(vec![], |res| {
            res.iter()
                .map(|obs| {
                    let b = obs.boundingBox();
                    let (x, y, bw, bh) = (b.origin.x as f32, b.origin.y as f32, b.size.width as f32, b.size.height as f32);
                    R { x0: x * w, y0: (1.0 - y - bh) * h, x1: (x + bw) * w, y1: (1.0 - y) * h }.expand(2.0)
                })
                .collect()
        })
    })
}

#[cfg(not(target_os = "macos"))]
pub fn text_boxes(_img: &Image) -> Vec<R> {
    vec![]
}

#[cfg(not(target_os = "macos"))]
pub fn recognize_in(_img: &Image, _language: &str) -> Result<OcrResult, String> {
    Err("Text recognition needs macOS".into())
}

/// The languages text recognition can read, as (Vision code, name in the user's language), by name.
/// Asked once: the answer doesn't change while the app runs.
pub fn ocr_languages() -> &'static [(String, String)] {
    static LANGUAGES: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    LANGUAGES.get_or_init(|| {
        let mut list = supported_ocr_languages();
        list.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        list
    })
}

#[cfg(target_os = "macos")]
fn supported_ocr_languages() -> Vec<(String, String)> {
    use objc2_foundation::{NSLocale, NSString};
    use objc2_vision::{VNRecognizeTextRequest, VNRequestTextRecognitionLevel};
    objc2::rc::autoreleasepool(|_| unsafe {
        let text = VNRecognizeTextRequest::new();
        text.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        let Ok(codes) = text.supportedRecognitionLanguagesAndReturnError() else { return vec![] };
        let locale = NSLocale::currentLocale();
        codes
            .iter()
            .map(|c| {
                let code = c.to_string();
                let name = locale.localizedStringForLocaleIdentifier(&NSString::from_str(&code)).to_string();
                (code.clone(), if name.is_empty() { code } else { name })
            })
            .collect()
    })
}

/// Without Vision there is nothing to offer.
#[cfg(not(target_os = "macos"))]
fn supported_ocr_languages() -> Vec<(String, String)> {
    vec![]
}

/// Opens System Settings on the keyboard pane, where the system's own screenshot shortcuts can be switched off.
pub fn open_keyboard_settings() {
    let _ = Command::new("open").arg("x-apple.systempreferences:com.apple.Keyboard-Settings.extension").spawn();
}

/// Opens the system print panel for the image, laid out as `menu_model::print_layout` says: at its
/// on-screen size, shrunk to fit the page, centred, in landscape when it is wider than tall. The
/// panel is modal, like the file dialogs, and the call returns when it is closed.
#[cfg(target_os = "macos")]
pub fn print_image(img: &Image, scale: f32, title: &str) -> Result<(), String> {
    use crate::menu_model::{print_landscape, print_layout};
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSImage, NSImageScaling, NSImageView, NSPaperOrientation, NSPrintInfo, NSPrintOperation, NSPrintingPaginationMode};
    use objc2_foundation::{NSData, NSPoint, NSRect, NSSize, NSString};
    let mtm = MainThreadMarker::new().ok_or("Printing must start on the main thread")?;
    let png = encode_png(img)?;
    objc2::rc::autoreleasepool(|_| {
        let data = NSData::with_bytes(&png);
        let ns_image = NSImage::initWithData(NSImage::alloc(), &data).ok_or("Can't read the image for printing")?;
        // The shared info belongs to the app's page setup; a copy keeps this job's orientation to itself.
        let info: objc2::rc::Retained<NSPrintInfo> = unsafe { objc2::msg_send![&*NSPrintInfo::sharedPrintInfo(), copy] };
        let px = (img.w as u32, img.h as u32);
        info.setOrientation(if print_landscape(px) { NSPaperOrientation::Landscape } else { NSPaperOrientation::Portrait });
        info.setHorizontallyCentered(true);
        info.setVerticallyCentered(true);
        info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
        info.setVerticalPagination(NSPrintingPaginationMode::Fit);
        let paper = info.paperSize();
        let margin = info.leftMargin().max(info.rightMargin()).max(info.topMargin()).max(info.bottomMargin());
        let layout = print_layout(px, scale, (paper.width, paper.height), margin);
        let view = NSImageView::initWithFrame(mtm.alloc::<NSImageView>(), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(layout.width, layout.height)));
        view.setImage(Some(&ns_image));
        view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        let op = NSPrintOperation::printOperationWithView_printInfo(&view, &info);
        op.setJobTitle(Some(&NSString::from_str(title)));
        op.setShowsPrintPanel(true);
        crate::app_mode::activate();
        // False when the user cancels as well as when printing fails: the two can't be told apart here.
        op.runOperation();
        Ok(())
    })
}

#[cfg(not(target_os = "macos"))]
pub fn print_image(_img: &Image, _scale: f32, _title: &str) -> Result<(), String> {
    Err("Printing needs macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_command_output_is_read_back() {
        let s = parse_stamp("2026 11 05 09 07 03").unwrap();
        assert_eq!((s.year, s.month, s.day, s.hour, s.minute, s.second), (2026, 11, 5, 9, 7, 3));
        assert!(parse_stamp("").is_none());
        assert!(parse_stamp("2026 11 05").is_none());
        assert!(parse_stamp("a b c d e f").is_none());
        assert!(parse_stamp("1 2 3 4 5 6 7").is_none());
    }

    #[test]
    fn a_save_never_takes_a_name_that_exists() {
        let dir = temp_png("unique").with_extension("d");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "shot", "png"), dir.join("shot.png"));
        std::fs::write(dir.join("shot.png"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "shot", "png"), dir.join("shot-2.png"));
        std::fs::write(dir.join("shot-2.png"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "shot", "png"), dir.join("shot-3.png"));
        assert_eq!(unique_path(&dir, "shot", "jpg"), dir.join("shot.jpg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 2 × 1 images, left pixel (200, 40, 20) and right pixel (10, 120, 230), one per format the open dialog offers.
    const GIF: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x37, 0x61, 0x02, 0x00, 0x01, 0x00, 0x81, 0x00, 0x00, 0xc8, 0x28, 0x14, 0x0a, 0x78, 0xe6, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x00, 0x00, 0x08, 0x05, 0x00, 0x01, 0x04, 0x08, 0x08, 0x00, 0x3b,
    ];
    const BMP: &[u8] = &[
        0x42, 0x4d, 0x3e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x28, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x28, 0xc8, 0xe6, 0x78, 0x0a, 0x00, 0x00,
    ];
    const TIFF: &[u8] = &[
        0x49, 0x49, 0x2a, 0x00, 0x08, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x01,
        0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x01, 0x03, 0x00, 0x03, 0x00, 0x00, 0x00, 0x86, 0x00, 0x00, 0x00, 0x03, 0x01,
        0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x11, 0x01,
        0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x8c, 0x00, 0x00, 0x00, 0x15, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x16, 0x01,
        0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x17, 0x01, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x1c, 0x01,
        0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x08, 0x00, 0x08, 0x00, 0xc8, 0x28, 0x14, 0x0a,
        0x78, 0xe6,
    ];
    const WEBP: &[u8] = &[
        0x52, 0x49, 0x46, 0x46, 0x22, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x4c, 0x16, 0x00, 0x00, 0x00, 0x2f, 0x01, 0x00, 0x00,
        0x00, 0x0f, 0x70, 0x14, 0xa8, 0x0b, 0x21, 0x9f, 0x42, 0x7a, 0xfe, 0x43, 0x0b, 0x15, 0x88, 0xe8, 0x7f, 0x00,
    ];

    /// Every extension the Open dialog offers must decode, or the picker lists files the app then rejects.
    #[test]
    fn open_dialog_formats_all_decode() {
        let dir = std::env::temp_dir().join(format!("shotcraft-formats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut failures = Vec::new();
        for ext in OPEN_EXTENSIONS {
            let bytes = match *ext {
                "gif" => GIF,
                "bmp" => BMP,
                "tif" | "tiff" => TIFF,
                "webp" => WEBP,
                "png" | "jpg" | "jpeg" => continue, // the formats the app saves itself, opened by every other path
                other => panic!("no sample file for .{other}: add one so the Open dialog and the decoders stay in step"),
            };
            let path = dir.join(format!("sample.{ext}"));
            std::fs::write(&path, bytes).unwrap();
            match load_image_file(&path) {
                Ok(img) => {
                    if (img.w, img.h) != (2, 1) || img.px[..4] != [200, 40, 20, 255] || img.px[4..8] != [10, 120, 230, 255] {
                        failures.push(format!("{ext}: decoded to the wrong pixels ({}x{}, {:?})", img.w, img.h, &img.px[..8]));
                    }
                }
                Err(e) => failures.push(format!("{ext}: {e}")),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(failures.is_empty(), "formats offered by the Open dialog that do not open:\n{}", failures.join("\n"));
    }

    /// A window capture with its shadow has far more than 6,000 colours once it holds real content,
    /// but JPEG has no alpha: "Auto" must keep it PNG, and a JPEG save must not turn the transparent
    /// margin black.
    #[test]
    fn a_picture_with_alpha_is_never_saved_as_a_black_jpeg() {
        let win = crate::headless::window_capture(640, 380, 2);
        // Add noise to the body so the colour count alone would call it a photo.
        let mut px = win.px.to_vec();
        let mut seed = 12345u32;
        for p in px.chunks_exact_mut(4) {
            if p[3] == 255 {
                for c in &mut p[..3] {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    *c = c.wrapping_add((seed >> 24) as u8);
                }
            }
        }
        let noisy = Image {
            w: win.w,
            h: win.h,
            px: Arc::new(px),
        };
        assert!(
            !looks_like_photo(&noisy),
            "Auto keeps a see-through picture as PNG"
        );
        let mut opaque = noisy.clone();
        opaque.px = Arc::new(
            noisy
                .px
                .chunks_exact(4)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
        );
        assert!(
            looks_like_photo(&opaque),
            "the same pixels without alpha are photo-like"
        );

        let dir = std::env::temp_dir().join(format!("shotcraft-alpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (jpg, png) = (dir.join("w.jpg"), dir.join("w.png"));
        save_image(&win, &jpg).unwrap();
        save_image(&win, &png).unwrap();
        let (j, p) = (
            load_image_file(&jpg).unwrap(),
            load_image_file(&png).unwrap(),
        );
        let _ = std::fs::remove_dir_all(&dir);
        let corner = j.get(2, 2);
        assert!(
            corner[0] > 245 && corner[1] > 245 && corner[2] > 245,
            "the transparent corner is white in a JPEG, got {corner:?}"
        );
        assert_eq!(p.get(2, 2)[3], 0, "a PNG keeps the transparent corner");
        assert_eq!(p.get(360, 340), win.get(360, 340), "and the opaque body");
    }
}

#[cfg(test)]
mod window_bg_tests {
    use super::*;
    use crate::headless::window_capture;
    use crate::targets::Rect;

    fn options(bg: WindowBg, snapshot: Snapshot) -> CaptureOptions {
        CaptureOptions { last_area: None, window_bg: bg, solid_color: [10, 20, 30], scale: 2.0, snapshot }
    }

    #[test]
    fn only_trim_shadow_asks_for_a_capture_without_shadow() {
        assert!(pick_window_args(&options(WindowBg::Trimmed, Snapshot::default())).contains(&"-o".to_string()));
        for bg in [WindowBg::Transparent, WindowBg::Solid, WindowBg::Wallpaper] {
            assert!(!pick_window_args(&options(bg, Snapshot::default())).contains(&"-o".to_string()), "{bg:?}");
        }
    }

    #[test]
    fn transparent_and_trim_keep_the_alpha_and_solid_makes_it_opaque() {
        let win = window_capture(300, 200, 2);
        let snap = Snapshot::default();
        assert!(apply_window_bg(win.clone(), 2.0, None, &options(WindowBg::Transparent, snap.clone())).has_alpha());
        assert!(apply_window_bg(win.clone(), 2.0, None, &options(WindowBg::Trimmed, snap.clone())).has_alpha());
        let solid = apply_window_bg(win.clone(), 2.0, None, &options(WindowBg::Solid, snap));
        assert!(!solid.has_alpha());
        assert_eq!(&solid.get(0, 0)[..3], &[10, 20, 30]);
    }

    #[test]
    fn an_unreadable_wallpaper_falls_back_to_the_solid_colour() {
        let win = window_capture(300, 200, 2);
        let expect = windowbg::on_color(&win, [10, 20, 30]);
        // No wallpaper known at all, then a path that is not a picture.
        let none = Snapshot { displays: vec![Rect::new(0.0, 0.0, 1000.0, 600.0)], scales: vec![2.0], ..Snapshot::default() };
        let out = apply_window_bg(win.clone(), 2.0, None, &options(WindowBg::Wallpaper, none.clone()));
        assert_eq!(out.px, expect.px);
        let bad = Snapshot { wallpapers: vec![Some(PathBuf::from("/nonexistent/wall.heic"))], ..none };
        let out = apply_window_bg(win, 2.0, None, &options(WindowBg::Wallpaper, bad));
        assert_eq!(out.px, expect.px);
    }

    #[test]
    fn a_readable_wallpaper_ends_up_behind_the_window() {
        let win = window_capture(300, 200, 2);
        let dir = std::env::temp_dir().join("shotcraft-wallpaper-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wall.png");
        image::RgbaImage::from_pixel(64, 36, image::Rgba([200, 30, 30, 255])).save(&path).unwrap();
        let snap = Snapshot {
            displays: vec![Rect::new(0.0, 0.0, 1000.0, 600.0)],
            scales: vec![2.0],
            wallpapers: vec![Some(path.clone())],
            ..Snapshot::default()
        };
        let out = apply_window_bg(win.clone(), 2.0, None, &options(WindowBg::Wallpaper, snap));
        let _ = std::fs::remove_file(&path);
        assert!(!out.has_alpha());
        assert_eq!(&out.get(0, 0)[..3], &[200, 30, 30], "the wallpaper shows in the margin");
        assert_eq!((out.w, out.h), (windowbg::layout(&win).w, windowbg::layout(&win).h));
    }
}
