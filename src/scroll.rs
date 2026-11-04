//! Scrolling capture: the user picks an area, ShotCraft scrolls the page under it (or the user does,
//! when it cannot), grabs the area again and again and stitches the frames (see `stitch`).
//!
//! The capture runs on its own thread, at its own pace, whatever the UI does: `run` is a loop over a
//! `Page` (grab, scroll, wait), which the app backs with `screencapture` and wheel events and the
//! tests back with a simulated page.
use crate::autoscroll;
use crate::model::{Image, R};
use crate::stitch::{frames_match, Push, Stitcher};
pub use crate::stitch::Direction;
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
/// In manual mode the capture ends by itself when the page has not moved for this long (once something
/// was stitched), so the user does not have to reach for Done.
pub const IDLE_FINISH: Duration = Duration::from_secs(3);
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
    /// Scrolling up: the page was scrolled back below the first frame.
    BelowStart,
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
            Note::BelowStart => "Below the start: not captured",
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
    pub fn start(rect: GrabRect, control_pos: [f32; 2], auto: bool, dir: Direction, queue: Queue, ctx: Context) -> Session {
        let stop = Arc::new(AtomicU8::new(RUNNING));
        let (mode, note) = if auto { (Mode::Auto, Note::ScrollingForYou) } else { (Mode::Manual, Note::NeedsAccessibility) };
        let progress = Arc::new(Mutex::new(Progress { height: 0, frames: 0, lost: 0, note, mode, esc_at: None }));
        let (s, p) = (stop.clone(), progress.clone());
        std::thread::spawn(move || {
            // A panic must not leave the control window up for ever: report it like any other error.
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut page = ScreenPage { rect, scroller: autoscroll::Scroller::new(rect), started: Instant::now() };
                let repaint = || ctx.request_repaint_of(control_id());
                run(&mut page, rect.w, rect.h, mode, dir, &s, &p, &repaint)
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
    /// Time since the capture started, on the same clock `sleep` advances.
    fn now(&self) -> Duration;
}

/// The real thing: `screencapture` and wheel events.
struct ScreenPage {
    rect: GrabRect,
    scroller: autoscroll::Scroller,
    started: Instant,
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

    fn now(&self) -> Duration {
        self.started.elapsed()
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

/// The capture loop. `area_w` is the area width in points, to know the display scale. Scrolling up
/// (`dir`) starts at the bottom of the page: ShotCraft scrolls the other way and the page grows upwards.
pub fn run(page: &mut dyn Page, area_w: u32, area_h: u32, mode: Mode, dir: Direction, stop: &AtomicU8, progress: &Mutex<Progress>, repaint: &dyn Fn()) -> Result<Option<Outcome>, String> {
    let lp = Loop { stop, progress, repaint };
    let mut mode = mode;
    let mut stitcher = Stitcher::with_direction(crate::stitch::MAX_HEIGHT, dir);
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
    let mut last_move = page.now();
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
                    Push::Revisited { .. } if stitcher.above_start() => if dir == Direction::Up { Note::BelowStart } else { Note::AboveStart },
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
                // The missing-permission warning stays until the page first moves.
                let kept = lp.progress.lock().unwrap().note == Note::NeedsAccessibility && stitcher.frames_used() <= 1 && note == Note::ScrollYourself;
                lp.show(&stitcher, lost, mode, if stitcher.is_capped() { Note::HeightLimit } else if kept { Note::NeedsAccessibility } else { note });
                if state == DONE || stitcher.is_capped() {
                    break;
                }
                if matches!(note, Note::Capturing | Note::AboveStart | Note::BelowStart | Note::ScrollSlower | Note::SizeChanged) || stitcher.frames_used() <= 1 {
                    last_move = page.now();
                } else if page.now().saturating_sub(last_move) >= IDLE_FINISH {
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
                if page.scroll(dir.sign() * step).is_err() {
                    // Cannot scroll (no permission, no events): leave it to the user.
                    mode = Mode::Manual;
                    last_move = page.now();
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
                            last_move = page.now();
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
                            last_move = page.now();
                            lp.show(&stitcher, lost, mode, if matches!(result, Push::SizeMismatch) { Note::SizeChanged } else { Note::LostTrack });
                            continue;
                        }
                        // Step back to the last frame that matched and try a shorter step.
                        let _ = page.scroll(-dir.sign() * step);
                        step = (step / 2).max(STEP_MIN);
                        undid = true;
                        note = Note::ScrollingForYou;
                        let _ = settle(page, &lp);
                    }
                    Push::First => note = Note::ScrollingForYou,
                    Push::Capped => note = Note::HeightLimit,
                }
                lp.show(&stitcher, lost, mode, if stitcher.is_capped() { Note::HeightLimit } else { note });
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use std::sync::atomic::AtomicU32;

    fn hash(a: u32, b: u32, c: u32) -> u32 {
        let mut x = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b) ^ c.wrapping_mul(0xc2b2_ae35);
        x ^= x >> 15;
        x = x.wrapping_mul(0x2c1b_3c6d);
        x ^= x >> 12;
        x
    }

    /// A mail-list-like page: look-alike rows (same checkbox, star and separator) with different words.
    fn inbox(w: u32, rows: u32, row_h: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, rows * row_h, Rgba([255, 255, 255, 255]));
        for r in 0..rows {
            let y0 = r * row_h;
            for x in 0..w {
                img.put_pixel(x, y0 + row_h - 1, Rgba([225, 228, 232, 255]));
            }
            for (x0, x1) in [(10, 20), (30, 40)] {
                for y in y0 + row_h / 2 - 5..y0 + row_h / 2 + 5 {
                    for x in x0..x1 {
                        img.put_pixel(x, y, Rgba([150, 150, 156, 255]));
                    }
                }
            }
            let mut x = 56;
            let mut k = 0;
            while x + 8 < w - 10 {
                let word = 10 + hash(1, r, k) % 36;
                for xx in x..(x + word).min(w - 10) {
                    for yy in y0 + row_h / 2 - 4..y0 + row_h / 2 + 4 {
                        if hash(2, xx / 2 + r * 977, yy) % 100 < 58 {
                            img.put_pixel(xx, yy, Rgba([40, 40, 48, 255]));
                        }
                    }
                }
                x += word + 7;
                k += 1;
            }
        }
        img
    }

    /// A page behind an area, scrolled by wheel events. Time is virtual: it moves with `sleep` and with each grab.
    struct FakePage {
        page: RgbaImage,
        area_h: u32,
        pos: i32,
        /// Where a smooth-scrolling page is heading; it covers `ease` of the distance per look.
        target: i32,
        ease: f32,
        /// The page has this many rows fixed at the top of the area (a sticky header).
        header: Option<RgbaImage>,
        /// The page has this many rows fixed at the bottom of the area (a sticky footer).
        footer: Option<RgbaImage>,
        grabs: u32,
        clock: Duration,
        /// Scroll events do nothing (no permission, or an app that ignores them).
        deaf: bool,
        /// Scripted positions for manual mode: the nth grab sees `script[n]` (the last one repeats).
        script: Vec<i32>,
        /// Press Done after this many grabs.
        done_after: Option<(u32, Arc<AtomicU8>)>,
        scrolls: Vec<i32>,
    }

    impl FakePage {
        fn new(page: RgbaImage, area_h: u32) -> FakePage {
            FakePage { page, area_h, pos: 0, target: 0, ease: 1.0, header: None, footer: None, grabs: 0, clock: Duration::ZERO, deaf: false, script: vec![], done_after: None, scrolls: vec![] }
        }

        /// The page is scrolled to its bottom, where a capture that goes up starts.
        fn at_bottom(mut self) -> FakePage {
            self.pos = self.max_pos();
            self.target = self.pos;
            self
        }

        fn max_pos(&self) -> i32 {
            (self.page.height() - self.area_h) as i32
        }
    }

    impl Page for FakePage {
        fn grab(&mut self) -> Result<RgbaImage, String> {
            self.grabs += 1;
            self.clock += Duration::from_millis(150);
            if let Some((n, stop)) = &self.done_after {
                if self.grabs >= *n {
                    stop.store(DONE, Ordering::SeqCst);
                }
            }
            if !self.script.is_empty() {
                self.pos = self.script[(self.grabs as usize - 1).min(self.script.len() - 1)];
            } else if self.pos != self.target {
                let d = ((self.target - self.pos) as f32 * self.ease).round() as i32;
                self.pos += if d == 0 { (self.target - self.pos).signum() } else { d };
            }
            let mut f = image::imageops::crop_imm(&self.page, 0, self.pos as u32, self.page.width(), self.area_h).to_image();
            if let Some(h) = &self.header {
                image::imageops::replace(&mut f, h, 0, 0);
            }
            if let Some(ft) = &self.footer {
                image::imageops::replace(&mut f, ft, 0, (self.area_h - ft.height()) as i64);
            }
            Ok(f)
        }

        fn scroll(&mut self, dy: i32) -> Result<(), String> {
            self.scrolls.push(dy);
            if !self.deaf {
                self.target = (self.target + dy).clamp(0, self.max_pos());
            }
            Ok(())
        }

        fn sleep(&mut self, d: Duration) {
            self.clock += d;
        }

        fn now(&self) -> Duration {
            self.clock
        }
    }

    fn progress(mode: Mode) -> Mutex<Progress> {
        Mutex::new(Progress { height: 0, frames: 0, lost: 0, note: Note::ScrollingForYou, mode, esc_at: None })
    }

    fn capture(page: &mut FakePage, mode: Mode, stop: &AtomicU8) -> (Result<Option<Outcome>, String>, Mutex<Progress>) {
        capture_dir(page, mode, Direction::Down, stop)
    }

    fn capture_dir(page: &mut FakePage, mode: Mode, dir: Direction, stop: &AtomicU8) -> (Result<Option<Outcome>, String>, Mutex<Progress>) {
        let p = progress(mode);
        let (w, h) = (page.page.width(), page.area_h);
        let repaints = AtomicU32::new(0);
        let r = run(page, w, h, mode, dir, stop, &p, &|| {
            repaints.fetch_add(1, Ordering::SeqCst);
        });
        assert!(repaints.load(Ordering::SeqCst) > 0, "the control is told to repaint");
        (r, p)
    }

    fn whole(page: &RgbaImage, h: u32) -> RgbaImage {
        image::imageops::crop_imm(page, 0, 0, page.width(), h).to_image()
    }

    #[test]
    fn auto_scrolling_captures_a_page_of_look_alike_rows_down_to_its_end() {
        let page = inbox(300, 90, 40);
        let mut fake = FakePage::new(page.clone(), 200);
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.lost, 0);
        assert!(!o.capped);
        assert_eq!(o.img.h, page.height());
        assert_eq!(o.img.px.as_slice(), page.as_raw().as_slice(), "stitched image is the page");
        assert_eq!(p.lock().unwrap().note, Note::EndOfPage);
        // Steps are half the area, never more than 90%.
        assert!(fake.scrolls.iter().all(|&s| s.abs() <= 180), "{:?}", fake.scrolls);
        assert_eq!(fake.scrolls[0], step_points(200));
    }

    #[test]
    fn auto_scrolling_up_captures_a_page_from_its_bottom_to_its_top() {
        let page = inbox(300, 90, 40);
        let mut fake = FakePage::new(page.clone(), 200).at_bottom();
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture_dir(&mut fake, Mode::Auto, Direction::Up, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.lost, 0);
        assert!(!o.capped);
        assert_eq!(o.img.h, page.height());
        assert_eq!(o.img.px.as_slice(), page.as_raw().as_slice(), "stitched image is the page");
        assert_eq!(p.lock().unwrap().note, Note::EndOfPage);
        // It scrolls up by half the area at a time.
        assert!(fake.scrolls.iter().all(|&s| s < 0 && s.abs() <= 180), "{:?}", fake.scrolls);
        assert_eq!(fake.scrolls[0], -step_points(200));
    }

    #[test]
    fn auto_scrolling_up_waits_for_smooth_scrolling_and_keeps_a_sticky_footer_once() {
        let page = inbox(300, 70, 40);
        let footer = {
            let mut f = RgbaImage::from_pixel(300, 36, Rgba([30, 90, 200, 255]));
            for x in (0..300).step_by(7) {
                for y in 0..36 {
                    f.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                }
            }
            f
        };
        let mut fake = FakePage::new(page.clone(), 200).at_bottom();
        fake.ease = 0.35;
        fake.footer = Some(footer.clone());
        let stop = AtomicU8::new(RUNNING);
        let (r, _) = capture_dir(&mut fake, Mode::Auto, Direction::Up, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.lost, 0);
        let mut expected = whole(&page, page.height());
        image::imageops::replace(&mut expected, &footer, 0, page.height() as i64 - 36);
        assert_eq!(o.img.h, page.height());
        assert_eq!(o.img.px.as_slice(), expected.as_raw().as_slice());
    }

    #[test]
    fn auto_scrolling_up_from_the_top_of_a_page_hands_over_to_the_user() {
        // The page is already at its top: nothing moves when scrolled up.
        let mut fake = FakePage::new(inbox(300, 40, 40), 200);
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((40, stop.clone()));
        let (r, p) = capture_dir(&mut fake, Mode::Auto, Direction::Up, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.frames, 1);
        assert_eq!(p.lock().unwrap().mode, Mode::Manual);
        assert!(fake.scrolls.iter().all(|&s| s < 0), "{:?}", fake.scrolls);
    }

    #[test]
    fn auto_scrolling_up_stops_at_the_height_limit_keeping_the_bottom() {
        let page = inbox(120, 2200, 10);
        let mut fake = FakePage::new(page.clone(), 200).at_bottom();
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture_dir(&mut fake, Mode::Auto, Direction::Up, &stop);
        let o = r.unwrap().unwrap();
        assert!(o.capped);
        assert!(o.img.h <= crate::stitch::MAX_HEIGHT && o.img.h > crate::stitch::MAX_HEIGHT - 400);
        let from = (page.height() - o.img.h) as usize * 120 * 4;
        assert_eq!(o.img.px.as_slice(), &page.as_raw()[from..], "the last rows of the page are kept");
        assert_eq!(p.lock().unwrap().note, Note::HeightLimit);
    }

    #[test]
    fn manual_capture_going_up_follows_the_user_and_ends_by_itself() {
        let page = inbox(300, 40, 40);
        let top = page.height() as i32 - 200;
        let mut fake = FakePage::new(page.clone(), 200);
        // The user scrolls up for ten grabs, 60 rows each, then lets go.
        fake.script = (0..10).map(|i| top - i * 60).collect();
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture_dir(&mut fake, Mode::Manual, Direction::Up, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.img.h, 540 + 200);
        let from = (page.height() - o.img.h) as usize * 300 * 4;
        assert_eq!(o.img.px.as_slice(), &page.as_raw()[from..]);
        assert_eq!(p.lock().unwrap().mode, Mode::Manual);
    }

    #[test]
    fn manual_capture_going_back_down_is_flagged() {
        let page = inbox(300, 40, 40);
        // Started in the middle of the page, not at its bottom: going down again leaves the captured part.
        let start = page.height() as i32 - 600;
        let mut fake = FakePage::new(page, 200);
        fake.script = vec![start, start - 60, start - 120, start - 60, start + 60];
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((5, stop.clone()));
        let p = progress(Mode::Manual);
        let notes = Mutex::new(Vec::new());
        let r = run(&mut fake, 300, 200, Mode::Manual, Direction::Up, &stop, &p, &|| notes.lock().unwrap().push(p.lock().unwrap().note)).unwrap().unwrap();
        assert_eq!(r.img.h, 120 + 200);
        assert!(notes.lock().unwrap().contains(&Note::BelowStart), "{:?}", notes.lock().unwrap());
        assert_eq!(Note::BelowStart.text(), "Below the start: not captured");
    }

    #[test]
    fn auto_scrolling_waits_for_smooth_scrolling_to_settle() {
        let page = inbox(300, 60, 40);
        let mut fake = FakePage::new(page.clone(), 200);
        fake.ease = 0.35;
        let stop = AtomicU8::new(RUNNING);
        let (r, _) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.img.px.as_slice(), page.as_raw().as_slice());
    }

    #[test]
    fn auto_scrolling_keeps_a_sticky_header_once() {
        let page = inbox(300, 80, 40);
        let header = {
            let mut h = RgbaImage::from_pixel(300, 36, Rgba([30, 90, 200, 255]));
            for x in (0..300).step_by(7) {
                for y in 0..36 {
                    h.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                }
            }
            h
        };
        let mut fake = FakePage::new(page.clone(), 200);
        fake.header = Some(header.clone());
        let stop = AtomicU8::new(RUNNING);
        let (r, _) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.lost, 0);
        let mut expected = whole(&page, page.height());
        image::imageops::replace(&mut expected, &header, 0, 0);
        assert_eq!(o.img.h, page.height());
        assert_eq!(o.img.px.as_slice(), expected.as_raw().as_slice());
    }

    #[test]
    fn auto_scrolling_that_moves_nothing_hands_over_to_the_user() {
        let mut fake = FakePage::new(inbox(300, 40, 40), 200);
        fake.deaf = true;
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((40, stop.clone()));
        let (r, p) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.frames, 1);
        let p = p.lock().unwrap();
        assert_eq!(p.mode, Mode::Manual);
        assert!(matches!(p.note, Note::PageDidNotMove | Note::CheckScreenRecording | Note::ScrollYourself), "{:?}", p.note);
    }

    #[test]
    fn auto_scrolling_stops_at_the_height_limit() {
        // 40 rows of 10 px per screen: the page is much taller than the limit.
        let page = inbox(120, 2200, 10);
        let mut fake = FakePage::new(page, 200);
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert!(o.capped);
        assert!(o.img.h <= crate::stitch::MAX_HEIGHT);
        assert!(o.img.h > crate::stitch::MAX_HEIGHT - 400);
        assert_eq!(p.lock().unwrap().note, Note::HeightLimit);
    }

    #[test]
    fn cancel_throws_the_capture_away() {
        let mut fake = FakePage::new(inbox(300, 40, 40), 200);
        let stop = AtomicU8::new(CANCEL);
        let (r, _) = capture(&mut fake, Mode::Auto, &stop);
        assert!(matches!(r, Ok(None)));
    }

    #[test]
    fn done_keeps_what_was_captured() {
        let page = inbox(300, 40, 40);
        let mut fake = FakePage::new(page.clone(), 200);
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((12, stop.clone()));
        let (r, _) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert!(o.frames >= 2 && o.img.h > 200);
        assert_eq!(o.img.px.as_slice(), &page.as_raw()[..(o.img.w * o.img.h * 4) as usize]);
    }

    #[test]
    fn manual_capture_ends_by_itself_three_seconds_after_the_page_stops_moving() {
        let page = inbox(300, 40, 40);
        let mut fake = FakePage::new(page.clone(), 200);
        // The user scrolls for ten grabs, 60 rows each, then lets go.
        fake.script = (0..10).map(|i| i * 60).collect();
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture(&mut fake, Mode::Manual, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.img.h, 540 + 200);
        assert_eq!(o.img.px.as_slice(), &page.as_raw()[..(o.img.w * o.img.h * 4) as usize]);
        // Virtual time spent after the last movement: about the idle delay, not much more.
        assert!(fake.clock >= Duration::from_millis(150 * 10) + IDLE_FINISH);
        assert!(fake.clock <= Duration::from_millis(150 * 10) + IDLE_FINISH + Duration::from_secs(2), "{:?}", fake.clock);
        assert_eq!(p.lock().unwrap().mode, Mode::Manual);
    }

    #[test]
    fn manual_capture_that_never_moved_waits_for_the_user() {
        let mut fake = FakePage::new(inbox(300, 40, 40), 200);
        let stop = Arc::new(AtomicU8::new(RUNNING));
        // 100 grabs is 15 s of virtual time: far more than the idle delay.
        fake.done_after = Some((100, stop.clone()));
        let (r, _) = capture(&mut fake, Mode::Manual, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.frames, 1);
        assert!(fake.grabs >= 100);
    }

    #[test]
    fn manual_scrolling_too_fast_says_so_and_keeps_the_rest() {
        let page = inbox(300, 60, 40);
        let mut fake = FakePage::new(page, 200);
        // From 0 to 1500 in one jump: no overlap left to match.
        fake.script = vec![0, 0, 60, 120, 1500, 1500, 1500];
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((7, stop.clone()));
        let p = progress(Mode::Manual);
        let r = run(&mut fake, 300, 200, Mode::Manual, Direction::Down, &stop, &p, &|| {}).unwrap().unwrap();
        assert!(r.lost > 0);
        assert_eq!(p.lock().unwrap().note, Note::ScrollSlower);
        assert_eq!(r.img.h, 120 + 200);
    }

    #[test]
    fn escape_cancels_at_once_when_the_app_scrolls_and_asks_twice_when_the_user_does() {
        let stop = AtomicU8::new(RUNNING);
        let auto = progress(Mode::Auto);
        press_escape(&stop, &auto);
        assert_eq!(stop.load(Ordering::SeqCst), CANCEL);

        let stop = AtomicU8::new(RUNNING);
        let manual = progress(Mode::Manual);
        manual.lock().unwrap().frames = 5;
        press_escape(&stop, &manual);
        assert_eq!(stop.load(Ordering::SeqCst), RUNNING, "first press only asks");
        press_escape(&stop, &manual);
        assert_eq!(stop.load(Ordering::SeqCst), CANCEL);

        let stop = AtomicU8::new(RUNNING);
        let empty = progress(Mode::Manual);
        empty.lock().unwrap().frames = 1;
        press_escape(&stop, &empty);
        assert_eq!(stop.load(Ordering::SeqCst), CANCEL, "nothing to lose");
    }

    #[test]
    fn the_scroll_step_is_half_the_area_within_bounds() {
        assert_eq!(step_points(400), 200);
        assert_eq!(step_points(20), STEP_MIN);
        assert_eq!(step_points(5000), STEP_MAX);
        for h in [48, 100, 400, 984, 3000] {
            assert!(step_points(h) as f32 <= h as f32 * 0.9 || step_points(h) == STEP_MIN);
        }
    }

    // Control placement.

    const SCREEN: R = R { x0: 0.0, y0: 24.0, x1: 1512.0, y1: 944.0 };

    fn area(x0: f32, y0: f32, x1: f32, y1: f32) -> R {
        R { x0, y0, x1, y1 }
    }

    fn placed(a: &R, screens: &[R]) -> Option<R> {
        control_position(a, CONTROL_SIZE, screens).map(|[x, y]| R { x0: x, y0: y, x1: x + CONTROL_SIZE[0], y1: y + CONTROL_SIZE[1] })
    }

    fn overlaps(a: &R, b: &R) -> bool {
        a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
    }

    #[test]
    fn the_control_goes_below_the_area_centred_on_it() {
        let a = area(200.0, 100.0, 800.0, 500.0);
        let c = placed(&a, &[SCREEN]).unwrap();
        assert_eq!(c.y0, a.y1 + GAP);
        assert!((((c.x0 + c.x1) / 2.0) - 500.0).abs() < 0.01);
        assert!(!overlaps(&c, &a));
    }

    #[test]
    fn the_control_goes_above_when_there_is_no_room_below() {
        let a = area(200.0, 300.0, 800.0, 930.0);
        let c = placed(&a, &[SCREEN]).unwrap();
        assert_eq!(c.y1, a.y0 - GAP);
        assert!(!overlaps(&c, &a));
    }

    #[test]
    fn the_control_goes_beside_a_full_height_area() {
        let a = area(300.0, 24.0, 900.0, 944.0);
        let c = placed(&a, &[SCREEN]).unwrap();
        assert!(c.x0 >= a.x1 + GAP, "{c:?}");
        assert!(c.y0 >= SCREEN.y0 && c.y1 <= SCREEN.y1);
        let left = area(900.0, 24.0, 1500.0, 944.0);
        let c = placed(&left, &[SCREEN]).unwrap();
        assert!(c.x1 <= left.x0 - GAP, "{c:?}");
    }

    #[test]
    fn the_control_stays_inside_the_screen_when_the_area_touches_an_edge() {
        let a = area(0.0, 100.0, 120.0, 500.0);
        let c = placed(&a, &[SCREEN]).unwrap();
        assert!(c.x0 >= SCREEN.x0 && c.x1 <= SCREEN.x1);
        let a = area(1400.0, 100.0, 1512.0, 500.0);
        let c = placed(&a, &[SCREEN]).unwrap();
        assert!(c.x0 >= SCREEN.x0 && c.x1 <= SCREEN.x1, "{c:?}");
    }

    #[test]
    fn no_room_means_no_placement_instead_of_covering_the_area() {
        assert!(placed(&SCREEN, &[SCREEN]).is_none());
        let nearly = area(10.0, 30.0, 1500.0, 940.0);
        let c = placed(&nearly, &[SCREEN]);
        assert!(c.is_none_or(|c| !overlaps(&c, &nearly)));
    }

    #[test]
    fn the_control_follows_the_display_the_area_is_on() {
        let second = R { x0: 1512.0, y0: -200.0, x1: 3432.0, y1: 880.0 };
        let screens = [SCREEN, second];
        let a = area(2000.0, 100.0, 2600.0, 500.0);
        let c = placed(&a, &screens).unwrap();
        assert!(c.x0 >= second.x0 && c.x1 <= second.x1 && c.y0 >= second.y0 && c.y1 <= second.y1, "{c:?}");
        assert!(!overlaps(&c, &a));
        // A display to the left has negative coordinates.
        let left = R { x0: -1920.0, y0: 0.0, x1: 0.0, y1: 1080.0 };
        let a = area(-1500.0, 100.0, -600.0, 1000.0);
        let c = placed(&a, &[SCREEN, left]).unwrap();
        assert!(c.x0 >= left.x0 && c.x1 <= left.x1 && c.y1 <= left.y1, "{c:?}");
        assert!(!overlaps(&c, &a));
    }

    #[test]
    fn grab_rect_rounds_outwards_and_refuses_tiny_areas() {
        let g = GrabRect::from_area(&area(10.4, 20.6, 110.2, 90.1)).unwrap();
        assert_eq!(g, GrabRect { x: 10, y: 20, w: 101, h: 71 });
        assert!(GrabRect::from_area(&area(0.0, 0.0, 30.0, 300.0)).is_none());
    }
}
