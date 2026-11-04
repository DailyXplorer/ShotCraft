//! What a capture points at: the window of the frontmost app, the display under the mouse, or all
//! displays. The functions here are pure so they run on any OS; the macOS calls that feed them
//! live in `system.rs`.
//!
//! Coordinates are global points with the origin at the top left of the main display. A display
//! left of or above the main one has negative coordinates, which is what `screencapture -R` expects.

use crate::model::Image;
use crate::system::CaptureMode;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    /// Half open: the left and top edges belong to the rectangle, the right and bottom ones to the neighbour.
    pub fn contains(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }

    fn distance_sq(&self, p: (f64, f64)) -> f64 {
        let dx = (self.x - p.0).max(0.0).max(p.0 - (self.x + self.w));
        let dy = (self.y - p.1).max(0.0).max(p.1 - (self.y + self.h));
        dx * dx + dy * dy
    }
}

/// A window as `CGWindowListCopyWindowInfo` describes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowInfo {
    pub id: u32,
    pub pid: i32,
    pub layer: i32,
    pub alpha: f64,
    pub bounds: Rect,
}

/// Windows narrower or shorter than this are helper windows, not something to screenshot.
pub const MIN_WINDOW_SIDE: f64 = 50.0;

/// The frontmost normal window of the app with `frontmost_pid`. `windows` is in front-to-back order,
/// as the window server returns it. Menu bar items, the Dock, floating panels and overlays live on
/// layers other than 0. ShotCraft's own windows are never the target.
pub fn pick_window(windows: &[WindowInfo], frontmost_pid: i32, own_pid: i32) -> Option<WindowInfo> {
    if frontmost_pid == own_pid {
        return None;
    }
    windows
        .iter()
        .find(|w| w.pid == frontmost_pid && w.layer == 0 && w.alpha > 0.0 && w.bounds.w >= MIN_WINDOW_SIDE && w.bounds.h >= MIN_WINDOW_SIDE)
        .copied()
}

/// A `NSScreen` frame has its origin at the bottom left of the main display; flip it to the top-left
/// origin that `screencapture` uses. `primary_height` is the height of the main display.
pub fn flip_frame(frame: Rect, primary_height: f64) -> Rect {
    Rect { x: frame.x, y: primary_height - (frame.y + frame.h), w: frame.w, h: frame.h }
}

pub fn flip_point(p: (f64, f64), primary_height: f64) -> (f64, f64) {
    (p.0, primary_height - p.1)
}

/// The display holding `p`, or the nearest one when the point sits on a seam or just outside.
pub fn display_at(displays: &[Rect], p: (f64, f64)) -> Option<usize> {
    if let Some(i) = displays.iter().position(|d| d.contains(p)) {
        return Some(i);
    }
    displays.iter().enumerate().min_by(|a, b| a.1.distance_sq(p).total_cmp(&b.1.distance_sq(p))).map(|(i, _)| i)
}

/// What the machine looks like when the capture is requested. Read on the main thread, before the
/// editor hides, so a capture started from the menu targets the app that was in front of the menu.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub own_pid: i32,
    pub frontmost_pid: Option<i32>,
    /// Displays in `NSScreen` order: the main display first.
    pub displays: Vec<Rect>,
    /// Pixels per point of each display, in the same order.
    pub scales: Vec<f64>,
    pub mouse: Option<(f64, f64)>,
    /// On-screen windows front to back. Read together with `frontmost_pid` so the two agree.
    pub windows: Vec<WindowInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// One non-interactive window capture by window id; the second field is the picture's pixels per point.
    Window(u32, f64),
    /// No usable window: let the user pick one.
    PickWindow,
    /// These display rectangles, one `screencapture` run each.
    Displays(Vec<Rect>),
    /// Display layout unknown: the main display.
    MainDisplay,
}

/// Pixels per point of the display that holds the middle of `r`; 1 when there are no displays.
pub fn scale_at(displays: &[Rect], scales: &[f64], r: Rect) -> f64 {
    display_at(displays, (r.x + r.w / 2.0, r.y + r.h / 2.0)).and_then(|i| scales.get(i).copied()).unwrap_or(1.0)
}

/// Where a non-interactive capture of `mode` goes.
pub fn resolve(mode: CaptureMode, snap: &Snapshot) -> Option<Target> {
    Some(match mode {
        CaptureMode::ActiveWindow => match snap.frontmost_pid.and_then(|pid| pick_window(&snap.windows, pid, snap.own_pid)) {
            Some(w) => Target::Window(w.id, scale_at(&snap.displays, &snap.scales, w.bounds)),
            None => Target::PickWindow,
        },
        CaptureMode::Fullscreen | CaptureMode::Delayed => match snap.mouse.and_then(|m| display_at(&snap.displays, m)) {
            Some(i) => Target::Displays(vec![snap.displays[i]]),
            None => Target::MainDisplay,
        },
        CaptureMode::AllDisplays if snap.displays.is_empty() => Target::MainDisplay,
        CaptureMode::AllDisplays => Target::Displays(snap.displays.clone()),
        CaptureMode::Area | CaptureMode::RepeatArea | CaptureMode::Window => return None,
    })
}

/// `-R x,y,w,h` in whole points.
pub fn rect_arg(r: Rect) -> String {
    format!("-R{},{},{},{}", r.x.round() as i64, r.y.round() as i64, r.w.round() as i64, r.h.round() as i64)
}

/// `screencapture` arguments for one run, without the output file. `-x` mutes the shutter sound.
pub fn window_args(id: u32, trim_shadow: bool) -> Vec<String> {
    let mut a = vec!["-x".to_string(), format!("-l{id}")];
    if trim_shadow {
        a.push("-o".into());
    }
    a
}

pub fn display_args(r: Rect) -> Vec<String> {
    vec!["-x".to_string(), rect_arg(r)]
}

/// Where each display lands in the one image that holds all of them. The canvas has the pixel density
/// of the sharpest display; a display with fewer pixels per point is scaled up to match.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    /// Top-left corner and size in pixels of each input rectangle, in input order.
    pub cells: Vec<(u32, u32, u32, u32)>,
}

pub fn layout(displays: &[Rect], scales: &[f64]) -> Option<Layout> {
    if displays.is_empty() || displays.len() != scales.len() {
        return None;
    }
    let min_x = displays.iter().map(|d| d.x).fold(f64::INFINITY, f64::min);
    let min_y = displays.iter().map(|d| d.y).fold(f64::INFINITY, f64::min);
    let max_x = displays.iter().map(|d| d.x + d.w).fold(f64::NEG_INFINITY, f64::max);
    let max_y = displays.iter().map(|d| d.y + d.h).fold(f64::NEG_INFINITY, f64::max);
    let scale = scales.iter().cloned().fold(1.0, f64::max);
    let px = |v: f64| (v * scale).round() as u32;
    let cells = displays.iter().map(|d| (px(d.x - min_x), px(d.y - min_y), px(d.w), px(d.h))).collect();
    Some(Layout { width: px(max_x - min_x), height: px(max_y - min_y), scale, cells })
}

/// Draws one captured image per display on a transparent canvas laid out like the displays.
pub fn compose(displays: &[Rect], shots: &[Image]) -> Option<Image> {
    let scales: Vec<f64> = displays.iter().zip(shots).map(|(d, s)| s.w as f64 / d.w).collect();
    if shots.len() != displays.len() {
        return None;
    }
    let lay = layout(displays, &scales)?;
    let mut out = vec![0u8; lay.width as usize * lay.height as usize * 4];
    for (shot, &(cx, cy, cw, ch)) in shots.iter().zip(&lay.cells) {
        let rgba;
        // A picture a pixel off its cell (fractional point sizes) is copied as it is; a different density is resized.
        let close = shot.w.abs_diff(cw) <= 1 && shot.h.abs_diff(ch) <= 1;
        let (cw, ch) = if close { (shot.w, shot.h) } else { (cw, ch) };
        let src: &[u8] = if close {
            &shot.px
        } else {
            let from = image::RgbaImage::from_raw(shot.w, shot.h, shot.px.to_vec())?;
            rgba = image::imageops::resize(&from, cw, ch, image::imageops::FilterType::Triangle);
            rgba.as_raw()
        };
        for row in 0..ch.min(lay.height.saturating_sub(cy)) {
            let w = cw.min(lay.width.saturating_sub(cx)) as usize * 4;
            let from = row as usize * cw as usize * 4;
            let to = ((cy + row) as usize * lay.width as usize + cx as usize) * 4;
            out[to..to + w].copy_from_slice(&src[from..from + w]);
        }
    }
    Some(Image { w: lay.width, h: lay.height, px: Arc::new(out) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u32, pid: i32, layer: i32, alpha: f64, w: f64, h: f64) -> WindowInfo {
        WindowInfo { id, pid, layer, alpha, bounds: Rect::new(10.0, 10.0, w, h) }
    }

    #[test]
    fn picks_the_first_normal_window_of_the_frontmost_app() {
        let windows = [
            win(1, 99, 0, 1.0, 800.0, 600.0),  // another app, in front of it
            win(2, 42, 25, 1.0, 300.0, 30.0),  // its status item, layer 25
            win(3, 42, 0, 0.0, 800.0, 600.0),  // invisible
            win(4, 42, 0, 1.0, 0.0, 0.0),      // zero size
            win(5, 42, 0, 1.0, 20.0, 20.0),    // helper window
            win(6, 42, 0, 1.0, 900.0, 700.0),  // the one
            win(7, 42, 0, 1.0, 1000.0, 800.0), // behind it
        ];
        assert_eq!(pick_window(&windows, 42, 7).map(|w| w.id), Some(6));
    }

    #[test]
    fn no_window_when_the_app_has_none_or_is_this_app() {
        let windows = [win(1, 99, 0, 1.0, 800.0, 600.0)];
        assert_eq!(pick_window(&windows, 42, 7), None);
        assert_eq!(pick_window(&windows, 99, 99), None);
        assert_eq!(pick_window(&[], 42, 7), None);
    }

    #[test]
    fn flips_screen_frames_to_the_top_left_origin() {
        // Main 1440×900; a 1920×1080 display above it and one to its left, in bottom-left coordinates.
        let above = flip_frame(Rect::new(0.0, 900.0, 1920.0, 1080.0), 900.0);
        assert_eq!(above, Rect::new(0.0, -1080.0, 1920.0, 1080.0));
        let left = flip_frame(Rect::new(-1920.0, 0.0, 1920.0, 1080.0), 900.0);
        assert_eq!(left, Rect::new(-1920.0, -180.0, 1920.0, 1080.0));
        assert_eq!(flip_frame(Rect::new(0.0, 0.0, 1440.0, 900.0), 900.0), Rect::new(0.0, 0.0, 1440.0, 900.0));
        assert_eq!(flip_point((100.0, 850.0), 900.0), (100.0, 50.0));
    }

    fn setup() -> Vec<Rect> {
        vec![Rect::new(0.0, 0.0, 1440.0, 900.0), Rect::new(-1920.0, -180.0, 1920.0, 1080.0), Rect::new(1440.0, 100.0, 1280.0, 720.0)]
    }

    #[test]
    fn finds_the_display_under_the_point_with_negative_coordinates() {
        let d = setup();
        assert_eq!(display_at(&d, (700.0, 450.0)), Some(0));
        assert_eq!(display_at(&d, (-1.0, 10.0)), Some(1));
        assert_eq!(display_at(&d, (-1920.0, -180.0)), Some(1));
        assert_eq!(display_at(&d, (1440.0, 100.0)), Some(2));
        assert_eq!(display_at(&d, (1439.5, 300.0)), Some(0));
    }

    #[test]
    fn a_point_outside_every_display_goes_to_the_nearest_one() {
        let d = setup();
        assert_eq!(display_at(&d, (700.0, -500.0)), Some(0)); // above the main display
        assert_eq!(display_at(&d, (0.0, -500.0)), Some(1)); // above the seam: the left display is 320 pt away, the main one 500
        assert_eq!(display_at(&d, (3000.0, 400.0)), Some(2));
        assert_eq!(display_at(&d, (-5000.0, 0.0)), Some(1));
        assert_eq!(display_at(&[], (0.0, 0.0)), None);
    }

    fn snap(pid: Option<i32>, mouse: Option<(f64, f64)>) -> Snapshot {
        let windows = vec![win(9, 42, 0, 1.0, 800.0, 600.0)]; // at (10, 10) on the main display
        Snapshot { own_pid: 7, frontmost_pid: pid, displays: setup(), scales: vec![2.0, 1.0, 1.0], mouse, windows }
    }

    #[test]
    fn resolves_each_mode() {
        assert_eq!(resolve(CaptureMode::ActiveWindow, &snap(Some(42), None)), Some(Target::Window(9, 2.0)));
        assert_eq!(resolve(CaptureMode::ActiveWindow, &snap(Some(43), None)), Some(Target::PickWindow));
        assert_eq!(resolve(CaptureMode::ActiveWindow, &snap(None, None)), Some(Target::PickWindow));
        assert_eq!(resolve(CaptureMode::ActiveWindow, &snap(Some(7), None)), Some(Target::PickWindow));
        let left = setup()[1];
        assert_eq!(resolve(CaptureMode::Fullscreen, &snap(None, Some((-300.0, 20.0)))), Some(Target::Displays(vec![left])));
        assert_eq!(resolve(CaptureMode::Delayed, &snap(None, Some((-300.0, 20.0)))), Some(Target::Displays(vec![left])));
        assert_eq!(resolve(CaptureMode::Fullscreen, &snap(None, None)), Some(Target::MainDisplay));
        assert_eq!(resolve(CaptureMode::AllDisplays, &snap(None, None)), Some(Target::Displays(setup())));
        assert_eq!(resolve(CaptureMode::AllDisplays, &Snapshot::default()), Some(Target::MainDisplay));
        assert_eq!(resolve(CaptureMode::Area, &snap(None, None)), None);
    }

    #[test]
    fn a_window_takes_the_density_of_its_display() {
        let d = setup();
        let on_left = Rect::new(-1500.0, 0.0, 400.0, 300.0);
        assert_eq!(scale_at(&d, &[2.0, 1.0, 1.0], Rect::new(10.0, 10.0, 800.0, 600.0)), 2.0);
        assert_eq!(scale_at(&d, &[2.0, 1.5, 1.0], on_left), 1.5);
        assert_eq!(scale_at(&[], &[], on_left), 1.0);
    }

    #[test]
    fn builds_screencapture_arguments() {
        assert_eq!(window_args(120, false), ["-x", "-l120"]);
        assert_eq!(window_args(120, true), ["-x", "-l120", "-o"]);
        assert_eq!(display_args(Rect::new(-1920.0, -180.0, 1920.0, 1080.0)), ["-x", "-R-1920,-180,1920,1080"]);
    }

    #[test]
    fn lays_out_all_displays_on_one_canvas() {
        let d = setup();
        let lay = layout(&d, &[2.0, 1.0, 1.0]).unwrap();
        // Union: x -1920..2720 (4640 pt), y -180..900 (1080 pt) at 2 px per point.
        assert_eq!((lay.width, lay.height, lay.scale), (9280, 2160, 2.0));
        assert_eq!(lay.cells[0], (3840, 360, 2880, 1800));
        assert_eq!(lay.cells[1], (0, 0, 3840, 2160));
        assert_eq!(lay.cells[2], (6720, 560, 2560, 1440));
        assert_eq!(layout(&[], &[]), None);
        assert_eq!(layout(&d, &[1.0]), None);
    }

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Image {
        Image { w, h, px: Arc::new(c.iter().cycle().take(w as usize * h as usize * 4).cloned().collect()) }
    }

    fn pixel(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * img.w as usize + x as usize) * 4;
        img.px[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn composes_displays_of_different_densities() {
        // A Retina display (4×2 points, 8×4 px) with a plain one (2×2 points, 2×2 px) on its left, 1 point lower.
        let displays = [Rect::new(0.0, 0.0, 4.0, 2.0), Rect::new(-2.0, 1.0, 2.0, 2.0)];
        let shots = [solid(8, 4, [255, 0, 0, 255]), solid(2, 2, [0, 0, 255, 255])];
        let out = compose(&displays, &shots).unwrap();
        assert_eq!((out.w, out.h), (12, 6));
        assert_eq!(pixel(&out, 4, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&out, 11, 3), [255, 0, 0, 255]);
        assert_eq!(pixel(&out, 0, 2), [0, 0, 255, 255]);
        assert_eq!(pixel(&out, 3, 5), [0, 0, 255, 255]);
        // Corner not covered by any display stays transparent.
        assert_eq!(pixel(&out, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&out, 11, 5), [0, 0, 0, 0]);
    }

    #[test]
    fn a_picture_one_pixel_off_its_cell_is_copied_not_resized() {
        // 2.5 × 2 points at 2 px per point: the cell is 5 × 4 px, the picture 5 × 5.
        let displays = [Rect::new(0.0, 0.0, 2.5, 2.0)];
        let out = compose(&displays, &[solid(5, 5, [9, 9, 9, 255])]).unwrap();
        assert_eq!((out.w, out.h), (5, 4));
        assert_eq!(pixel(&out, 4, 3), [9, 9, 9, 255]);
    }

    #[test]
    fn compose_needs_one_shot_per_display() {
        assert!(compose(&setup(), &[solid(1, 1, [0; 4])]).is_none());
    }
}
