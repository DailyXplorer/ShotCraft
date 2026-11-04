//! Manual scrolling capture: the user picks an area and scrolls the page by hand, ShotCraft grabs the
//! area again and again and stitches the frames (see `stitch`).
use crate::model::{Image, R};
use crate::stitch::{Push, Stitcher};
use crate::{system, Action, Queue};
use eframe::egui::{self, Color32, Context, ViewportId};
use image::RgbaImage;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Time between two grabs. Each grab runs `screencapture`, so the real rate is a little lower:
/// about four to five frames per second.
const FRAME_INTERVAL: Duration = Duration::from_millis(200);
/// Consecutive failed grabs before the capture gives up (usually a missing Screen Recording permission).
const MAX_FAILURES: u32 = 3;
/// The control window, in points.
pub const CONTROL_SIZE: [f32; 2] = [264.0, 112.0];
/// Space kept between the captured area and the control window, in points.
const GAP: f32 = 16.0;
/// Areas smaller than this (points) are refused.
const MIN_SIDE: f32 = 48.0;

const RUNNING: u8 = 0;
const DONE: u8 = 1;
const CANCEL: u8 = 2;

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

#[derive(Default)]
pub struct Progress {
    pub height: u32,
    pub frames: u32,
    pub note: &'static str,
}

/// What a finished capture hands to the app.
pub struct Outcome {
    pub img: Image,
    /// Pixels per point of the display the area is on: frame width over area width.
    pub scale: f32,
    /// Frames that counted. 1 means the page never moved.
    pub frames: u32,
    pub capped: bool,
}

pub struct Session {
    pub control_pos: [f32; 2],
    stop: Arc<AtomicU8>,
    progress: Arc<Mutex<Progress>>,
}

impl Session {
    pub fn start(rect: GrabRect, control_pos: [f32; 2], queue: Queue, ctx: Context) -> Session {
        let stop = Arc::new(AtomicU8::new(RUNNING));
        let progress = Arc::new(Mutex::new(Progress { note: "Scroll the page", ..Progress::default() }));
        let (s, p) = (stop.clone(), progress.clone());
        std::thread::spawn(move || {
            let res = run(rect, &s, &p, &ctx);
            queue.lock().unwrap().push(Action::ScrollingFinished(res));
            ctx.request_repaint();
        });
        Session { control_pos, stop, progress }
    }

    /// Ends the capture and keeps what was stitched.
    pub fn finish(&self) {
        let _ = self.stop.compare_exchange(RUNNING, DONE, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Ends the capture and drops everything.
    pub fn cancel(&self) {
        self.stop.store(CANCEL, Ordering::SeqCst);
    }

    /// Draws the control window's content. `body` runs on the deferred viewport, so it only takes shared state.
    pub fn control_body(&self) -> impl Fn(&mut egui::Ui) + Send + Sync + 'static {
        let (stop, progress) = (self.stop.clone(), self.progress.clone());
        move |ui| control_ui(ui, &stop, &progress)
    }
}

fn control_ui(ui: &mut egui::Ui, stop: &AtomicU8, progress: &Mutex<Progress>) {
    let (height, frames, note) = {
        let p = progress.lock().unwrap();
        (p.height, p.frames, p.note)
    };
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 10.0, Color32::from_gray(30));
    let mut done = false;
    let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(12.0)), |ui| {
        ui.visuals_mut().override_text_color = Some(Color32::WHITE);
        ui.label(egui::RichText::new(format!("Scrolling capture · {height} px")).strong());
        ui.label(egui::RichText::new(format!("{note} · {frames} frames")).small().color(Color32::from_gray(190)));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            done = ui.button("Done").clicked();
            cancel |= ui.button("Cancel (Esc)").clicked();
        });
    });
    if cancel {
        stop.store(CANCEL, Ordering::SeqCst);
    } else if done {
        let _ = stop.compare_exchange(RUNNING, DONE, Ordering::SeqCst, Ordering::SeqCst);
    }
    if cancel || done {
        ui.ctx().request_repaint_of(ViewportId::ROOT);
    }
}

fn run(rect: GrabRect, stop: &AtomicU8, progress: &Mutex<Progress>, ctx: &Context) -> Result<Option<Outcome>, String> {
    let mut stitcher = Stitcher::new();
    let mut failures = 0;
    let mut last_error = String::new();
    loop {
        let started = Instant::now();
        // Read before grabbing: a Done pressed now still gets one last frame.
        let state = stop.load(Ordering::SeqCst);
        if state == CANCEL {
            return Ok(None);
        }
        match grab(&rect) {
            Ok(frame) => {
                failures = 0;
                let note = match stitcher.push(&frame) {
                    Push::First | Push::Unchanged => "Scroll the page",
                    Push::Revisited { .. } if stitcher.above_start() => "Above the start: not captured",
                    Push::Appended { .. } | Push::Revisited { .. } => "Capturing",
                    Push::Lost => "Lost track: scroll back a little, slower",
                    Push::SizeMismatch => "The area changed size",
                    Push::Capped => "Height limit reached",
                };
                let mut p = progress.lock().unwrap();
                p.height = stitcher.height();
                p.frames = stitcher.frames_used();
                p.note = note;
            }
            Err(e) => {
                failures += 1;
                last_error = e;
                if failures >= MAX_FAILURES {
                    // Keep what was stitched so far; only report an error when there is nothing.
                    if stitcher.frames_used() > 1 {
                        break;
                    }
                    return Err(last_error);
                }
            }
        }
        ctx.request_repaint_of(control_id());
        if state == DONE || stitcher.is_capped() {
            break;
        }
        // Sleep in short slices so that Done and Cancel react at once.
        while started.elapsed() < FRAME_INTERVAL && stop.load(Ordering::SeqCst) == RUNNING {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let _ = last_error;
    let (frames, capped) = (stitcher.frames_used(), stitcher.is_capped());
    let img = stitcher.finish().map_err(|_| "No image was captured.".to_string())?;
    let (w, h) = img.dimensions();
    let scale = (w as f32 / rect.w as f32).max(1.0);
    Ok(Some(Outcome { img: Image { w, h, px: Arc::new(img.into_raw()) }, scale, frames, capped }))
}

/// One look at the area, as pixels.
fn grab(rect: &GrabRect) -> Result<RgbaImage, String> {
    let path = system::temp_png("scroll");
    Command::new("/usr/sbin/screencapture")
        .arg("-x")
        .arg(format!("-R{},{},{},{}", rect.x, rect.y, rect.w, rect.h))
        .arg(&path)
        .status()
        .map_err(|e| format!("Can't take a screenshot: {e}"))?;
    if !path.exists() {
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
/// frame): beside the area, on the right, left, below or above, inside the screen the area is on.
/// `None` when no side has room. Everything is in points from the top left of the main display.
pub fn control_position(area: &R, size: [f32; 2], screens: &[R]) -> Option<[f32; 2]> {
    let overlap = |a: &R, b: &R| (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0) * (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let screen = screens.iter().copied().max_by(|a, b| overlap(a, area).total_cmp(&overlap(b, area)))?;
    let [w, h] = size;
    let keep_out = area.expand(GAP);
    let clamp_y = |y: f32| y.clamp(screen.y0, (screen.y1 - h).max(screen.y0));
    let clamp_x = |x: f32| x.clamp(screen.x0, (screen.x1 - w).max(screen.x0));
    let candidates = [
        [area.x1 + GAP, clamp_y(area.y0)],
        [area.x0 - GAP - w, clamp_y(area.y0)],
        [clamp_x(area.x0), area.y1 + GAP],
        [clamp_x(area.x0), area.y0 - GAP - h],
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

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> R {
        R { x0, y0, x1, y1 }
    }

    fn inside(win: [f32; 2], s: &R) -> bool {
        win[0] >= s.x0 && win[1] >= s.y0 && win[0] + CONTROL_SIZE[0] <= s.x1 && win[1] + CONTROL_SIZE[1] <= s.y1
    }

    fn clear_of(win: [f32; 2], area: &R) -> bool {
        let w = r(win[0], win[1], win[0] + CONTROL_SIZE[0], win[1] + CONTROL_SIZE[1]);
        w.x1 <= area.x0 || w.x0 >= area.x1 || w.y1 <= area.y0 || w.y0 >= area.y1
    }

    #[test]
    fn the_control_window_goes_to_the_right_when_there_is_room() {
        let screen = r(0.0, 0.0, 1440.0, 900.0);
        let area = r(100.0, 100.0, 800.0, 700.0);
        let pos = control_position(&area, CONTROL_SIZE, &[screen]).unwrap();
        assert_eq!(pos, [816.0, 100.0]);
    }

    #[test]
    fn it_moves_to_another_side_when_the_right_is_taken() {
        let screen = r(0.0, 0.0, 1440.0, 900.0);
        let area = r(400.0, 100.0, 1400.0, 700.0);
        let pos = control_position(&area, CONTROL_SIZE, &[screen]).unwrap();
        assert!(inside(pos, &screen) && clear_of(pos, &area), "{pos:?}");
        assert!(pos[0] + CONTROL_SIZE[0] <= 400.0, "should sit on the left: {pos:?}");
    }

    #[test]
    fn it_goes_below_or_above_for_a_full_width_area() {
        let screen = r(0.0, 25.0, 1440.0, 875.0);
        let area = r(0.0, 25.0, 1440.0, 700.0);
        let pos = control_position(&area, CONTROL_SIZE, &[screen]).unwrap();
        assert!(inside(pos, &screen) && clear_of(pos, &area), "{pos:?}");
        assert!(pos[1] >= 700.0);
    }

    #[test]
    fn it_reports_when_the_area_leaves_no_room() {
        let screen = r(0.0, 25.0, 1440.0, 875.0);
        assert_eq!(control_position(&screen, CONTROL_SIZE, &[screen]), None);
        assert_eq!(control_position(&screen, CONTROL_SIZE, &[]), None);
    }

    #[test]
    fn it_stays_on_the_display_that_holds_the_area() {
        let main = r(0.0, 0.0, 1440.0, 900.0);
        let side = r(1440.0, -200.0, 3360.0, 880.0);
        let area = r(1500.0, 0.0, 3000.0, 800.0);
        let pos = control_position(&area, CONTROL_SIZE, &[main, side]).unwrap();
        assert!(inside(pos, &side) && clear_of(pos, &area), "{pos:?}");
    }

    #[test]
    fn the_grabbed_rectangle_uses_whole_points_and_refuses_tiny_areas() {
        let g = GrabRect::from_area(&r(10.4, 20.6, 510.4, 320.9)).unwrap();
        assert_eq!(g, GrabRect { x: 10, y: 20, w: 501, h: 301 });
        let exact = GrabRect::from_area(&r(10.0, 20.0, 510.0, 320.0)).unwrap();
        assert_eq!(exact, GrabRect { x: 10, y: 20, w: 500, h: 300 });
        assert_eq!(GrabRect::from_area(&r(0.0, 0.0, 30.0, 400.0)), None);
        let back = g.as_r();
        assert!(back.x0 <= 10.4 && back.y0 <= 20.6 && back.x1 >= 510.4 && back.y1 >= 320.9);
    }

    #[test]
    fn a_session_can_be_stopped_once_and_cancel_wins() {
        let stop = AtomicU8::new(RUNNING);
        assert!(stop.compare_exchange(RUNNING, DONE, Ordering::SeqCst, Ordering::SeqCst).is_ok());
        stop.store(CANCEL, Ordering::SeqCst);
        assert!(stop.compare_exchange(RUNNING, DONE, Ordering::SeqCst, Ordering::SeqCst).is_err());
        assert_eq!(stop.load(Ordering::SeqCst), CANCEL);
    }
}
