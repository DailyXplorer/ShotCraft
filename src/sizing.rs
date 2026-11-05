//! Window sizes derived from a picture. Every one stays inside a bounded box and keeps the aspect
//! ratio, so a very tall picture (a scrolling capture) is never squashed or opens a huge window.
//! Pure functions on `(width, height)` pairs, in points, so they can be tested without a display.

/// Padding between the thumbnail window edge and its picture.
pub const THUMB_PAD: f32 = 4.0;
pub const THUMB_MIN_HEIGHT: f32 = 90.0;

/// A pin never grows beyond this share of the screen on either axis.
pub const PIN_SCREEN_FRACTION: f32 = 0.9;

pub type Size = (f32, f32);

fn valid(s: Size) -> bool {
    s.0.is_finite() && s.1.is_finite() && s.0 > 0.0 && s.1 > 0.0
}

/// Largest size with the aspect ratio of `size` that fits in `max`. Pictures that already fit keep
/// their size unless `upscale` is set.
pub fn fit_within(size: Size, max: Size, upscale: bool) -> Size {
    if !valid(size) || !valid(max) {
        return (0.0, 0.0);
    }
    let k = (max.0 / size.0).min(max.1 / size.1);
    let k = if upscale { k } else { k.min(1.0) };
    (size.0 * k, size.1 * k)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbLayout {
    /// The window: always as wide as the action buttons need, and at most `max_height` high.
    pub window: Size,
    /// The picture, centred in the window.
    pub image: Size,
}

pub fn thumb_layout(img_px: (u32, u32), width: f32, max_height: f32) -> ThumbLayout {
    let inner = (width - 2.0 * THUMB_PAD, max_height - 2.0 * THUMB_PAD);
    let image = fit_within((img_px.0 as f32, img_px.1 as f32), inner, true);
    let height = (image.1 + 2.0 * THUMB_PAD).clamp(THUMB_MIN_HEIGHT.min(max_height), max_height);
    ThumbLayout { window: (width, height), image }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinLayout {
    /// The whole picture as drawn. Same aspect ratio as the image.
    pub content: Size,
    /// The window. Shorter than `content` when the picture is taller than the screen: the pin then scrolls.
    pub window: Size,
}

impl PinLayout {
    pub fn max_scroll(&self) -> f32 {
        (self.content.1 - self.window.1).max(0.0)
    }
}

/// `content` is the picture at the wanted size (image points × zoom). Too wide for `max`: shrunk
/// uniformly. Too tall: the window stops at `max` height and the picture scrolls inside it.
pub fn pin_layout(content: Size, max: Size) -> PinLayout {
    if !valid(content) || !valid(max) {
        return PinLayout { content: (1.0, 1.0), window: (1.0, 1.0) };
    }
    let k = (max.0 / content.0).min(1.0);
    let content = (content.0 * k, content.1 * k);
    PinLayout { content, window: (content.0, content.1.min(max.1)) }
}

pub fn pin_max(screen: Size) -> Size {
    (screen.0 * PIN_SCREEN_FRACTION, screen.1 * PIN_SCREEN_FRACTION)
}

/// A pin never gets narrower than this when the wheel zooms it out (a smaller picture can still
/// be zoomed in).
pub const PIN_MIN_WIDTH: f32 = 100.0;
/// A trackpad swipe moves the pin one zoom step per this many points of scrolling.
pub const PIN_WHEEL_THRESHOLD: f32 = 20.0;

/// The zooms the wheel steps through, in percent of the picture at its own size: about 20 % apart,
/// and including the 50, 100 and 200 % of the pin menu. One table for both directions, so a notch
/// up followed by a notch down always comes back.
pub const PIN_ZOOM_STOPS: [u32; 31] = [
    5, 10, 15, 20, 25, 30, 40, 50, 60, 70, 80, 90, 100, 125, 150, 175, 200, 250, 300, 350, 400, 500, 600, 700, 800, 1000, 1200, 1500, 2000, 3000, 4000,
];

/// The zoom (in percent) one wheel step away from `current_pct`, or `None` when the pin is at its
/// limit. `shown_width` is the pin's width now and `max_width` the widest it may get. A zoom that
/// is not on the table (the pin was capped by the screen) goes to the next stop in that direction.
pub fn pin_zoom_step(current_pct: u32, zoom_in: bool, shown_width: f32, max_width: f32) -> Option<u32> {
    if !shown_width.is_finite() || !max_width.is_finite() {
        return None;
    }
    if zoom_in && shown_width >= max_width || !zoom_in && shown_width <= PIN_MIN_WIDTH {
        return None;
    }
    if zoom_in {
        PIN_ZOOM_STOPS.iter().copied().find(|&z| z > current_pct)
    } else {
        PIN_ZOOM_STOPS.iter().rev().copied().find(|&z| z < current_pct)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelPhase {
    Start,
    Move,
    End,
}

/// A swipe that starts less than this long after the fingers lifted is taken for the start of the
/// inertia (macOS reports both as "started"), not for a new swipe.
pub const WHEEL_INERTIA_GAP_SECS: f64 = 0.15;

/// Turns wheel events over a pin into zoom steps.
///
/// A classic wheel steps once per notch. A trackpad (`precise`) adds its deltas up and steps once
/// per `PIN_WHEEL_THRESHOLD` points, so a slow swipe is a few steps and not one per event. The
/// inertia that follows a swipe is dropped: otherwise one flick would zoom the pin all the way.
#[derive(Debug, Clone, Copy)]
pub struct WheelZoom {
    acc: f32,
    coasting: bool,
    last_end: f64,
}

impl Default for WheelZoom {
    fn default() -> Self {
        WheelZoom { acc: 0.0, coasting: false, last_end: f64::NEG_INFINITY }
    }
}

impl WheelZoom {
    /// `dy` > 0 is a scroll up, which zooms in unless `reverse`; `now` is in seconds. Returns
    /// `Some(zoom_in)` for a step.
    pub fn feed(&mut self, precise: bool, dy: f32, phase: WheelPhase, reverse: bool, now: f64) -> Option<bool> {
        match phase {
            WheelPhase::Start => {
                if now - self.last_end > WHEEL_INERTIA_GAP_SECS {
                    self.coasting = false;
                    self.acc = 0.0;
                }
            }
            WheelPhase::End => {
                self.coasting = precise;
                self.last_end = now;
                self.acc = 0.0;
                return None;
            }
            WheelPhase::Move => {}
        }
        if !dy.is_finite() || dy == 0.0 || (precise && self.coasting) {
            return None;
        }
        let up = if precise {
            self.acc += dy;
            if self.acc.abs() <= PIN_WHEEL_THRESHOLD {
                return None;
            }
            let up = self.acc > 0.0;
            self.acc -= PIN_WHEEL_THRESHOLD.copysign(self.acc);
            up
        } else {
            dy > 0.0
        };
        Some(up != reverse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinWheel {
    Zoom,
    Scroll,
}

/// What the wheel does over a pin. It resizes the pin, as in the original. A pin whose picture is
/// taller than the screen at its own size (a long scrolling capture, `tall`) scrolls instead,
/// since the wheel is the only way to read it; Option or Command held with the wheel does the
/// other one. `tall` is a property of the picture, not of the current zoom, so the wheel does not
/// change meaning in the middle of a gesture.
pub fn pin_wheel_action(tall: bool, flip: bool) -> PinWheel {
    if tall != flip {
        PinWheel::Scroll
    } else {
        PinWheel::Zoom
    }
}

/// Window origin that keeps the window centred on the same point when it goes from `old` to `new`.
pub fn recentered(origin: Size, old: Size, new: Size) -> Size {
    (origin.0 + (old.0 - new.0) / 2.0, origin.1 + (old.1 - new.1) / 2.0)
}

/// Scroll offset after the picture goes from height `old_h` to `new_h`: the same part of the picture
/// stays at the top of the window.
pub fn rescaled_scroll(scroll: f32, old_h: f32, new_h: f32) -> f32 {
    if old_h > 0.0 && new_h > 0.0 && scroll.is_finite() {
        scroll * new_h / old_h
    } else {
        0.0
    }
}

/// Gap between the thumbnail and the right edge of the screen, and its bottom edge (which leaves
/// room for the Dock).
pub const THUMB_MARGIN_RIGHT: f32 = 24.0;
pub const THUMB_MARGIN_BOTTOM: f32 = 72.0;
/// In automatic mode, the thumbnail goes away this long after the pointer last left it.
pub const THUMB_AUTO_HIDE_SECS: f32 = 6.0;

/// Top-left corner of the thumbnail window: bottom right of the screen.
pub fn thumb_origin(screen: Size, window: Size) -> Size {
    (screen.0 - window.0 - THUMB_MARGIN_RIGHT, screen.1 - window.1 - THUMB_MARGIN_BOTTOM)
}

/// Whether the thumbnail should go away. Manual mode never closes it (only its close button, or
/// using the picture, does). Automatic mode closes it when it loses focus (a click anywhere else),
/// or after `THUMB_AUTO_HIDE_SECS` without the pointer on it.
pub fn thumb_should_close(auto: bool, idle_secs: f32, hovered: bool, lost_focus: bool) -> bool {
    auto && (lost_focus || (!hovered && idle_secs > THUMB_AUTO_HIDE_SECS))
}

/// Focus tracking for the thumbnail: "lost focus" means it had focus and no longer has it. A
/// thumbnail that never got focus is left to the timer.
pub fn lost_focus(had_focus: bool, has_focus: bool, already_lost: bool) -> bool {
    already_lost || (had_focus && !has_focus)
}

/// Editor window for a picture of `content` points: close to a square whatever the shape of the
/// picture, at least `min_width` wide, never larger than the screen.
pub fn editor_window(content: Size, chrome: f32, margin: f32, min_width: f32, min_height_ratio: f32, screen: Size) -> Size {
    let width = (content.0 + margin).max(min_width).min(screen.0);
    let height = (content.1 + chrome + margin).max(width * min_height_ratio).min(screen.1);
    (width, height)
}

/// Editor chrome, in points. The toolbar sits in the title bar (traffic lights on its left), the
/// options bar under it holds the active tool's settings, the status bar is the bottom strip.
pub const TOOLBAR_H: f32 = 38.0;
pub const OPTIONS_H: f32 = 48.0;
pub const STATUS_H: f32 = 34.0;
/// The editor window never gets smaller than this.
pub const EDITOR_MIN_SIZE: Size = (480.0, 270.0);
pub const TRAFFIC_LIGHTS_W: f32 = 78.0;
pub const TOOLBAR_SEG_W: f32 = 30.0;
pub const TOOLBAR_GROUP_GAP: f32 = 10.0;
pub const TOOLBAR_EDGE: f32 = 12.0;
/// Segments in the toolbar groups: Copy/Save/Pin, the drawing tools, the selection actions, Preferences.
pub const TOOLBAR_COPY_ITEMS: usize = 3;
pub const TOOLBAR_TOOL_ITEMS: usize = crate::model::Tool::TOOLBAR.len();
pub const TOOLBAR_ACTION_ITEMS: usize = 7;

/// Width of a segmented control of `items` segments (a 2 pt frame on each side).
pub fn seg_group_w(items: usize) -> f32 {
    TOOLBAR_SEG_W * items as f32 + 4.0
}

/// What the toolbar shows in a window of a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarFit {
    /// How many of the tool and action segments (tools first, then the selection actions) are laid
    /// out; the others go in the overflow menu.
    pub visible: usize,
}

impl ToolbarFit {
    pub const ALL: usize = TOOLBAR_TOOL_ITEMS + TOOLBAR_ACTION_ITEMS;

    pub fn overflows(self) -> bool {
        self.visible < Self::ALL
    }

    pub fn tools(self) -> usize {
        self.visible.min(TOOLBAR_TOOL_ITEMS)
    }

    pub fn actions(self) -> usize {
        self.visible.saturating_sub(TOOLBAR_TOOL_ITEMS)
    }

    /// Width taken by the segments laid out, with the overflow button when needed, and the
    /// Preferences button at the right.
    pub fn width(self) -> f32 {
        let mut w = seg_group_w(TOOLBAR_COPY_ITEMS) + seg_group_w(1) + TOOLBAR_GROUP_GAP;
        for n in [self.tools(), self.actions()] {
            if n > 0 {
                w += seg_group_w(n) + TOOLBAR_GROUP_GAP;
            }
        }
        if self.overflows() {
            w += seg_group_w(1) + TOOLBAR_GROUP_GAP;
        }
        w
    }
}

impl ToolbarFit {
    /// Width of the groups that sit together in the middle of the toolbar: everything but the
    /// Preferences button, which is pinned to the right edge.
    pub fn cluster_width(self) -> f32 {
        self.width() - seg_group_w(1) - TOOLBAR_GROUP_GAP
    }
}

/// Most segments that fit in a toolbar of `window_width`: Copy/Save/Pin and Preferences stay, the
/// selection actions then the last tools move into the overflow menu as the window narrows. The
/// middle groups are centred in the window, so they must fit between two margins as wide as the
/// traffic lights' (the Preferences button sits in the right one).
pub fn toolbar_fit(window_width: f32) -> ToolbarFit {
    let room = window_width - 2.0 * TRAFFIC_LIGHTS_W;
    (0..=ToolbarFit::ALL)
        .rev()
        .map(|visible| ToolbarFit { visible })
        .find(|f| f.cluster_width() <= room)
        .unwrap_or(ToolbarFit { visible: 0 })
}

/// Where the middle groups start, from the left edge of the toolbar: centred in the window, never
/// under the traffic lights nor into the Preferences button.
pub fn toolbar_cluster_left(window_width: f32, fit: ToolbarFit) -> f32 {
    let last_left = window_width - TOOLBAR_EDGE - seg_group_w(1) - TOOLBAR_GROUP_GAP - fit.cluster_width();
    ((window_width - fit.cluster_width()) / 2.0).min(last_left).max(TRAFFIC_LIGHTS_W)
}

/// Narrowest window that shows every tool (and the selection actions) in the toolbar.
pub fn full_toolbar_width() -> f32 {
    ToolbarFit { visible: ToolbarFit::ALL }.cluster_width() + 2.0 * TRAFFIC_LIGHTS_W
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(s: Size) -> f32 {
        s.1 / s.0
    }

    #[test]
    fn fit_keeps_aspect_ratio_inside_the_box() {
        for (img, max) in [((1000.0, 8000.0), (196.0, 182.0)), ((8000.0, 1000.0), (196.0, 182.0)), ((300.0, 200.0), (196.0, 182.0))] {
            let f = fit_within(img, max, true);
            assert!(f.0 <= max.0 + 0.01 && f.1 <= max.1 + 0.01, "{f:?} exceeds {max:?}");
            assert!((ratio(f) - ratio(img)).abs() < 1e-3, "{f:?} distorts {img:?}");
        }
    }

    #[test]
    fn fit_only_upscales_on_request() {
        assert_eq!(fit_within((100.0, 50.0), (400.0, 400.0), false), (100.0, 50.0));
        assert_eq!(fit_within((100.0, 50.0), (400.0, 400.0), true), (400.0, 200.0));
    }

    #[test]
    fn fit_rejects_degenerate_sizes() {
        assert_eq!(fit_within((0.0, 10.0), (100.0, 100.0), true), (0.0, 0.0));
        assert_eq!(fit_within((10.0, 10.0), (0.0, 100.0), true), (0.0, 0.0));
        assert_eq!(fit_within((f32::NAN, 10.0), (100.0, 100.0), true), (0.0, 0.0));
    }

    #[test]
    fn tall_thumbnail_is_not_squashed() {
        let t = thumb_layout((1000, 8000), 204.0, 190.0);
        assert_eq!(t.window, (204.0, 190.0));
        assert!((ratio(t.image) - 8.0).abs() < 1e-3, "{:?}", t.image);
        assert!(t.image.1 <= 190.0 - 2.0 * THUMB_PAD + 0.01);
        assert!(t.image.0 < 40.0);
    }

    #[test]
    fn wide_thumbnail_fills_the_width_and_keeps_a_minimum_height() {
        let t = thumb_layout((4000, 100), 204.0, 190.0);
        assert_eq!(t.window, (204.0, THUMB_MIN_HEIGHT));
        assert!((t.image.0 - 196.0).abs() < 1e-3);
        assert!((ratio(t.image) - 0.025).abs() < 1e-4);
    }

    #[test]
    fn thumbnail_window_always_fits_its_picture() {
        for (w, h) in [(1, 1), (10, 5000), (5000, 10), (640, 480), (480, 640), (1000, 8000)] {
            let t = thumb_layout((w, h), 204.0, 190.0);
            assert!(t.window.1 <= 190.0 && t.window.0 == 204.0);
            assert!(t.image.0 + 2.0 * THUMB_PAD <= t.window.0 + 0.01 && t.image.1 + 2.0 * THUMB_PAD <= t.window.1 + 0.01, "{w}x{h}: {t:?}");
        }
    }

    #[test]
    fn small_pin_keeps_its_size() {
        let p = pin_layout((400.0, 300.0), pin_max((1440.0, 900.0)));
        assert_eq!(p.content, (400.0, 300.0));
        assert_eq!(p.window, (400.0, 300.0));
        assert_eq!(p.max_scroll(), 0.0);
    }

    #[test]
    fn tall_pin_window_is_bounded_and_scrolls() {
        // 1000×8000 px captured at 2×: 500×4000 points.
        let max = pin_max((1440.0, 900.0));
        let p = pin_layout((500.0, 4000.0), max);
        assert_eq!(p.content, (500.0, 4000.0));
        assert_eq!(p.window, (500.0, max.1));
        assert!(p.window.1 <= 900.0 * PIN_SCREEN_FRACTION + 0.01);
        assert!((p.max_scroll() - (4000.0 - max.1)).abs() < 1e-3);
    }

    #[test]
    fn wide_pin_shrinks_uniformly_to_the_screen() {
        let max = pin_max((1000.0, 800.0));
        let p = pin_layout((3000.0, 1500.0), max);
        assert!((p.content.0 - max.0).abs() < 1e-3);
        assert!((ratio(p.content) - 0.5).abs() < 1e-4);
        assert!(p.window.1 <= max.1);
    }

    #[test]
    fn zoomed_tall_pin_stays_inside_the_screen() {
        let max = pin_max((1440.0, 900.0));
        for zoom in [0.5, 1.0, 2.0] {
            let p = pin_layout((500.0 * zoom, 4000.0 * zoom), max);
            assert!(p.window.0 <= max.0 + 0.01 && p.window.1 <= max.1 + 0.01);
            assert!((ratio(p.content) - 8.0).abs() < 1e-3);
        }
    }

    #[test]
    fn degenerate_pin_does_not_produce_an_empty_window() {
        let p = pin_layout((0.0, 0.0), (100.0, 100.0));
        assert!(p.window.0 >= 1.0 && p.window.1 >= 1.0);
    }

    #[test]
    fn editor_window_never_exceeds_the_screen() {
        let screen = (1440.0, 900.0);
        for content in [(300.0, 200.0), (500.0, 4000.0), (6000.0, 300.0), (6000.0, 6000.0)] {
            let (w, h) = editor_window(content, 120.0, 48.0, 940.0, 0.9, screen);
            assert!(w <= screen.0 && h <= screen.1, "{content:?} -> {w}x{h}");
            assert!(w >= 940.0);
        }
        assert_eq!(editor_window((500.0, 4000.0), 120.0, 48.0, 940.0, 0.9, screen), (940.0, 900.0));
    }

    #[test]
    fn thumbnail_has_the_original_size() {
        assert_eq!(crate::model::THUMB_WIDTH, 204.0);
        assert_eq!(crate::model::THUMB_MAX_HEIGHT, 190.0);
        let t = thumb_layout((1920, 1080), crate::model::THUMB_WIDTH, crate::model::THUMB_MAX_HEIGHT);
        assert_eq!(t.window.0, 204.0);
        assert!(t.window.1 <= 190.0);
    }

    #[test]
    fn thumbnail_sits_at_the_bottom_right() {
        let screen = (1440.0, 900.0);
        let window = (204.0, 150.0);
        let o = thumb_origin(screen, window);
        assert_eq!(o, (1440.0 - 204.0 - THUMB_MARGIN_RIGHT, 900.0 - 150.0 - THUMB_MARGIN_BOTTOM));
        assert!(o.0 + window.0 <= screen.0 && o.1 + window.1 <= screen.1);
    }

    #[test]
    fn manual_thumbnail_never_closes_itself() {
        assert!(!thumb_should_close(false, 1000.0, false, true));
    }

    #[test]
    fn automatic_thumbnail_closes_when_it_loses_focus() {
        assert!(thumb_should_close(true, 0.0, true, true));
        assert!(!thumb_should_close(true, 0.0, false, false));
    }

    #[test]
    fn automatic_thumbnail_times_out_unless_the_pointer_is_on_it() {
        assert!(!thumb_should_close(true, THUMB_AUTO_HIDE_SECS - 0.1, false, false));
        assert!(thumb_should_close(true, THUMB_AUTO_HIDE_SECS + 0.1, false, false));
        assert!(!thumb_should_close(true, 100.0, true, false));
    }

    #[test]
    fn focus_loss_needs_focus_first() {
        assert!(!lost_focus(false, false, false), "never focused: the timer decides");
        assert!(!lost_focus(true, true, false));
        assert!(lost_focus(true, false, false));
        assert!(lost_focus(false, true, true), "once lost, stays lost");
    }

    #[test]
    fn zoom_steps_walk_a_ladder_in_both_directions() {
        let up = |z| pin_zoom_step(z, true, 300.0, 5000.0);
        let down = |z| pin_zoom_step(z, false, 300.0, 5000.0);
        assert_eq!(up(100), Some(125));
        assert_eq!(down(100), Some(90));
        assert_eq!(up(50), Some(60));
        assert_eq!(down(200), Some(175));
        // off the table (capped by the screen): next stop that way
        assert_eq!(up(259), Some(300));
        assert_eq!(down(259), Some(250));
        for pct in PIN_ZOOM_STOPS {
            if let Some(u) = up(pct) {
                assert!(u > pct && down(u) == Some(pct), "{pct} up then down");
            }
        }
        assert_eq!(up(4000), None);
        assert_eq!(down(5), None);
    }

    #[test]
    fn zoom_steps_are_about_twenty_percent_apart_and_cover_the_menu_sizes() {
        for w in PIN_ZOOM_STOPS.windows(2) {
            assert!(w[1] > w[0] && w[1] as f32 / w[0] as f32 <= 2.0 + 1e-3, "{w:?}");
        }
        for menu in [50, 100, 200] {
            assert!(PIN_ZOOM_STOPS.contains(&menu));
        }
    }

    #[test]
    fn zoom_out_stops_at_the_minimum_width_and_zoom_in_at_the_screen() {
        assert_eq!(pin_zoom_step(100, false, PIN_MIN_WIDTH, 1000.0), None);
        assert_eq!(pin_zoom_step(100, false, PIN_MIN_WIDTH - 40.0, 1000.0), None);
        assert!(pin_zoom_step(100, false, PIN_MIN_WIDTH + 1.0, 1000.0).is_some());
        assert_eq!(pin_zoom_step(100, true, 1000.0, 1000.0), None);
        assert!(pin_zoom_step(100, true, 999.0, 1000.0).is_some());
        // a tiny picture can still grow
        assert!(pin_zoom_step(100, true, 40.0, 1000.0).is_some());
        assert_eq!(pin_zoom_step(100, true, f32::NAN, 1000.0), None);
    }

    #[test]
    fn a_wheel_notch_is_one_step_and_reverse_flips_it() {
        let mut w = WheelZoom::default();
        assert_eq!(w.feed(false, 1.0, WheelPhase::Move, false, 0.0), Some(true));
        assert_eq!(w.feed(false, -1.0, WheelPhase::Move, false, 0.1), Some(false));
        assert_eq!(w.feed(false, 1.0, WheelPhase::Move, true, 0.2), Some(false));
        assert_eq!(w.feed(false, 0.0, WheelPhase::Move, false, 0.3), None);
    }

    #[test]
    fn a_trackpad_steps_once_per_twenty_points() {
        let mut w = WheelZoom::default();
        assert_eq!(w.feed(true, 1.0, WheelPhase::Start, false, 0.0), None);
        let mut steps = 0;
        for i in 0..30 {
            if w.feed(true, 3.0, WheelPhase::Move, false, 0.01 * i as f64) == Some(true) {
                steps += 1;
            }
        }
        // 1 + 90 points in: a step each time the sum passes 20 (and 20 is taken off).
        assert_eq!(steps, 4);
        assert_eq!(w.feed(true, -50.0, WheelPhase::Move, false, 1.0), Some(false));
    }

    #[test]
    fn trackpad_inertia_is_ignored_until_a_new_swipe() {
        let mut w = WheelZoom::default();
        w.feed(true, 0.0, WheelPhase::Start, false, 0.0);
        assert_eq!(w.feed(true, 30.0, WheelPhase::Move, false, 0.1), Some(true));
        w.feed(true, 0.0, WheelPhase::End, false, 0.2);
        // inertia: reported as a start 40 ms after the fingers lifted, then moves
        w.feed(true, 40.0, WheelPhase::Start, false, 0.24);
        for i in 0..20 {
            assert_eq!(w.feed(true, 40.0, WheelPhase::Move, false, 0.3 + 0.01 * i as f64), None);
        }
        // a mouse wheel is not affected by it
        assert_eq!(w.feed(false, 1.0, WheelPhase::Move, false, 0.6), Some(true));
        // a new swipe, well after the last end, works again
        assert_eq!(w.feed(true, 30.0, WheelPhase::Start, false, 1.5), Some(true));
    }

    #[test]
    fn tall_pins_scroll_and_the_others_zoom_and_a_modifier_flips_it() {
        assert_eq!(pin_wheel_action(false, false), PinWheel::Zoom);
        assert_eq!(pin_wheel_action(true, false), PinWheel::Scroll);
        assert_eq!(pin_wheel_action(true, true), PinWheel::Zoom);
        assert_eq!(pin_wheel_action(false, true), PinWheel::Scroll);
    }

    #[test]
    fn zooming_keeps_the_window_centred() {
        assert_eq!(recentered((100.0, 100.0), (200.0, 100.0), (250.0, 125.0)), (75.0, 87.5));
        assert_eq!(recentered((100.0, 100.0), (200.0, 100.0), (200.0, 100.0)), (100.0, 100.0));
    }

    #[test]
    fn zooming_a_scrolled_pin_keeps_the_top_of_the_view() {
        assert_eq!(rescaled_scroll(100.0, 1000.0, 2000.0), 200.0);
        assert_eq!(rescaled_scroll(100.0, 0.0, 2000.0), 0.0);
    }

    #[test]
    fn wheel_zoom_then_layout_stays_inside_the_screen() {
        // Zoom a 500x400 pt picture in until the wheel stops it, on a 1440x900 screen.
        let max = pin_max((1440.0, 900.0));
        let mut pct = 100;
        for _ in 0..40 {
            let shown = pin_layout((500.0 * pct as f32 / 100.0, 400.0 * pct as f32 / 100.0), max);
            match pin_zoom_step(pct, true, shown.content.0, max.0) {
                Some(next) => pct = next,
                None => break,
            }
        }
        let p = pin_layout((500.0 * pct as f32 / 100.0, 400.0 * pct as f32 / 100.0), max);
        assert!(p.window.0 <= max.0 + 0.01 && p.window.1 <= max.1 + 0.01);
        assert!(pct > 100);
    }

    #[test]
    fn editor_minimum_window_is_480_by_270() {
        assert_eq!(EDITOR_MIN_SIZE, (480.0, 270.0));
        assert!(TOOLBAR_H + OPTIONS_H + STATUS_H < EDITOR_MIN_SIZE.1, "the canvas keeps some room at the minimum height");
        assert_eq!(OPTIONS_H, 48.0);
    }

    #[test]
    fn toolbar_shows_every_tool_when_the_window_is_wide_enough() {
        let full = full_toolbar_width();
        assert!(toolbar_fit(full).visible == ToolbarFit::ALL && !toolbar_fit(full).overflows());
        assert!(toolbar_fit(full + 400.0).visible == ToolbarFit::ALL);
        assert!(toolbar_fit(full - 1.0).overflows());
        // The default window and a typical screenshot window show everything.
        assert!(toolbar_fit(1180.0).visible == ToolbarFit::ALL);
        assert!(full <= 940.0, "the window a picture opens in shows every tool: {full}");
    }

    #[test]
    fn toolbar_drops_actions_first_then_tools_from_the_end() {
        let full = full_toolbar_width();
        let f = toolbar_fit(full - 1.0);
        assert!(f.overflows() && f.tools() == TOOLBAR_TOOL_ITEMS && f.actions() < TOOLBAR_ACTION_ITEMS);
        let narrow = toolbar_fit(EDITOR_MIN_SIZE.0);
        assert!(narrow.actions() == 0 && narrow.tools() >= 4 && narrow.tools() < TOOLBAR_TOOL_ITEMS, "{narrow:?}");
    }

    #[test]
    fn toolbar_never_needs_more_than_the_window_and_only_shrinks_with_it() {
        let mut last = ToolbarFit::ALL;
        let mut w = 2000.0;
        while w >= EDITOR_MIN_SIZE.0 {
            let f = toolbar_fit(w);
            assert!(f.visible <= last, "monotonic at {w}");
            assert!(f.width() + TRAFFIC_LIGHTS_W + TOOLBAR_EDGE <= w, "fits at {w}: {f:?}");
            last = f.visible;
            w -= 7.0;
        }
        assert_eq!(toolbar_fit(0.0).visible, 0);
    }

    #[test]
    fn toolbar_groups_are_centred_in_the_window_at_every_width() {
        let mut w = 2000.0;
        while w >= EDITOR_MIN_SIZE.0 {
            let fit = toolbar_fit(w);
            let left = toolbar_cluster_left(w, fit);
            let right_gap = w - left - fit.cluster_width();
            assert!(left >= TRAFFIC_LIGHTS_W, "clear of the traffic lights at {w}");
            // The Preferences button (and its edge margin) fits in the space at the right.
            assert!(right_gap >= TOOLBAR_GROUP_GAP + seg_group_w(1) + TOOLBAR_EDGE - 0.01, "room for Preferences at {w}");
            assert!((left - right_gap).abs() < 0.01, "centred at {w}: {left} left, {right_gap} right");
            w -= 5.0;
        }
    }
}
