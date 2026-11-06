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
use eframe::egui::Context;
use image::RgbaImage;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Pause between two grabs in manual mode. Each grab runs `screencapture` (about 150 ms), so the real rate is
/// about four frames per second.
const FRAME_INTERVAL: Duration = Duration::from_millis(100);
/// In manual mode the capture ends by itself when the page has not moved for this long (once something
/// was stitched), so the user does not have to press Esc.
pub const IDLE_FINISH: Duration = Duration::from_secs(3);
/// Consecutive failed grabs before the capture gives up (usually a missing Screen Recording permission).
const MAX_FAILURES: u32 = 3;
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

/// Memory a stitched picture may take, in bytes (4 per pixel): a wide picture stops short of the height limit.
const MAX_BYTES: u64 = 1_000_000_000;
/// How long the page gets to settle after a scroll, by speed level (see `Limits::wait`): slow levels give it
/// more time, fast ones less.
const WAIT_SCALE: [f32; 5] = [2.0, 1.4, 1.0, 0.7, 0.5];
/// Share of the area height scrolled between two grabs, by speed level. Level 2 is `STEP_SHARE`.
const STEP_SHARES: [f32; 5] = [0.3, 0.4, STEP_SHARE, 0.6, 0.7];

/// What the scrolling settings ask of a capture.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Limits {
    /// Height the picture stops at, in pixels.
    pub max_height: u32,
    /// Speed level, 0 to `settings::SCROLL_SPEED_MAX`.
    pub speed: u8,
    /// The page moves the other way than the wheel says (an app that reverses the wheel).
    pub reverse: bool,
    /// The user scrolls, even when ShotCraft could.
    pub manual: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_height: crate::settings::SCROLL_MAX_HEIGHT, speed: crate::settings::SCROLL_SPEED_DEFAULT, reverse: false, manual: false }
    }
}

impl Limits {
    pub fn from_settings(s: &crate::settings::Settings) -> Limits {
        Limits { max_height: s.scroll_max_height(), speed: s.scroll_speed_level(), reverse: s.scroll_reverse, manual: s.scroll_manual }
    }

    /// Rows the picture may have when the frames are `width` pixels wide.
    pub fn max_rows(&self, width: u32) -> u32 {
        let by_memory = MAX_BYTES / (4 * width.max(1) as u64);
        self.max_height.min(by_memory.min(u32::MAX as u64) as u32).max(1)
    }

    /// `base` stretched or shrunk for the speed level.
    pub fn wait(&self, base: Duration) -> Duration {
        base.mul_f32(WAIT_SCALE[self.speed.min(4) as usize])
    }

    /// Points to scroll between two grabs for an area `area_h` points tall.
    pub fn step(&self, area_h: u32) -> i32 {
        ((area_h as f32 * STEP_SHARES[self.speed.min(4) as usize]) as i32).clamp(STEP_MIN, STEP_MAX)
    }

    /// The wheel amount that moves the page down by `dy`.
    pub fn wheel(&self, dy: i32) -> i32 {
        if self.reverse {
            -dy
        } else {
            dy
        }
    }
}

const RUNNING: u8 = 0;
/// Esc (or the hotkey again): stop and keep what was stitched. There is no way to throw a capture away.
const DONE: u8 = 1;

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
}

/// Who scrolls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// ShotCraft posts the wheel events.
    Auto,
    /// The user scrolls.
    Manual,
}

/// What the capture is doing, and what went wrong. There is no window to show it: the notes that
/// need the user's attention (`notice`) become one macOS notification per capture, at most.
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
}

impl Note {
    /// The notification text for a note the user should act on; `None` for the others. Every text
    /// that hands the scrolling over says how to end the capture.
    pub fn notice(self) -> Option<&'static str> {
        Some(match self {
            Note::ScrollSlower => "Scroll a bit slower: part of the page was missed. Press Esc when you are done.",
            Note::LostTrack => "ShotCraft lost track of the page. Scroll by hand, slowly, and press Esc when you are done.",
            Note::SizeChanged => "The captured area changed size. Press Esc to keep what was captured.",
            #[cfg(target_os = "macos")]
            Note::NeedsAccessibility => "To scroll for you, ShotCraft needs the Accessibility permission (System Settings > Privacy & Security). For now, scroll the page by hand and press Esc when you are done.",
            // Windows always lets ShotCraft scroll; an X server without XTest doesn't.
            #[cfg(not(target_os = "macos"))]
            Note::NeedsAccessibility => "ShotCraft can't scroll this page for you (the X server has no XTest). Scroll it by hand and press Esc when you are done.",
            Note::PageDidNotMove => "The page didn't move. Scroll it by hand and press Esc when you are done.",
            Note::CheckScreenRecording => "Nothing changes in the area. Check the Screen Recording permission in System Settings, then press Esc.",
            _ => return None,
        })
    }
}

pub struct Progress {
    pub height: u32,
    pub frames: u32,
    pub lost: u32,
    pub note: Note,
    pub mode: Mode,
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
    /// The height limit the capture ran with, in pixels.
    pub max_height: u32,
}

/// What to do with the notes of a capture: the first that needs the user becomes one message, the
/// others are dropped. `told` is set once `send` was called.
fn notifier<'a>(told: &'a AtomicBool, send: impl Fn(&str) + 'a) -> impl Fn(Note) + 'a {
    move |n| {
        if let Some(text) = n.notice() {
            if !told.swap(true, Ordering::SeqCst) {
                send(text);
            }
        }
    }
}

pub struct Session {
    stop: Arc<AtomicU8>,
    /// A notification was shown during the capture (see `Note::notice`): the app adds no second one.
    notified: Arc<AtomicBool>,
}

impl Session {
    /// Starts the capture thread. `auto` is whether ShotCraft may scroll the page (the Accessibility
    /// permission is granted).
    pub fn start(rect: GrabRect, auto: bool, dir: Direction, limits: Limits, queue: Queue, ctx: Context) -> Session {
        let stop = Arc::new(AtomicU8::new(RUNNING));
        let (mode, note) = match (auto, limits.manual) {
            (true, _) => (Mode::Auto, Note::ScrollingForYou),
            (false, true) => (Mode::Manual, Note::ScrollYourself),
            (false, false) => (Mode::Manual, Note::NeedsAccessibility),
        };
        let progress = Arc::new(Mutex::new(Progress { height: 0, frames: 0, lost: 0, note, mode }));
        let (s, notified) = (stop.clone(), Arc::new(AtomicBool::new(false)));
        let flag = notified.clone();
        std::thread::spawn(move || {
            // Windows and Linux: the area was drawn on ShotCraft's own frozen screens; they close in the
            // next frames, and the first look must see the page, not them.
            #[cfg(not(target_os = "macos"))]
            std::thread::sleep(Duration::from_millis(300));
            // A panic must not leave the app waiting for ever: report it like any other error.
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut page = ScreenPage { rect, scroller: autoscroll::Scroller::new(rect), started: Instant::now(), limits };
                let notify = notifier(&flag, |text| system::notify("ShotCraft", text));
                run_with(&mut page, rect.w, rect.h, mode, dir, &limits, &s, &progress, &notify)
            }))
            .unwrap_or_else(|_| Err("The scrolling capture stopped unexpectedly.".to_string()));
            // Windows and Linux grab the area in pixels: the picture has the scale of its screen.
            #[cfg(not(target_os = "macos"))]
            let res = res.map(|o| o.map(|o| Outcome { scale: crate::capture::scale_at(rect.x + (rect.w / 2) as i32, rect.y + (rect.h / 2) as i32), ..o }));
            queue.lock().unwrap().push(Action::ScrollingFinished(res));
            ctx.request_repaint();
        });
        Session { stop, notified }
    }

    /// Whether the capture already told the user something by notification.
    pub fn notified(&self) -> bool {
        self.notified.load(Ordering::SeqCst)
    }

    /// A session with no thread behind it, for the app's tests.
    #[cfg(test)]
    pub fn idle() -> Session {
        Session { stop: Arc::new(AtomicU8::new(RUNNING)), notified: Arc::new(AtomicBool::new(false)) }
    }

    #[cfg(test)]
    pub fn is_done(&self) -> bool {
        self.stop.load(Ordering::SeqCst) == DONE
    }

    /// Ends the capture and keeps what was stitched (Esc, or the hotkey again).
    pub fn finish(&self) {
        self.stop.store(DONE, Ordering::SeqCst);
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
    limits: Limits,
}

impl Page for ScreenPage {
    fn grab(&mut self) -> Result<RgbaImage, String> {
        grab(&self.rect)
    }

    fn scroll(&mut self, dy: i32) -> Result<(), String> {
        self.scroller.scroll(self.limits.wheel(dy))
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
    Limits::default().step(area_h)
}

struct Loop<'a> {
    stop: &'a AtomicU8,
    progress: &'a Mutex<Progress>,
    /// Told of every note the capture shows. The app turns the first one worth a word into a notification.
    notify: &'a dyn Fn(Note),
    limits: Limits,
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
        (self.notify)(note);
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
    page.sleep(lp.limits.wait(SETTLE_FIRST));
    let mut prev = grab_retrying(page)?;
    for _ in 0..SETTLE_MAX_LOOKS {
        if lp.state() != RUNNING {
            break;
        }
        page.sleep(lp.limits.wait(SETTLE_POLL));
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
pub fn run(page: &mut dyn Page, area_w: u32, area_h: u32, mode: Mode, dir: Direction, stop: &AtomicU8, progress: &Mutex<Progress>, notify: &dyn Fn(Note)) -> Result<Option<Outcome>, String> {
    run_with(page, area_w, area_h, mode, dir, &Limits::default(), stop, progress, notify)
}

/// `run` with the limits from the settings.
pub fn run_with(page: &mut dyn Page, area_w: u32, area_h: u32, mode: Mode, dir: Direction, limits: &Limits, stop: &AtomicU8, progress: &Mutex<Progress>, notify: &dyn Fn(Note)) -> Result<Option<Outcome>, String> {
    let lp = Loop { stop, progress, notify, limits: *limits };
    let mut mode = mode;
    let mut lost = 0u32;
    // Esc before anything was grabbed: nothing to keep.
    if lp.state() != RUNNING {
        return Ok(None);
    }
    let first = grab_retrying(page)?;
    let max_height = limits.max_rows(first.width());
    let mut stitcher = Stitcher::with_direction(max_height, dir);
    let scale = (first.width() as f32 / area_w as f32).max(1.0);
    stitcher.push(&first);
    let start_note = if mode == Mode::Auto { Note::ScrollingForYou } else { lp.progress.lock().unwrap().note };
    lp.show(&stitcher, lost, mode, if mode == Mode::Manual && start_note == Note::ScrollingForYou { Note::ScrollYourself } else { start_note });

    // Auto mode state: points per step, how much the page really moves per point asked, and the
    // runs of dropped and motionless frames.
    let mut step = limits.step(area_h);
    let mut ratio = 1.0f32;
    let mut lost_in_row = 0u32;
    let mut undid = false;
    let mut last_move = page.now();
    loop {
        let state = lp.state();
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
                // Sleep in short slices so that Esc reacts at once.
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
                    page.sleep(limits.wait(SETTLE_RETRY));
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
    if frames == 0 {
        return Ok(None);
    }
    let img = stitcher.finish().map_err(|_| "No image was captured.".to_string())?;
    let (w, h) = img.dimensions();
    let scale = (w as f32 / area_w as f32).max(1.0);
    Ok(Some(Outcome { img: Image { w, h, px: Arc::new(img.into_raw()) }, scale, frames, lost, capped, max_height }))
}

/// One look at the area, as pixels.
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
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

/// Windows and Linux: the area from the screen (`capture`). The user draws it on the frozen screens
/// (`picker`), so there is no `select_area` here.
#[cfg(not(target_os = "macos"))]
fn grab(rect: &GrabRect) -> Result<RgbaImage, String> {
    crate::capture::grab_area(rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

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
        /// Press Esc after this many grabs.
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
        Mutex::new(Progress { height: 0, frames: 0, lost: 0, note: Note::ScrollingForYou, mode })
    }

    fn capture(page: &mut FakePage, mode: Mode, stop: &AtomicU8) -> (Result<Option<Outcome>, String>, Mutex<Progress>) {
        capture_dir(page, mode, Direction::Down, stop)
    }

    fn capture_dir(page: &mut FakePage, mode: Mode, dir: Direction, stop: &AtomicU8) -> (Result<Option<Outcome>, String>, Mutex<Progress>) {
        let p = progress(mode);
        let (w, h) = (page.page.width(), page.area_h);
        let r = run(page, w, h, mode, dir, stop, &p, &|_| {});
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
        let r = run(&mut fake, 300, 200, Mode::Manual, Direction::Up, &stop, &p, &|n| notes.lock().unwrap().push(n)).unwrap().unwrap();
        assert_eq!(r.img.h, 120 + 200);
        assert!(notes.lock().unwrap().contains(&Note::BelowStart), "{:?}", notes.lock().unwrap());
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
    fn esc_before_anything_was_grabbed_ends_without_a_result() {
        let mut fake = FakePage::new(inbox(300, 40, 40), 200);
        let stop = AtomicU8::new(DONE);
        for mode in [Mode::Auto, Mode::Manual] {
            let (r, _) = capture(&mut fake, mode, &stop);
            assert!(matches!(r, Ok(None)), "{mode:?}");
        }
        assert_eq!(fake.grabs, 0, "nothing was even grabbed");
    }

    #[test]
    fn esc_keeps_what_was_captured() {
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
        let r = run(&mut fake, 300, 200, Mode::Manual, Direction::Down, &stop, &p, &|_| {}).unwrap().unwrap();
        assert!(r.lost > 0);
        assert_eq!(p.lock().unwrap().note, Note::ScrollSlower);
        assert_eq!(r.img.h, 120 + 200);
    }

    #[test]
    fn esc_in_manual_mode_keeps_the_frames_captured_so_far() {
        let page = inbox(300, 40, 40);
        let mut fake = FakePage::new(page.clone(), 200);
        fake.script = (0..30).map(|i| i * 40).collect();
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((8, stop.clone()));
        let (r, _) = capture(&mut fake, Mode::Manual, &stop);
        let o = r.unwrap().unwrap();
        assert!(o.frames >= 4 && o.img.h > 200, "{} frames", o.frames);
        assert_eq!(o.img.px.as_slice(), &page.as_raw()[..(o.img.w * o.img.h * 4) as usize]);
    }

    #[test]
    fn esc_in_a_scrolling_up_capture_keeps_the_bottom_of_the_page() {
        let page = inbox(300, 90, 40);
        let mut fake = FakePage::new(page.clone(), 200).at_bottom();
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((10, stop.clone()));
        let (r, _) = capture_dir(&mut fake, Mode::Auto, Direction::Up, &stop);
        let o = r.unwrap().unwrap();
        assert!(o.frames >= 2 && o.img.h > 200 && o.img.h < page.height());
        let from = (page.height() - o.img.h) as usize * 300 * 4;
        assert_eq!(o.img.px.as_slice(), &page.as_raw()[from..]);
    }

    #[test]
    fn only_the_notes_that_need_the_user_become_a_notification() {
        use Note::*;
        for n in [ScrollSlower, LostTrack, SizeChanged, NeedsAccessibility, PageDidNotMove, CheckScreenRecording] {
            let text = n.notice().unwrap_or_else(|| panic!("{n:?}"));
            assert!(text.contains("Esc"), "{n:?} says how to end the capture: {text}");
        }
        for n in [ScrollingForYou, ScrollYourself, Capturing, AboveStart, BelowStart, HeightLimit, EndOfPage] {
            assert!(n.notice().is_none(), "{n:?}");
        }
    }

    #[test]
    fn a_capture_tells_the_user_once_even_when_several_things_go_wrong() {
        let page = inbox(300, 60, 40);
        let mut fake = FakePage::new(page, 200);
        // Too fast twice: two frames the stitcher cannot place.
        fake.script = vec![0, 0, 60, 1500, 1500, 0, 1500, 1500];
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((9, stop.clone()));
        let p = progress(Mode::Manual);
        let (told, sent) = (AtomicBool::new(false), Mutex::new(Vec::new()));
        run(&mut fake, 300, 200, Mode::Manual, Direction::Down, &stop, &p, &notifier(&told, |t| sent.lock().unwrap().push(t.to_string()))).unwrap();
        let sent = sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(told.load(Ordering::SeqCst));
    }

    #[test]
    fn a_quiet_capture_sends_nothing() {
        let page = inbox(300, 40, 40);
        let mut fake = FakePage::new(page, 200);
        let stop = AtomicU8::new(RUNNING);
        let p = progress(Mode::Auto);
        let (told, sent) = (AtomicBool::new(false), Mutex::new(Vec::new()));
        run(&mut fake, 300, 200, Mode::Auto, Direction::Down, &stop, &p, &notifier(&told, |t| sent.lock().unwrap().push(t.to_string()))).unwrap();
        assert!(sent.lock().unwrap().is_empty());
        assert!(!told.load(Ordering::SeqCst));
    }

    #[test]
    fn a_missing_accessibility_permission_is_told_once_at_the_start() {
        let mut fake = FakePage::new(inbox(300, 40, 40), 200);
        let stop = Arc::new(AtomicU8::new(RUNNING));
        fake.done_after = Some((5, stop.clone()));
        let p = Mutex::new(Progress { height: 0, frames: 0, lost: 0, note: Note::NeedsAccessibility, mode: Mode::Manual });
        let told = Mutex::new(Vec::new());
        run(&mut fake, 300, 200, Mode::Manual, Direction::Down, &stop, &p, &|n| told.lock().unwrap().push(n)).unwrap();
        assert_eq!(told.lock().unwrap().first(), Some(&Note::NeedsAccessibility));
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

    // Settings.

    fn capture_with(page: &mut FakePage, mode: Mode, limits: &Limits, stop: &AtomicU8) -> Outcome {
        let p = progress(mode);
        let (w, h) = (page.page.width(), page.area_h);
        run_with(page, w, h, mode, Direction::Down, limits, stop, &p, &|_| {}).unwrap().unwrap()
    }

    #[test]
    fn the_default_limits_are_the_old_behaviour() {
        let l = Limits::default();
        assert_eq!((l.max_height, l.speed, l.reverse, l.manual), (20_000, 2, false, false));
        assert_eq!(l.step(400), 200);
        assert_eq!(l.wait(SETTLE_FIRST), SETTLE_FIRST);
        assert_eq!(l.wheel(120), 120);
        assert_eq!(Limits::from_settings(&crate::settings::Settings::default()), l);
    }

    #[test]
    fn limits_come_from_the_settings_in_range() {
        let mut s = crate::settings::Settings::default();
        s.scroll_max_height = 10;
        s.scroll_speed = 9;
        s.scroll_reverse = true;
        s.scroll_manual = true;
        let l = Limits::from_settings(&s);
        assert_eq!((l.max_height, l.speed, l.reverse, l.manual), (1_000, 4, true, true));
    }

    #[test]
    fn the_height_limit_leaves_room_in_memory_for_a_wide_picture() {
        let l = Limits { max_height: 400_000, ..Limits::default() };
        assert_eq!(l.max_rows(1000), 250_000);
        assert_eq!(l.max_rows(3024), 82_671);
        assert_eq!(Limits::default().max_rows(3024), 20_000);
        assert_eq!(l.max_rows(0), 250_000_000_u32.min(400_000));
        assert!(l.max_rows(u32::MAX) >= 1);
    }

    #[test]
    fn faster_levels_scroll_further_and_wait_less() {
        let steps: Vec<i32> = (0..=4).map(|speed| Limits { speed, ..Limits::default() }.step(400)).collect();
        assert!(steps.windows(2).all(|w| w[0] < w[1]), "{steps:?}");
        assert_eq!(steps[2], step_points(400));
        let waits: Vec<Duration> = (0..=4).map(|speed| Limits { speed, ..Limits::default() }.wait(Duration::from_millis(100))).collect();
        assert!(waits.windows(2).all(|w| w[0] > w[1]), "{waits:?}");
        assert!(steps.iter().all(|&s| s as f32 <= 400.0 * 0.9));
        assert_eq!(Limits { speed: 250, ..Limits::default() }.step(400), steps[4], "out of range counts as the fastest");
    }

    #[test]
    fn reverse_flips_the_wheel_both_ways() {
        let l = Limits { reverse: true, ..Limits::default() };
        assert_eq!((l.wheel(100), l.wheel(-100), l.wheel(0)), (-100, 100, 0));
    }

    #[test]
    fn a_page_taller_than_the_limit_stops_at_it_and_says_so() {
        let page = inbox(300, 90, 40);
        let mut fake = FakePage::new(page.clone(), 200);
        let stop = AtomicU8::new(RUNNING);
        let limits = Limits { max_height: 1_000, ..Limits::default() };
        let o = capture_with(&mut fake, Mode::Auto, &limits, &stop);
        assert!(o.capped);
        assert_eq!(o.max_height, 1_000);
        assert!(o.img.h <= 1_000 && o.img.h > 800, "{}", o.img.h);
        let n = o.img.h as usize * 300 * 4;
        assert_eq!(o.img.px[..n], page.as_raw()[..n], "what is kept is the top of the page");
    }

    #[test]
    fn the_speed_level_changes_how_far_each_step_goes() {
        let page = inbox(300, 90, 40);
        let (mut slow, mut fast) = (FakePage::new(page.clone(), 200), FakePage::new(page.clone(), 200));
        let stop = AtomicU8::new(RUNNING);
        let a = capture_with(&mut slow, Mode::Auto, &Limits { speed: 0, ..Limits::default() }, &stop);
        let b = capture_with(&mut fast, Mode::Auto, &Limits { speed: 4, ..Limits::default() }, &stop);
        assert_eq!(slow.scrolls[0], Limits { speed: 0, ..Limits::default() }.step(200));
        assert_eq!(fast.scrolls[0], Limits { speed: 4, ..Limits::default() }.step(200));
        assert!(slow.scrolls[0] < fast.scrolls[0]);
        assert!(slow.grabs > fast.grabs, "{} vs {}", slow.grabs, fast.grabs);
        for o in [a, b] {
            assert_eq!(o.img.px.as_slice(), page.as_raw().as_slice(), "every speed gets the whole page");
        }
    }

    fn area(x0: f32, y0: f32, x1: f32, y1: f32) -> R {
        R { x0, y0, x1, y1 }
    }

    #[test]
    fn grab_rect_rounds_outwards_and_refuses_tiny_areas() {
        let g = GrabRect::from_area(&area(10.4, 20.6, 110.2, 90.1)).unwrap();
        assert_eq!(g, GrabRect { x: 10, y: 20, w: 101, h: 71 });
        assert!(GrabRect::from_area(&area(0.0, 0.0, 30.0, 300.0)).is_none());
    }
}
