//! Scrolling capture: the user picks an area, ShotCraft scrolls the page under it (or the user does,
//! when it cannot), grabs the area again and again and stitches the frames (see `stitch`).
//!
//! The capture runs on its own thread, at its own pace, whatever the UI does: `run` is a loop over a
//! `Page` (grab, scroll, wait), which the app backs with the screen and wheel events and the tests
//! back with a simulated page.
//!
//! Automatic scrolling goes by small, regular scrolls (`PACES`): each one moves the page by at most
//! a third of the area, so two looks in a row share most of their rows and the stitcher always has
//! plenty to match on. A look is cheap (macOS: `macgrab`, about 16 ms), so the page is looked at
//! until two looks agree before a frame is kept.
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

/// Pause between two grabs in manual mode. With the grab and the stitching this is about fifteen
/// frames per second: a fast hand still leaves several frames per screenful.
const FRAME_INTERVAL: Duration = Duration::from_millis(40);
/// In manual mode the capture ends by itself when the page has not moved for this long (once something
/// was stitched), so the user does not have to press Esc.
pub const IDLE_FINISH: Duration = Duration::from_secs(3);
/// ... and this long after the last change on screen when the page was never followed at all (it
/// can't scroll, or nobody scrolled it): time to read the notification and start scrolling by hand.
pub const NEVER_MOVED_FINISH: Duration = Duration::from_secs(10);
/// Consecutive failed grabs before the capture gives up (usually a missing Screen Recording permission).
const MAX_FAILURES: u32 = 3;
/// Areas smaller than this (points) are refused.
const MIN_SIDE: f32 = 48.0;

/// By speed level: the time one scroll gets, in milliseconds, and the points it moves the page by.
/// Levels 0, 1, 2 and 4 are the four speeds of the app this one is modelled on (100 points every
/// 0.8 s, 0.2 s and 0.12 s, then 300 points every 0.1 s; its default is the third). Level 3 sits
/// between its last two.
const PACES: [(u64, i32); 5] = [(800, 100), (200, 100), (120, 100), (100, 200), (100, 300)];
/// One scroll moves the page by at most this share of the area height (1/n), whatever the level.
const STEP_DIVISOR: u32 = 3;
/// The smallest scroll, in points: a third of the smallest area (`MIN_SIDE`).
const STEP_MIN: i32 = 16;
/// After a scroll, the page gets this share of its pace (n/10) before the first look...
const FIRST_LOOK_TENTHS: u32 = 3;
/// ... then it is looked at again after a sixth of the pace, within these bounds (milliseconds),
/// until two looks agree (the page has stopped moving and is drawn)...
const NEXT_LOOK_MS: (u64, u64) = (15, 60);
/// ... or this many looks have been taken (a page that never stops: a video, an animation).
const SETTLE_MAX_LOOKS: u32 = 15;
/// A page that does not move after a scroll gets three paces, and at least this long (milliseconds),
/// to start moving before it is taken to be at its end.
const END_WAIT_MIN_MS: u64 = 350;
/// Frames dropped in a row before automatic scrolling gives up and leaves the scrolling to the user.
const MAX_LOST_IN_ROW: u32 = 3;

/// Memory a stitched picture may take, in bytes (4 per pixel): a wide picture stops short of the height limit.
const MAX_BYTES: u64 = 1_000_000_000;

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

    fn pace(&self) -> (u64, i32) {
        PACES[(self.speed as usize).min(PACES.len() - 1)]
    }

    /// The time one scroll gets at this speed level.
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.pace().0)
    }

    /// How long the page gets after a scroll before it is first looked at.
    fn first_look(&self) -> Duration {
        self.interval() * FIRST_LOOK_TENTHS / 10
    }

    /// The pause between two looks at a page that may still be moving.
    fn next_look(&self) -> Duration {
        Duration::from_millis((self.pace().0 / 6).clamp(NEXT_LOOK_MS.0, NEXT_LOOK_MS.1))
    }

    /// How long a page that did not move gets to start moving.
    fn end_wait(&self) -> Duration {
        Duration::from_millis((self.pace().0 * 3).max(END_WAIT_MIN_MS))
    }

    /// Points to scroll between two frames for an area `area_h` points tall.
    pub fn step(&self, area_h: u32) -> i32 {
        self.pace().1.min((area_h / STEP_DIVISOR) as i32).max(STEP_MIN)
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
    page.sleep(lp.limits.first_look());
    let first = grab_retrying(page)?;
    settle_from(page, lp, first)
}

/// `settle` for a page that was just looked at.
fn settle_from(page: &mut dyn Page, lp: &Loop, mut prev: RgbaImage) -> Result<RgbaImage, String> {
    for _ in 0..SETTLE_MAX_LOOKS {
        if lp.state() != RUNNING {
            break;
        }
        page.sleep(lp.limits.next_look());
        let cur = grab_retrying(page)?;
        if frames_match(&prev, &cur) {
            return Ok(cur);
        }
        prev = cur;
    }
    Ok(prev)
}

/// The page still looks like `seen` after a scroll: watches it for `Limits::end_wait` and returns
/// how it looks once it has moved and settled, or `None` when it stays as it is (the end of the
/// page, or a page that takes no scrolling).
fn wait_for_movement(page: &mut dyn Page, lp: &Loop, seen: &RgbaImage) -> Result<Option<RgbaImage>, String> {
    let until = page.now() + lp.limits.end_wait();
    while page.now() < until && lp.state() == RUNNING {
        page.sleep(lp.limits.next_look());
        let cur = grab_retrying(page)?;
        if !frames_match(seen, &cur) {
            return settle_from(page, lp, cur).map(Some);
        }
    }
    Ok(None)
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
    // Manual mode: the previous look, to tell a screen that changes from one that stands still.
    let mut seen: Option<RgbaImage> = None;
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
                // The capture ends by itself once the screen has stood still: soon after a page
                // that was followed, later when nothing came after the first look. A frame that fits
                // nowhere on a screen that no longer changes is no movement either, so a capture
                // with nothing on screen to show for it never waits for good.
                let still = seen.as_ref().map_or(false, |s| frames_match(s, &frame));
                seen = Some(frame);
                let patience = if stitcher.frames_used() <= 1 { NEVER_MOVED_FINISH } else { IDLE_FINISH };
                if !still || matches!(note, Note::Capturing | Note::AboveStart | Note::BelowStart) {
                    last_move = page.now();
                } else if page.now().saturating_sub(last_move) >= patience {
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
                    // Not moving yet? A slow page gets a moment to start before this is called the end.
                    if let Ok(Some(frame)) = wait_for_movement(page, &lp, &frame) {
                        result = stitcher.push_hinted(&frame, hint);
                    }
                } else if matches!(result, Push::Lost) {
                    // A frame that fits nowhere is often a page still loading or fading something in:
                    // one more look, a pace later, before stepping back.
                    page.sleep(limits.interval());
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

/// One look at the area, as pixels: taken in the process when macOS allows it (`macgrab`), by
/// `screencapture` otherwise.
#[cfg(target_os = "macos")]
fn grab(rect: &GrabRect) -> Result<RgbaImage, String> {
    match crate::macgrab::area(rect) {
        Some(frame) => Ok(frame),
        None => grab_with_screencapture(rect),
    }
}

/// One look at the area through `screencapture`: about 150 ms, and it works for any area.
#[cfg(target_os = "macos")]
fn grab_with_screencapture(rect: &GrabRect) -> Result<RgbaImage, String> {
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
        /// A scroll shows on screen this long after it was sent (a busy page).
        lag: Duration,
        /// Scrolls sent and not yet shown: when each one shows, and by how much.
        pending: Vec<(Duration, i32)>,
        /// Scripted positions for manual mode: the nth grab sees `script[n]` (the last one repeats).
        script: Vec<i32>,
        /// Press Esc after this many grabs.
        done_after: Option<(u32, Arc<AtomicU8>)>,
        scrolls: Vec<i32>,
    }

    /// What one look costs on the virtual clock: the time `macgrab` takes for a Retina-sized area.
    const GRAB_COST: Duration = Duration::from_millis(16);

    impl FakePage {
        fn new(page: RgbaImage, area_h: u32) -> FakePage {
            FakePage { page, area_h, pos: 0, target: 0, ease: 1.0, header: None, footer: None, grabs: 0, clock: Duration::ZERO, deaf: false, lag: Duration::ZERO, pending: vec![], script: vec![], done_after: None, scrolls: vec![] }
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
            self.clock += GRAB_COST;
            if let Some((n, stop)) = &self.done_after {
                if self.grabs >= *n {
                    stop.store(DONE, Ordering::SeqCst);
                }
            }
            let (now, max) = (self.clock, self.max_pos());
            let shown: i32 = self.pending.iter().filter(|(at, _)| *at <= now).map(|(_, dy)| dy).sum();
            self.pending.retain(|(at, _)| *at > now);
            self.target = (self.target + shown).clamp(0, max);
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
            // The wheel events of one scroll go out 8 ms apart (`autoscroll::Scroller`).
            self.clock += Duration::from_millis(8) * crate::autoscroll::chunks(dy).len() as u32;
            if !self.deaf {
                self.pending.push((self.clock + self.lag, dy));
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
        // Steps are a third of the area at most.
        assert!(fake.scrolls.iter().all(|&s| s > 0 && s <= 66), "{:?}", fake.scrolls);
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
        // It scrolls up by a third of the area at most.
        assert!(fake.scrolls.iter().all(|&s| s < 0 && s.abs() <= 66), "{:?}", fake.scrolls);
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
    fn a_page_that_never_moves_ends_the_capture_by_itself_after_ten_seconds() {
        let mut fake = FakePage::new(inbox(300, 40, 40), 200);
        fake.deaf = true;
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!((o.frames, o.img.h), (1, 200), "what the area showed is kept");
        assert_eq!(p.lock().unwrap().mode, Mode::Manual);
        assert!(fake.clock >= NEVER_MOVED_FINISH && fake.clock <= NEVER_MOVED_FINISH + Duration::from_secs(2), "{:?}", fake.clock);
    }

    #[test]
    fn a_capture_that_lost_the_page_ends_once_the_screen_stands_still() {
        let mut fake = FakePage::new(inbox(300, 60, 40), 200);
        // By hand: one step that is followed, then a jump too far to follow, and nothing more.
        fake.script = vec![0, 60, 1000];
        let stop = AtomicU8::new(RUNNING);
        let (r, _) = capture(&mut fake, Mode::Manual, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.img.h, 260);
        assert!(o.lost > 0);
        assert!(fake.clock >= IDLE_FINISH && fake.clock <= IDLE_FINISH + Duration::from_secs(2), "{:?}", fake.clock);
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
        assert!(fake.clock >= GRAB_COST * 10 + IDLE_FINISH);
        assert!(fake.clock <= (GRAB_COST + FRAME_INTERVAL) * 10 + IDLE_FINISH + Duration::from_secs(1), "{:?}", fake.clock);
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
    fn a_scroll_is_a_hundred_points_and_never_more_than_a_third_of_the_area() {
        assert_eq!(step_points(400), 100);
        assert_eq!(step_points(5000), 100);
        assert_eq!(step_points(240), 80);
        assert_eq!(step_points(20), STEP_MIN);
        for h in [48, 100, 400, 984, 3000] {
            assert!(step_points(h) as u32 * STEP_DIVISOR <= h, "{h}");
        }
    }

    // Settings.

    fn capture_with(page: &mut FakePage, mode: Mode, limits: &Limits, stop: &AtomicU8) -> Outcome {
        let p = progress(mode);
        let (w, h) = (page.page.width(), page.area_h);
        run_with(page, w, h, mode, Direction::Down, limits, stop, &p, &|_| {}).unwrap().unwrap()
    }

    #[test]
    fn the_default_pace_is_a_hundred_points_every_120_ms() {
        let l = Limits::default();
        assert_eq!((l.max_height, l.speed, l.reverse, l.manual), (20_000, 2, false, false));
        assert_eq!((l.step(900), l.interval()), (100, Duration::from_millis(120)));
        assert_eq!((l.first_look(), l.next_look(), l.end_wait()), (Duration::from_millis(36), Duration::from_millis(20), Duration::from_millis(360)));
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
    fn every_level_covers_more_points_per_second_than_the_one_before() {
        let level = |speed| Limits { speed, ..Limits::default() };
        let per_second: Vec<u128> = (0..=4).map(|s| level(s).step(1200) as u128 * 1000 / level(s).interval().as_millis()).collect();
        assert_eq!(per_second, [125, 500, 833, 2000, 3000]);
        let waits: Vec<Duration> = (0..=4).map(|s| level(s).first_look() + level(s).next_look()).collect();
        assert!(waits.windows(2).all(|w| w[0] >= w[1]), "{waits:?}");
        assert_eq!(level(4).step(600), 200, "a third of the area at most");
        assert_eq!((level(250).step(1200), level(250).interval()), (level(4).step(1200), level(4).interval()), "out of range counts as the fastest");
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
    fn the_speed_level_changes_how_far_each_step_goes_and_how_long_the_page_takes() {
        let page = inbox(300, 120, 40);
        let (mut slow, mut fast) = (FakePage::new(page.clone(), 900), FakePage::new(page.clone(), 900));
        let stop = AtomicU8::new(RUNNING);
        let a = capture_with(&mut slow, Mode::Auto, &Limits { speed: 0, ..Limits::default() }, &stop);
        let b = capture_with(&mut fast, Mode::Auto, &Limits { speed: 4, ..Limits::default() }, &stop);
        assert_eq!((slow.scrolls[0], fast.scrolls[0]), (100, 300));
        assert!(slow.clock > fast.clock * 5, "{:?} vs {:?}", slow.clock, fast.clock);
        for o in [a, b] {
            assert_eq!(o.img.px.as_slice(), page.as_raw().as_slice(), "every speed gets the whole page");
        }
    }

    #[test]
    fn the_default_speed_covers_a_page_at_about_a_hundred_points_every_120_ms() {
        // 12,000 rows behind a 900-row area: about 111 scrolls of 100 points.
        let page = inbox(300, 300, 40);
        let mut fake = FakePage::new(page.clone(), 900);
        let stop = AtomicU8::new(RUNNING);
        let o = capture_with(&mut fake, Mode::Auto, &Limits::default(), &stop);
        assert_eq!(o.img.px.as_slice(), page.as_raw().as_slice());
        assert_eq!(o.lost, 0);
        let per_second = (page.height() - 900) as f32 / fake.clock.as_secs_f32();
        assert!((700.0..=950.0).contains(&per_second), "{per_second} points per second, {:?} in all", fake.clock);
    }

    #[test]
    fn a_page_that_answers_late_is_not_taken_for_its_end() {
        // Each scroll shows on screen 200 ms after it was sent: later than the first two looks.
        let page = inbox(300, 60, 40);
        let mut fake = FakePage::new(page.clone(), 300);
        fake.lag = Duration::from_millis(200);
        let stop = AtomicU8::new(RUNNING);
        let (r, p) = capture(&mut fake, Mode::Auto, &stop);
        let o = r.unwrap().unwrap();
        assert_eq!(o.img.px.as_slice(), page.as_raw().as_slice());
        assert_eq!(p.lock().unwrap().note, Note::EndOfPage);
    }

    // A real page on a real screen. `tests/live/scroll-page.html` writes each row's number next to
    // it as a strip of cells, so a program can say what a capture of it holds.

    /// The page rows a capture of the test page shows, top to bottom: row number and pixel rows.
    /// Pixel rows without a strip (the header, the footer) are left out.
    fn strips(img: &RgbaImage) -> Vec<(u32, u32)> {
        let is = |p: &Rgba<u8>, c: usize| p.0[c] > 170 && (0..3).filter(|&o| o != c).all(|o| p.0[o] < 100);
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for y in 0..img.height() {
            let Some(x0) = (0..img.width()).find(|&x| is(img.get_pixel(x, y), 0)) else { continue };
            let cell = (x0..img.width()).take_while(|&x| is(img.get_pixel(x, y), 0)).count() as u32;
            let at = |i: u32| x0 + cell * i + cell / 2;
            if cell < 4 || at(13) >= img.width() || !is(img.get_pixel(at(13), y), 2) {
                continue;
            }
            let number = (1..=12).fold(0, |n, i| {
                let p = img.get_pixel(at(i), y).0;
                n << 1 | (p[0] as u32 + p[1] as u32 + p[2] as u32 > 384) as u32
            });
            match runs.last_mut() {
                Some((n, rows)) if *n == number => *rows += 1,
                _ => runs.push((number, 1)),
            }
        }
        runs
    }

    /// What is wrong with a capture of the test page, in words. Empty when rows 0 to the last one
    /// are all there once, in order and `row_px` pixel rows tall.
    fn faults(runs: &[(u32, u32)], row_px: u32) -> Vec<String> {
        let mut out = Vec::new();
        if runs.first().map(|r| r.0) != Some(0) {
            out.push(format!("starts at row {:?}, not 0", runs.first().map(|r| r.0)));
        }
        for w in runs.windows(2) {
            if w[1].0 != w[0].0 + 1 {
                out.push(format!("row {} is followed by row {}", w[0].0, w[1].0));
            }
        }
        // The first row may start under a sticky header and the last one may end at the bottom of
        // the area; every other row is whole.
        let last = runs.len().saturating_sub(1);
        for (i, (number, rows)) in runs.iter().enumerate().skip(1) {
            if *rows != row_px && !(i == last && *rows < row_px) {
                out.push(format!("row {number} is {rows} px tall, not {row_px}"));
            }
        }
        out
    }

    #[test]
    fn a_capture_of_the_test_page_is_read_back_from_its_strips() {
        // Three rows of 8 px drawn the way the page draws them, with 4 px cells.
        let mut img = RgbaImage::from_pixel(80, 30, Rgba([255, 255, 255, 255]));
        for (i, number) in [0u32, 1, 2].into_iter().enumerate() {
            for y in 4 + i as u32 * 8..12 + i as u32 * 8 {
                for c in 0..14u32 {
                    let colour = match c {
                        0 => [234, 51, 35, 255],
                        13 => [0, 0, 245, 255],
                        _ if number >> (12 - c) & 1 == 1 => [255, 255, 255, 255],
                        _ => [0, 0, 0, 255],
                    };
                    for x in 5 + c * 4..9 + c * 4 {
                        img.put_pixel(x, y, Rgba(colour));
                    }
                }
            }
        }
        let runs = strips(&img);
        assert_eq!(runs, [(0, 8), (1, 8), (2, 8)]);
        assert!(faults(&runs, 8).is_empty());
        assert_eq!(faults(&[(0, 8), (1, 5), (3, 8), (3, 5)], 8), ["row 1 is followed by row 3", "row 3 is followed by row 3", "row 1 is 5 px tall, not 8"]);
        assert_eq!(faults(&[(0, 8), (1, 8), (2, 9)], 8), ["row 2 is 9 px tall, not 8"]);
        assert_eq!(faults(&[(4, 8), (5, 8)], 8), ["starts at row Some(4), not 0"]);
    }

    /// A page that keeps what the capture saw of it and asked from it, so that a capture that went
    /// wrong on a real screen can be run again without one (`a_recorded_capture_replayed`).
    struct Recorded<P: Page> {
        page: P,
        frames: Vec<RgbaImage>,
        calls: Vec<String>,
    }

    impl<P: Page> Recorded<P> {
        /// The first frames are kept, whole and in memory: enough for a capture of a few screens.
        const MAX_FRAMES: usize = 60;

        fn save(&self, dir: &std::path::Path) {
            std::fs::create_dir_all(dir).expect("the folder for the frames can be made");
            for (i, f) in self.frames.iter().enumerate() {
                f.save(dir.join(format!("{i:04}.png"))).expect("a frame can be written");
            }
            std::fs::write(dir.join("calls.txt"), self.calls.join("\n")).expect("the calls can be written");
        }
    }

    impl<P: Page> Page for Recorded<P> {
        fn grab(&mut self) -> Result<RgbaImage, String> {
            let frame = self.page.grab()?;
            self.calls.push(format!("{} ms: grab {}", self.page.now().as_millis(), self.frames.len()));
            if self.frames.len() < Self::MAX_FRAMES {
                self.frames.push(frame.clone());
            }
            Ok(frame)
        }

        fn scroll(&mut self, dy: i32) -> Result<(), String> {
            self.calls.push(format!("{} ms: scroll {dy}", self.page.now().as_millis()));
            self.page.scroll(dy)
        }

        fn sleep(&mut self, d: Duration) {
            self.page.sleep(d)
        }

        fn now(&self) -> Duration {
            self.page.now()
        }
    }

    /// The frames of a recorded capture, handed out in the order they were seen; the last one
    /// stays once they are used up, like a page at its end.
    struct Replay {
        frames: Vec<RgbaImage>,
        next: usize,
        clock: Duration,
    }

    impl Page for Replay {
        fn grab(&mut self) -> Result<RgbaImage, String> {
            let i = self.next.min(self.frames.len() - 1);
            self.next += 1;
            Ok(self.frames[i].clone())
        }

        fn scroll(&mut self, _dy: i32) -> Result<(), String> {
            Ok(())
        }

        fn sleep(&mut self, d: Duration) {
            self.clock += d;
        }

        fn now(&self) -> Duration {
            self.clock
        }
    }

    /// The height limit of a replay, in pixel rows.
    const REPLAY_MAX_ROWS: u32 = 60_000;

    /// Runs the frames SHOTCRAFT_LIVE_FRAMES kept of a live capture through the stitcher again and
    /// says what it makes of each one:
    ///
    ///   SHOTCRAFT_REPLAY=folder cargo test a_recorded_capture_replayed -- --ignored --nocapture
    ///
    /// SHOTCRAFT_REPLAY_OUT names a PNG for the stitched result.
    #[test]
    #[ignore = "needs the frames of a recorded capture"]
    fn a_recorded_capture_replayed() {
        let dir = std::path::PathBuf::from(std::env::var("SHOTCRAFT_REPLAY").expect("set SHOTCRAFT_REPLAY=folder"));
        let mut names: Vec<_> = std::fs::read_dir(&dir).expect("the folder can be read").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map_or(false, |x| x == "png")).collect();
        names.sort();
        let frames: Vec<RgbaImage> = names.iter().map(|p| image::open(p).expect("a frame can be read").to_rgba8()).collect();
        assert!(!frames.is_empty(), "no frame in {dir:?}");
        let mut stitcher = Stitcher::with_direction(REPLAY_MAX_ROWS, Direction::Down);
        for (i, f) in frames.iter().enumerate() {
            println!("frame {i}: {:?}", stitcher.push(f));
        }
        let img = stitcher.finish().expect("something was stitched");
        println!("stitched {} x {} px from {} frames", img.width(), img.height(), frames.len());
        if let Ok(path) = std::env::var("SHOTCRAFT_REPLAY_OUT") {
            img.save(&path).expect("SHOTCRAFT_REPLAY_OUT can be written");
        }
    }

    /// Captures the area of the screen that shows `tests/live/scroll-page.html`, scrolled to its
    /// top, and says how long it took and what the picture holds:
    ///
    ///   SHOTCRAFT_LIVE_SCROLL=x,y,w,h cargo test live_scroll -- --ignored --nocapture
    ///
    /// The area is in points from the top left of the main display and must show the strips.
    /// SHOTCRAFT_LIVE_SPEED sets the speed level, SHOTCRAFT_LIVE_OUT a PNG to write and
    /// SHOTCRAFT_LIVE_FRAMES a folder for the frames the capture saw (`a_recorded_capture_replayed`
    /// reads them). SHOTCRAFT_LIVE_ANY=1 captures any page: the picture is not read for strips. With
    /// SHOTCRAFT_LIVE_CHECK=file.png nothing is captured: that picture (from any app) is read instead.
    #[test]
    #[ignore = "needs a screen that shows tests/live/scroll-page.html"]
    fn live_scroll_capture_of_the_test_page() {
        // A test has no NSApplication, and Core Graphics aborts a process that moves the pointer
        // before it has a window server connection. Asking for the main display opens one, and
        // the process stays a plain command line tool: macOS then applies the terminal's Screen
        // Recording permission to it instead of asking for one of its own.
        #[cfg(target_os = "macos")]
        {
            #[link(name = "CoreGraphics", kind = "framework")]
            extern "C" {
                fn CGMainDisplayID() -> u32;
            }
            // SAFETY: takes no argument and returns a display id.
            unsafe { CGMainDisplayID() };
        }
        let var = |name: &str| std::env::var(name).ok();
        let (img, row_px) = if let Some(path) = var("SHOTCRAFT_LIVE_CHECK") {
            let img = image::open(&path).expect("SHOTCRAFT_LIVE_CHECK names a picture").to_rgba8();
            let row_px = var("SHOTCRAFT_LIVE_ROW_PX").and_then(|v| v.parse().ok()).unwrap_or(80);
            (img, row_px)
        } else {
            let spec = var("SHOTCRAFT_LIVE_SCROLL").expect("set SHOTCRAFT_LIVE_SCROLL=x,y,w,h");
            let v: Vec<i32> = spec.split(',').map(|n| n.trim().parse().expect("whole numbers")).collect();
            let rect = GrabRect { x: v[0], y: v[1], w: v[2] as u32, h: v[3] as u32 };
            let speed = var("SHOTCRAFT_LIVE_SPEED").and_then(|v| v.parse().ok()).unwrap_or(crate::settings::SCROLL_SPEED_DEFAULT);
            let limits = Limits { speed, max_height: crate::settings::SCROLL_MAX_HEIGHT_RANGE.1, ..Limits::default() };
            let mut page = Recorded { page: ScreenPage { rect, scroller: autoscroll::Scroller::new(rect), started: Instant::now(), limits }, frames: Vec::new(), calls: Vec::new() };
            let (stop, p) = (AtomicU8::new(RUNNING), progress(Mode::Auto));
            let notes = Mutex::new(Vec::new());
            let started = Instant::now();
            // Nobody is there to scroll by hand or to press Esc: a hand-over ends the capture.
            let hand_over = |n: &Note| matches!(n, Note::NeedsAccessibility | Note::PageDidNotMove | Note::CheckScreenRecording | Note::LostTrack | Note::SizeChanged);
            let note = |n: Note| {
                if hand_over(&n) {
                    stop.store(DONE, Ordering::SeqCst);
                }
                notes.lock().unwrap().push(n);
            };
            let o = run_with(&mut page, rect.w, rect.h, Mode::Auto, Direction::Down, &limits, &stop, &p, &note).expect("the capture ran").expect("something was captured");
            let took = started.elapsed();
            if let Some(dir) = var("SHOTCRAFT_LIVE_FRAMES") {
                page.save(std::path::Path::new(&dir));
            }
            drop(page);
            let points = o.img.h as f32 / o.scale;
            println!("captured {} x {} px ({points:.0} points) in {:.2} s: {:.0} points per second, {} frames, {} lost, speed level {speed}", o.img.w, o.img.h, took.as_secs_f32(), (points - rect.h as f32) / took.as_secs_f32(), o.frames, o.lost);
            let mut seen = notes.into_inner().unwrap();
            seen.dedup();
            println!("notes: {seen:?}");
            let img = RgbaImage::from_raw(o.img.w, o.img.h, o.img.px.to_vec()).unwrap();
            if let Some(path) = var("SHOTCRAFT_LIVE_OUT") {
                img.save(&path).expect("SHOTCRAFT_LIVE_OUT can be written");
            }
            if var("SHOTCRAFT_LIVE_ANY").is_some() {
                return;
            }
            assert!(!seen.iter().any(hand_over), "automatic scrolling handed over: is the page at its top and under the area, and may this program post events and see the screen?");
            (img, (40.0 * o.scale).round() as u32)
        };
        let runs = strips(&img);
        let found = faults(&runs, row_px);
        println!("{} x {} px, rows {:?} to {:?} ({} runs), {} faults", img.width(), img.height(), runs.first().map(|r| r.0), runs.last().map(|r| r.0), runs.len(), found.len());
        for f in found.iter().take(20) {
            println!("  {f}");
        }
        assert!(!runs.is_empty(), "no strip was found: is the page in the area?");
        assert!(found.is_empty(), "{} faults", found.len());
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
