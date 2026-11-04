//! Scrolling capture: the user picks an area, ShotCraft scrolls the page under it (or the user does,
//! when it cannot), grabs the area again and again and stitches the frames (see `stitch`).
//!
//! The capture runs on its own thread, at its own pace, whatever the UI does: `run` is a loop over a
//! `Page` (grab, scroll, wait), which the app backs with `screencapture` and wheel events and the
//! tests back with a simulated page.
use crate::autoscroll;
use crate::model::{Image, R};
use crate::stitch::{frames_match, Push, Stitcher};
use crate::{system, Action, Queue};
use eframe::egui::{Context, ViewportId};
use image::RgbaImage;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Pause between two grabs in manual mode. Each grab runs `screencapture` (about 150 ms), so the real rate is
/// about four frames per second.
const FRAME_INTERVAL: Duration = Duration::from_millis(100);
/// Consecutive failed grabs before the capture gives up (usually a missing Screen Recording permission).
const MAX_FAILURES: u32 = 3;
/// The control window, in points.
pub const CONTROL_SIZE: [f32; 2] = [280.0, 124.0];
/// Space kept between the captured area and the control window, in points.
const GAP: f32 = 14.0;
/// Areas smaller than this (points) are refused.
const MIN_SIDE: f32 = 48.0;

/// Share of the area height the page is scrolled by between two grabs.
const STEP_SHARE: f32 = 0.5;
/// Bounds of one scroll step, in points.
const STEP_MIN: i32 = 40;
const STEP_MAX: i32 = 700;
/// After a scroll, wait this long before the first look: the page needs a moment to start moving.
const SETTLE_FIRST: Duration = Duration::from_millis(110);
/// Then look again after this, until two looks agree (the page has stopped moving)...
const SETTLE_POLL: Duration = Duration::from_millis(60);
/// ... or this many looks have been taken (a page that never stops: a video, an animation).
const SETTLE_MAX_LOOKS: u32 = 15;
/// Before the page is declared unable to move, look once more after this.
const SETTLE_RETRY: Duration = Duration::from_millis(300);
/// Frames dropped in a row before automatic scrolling gives up and leaves the scrolling to the user.
const MAX_LOST_IN_ROW: u32 = 3;

const RUNNING: u8 = 0;
const DONE: u8 = 1;
const CANCEL: u8 = 2;

/// Window title of the control; `app_mode::bring_to_front` finds the window by it.
pub const CONTROL_TITLE: &str = "ShotCraft Scrolling Capture";

pub fn control_id() -> ViewportId {
    ViewportId::from_hash_of("scroll-control")
}

/// The area to grab, in whole points from the top left of the main display (what `screencapture -R` takes).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GrabRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl GrabRect {
    pub fn from_area(r: &R) -> Option<GrabRect> {
        if r.w() < MIN_SIDE || r.h() < MIN_SIDE {
            return None;
        }
        // Round outwards so that the grabbed area always holds the selected one.
        let (x, y) = (r.x0.floor() as i32, r.y0.floor() as i32);
        Some(GrabRect { x, y, w: (r.x1.ceil() as i32 - x) as u32, h: (r.y1.ceil() as i32 - y) as u32 })
    }

    pub fn as_r(&self) -> R {
        R { x0: self.x as f32, y0: self.y as f32, x1: (self.x + self.w as i32) as f32, y1: (self.y + self.h as i32) as f32 }
    }
}

/// Who scrolls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// ShotCraft posts the wheel events.
    Auto,
    /// The user scrolls.
    Manual,
}

/// What the control tells the user besides the numbers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    ScrollingForYou,
    ScrollYourself,
    Capturing,
    /// The page was scrolled above the first frame.
    AboveStart,
    /// The stitcher could not match a frame in manual mode.
    ScrollSlower,
    /// Automatic scrolling could not follow the page and handed over.
    LostTrack,
    SizeChanged,
    HeightLimit,
    EndOfPage,
    /// Automatic scrolling is off: the Accessibility permission is missing.
    NeedsAccessibility,
    /// Automatic scrolling could not move the page.
    PageDidNotMove,
    /// The page did not move and the Screen Recording permission looks off.
    CheckScreenRecording,
    EscAgain,
}

impl Note {
    pub fn text(self) -> &'static str {
        match self {
            Note::ScrollingForYou => "Scrolling for you",
            Note::ScrollYourself => "Scroll the page",
            Note::Capturing => "Capturing",
            Note::AboveStart => "Above the start: not captured",
            Note::ScrollSlower => "Scroll a bit slower",
            Note::LostTrack => "Lost track: scroll by hand, slowly",
            Note::SizeChanged => "The area changed size",
            Note::HeightLimit => "Height limit reached",
            Note::EndOfPage => "End of the page",
            Note::NeedsAccessibility => "Allow Accessibility to scroll for you",
            Note::PageDidNotMove => "The page didn't move: scroll it yourself",
            Note::CheckScreenRecording => "Nothing changes: check Screen Recording",
            Note::EscAgain => "Press Esc again to cancel",
        }
    }

    /// Notes the user should act on.
    pub fn is_warning(self) -> bool {
        matches!(self, Note::ScrollSlower | Note::LostTrack | Note::NeedsAccessibility | Note::PageDidNotMove | Note::CheckScreenRecording | Note::SizeChanged)
    }
}

pub struct Progress {
    pub height: u32,
    pub frames: u32,
    pub lost: u32,
    pub note: Note,
    pub mode: Mode,
    /// When Esc was pressed once; a second press within `ESC_WINDOW` cancels.
    esc_at: Option<Instant>,
}

/// Esc can be a key meant for the page being scrolled by hand, so a manual capture with something
/// in it needs two presses.
const ESC_WINDOW: Duration = Duration::from_secs(2);

/// Esc: cancels at once when ShotCraft does the scrolling or nothing is captured yet, otherwise asks for a second press.
fn press_escape(stop: &AtomicU8, progress: &Mutex<Progress>) {
    let mut p = progress.lock().unwrap();
    if p.mode == Mode::Auto || p.frames <= 1 || p.esc_at.is_some_and(|t| t.elapsed() < ESC_WINDOW) {
        stop.store(CANCEL, Ordering::SeqCst);
    } else {
        p.esc_at = Some(Instant::now());
    }
}

/// What a finished capture hands to the app.
pub struct Outcome {
    pub img: Image,
    /// Pixels per point of the display the area is on: frame width over area width.
    pub scale: f32,
    /// Frames that counted. 1 means the page never moved.
    pub frames: u32,
    /// Frames dropped because the offset could not be found.
    pub lost: u32,
    pub capped: bool,
}

pub struct Session {
    pub control_pos: [f32; 2],
    stop: Arc<AtomicU8>,
    progress: Arc<Mutex<Progress>>,
}

impl Session {
    /// Starts the capture thread. `auto` is whether ShotCraft may scroll the page (the Accessibility
    /// permission is granted).
    pub fn start(rect: GrabRect, control_pos: [f32; 2], auto: bool, queue: Queue, ctx: Context) -> Session {
        let stop = Arc::new(AtomicU8::new(RUNNING));
        let (mode, note) = if auto { (Mode::Auto, Note::ScrollingForYou) } else { (Mode::Manual, Note::NeedsAccessibility) };
        let progress = Arc::new(Mutex::new(Progress { height: 0, frames: 0, lost: 0, note, mode, esc_at: None }));
        let (s, p) = (stop.clone(), progress.clone());
        std::thread::spawn(move || {
            // A panic must not leave the control window up for ever: report it like any other error.
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut page = ScreenPage { rect, scroller: autoscroll::Scroller::new(rect) };
                let repaint = || ctx.request_repaint_of(control_id());
                run(&mut page, rect.w, rect.h, mode, &s, &p, &repaint)
            }))
            .unwrap_or_else(|_| Err("The scrolling capture stopped unexpectedly.".to_string()));
            queue.lock().unwrap().push(Action::ScrollingFinished(res));
            ctx.request_repaint();
        });
        Session { control_pos, stop, progress }
    }

    /// Ends the capture and keeps what was stitched.
    pub fn finish(&self) {
        let _ = self.stop.compare_exchange(RUNNING, DONE, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// The Esc key.
    pub fn press_escape(&self) {
        press_escape(&self.stop, &self.progress);
    }

    /// Draws the control window's content. `body` runs on the deferred viewport, so it only takes shared state.
    pub fn control_body(&self) -> impl Fn(&mut eframe::egui::Ui) + Send + Sync + 'static {
        let (stop, progress) = (self.stop.clone(), self.progress.clone());
        move |ui| control_ui(ui, &stop, &progress)
    }
}

fn control_ui(ui: &mut eframe::egui::Ui, stop: &AtomicU8, progress: &Mutex<Progress>) {
    use crate::scroll_control::{draw, Click, View};
    let view = {
        let p = progress.lock().unwrap();
        let asked = p.esc_at.is_some_and(|t| t.elapsed() < ESC_WINDOW);
        View { height: p.height, frames: p.frames, note: if asked { Note::EscAgain } else { p.note }, auto: p.mode == Mode::Auto }
    };
    if ui.input(|i| i.key_pressed(eframe::egui::Key::Escape)) {
        press_escape(stop, progress);
        ui.ctx().request_repaint_after(ESC_WINDOW);
    }
    let click = draw(ui, &view);
    let done = click == Click::Done || ui.input(|i| i.key_pressed(eframe::egui::Key::Enter));
    if click == Click::Cancel {
        stop.store(CANCEL, Ordering::SeqCst);
    } else if done {
        let _ = stop.compare_exchange(RUNNING, DONE, Ordering::SeqCst, Ordering::SeqCst);
    }
    if click != Click::None || done {
        ui.ctx().request_repaint_of(ViewportId::ROOT);
    }
}

/// Everything the capture loop needs from the outside world.
pub trait Page {
    /// One look at the area, as pixels.
    fn grab(&mut self) -> Result<RgbaImage, String>;
    /// Scrolls the page down by `dy` points (up when negative).
    fn scroll(&mut self, dy: i32) -> Result<(), String>;
    fn sleep(&mut self, d: Duration);
}

/// The real thing: `screencapture` and wheel events.
struct ScreenPage {
    rect: GrabRect,
    scroller: autoscroll::Scroller,
}

impl Page for ScreenPage {
    fn grab(&mut self) -> Result<RgbaImage, String> {
        grab(&self.rect)
    }

    fn scroll(&mut self, dy: i32) -> Result<(), String> {
        self.scroller.scroll(dy)
    }

    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Points the page is scrolled by between two grabs for an area this tall.
pub fn step_points(area_h: u32) -> i32 {
    ((area_h as f32 * STEP_SHARE) as i32).clamp(STEP_MIN, STEP_MAX)
}

struct Loop<'a> {
    stop: &'a AtomicU8,
    progress: &'a Mutex<Progress>,
    repaint: &'a dyn Fn(),
}

impl Loop<'_> {
    fn state(&self) -> u8 {
        self.stop.load(Ordering::SeqCst)
    }

    fn show(&self, stitcher: &Stitcher, lost: u32, mode: Mode, note: Note) {
        let mut p = self.progress.lock().unwrap();
        p.height = stitcher.height();
        p.frames = stitcher.frames_used();
        p.lost = lost;
        p.mode = mode;
        p.note = note;
        drop(p);
        (self.repaint)();
    }
}

/// A grab that survives a hiccup: `MAX_FAILURES` failures in a row are an error.
fn grab_retrying(page: &mut dyn Page) -> Result<RgbaImage, String> {
    let mut failures = 0;
    loop {
        match page.grab() {
            Ok(f) => return Ok(f),
            Err(e) => {
                failures += 1;
                if failures >= MAX_FAILURES {
                    return Err(e);
                }
                page.sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Waits for the page to stop moving after a scroll and returns how it looks then: the last of two
/// looks that agree, or the last look when the page never settles.
fn settle(page: &mut dyn Page, lp: &Loop) -> Result<RgbaImage, String> {
    page.sleep(SETTLE_FIRST);
    let mut prev = grab_retrying(page)?;
    for _ in 0..SETTLE_MAX_LOOKS {
        if lp.state() != RUNNING {
            break;
        }
        page.sleep(SETTLE_POLL);
        let cur = grab_retrying(page)?;
        if frames_match(&prev, &cur) {
            return Ok(cur);
        }
        prev = cur;
    }
    Ok(prev)
}

/// The capture loop. `area_w` is the area width in points, to know the display scale.
pub fn run(page: &mut dyn Page, area_w: u32, area_h: u32, mode: Mode, stop: &AtomicU8, progress: &Mutex<Progress>, repaint: &dyn Fn()) -> Result<Option<Outcome>, String> {
    let lp = Loop { stop, progress, repaint };
    let mut mode = mode;
    let mut stitcher = Stitcher::new();
    let mut lost = 0u32;
    let first = grab_retrying(page)?;
    let scale = (first.width() as f32 / area_w as f32).max(1.0);
    stitcher.push(&first);
    let start_note = if mode == Mode::Auto { Note::ScrollingForYou } else { lp.progress.lock().unwrap().note };
    lp.show(&stitcher, lost, mode, if mode == Mode::Manual && start_note == Note::ScrollingForYou { Note::ScrollYourself } else { start_note });

    // Auto mode state: points per step, how much the page really moves per point asked, and the
    // runs of dropped and motionless frames.
    let mut step = step_points(area_h);
    let mut ratio = 1.0f32;
    let mut lost_in_row = 0u32;
    let mut undid = false;
    loop {
        let state = lp.state();
        if state == CANCEL {
            return Ok(None);
        }
        let note;
        match mode {
            Mode::Manual => {
                let frame = match grab_retrying(page) {
                    Ok(f) => f,
                    Err(e) => {
                        // Keep what was stitched so far; only report an error when there is nothing.
                        if stitcher.frames_used() >= 1 {
                            break;
                        }
                        return Err(e);
                    }
                };
                note = match stitcher.push(&frame) {
                    Push::First | Push::Unchanged => Note::ScrollYourself,
                    Push::Revisited { .. } if stitcher.above_start() => Note::AboveStart,
                    Push::Appended { .. } | Push::Revisited { .. } => Note::Capturing,
                    Push::Lost => {
                        lost += 1;
                        Note::ScrollSlower
                    }
                    Push::SizeMismatch => {
                        lost += 1;
                        Note::SizeChanged
                    }
                    Push::Capped => Note::HeightLimit,
                };
                lp.show(&stitcher, lost, mode, note);
                if state == DONE || stitcher.is_capped() {
                    break;
                }
                // Sleep in short slices so that Done and Cancel react at once.
                for _ in 0..(FRAME_INTERVAL.as_millis() / 20) {
                    if lp.state() != RUNNING {
                        break;
                    }
                    page.sleep(Duration::from_millis(20));
                }
            }
            Mode::Auto => {
                if state == DONE {
                    break;
                }
                if page.scroll(step).is_err() {
                    // Cannot scroll (no permission, no events): leave it to the user.
                    mode = Mode::Manual;
                    lp.show(&stitcher, lost, mode, Note::NeedsAccessibility);
                    continue;
                }
                let frame = match settle(page, &lp) {
                    Ok(f) => f,
                    Err(e) => {
                        if stitcher.frames_used() >= 1 {
                            break;
                        }
                        return Err(e);
                    }
                };
                let hint = Some((step as f32 * scale * ratio).round() as i32);
                let mut result = stitcher.push_hinted(&frame, hint);
                if matches!(result, Push::Unchanged | Push::Revisited { .. }) && !undid {
                    // Not moving yet? Give a slow page one more moment before calling it the end.
                    page.sleep(SETTLE_RETRY);
                    if let Ok(frame) = settle(page, &lp) {
                        result = stitcher.push_hinted(&frame, hint);
                    }
                }
                match result {
                    Push::Appended { offset, .. } => {
                        lost_in_row = 0;
                        undid = false;
                        let moved = offset as f32 / (step as f32 * scale);
                        if moved.is_finite() && moved > 0.0 {
                            ratio = moved.clamp(0.2, 4.0);
                        }
                        note = Note::ScrollingForYou;
                    }
                    Push::Unchanged | Push::Revisited { .. } if !undid => {
                        if stitcher.frames_used() <= 1 {
                            // It never moved: no wheel events arrive, or there is nothing to scroll.
                            mode = Mode::Manual;
                            let sees = autoscroll::screen_recording_ok();
                            lp.show(&stitcher, lost, mode, if sees { Note::PageDidNotMove } else { Note::CheckScreenRecording });
                            continue;
                        }
                        lp.show(&stitcher, lost, mode, Note::EndOfPage);
                        break;
                    }
                    Push::Unchanged | Push::Revisited { .. } => {
                        // Right after stepping back: this is the previous frame again, as it should be.
                        undid = false;
                        note = Note::ScrollingForYou;
                    }
                    Push::Lost | Push::SizeMismatch => {
                        lost += 1;
                        lost_in_row += 1;
                        if lost_in_row >= MAX_LOST_IN_ROW || matches!(result, Push::SizeMismatch) {
                            mode = Mode::Manual;
                            lp.show(&stitcher, lost, mode, if matches!(result, Push::SizeMismatch) { Note::SizeChanged } else { Note::LostTrack });
                            continue;
                        }
                        // Step back to the last frame that matched and try a shorter step.
                        let _ = page.scroll(-step);
                        step = (step / 2).max(STEP_MIN);
                        undid = true;
                        note = Note::ScrollingForYou;
                        let _ = settle(page, &lp);
                    }
                    Push::First => note = Note::ScrollingForYou,
                    Push::Capped => note = Note::HeightLimit,
                }
                lp.show(&stitcher, lost, mode, note);
                if stitcher.is_capped() {
                    break;
                }
            }
        }
    }
    let (frames, capped) = (stitcher.frames_used(), stitcher.is_capped());
    let img = stitcher.finish().map_err(|_| "No image was captured.".to_string())?;
    let (w, h) = img.dimensions();
    let scale = (w as f32 / area_w as f32).max(1.0);
    Ok(Some(Outcome { img: Image { w, h, px: Arc::new(img.into_raw()) }, scale, frames, lost, capped }))
}

/// One look at the area, as pixels.
fn grab(rect: &GrabRect) -> Result<RgbaImage, String> {
    let path = system::temp_png("scroll");
    let _ = std::fs::remove_file(&path);
    let status = Command::new("/usr/sbin/screencapture")
        .arg("-x")
        .arg(format!("-R{},{},{},{}", rect.x, rect.y, rect.w, rect.h))
        .arg(&path)
        .status()
        .map_err(|e| format!("Can't take a screenshot: {e}"))?;
    if !status.success() || !path.exists() {
        return Err("Can't take a screenshot. ShotCraft needs the Screen Recording permission: System Settings → Privacy & Security → Screen Recording.".into());
    }
    let frame = image::open(&path).map_err(|e| format!("Can't read the captured frame: {e}"));
    let _ = std::fs::remove_file(&path);
    Ok(frame?.to_rgba8())
}

/// Lets the user draw the area to capture. `None` when they cancel.
pub fn select_area() -> Result<Option<R>, String> {
    let path = system::temp_png("scroll-area");
    let status = Command::new("/usr/sbin/screencapture").args(["-x", "-i", "-s"]).arg(&path).status().map_err(|e| format!("Can't take a screenshot: {e}"))?;
    if !path.exists() {
        if status.success() {
            return Ok(None);
        }
        return Err("Can't take a screenshot. ShotCraft needs the Screen Recording permission: System Settings → Privacy & Security → Screen Recording.".into());
    }
    let rect = system::capture_rect(&path);
    let _ = std::fs::remove_file(&path);
    rect.map(Some).ok_or_else(|| "Can't tell which area was selected.".to_string())
}

/// Where to put the control window so it never covers the captured area (it would end up in every
/// frame, and catch the mouse wheel): below the area, centred on it; above when there is no room
/// below; else on the right, then on the left. It stays inside the screen the area is on.
/// `None` when no side has room. Everything is in points from the top left of the main display.
pub fn control_position(area: &R, size: [f32; 2], screens: &[R]) -> Option<[f32; 2]> {
    let overlap = |a: &R, b: &R| (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0) * (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let screen = screens.iter().copied().max_by(|a, b| overlap(a, area).total_cmp(&overlap(b, area)))?;
    let [w, h] = size;
    let keep_out = area.expand(GAP - 1.0);
    let clamp_y = |y: f32| y.clamp(screen.y0, (screen.y1 - h).max(screen.y0));
    let clamp_x = |x: f32| x.clamp(screen.x0, (screen.x1 - w).max(screen.x0));
    let centre_x = clamp_x((area.x0 + area.x1) / 2.0 - w / 2.0);
    let candidates = [
        [centre_x, area.y1 + GAP],
        [centre_x, area.y0 - GAP - h],
        [area.x1 + GAP, clamp_y(area.y0)],
        [area.x0 - GAP - w, clamp_y(area.y0)],
    ];
    candidates.into_iter().find(|[x, y]| {
        let win = R { x0: *x, y0: *y, x1: x + w, y1: y + h };
        win.x0 >= screen.x0 && win.y0 >= screen.y0 && win.x1 <= screen.x1 && win.y1 <= screen.y1 && overlap(&win, &keep_out) == 0.0
    })
}

/// Usable area (menu bar and Dock excluded) of every display, from the top left of the main display.
#[cfg(target_os = "macos")]
pub fn screens() -> Vec<R> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    let Some(mtm) = MainThreadMarker::new() else { return vec![] };
    let all = NSScreen::screens(mtm);
    let Some(main) = all.iter().next() else { return vec![] };
    let top = main.frame().size.height;
    all.iter()
        .map(|s| {
            let f = s.visibleFrame();
            let (x, y_bottom) = (f.origin.x as f32, f.origin.y as f32);
            let (w, h) = (f.size.width as f32, f.size.height as f32);
            R { x0: x, y0: top as f32 - (y_bottom + h), x1: x + w, y1: top as f32 - y_bottom }
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn screens() -> Vec<R> {
    vec![R { x0: 0.0, y0: 0.0, x1: 1440.0, y1: 900.0 }]
}
