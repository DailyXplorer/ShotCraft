use crate::model::{Image, R};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureMode {
    Fullscreen,
    Area,
    RepeatArea,
    Window,
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

pub fn load_image_file(path: &Path) -> Result<Image, String> {
    let img = image::open(path).map_err(|e| format!("Something went wrong. Is the image file okay? ({e})"))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok(Image { w, h, px: Arc::new(rgba.into_raw()) })
}

/// Read the capture rectangle macOS stores on the file (kMDItemScreenCaptureGlobalRect).
fn capture_rect(path: &Path) -> Option<R> {
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
    pub trim_shadow: bool,
    pub solid_bg: Option<[u8; 4]>,
    pub scale: f32,
}

pub fn capture(mode: CaptureMode, opt: &CaptureOptions) -> Result<Option<Captured>, String> {
    let path = temp_png("capture");
    let mut cmd = Command::new("/usr/sbin/screencapture");
    cmd.arg("-x");
    match mode {
        CaptureMode::Fullscreen => {
            cmd.arg("-m");
        }
        CaptureMode::Delayed => {
            cmd.args(["-m", "-T", "3"]);
        }
        CaptureMode::Area => {
            cmd.args(["-i", "-s"]);
        }
        CaptureMode::RepeatArea => {
            let Some(r) = opt.last_area else {
                return Err("No area to repeat. Repeat Area Capture takes a screenshot of the previously captured area. You need to perform the area capture first.".into());
            };
            cmd.arg(format!("-R{},{},{},{}", r.x0, r.y0, r.w(), r.h()));
        }
        CaptureMode::Window => {
            cmd.args(["-i", "-w"]);
            if opt.trim_shadow {
                cmd.arg("-o");
            }
        }
    }
    cmd.arg(&path);
    let status = cmd.status().map_err(|e| format!("Can't take a screenshot: {e}"))?;
    if !path.exists() {
        if status.success() {
            return Ok(None);
        }
        return Err("Can't take a screenshot. ShotCraft needs the Screen Recording permission: System Settings → Privacy & Security → Screen Recording.".into());
    }
    let area = if mode == CaptureMode::Area { capture_rect(&path) } else { opt.last_area.filter(|_| mode == CaptureMode::RepeatArea) };
    let mut img = load_image_file(&path)?;
    let _ = std::fs::remove_file(&path);
    if mode == CaptureMode::Window {
        if let Some(bg) = opt.solid_bg {
            img = flatten_on(&img, bg);
        }
    }
    Ok(Some(Captured { img, scale: opt.scale, area, mode }))
}

fn flatten_on(img: &Image, bg: [u8; 4]) -> Image {
    let mut out = Vec::with_capacity(img.px.len());
    for s in img.px.chunks_exact(4) {
        let a = s[3] as u32;
        for c in 0..3 {
            out.push(((s[c] as u32 * a + bg[c] as u32 * (255 - a)) / 255) as u8);
        }
        out.push(255);
    }
    Image { w: img.w, h: img.h, px: Arc::new(out) }
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
pub fn looks_like_photo(img: &Image) -> bool {
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
    let rgba = image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).ok_or("bad image")?;
    let is_jpeg = path.extension().map(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")).unwrap_or(false);
    if is_jpeg {
        let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 90);
        enc.encode_image(&rgb).map_err(|e| e.to_string())
    } else {
        rgba.save_with_format(path, image::ImageFormat::Png).map_err(|e| e.to_string())
    }
}

pub fn file_stamp() -> String {
    let out = Command::new("date").arg("+%Y%m%d-%H%M%S").output().ok();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "now".into())
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

#[cfg(target_os = "macos")]
pub fn recognize(img: &Image) -> Result<OcrResult, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{VNDetectBarcodesRequest, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};
    let png = encode_png(img)?;
    objc2::rc::autoreleasepool(|_| unsafe {
        let data = NSData::with_bytes(&png);
        let handler = VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &NSDictionary::new());
        let text = VNRecognizeTextRequest::new();
        text.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        text.setUsesLanguageCorrection(true);
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
pub fn recognize(_img: &Image) -> Result<OcrResult, String> {
    Err("Text recognition needs macOS".into())
}
