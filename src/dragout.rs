//! Drag-out: the drag handle in the editor toolbar starts a native drag of the edited picture, so
//! it can be dropped as a file into Finder, Mail, Slack, a browser...
//!
//! The picture goes through the normal export path and is written to a temporary PNG named by the
//! file name template; the drag carries that file's URL. Temporary files are removed when a drag
//! is cancelled, and swept once they are old enough that no drop target can still want them. The
//! naming, sweeping and preview sizing are plain code (tested); the drag session is native (macOS:
//! AppKit; Windows: the shell's data object and `SHDoDragDrop`; not on Linux yet).

use crate::model::Image;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Folder of the temporary files, inside the system temporary directory.
pub const TEMP_DIR_NAME: &str = "ShotCraft-drag";
/// How long a dropped file stays around: a target may read it a while after the drop (an upload
/// form reads it when it is submitted).
pub const KEEP_FOR: Duration = Duration::from_secs(10 * 60);
/// Longest side of the picture that follows the pointer, in points.
pub const PREVIEW_MAX: f32 = 160.0;
const STEM_MAX_CHARS: usize = 80;
const FALLBACK_STEM: &str = "ShotCraft";

pub fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(TEMP_DIR_NAME)
}

/// A file name stem that is safe to hand to any app: no path separators, control characters or
/// leading dots (a hidden file), not empty, not endless.
pub fn safe_stem(stem: &str) -> String {
    let cleaned: String = stem.chars().map(|c| if matches!(c, '/' | '\\' | ':') || c.is_control() { '-' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    let cut: String = cleaned.chars().take(STEM_MAX_CHARS).collect();
    let cut = cut.trim_end();
    if cut.is_empty() {
        FALLBACK_STEM.to_string()
    } else {
        cut.to_string()
    }
}

/// Regular files of `dir` last modified more than `max_age` before `now`.
pub fn stale_files(dir: &Path, now: SystemTime, max_age: Duration) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| {
            let age = e.metadata().and_then(|m| m.modified()).ok().and_then(|m| now.duration_since(m).ok());
            age.map_or(false, |a| a > max_age)
        })
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Deletes the stale files of `dir`; returns how many went.
pub fn sweep_dir(dir: &Path, max_age: Duration) -> usize {
    stale_files(dir, SystemTime::now(), max_age).into_iter().filter(|p| std::fs::remove_file(p).is_ok()).count()
}

/// Cleans the temporary folder of what earlier drags left behind.
pub fn sweep_stale() {
    sweep_dir(&temp_dir(), KEEP_FOR);
}

/// Writes `img` as `<stem>.png` in `dir` (created when needed), never over an existing file.
pub fn export_in(dir: &Path, img: &Image, stem: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Can't prepare the drag: {e}"))?;
    sweep_dir(dir, KEEP_FOR);
    let path = crate::system::unique_path(dir, &safe_stem(stem), "png");
    crate::system::save_image(img, &path)?;
    Ok(path)
}

/// The export for a drag: a PNG in the temporary folder.
pub fn export(img: &Image, stem: &str) -> Result<PathBuf, String> {
    export_in(&temp_dir(), img, stem)
}

/// Deletes `path` after `after` (a drop that happened; the sweep covers a quit before that).
pub fn remove_later(path: PathBuf, after: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(after);
        let _ = std::fs::remove_file(path);
    });
}

/// What to do with the file when the drag session ends: a cancelled drag removes it at once, a
/// drop keeps it for a while.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    Now,
    After(Duration),
}

pub fn cleanup_after(dropped: bool) -> Cleanup {
    if dropped {
        Cleanup::After(KEEP_FOR)
    } else {
        Cleanup::Now
    }
}

/// Applies [`cleanup_after`] to the file of a finished drag.
pub fn finish(path: &Path, dropped: bool) {
    match cleanup_after(dropped) {
        Cleanup::Now => {
            let _ = std::fs::remove_file(path);
        }
        Cleanup::After(d) => remove_later(path.to_path_buf(), d),
    }
}

/// Size of the picture that follows the pointer: the image brought within [`PREVIEW_MAX`].
pub fn preview_size(w: u32, h: u32) -> (f32, f32) {
    crate::sizing::fit_within((w as f32, h as f32), (PREVIEW_MAX, PREVIEW_MAX), false)
}

/// The drag-out picture, shrunk to a small PNG for the preview.
pub fn preview_png(img: &Image) -> Option<Vec<u8>> {
    let side = img.w.max(img.h) as f32;
    let small = if side > PREVIEW_MAX * 2.0 { crate::render::resize(img, PREVIEW_MAX * 2.0 / side) } else { img.clone() };
    crate::system::encode_png(&small).ok()
}

/// Whether a `pressedMouseButtons` bit mask has the primary button down.
pub fn primary_button_down(mask: usize) -> bool {
    mask & 1 != 0
}

/// What to tell egui when a native drag took over: the primary button went up where the pointer
/// was (the drag loop swallows the real release, so egui would keep the button held), and the
/// pointer left the window.
pub fn release_events(pos: eframe::egui::Pos2, modifiers: eframe::egui::Modifiers) -> Vec<eframe::egui::Event> {
    use eframe::egui::{Event, PointerButton};
    vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers }, Event::PointerGone]
}

/// Starts the native drag of `path` from the window the current mouse event belongs to.
#[cfg(target_os = "macos")]
pub fn begin(path: &Path, preview_png: &[u8], preview: (f32, f32)) -> Result<(), String> {
    native::begin(path, preview_png, preview)
}

/// Windows: the shell's own data object for the file (what Explorer drags), dragged with
/// `SHDoDragDrop`, which also draws the drag picture. The call returns when the drag ends.
#[cfg(all(target_os = "windows", not(test)))]
pub fn begin(path: &Path, _preview_png: &[u8], _preview: (f32, f32)) -> Result<(), String> {
    use windows::Win32::System::Ole::{IDropSource, DROPEFFECT_COPY};
    use windows::Win32::UI::Shell::SHDoDragDrop;
    let data = shell_data_object(path)?;
    // SAFETY: runs the drag loop on this (the UI, OLE-initialised) thread; no drop source of our own,
    // so the shell's default one decides when the drag ends.
    unsafe { SHDoDragDrop(None, &data, None::<&IDropSource>, DROPEFFECT_COPY) }.map_err(|e| e.message())?;
    // Explorer may copy the file after the drop returns, and the result doesn't tell a cancel from
    // such a drop: keep the file for a while either way (the sweep removes it later).
    finish(path, true);
    Ok(())
}

/// The shell's data object for a file: what Explorer hands to a drop target (the file itself,
/// `CF_HDROP`, and the shell's own formats).
#[cfg(target_os = "windows")]
fn shell_data_object(path: &Path) -> Result<windows::Win32::System::Com::IDataObject, String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{IBindCtx, IDataObject};
    use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName, BHID_DataObject};
    let wide = HSTRING::from(path.as_os_str());
    // SAFETY: the path is a valid wide string for the length of the call; the shell item is COM-counted.
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(&wide, None::<&IBindCtx>) }.map_err(|e| e.message())?;
    // SAFETY: asks the item for its data object; BHID_DataObject is a static GUID.
    let data: IDataObject = unsafe { item.BindToHandler(None::<&IBindCtx>, &BHID_DataObject) }.map_err(|e| e.message())?;
    Ok(data)
}

/// Linux has no drag here yet (it needs GTK or the X drag protocol), and tests never start a real
/// drag: with no button held it could drop the file on whatever window is under the pointer.
#[cfg(not(any(target_os = "macos", all(target_os = "windows", not(test)))))]
pub fn begin(_path: &Path, _preview_png: &[u8], _preview: (f32, f32)) -> Result<(), String> {
    Err(if cfg!(test) { "not in tests".into() } else { "Dragging the image out is only available on Windows and macOS".into() })
}

#[cfg(target_os = "macos")]
mod native {
    use super::finish;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEventType, NSImage};
    use objc2_foundation::{NSArray, NSData, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    struct Ivars {
        path: PathBuf,
    }

    define_class!(
        /// The source of a drag-out: copies only, and cleans the temporary file up at the end.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "ShotCraftDragOutSource"]
        #[ivars = Ivars]
        struct DragSource;

        unsafe impl NSObjectProtocol for DragSource {}

        unsafe impl NSDraggingSource for DragSource {
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn source_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
                NSDragOperation::Copy
            }

            #[unsafe(method(draggingSession:endedAtPoint:operation:))]
            fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, operation: NSDragOperation) {
                finish(&self.ivars().path, !operation.is_empty());
            }
        }
    );

    impl DragSource {
        fn new(mtm: MainThreadMarker, path: &Path) -> Retained<Self> {
            let this = mtm.alloc::<Self>().set_ivars(Ivars { path: path.to_path_buf() });
            unsafe { msg_send![super(this), init] }
        }
    }

    thread_local! {
        /// The session doesn't own its source: keep the current one alive (the previous one is
        /// released when the next drag starts).
        static LIVE: RefCell<Option<Retained<DragSource>>> = const { RefCell::new(None) };
    }

    pub fn begin(path: &Path, preview_png: &[u8], preview: (f32, f32)) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("The drag must start on the main thread")?;
        let text = path.to_str().ok_or("The file name isn't valid text")?;
        autoreleasepool(|_| {
            let app = NSApplication::sharedApplication(mtm);
            let event = app.currentEvent().ok_or("No mouse event to start the drag from")?;
            if !matches!(event.r#type(), NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged) {
                return Err("The mouse button was already released".to_string());
            }
            // The export before this call can take a while: the button may be up by now while the
            // release is still queued and the current event is still the old drag.
            if !super::primary_button_down(objc2_app_kit::NSEvent::pressedMouseButtons()) {
                return Err("The mouse button was already released".to_string());
            }
            let window = event.window(mtm).ok_or("The mouse event has no window")?;
            let view = window.contentView().ok_or("The window has no content view")?;

            let url = NSURL::fileURLWithPath(&NSString::from_str(text));
            let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*url));
            let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(preview_png));
            let at = view.convertPoint_fromView(event.locationInWindow(), None);
            let frame = NSRect::new(NSPoint::new(at.x - preview.0 as f64 / 2.0, at.y - preview.1 as f64 / 2.0), NSSize::new(preview.0 as f64, preview.1 as f64));
            let contents = image.as_deref().map(|i| i as &AnyObject);
            unsafe { item.setDraggingFrame_contents(frame, contents) };

            let source = DragSource::new(mtm, path);
            let items = NSArray::from_retained_slice(&[item]);
            view.beginDraggingSessionWithItems_event_source(&items, &event, ProtocolObject::from_ref(&*source));
            LIVE.with(|l| *l.borrow_mut() = Some(source));
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a drop target gets on Windows: the shell's data object for the exported PNG offers the
    /// file the way Explorer does (CF_HDROP). No drag is started.
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_offers_the_exported_png_as_a_file() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
        // SAFETY: COM for this test thread (a second call on the same thread is harmless).
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let dir = std::env::temp_dir().join(format!("shotcraft-dragout-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let img = Image { w: 4, h: 3, px: std::sync::Arc::new(vec![200; 4 * 3 * 4]) };
        let path = export_in(&dir, &img, "Drag test").expect("exported");
        let data = super::shell_data_object(&path).expect("the shell made a data object");
        let hdrop = FORMATETC { cfFormat: 15, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
        // SAFETY: asks whether the object has that format; nothing is read.
        assert!(unsafe { data.QueryGetData(&hdrop) }.is_ok(), "the file is offered as CF_HDROP");
        drop(data);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(super::shell_data_object(&dir.join("gone.png")).is_err(), "no object for a missing file");
    }
    use std::sync::Arc;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shotcraft-dragout-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn picture() -> Image {
        Image { w: 4, h: 3, px: Arc::new((0..4 * 3).flat_map(|i| [i as u8 * 20, 40, 200, 255]).collect()) }
    }

    fn age(path: &Path, by: Duration) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - by).unwrap();
    }

    #[test]
    fn names_are_safe_to_hand_to_other_apps() {
        assert_eq!(safe_stem("ShotCraft-20261104-101500"), "ShotCraft-20261104-101500");
        assert_eq!(safe_stem("a/b\\c:d"), "a-b-c-d");
        assert!(!safe_stem("../../etc/passwd").contains('/'));
        assert_eq!(safe_stem(".hidden"), "hidden");
        assert_eq!(safe_stem("tab\there\nnew"), "tab-here-new");
        assert_eq!(safe_stem("  "), FALLBACK_STEM);
        assert_eq!(safe_stem(""), FALLBACK_STEM);
        assert_eq!(safe_stem("..."), FALLBACK_STEM);
        let long = safe_stem(&"é".repeat(300));
        assert_eq!(long.chars().count(), STEM_MAX_CHARS);
    }

    #[test]
    fn export_writes_a_png_named_by_the_stem() {
        let dir = scratch("export");
        let path = export_in(&dir, &picture(), "ShotCraft-1").unwrap();
        assert_eq!(path, dir.join("ShotCraft-1.png"));
        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!((back.width(), back.height()), (4, 3));
        assert_eq!(back.as_raw().as_slice(), picture().px.as_slice(), "the file is the exported picture");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_drag_with_the_same_name_never_replaces_the_first_file() {
        let dir = scratch("unique");
        let a = export_in(&dir, &picture(), "same").unwrap();
        let b = export_in(&dir, &picture(), "same").unwrap();
        assert_ne!(a, b);
        assert_eq!(b, dir.join("same-2.png"));
        assert!(a.exists() && b.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sweeping_removes_only_old_files() {
        let dir = scratch("sweep");
        let old = dir.join("old.png");
        let fresh = dir.join("fresh.png");
        std::fs::write(&old, b"x").unwrap();
        std::fs::write(&fresh, b"x").unwrap();
        age(&old, KEEP_FOR + Duration::from_secs(60));
        assert_eq!(stale_files(&dir, SystemTime::now(), KEEP_FOR), vec![old.clone()]);
        assert_eq!(sweep_dir(&dir, KEEP_FOR), 1);
        assert!(!old.exists() && fresh.exists());
        assert_eq!(sweep_dir(&dir, KEEP_FOR), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exporting_sweeps_the_leftovers_of_earlier_drags() {
        let dir = scratch("export-sweep");
        let old = dir.join("left-over.png");
        std::fs::write(&old, b"x").unwrap();
        age(&old, KEEP_FOR * 2);
        let new = export_in(&dir, &picture(), "now").unwrap();
        assert!(!old.exists() && new.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sweeping_a_missing_folder_is_harmless() {
        let dir = std::env::temp_dir().join("shotcraft-dragout-test-missing-folder");
        assert!(stale_files(&dir, SystemTime::now(), KEEP_FOR).is_empty());
        assert_eq!(sweep_dir(&dir, KEEP_FOR), 0);
    }

    #[test]
    fn a_cancelled_drag_removes_its_file_at_once_and_a_drop_keeps_it() {
        assert_eq!(cleanup_after(false), Cleanup::Now);
        assert_eq!(cleanup_after(true), Cleanup::After(KEEP_FOR));
        let dir = scratch("finish");
        let path = dir.join("cancelled.png");
        std::fs::write(&path, b"x").unwrap();
        finish(&path, false);
        assert!(!path.exists());
        let kept = dir.join("dropped.png");
        std::fs::write(&kept, b"x").unwrap();
        finish(&kept, true);
        assert!(kept.exists(), "a dropped file stays for the target to read");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_native_drag_releases_the_button_for_egui() {
        use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
        let ev = release_events(Pos2::new(10.0, 20.0), Modifiers::NONE);
        assert!(matches!(ev[0], Event::PointerButton { pressed: false, button: PointerButton::Primary, pos, .. } if pos == Pos2::new(10.0, 20.0)));
        assert!(matches!(ev[1], Event::PointerGone));
    }

    #[test]
    fn only_the_primary_button_counts_for_a_drag() {
        assert!(primary_button_down(0b1) && primary_button_down(0b11));
        assert!(!primary_button_down(0) && !primary_button_down(0b10));
    }

    #[test]
    fn temp_folder_is_ours_inside_the_system_one() {
        assert_eq!(temp_dir().file_name().unwrap(), TEMP_DIR_NAME);
        assert_eq!(temp_dir().parent().unwrap(), std::env::temp_dir());
    }

    #[test]
    fn preview_keeps_the_aspect_and_never_grows() {
        let (w, h) = preview_size(1600, 800);
        assert_eq!((w, h), (PREVIEW_MAX, PREVIEW_MAX / 2.0));
        assert_eq!(preview_size(40, 30), (40.0, 30.0));
        let (w, h) = preview_size(100, 4000);
        assert!(h <= PREVIEW_MAX && (w / h - 100.0 / 4000.0).abs() < 1e-3);
    }

    #[test]
    fn preview_png_is_a_small_picture() {
        let big = Image { w: 1000, h: 500, px: Arc::new(vec![200; 1000 * 500 * 4]) };
        let png = preview_png(&big).unwrap();
        let small = image::load_from_memory(&png).unwrap();
        assert_eq!(small.width() as f32, PREVIEW_MAX * 2.0);
        let tiny = preview_png(&picture()).unwrap();
        assert_eq!(image::load_from_memory(&tiny).unwrap().width(), 4);
    }
}
