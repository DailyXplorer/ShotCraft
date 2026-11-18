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

/// The split arrangement keeps this much free around a picture fitted to the canvas, both sides
/// together, and opens a window that much larger than the picture.
pub const SPLIT_FIT_MARGIN: f32 = 48.0;
/// The split arrangement's window is at least this tall for its width.
const SPLIT_MIN_HEIGHT_RATIO: f32 = 0.9;
/// The smallest window the unified arrangement opens, whatever the picture. Measured on the
/// reference app: a 200 x 100 point picture opens in 836 x 470.
pub const UNIFIED_OPEN_MIN: Size = (836.0, 470.0);
/// The largest one: this share of the screen's width, and its height less these points. Measured
/// on the reference app: 1210 x 837 on a screen with 1512 x 949 points free.
const UNIFIED_OPEN_WIDTH_SHARE: f32 = 0.8;
const UNIFIED_OPEN_FREE_HEIGHT: f32 = 112.0;
/// What the unified arrangement keeps free around a picture that had to be shrunk to fit, both
/// sides together. A picture that fits at its own size touches the edges of the canvas.
pub const UNIFIED_FIT_MARGIN: f32 = 18.0;
/// Share of the free height a window gets above it when it opens: a little above the middle of
/// the screen, where AppKit's own `center` puts a window.
pub const OPEN_SPACE_ABOVE: f32 = 5.0 / 11.0;

/// What the unified bar starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    /// macOS: the window buttons. The bar is the window's title bar, and the system draws them in it.
    WindowButtons,
    /// Windows: the button that opens the menus. The window buttons are in the system's title
    /// bar, above the bar.
    Menu,
}

/// Where a window of `outer` size opens in the free area of a screen (`origin` and `size`, the
/// screen without its bars): centred, with `OPEN_SPACE_ABOVE` of the free height above it. A
/// window larger than the area starts at its corner.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn open_origin(origin: Size, size: Size, outer: Size) -> Size {
    (origin.0 + ((size.0 - outer.0) / 2.0).max(0.0), origin.1 + (size.1 - outer.1).max(0.0) * OPEN_SPACE_ABOVE)
}

/// The top and the height of a window that spans `top..top + height` once it is asked for `dy`
/// more at its bottom edge (less for a negative `dy`). The screen has `free_height` for windows
/// from `free_top` down. The window gets no taller than that (or than it already is) and no
/// shorter than `smallest`. One that would end below the free area moves up instead; one that
/// already ends below it is not pulled up, and a window that shrinks keeps its top.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn grow_at_bottom(top: f32, height: f32, dy: f32, smallest: f32, free_top: f32, free_height: f32) -> Size {
    let grown = (height + dy).min(free_height.max(height)).max(smallest);
    let lowest = (free_top + free_height).max(top + height);
    (if dy > 0.0 { top.min(lowest - grown) } else { top }, grown)
}

/// How the editor's chrome is arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// macOS and Windows: one bar, as in the app this one is modelled on. Its `Lead`, then the
    /// tools from the left; the colour, size and zoom cells at the right. There is no status bar.
    Unified(Lead),
    /// Linux: a menu bar, the tools centred in their bar, the cells in a status bar at the bottom.
    Split,
}

/// The arrangement on this system.
pub const CHROME: Chrome = if cfg!(target_os = "macos") {
    Chrome::Unified(Lead::WindowButtons)
} else if cfg!(target_os = "windows") {
    Chrome::Unified(Lead::Menu)
} else {
    Chrome::Split
};

/// Editor chrome, in points. The options bar under the toolbar holds the active tool's settings,
/// the status bar is the bottom strip. The unified bar is as tall as the reference app's: a 24 pt
/// row with 14 pt above and below.
pub const TOOLBAR_H: f32 = match CHROME {
    Chrome::Unified(_) => 52.0,
    Chrome::Split => 38.0,
};
pub const OPTIONS_H: f32 = 48.0;
pub const STATUS_H: f32 = match CHROME {
    Chrome::Unified(_) => 0.0,
    Chrome::Split => STATUS_CELL_H,
};
/// The split arrangement's menu bar, above the toolbar: at 24 points its labels sat 1 px above
/// the line under the bar. The unified one has none: macOS keeps its menus in the menu bar icon,
/// Windows behind the bar's menu button.
pub const MENUBAR_H: f32 = match CHROME {
    Chrome::Unified(_) => 0.0,
    Chrome::Split => 30.0,
};
/// Height of the colour, size and zoom cells, wherever they are.
pub const STATUS_CELL_H: f32 = 34.0;
/// Width of the line between two of those cells. The cells' own widths depend on the arrangement
/// (`Chrome::cell_widths`).
pub const STATUS_SEP_W: f32 = 6.0;
/// The editor window never gets smaller than this.
pub const EDITOR_MIN_SIZE: Size = (480.0, 270.0);
pub const TOOLBAR_SEG_W: f32 = 30.0;
pub const TOOLBAR_GROUP_GAP: f32 = 10.0;
pub const TOOLBAR_EDGE: f32 = 12.0;
/// Segments in the toolbar groups: Copy/Save/Pin, the drawing tools, the selection actions, Preferences.
pub const TOOLBAR_COPY_ITEMS: usize = 3;
/// The drag-out handle beside Copy/Save/Pin: a little wider than a segment, narrow enough that the smallest window still shows 4 tools.
pub const TOOLBAR_DRAG_W: f32 = 40.0;
pub const TOOLBAR_TOOL_ITEMS: usize = crate::model::Tool::TOOLBAR.len();
pub const TOOLBAR_ACTION_ITEMS: usize = 7;

/// Width of a segmented control of `items` segments (a 2 pt frame on each side).
pub const fn seg_group_w(items: usize) -> f32 {
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
        let mut w = seg_group_w(TOOLBAR_COPY_ITEMS) + TOOLBAR_DRAG_W + TOOLBAR_GROUP_GAP + seg_group_w(1) + TOOLBAR_GROUP_GAP;
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

/// Which of the colour, size and zoom cells the unified bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCells {
    pub color: bool,
    pub size: bool,
    pub zoom: bool,
}

impl StatusCells {
    pub const ALL: StatusCells = StatusCells { color: true, size: true, zoom: true };

    /// Width of the cells shown, with the lines between them.
    pub fn width(self, chrome: Chrome) -> f32 {
        let [color, size, zoom] = chrome.cell_widths();
        let widths = [(self.color, color), (self.size, size), (self.zoom, zoom)];
        let shown = widths.iter().filter(|c| c.0).count();
        widths.iter().filter(|c| c.0).map(|c| c.1).sum::<f32>() + STATUS_SEP_W * shown.saturating_sub(1) as f32
    }
}

impl Chrome {
    /// The window the editor opens in for a picture of `content` points under `bars` points of
    /// bars, on a screen with `screen` points free.
    ///
    /// Unified: the window is the picture with the bars above it, so a picture at its own size
    /// fills the canvas edge to edge, as in the reference app; small pictures get the smallest
    /// window and large ones the largest, where they are shrunk to fit. Split: close to a square
    /// wide enough for every tool, with a margin around the picture.
    pub fn editor_window(self, content: Size, bars: f32, screen: Size) -> Size {
        match self {
            Chrome::Split => editor_window(content, bars, SPLIT_FIT_MARGIN, self.full_toolbar_width(), SPLIT_MIN_HEIGHT_RATIO, screen),
            Chrome::Unified(_) => {
                let max = ((screen.0 * UNIFIED_OPEN_WIDTH_SHARE).round(), screen.1 - UNIFIED_OPEN_FREE_HEIGHT);
                let min = (UNIFIED_OPEN_MIN.0.min(max.0), UNIFIED_OPEN_MIN.1.min(max.1));
                (content.0.clamp(min.0, max.0), (content.1 + bars).clamp(min.1, max.1))
            }
        }
    }

    /// What stays free around a picture fitted to a canvas, both sides together. `fits` says
    /// whether the picture fits in the canvas at its own size.
    pub const fn fit_margin(self, fits: bool) -> f32 {
        match self {
            Chrome::Split => SPLIT_FIT_MARGIN,
            Chrome::Unified(_) if fits => 0.0,
            Chrome::Unified(_) => UNIFIED_FIT_MARGIN,
        }
    }

    /// What the toolbar leaves free before its first group. On macOS the unified bar keeps it for
    /// the window buttons, which end 79 points from the left edge in a window with a unified
    /// toolbar (`app_mode::unify_title_bar`); on Windows for the menu button, between the bar's
    /// edge and the gap before the first group. The split one has nothing there, but its groups
    /// are centred, so the margin must be as wide as the one that holds the Preferences button on
    /// the right.
    pub const fn lead(self) -> f32 {
        match self {
            Chrome::Unified(Lead::WindowButtons) => 86.0,
            Chrome::Unified(Lead::Menu) => TOOLBAR_EDGE + seg_group_w(1) + TOOLBAR_GROUP_GAP,
            Chrome::Split => 56.0,
        }
    }

    /// Widths of the colour, size and zoom cells. The colour cell is as wide as #RRGGBB needs;
    /// longer colour formats widen it in the status bar and are cut short in the unified bar,
    /// whose cells are as narrow as their texts allow so that the tools keep the room.
    pub const fn cell_widths(self) -> [f32; 3] {
        match self {
            Chrome::Unified(_) => [104.0, 100.0, 56.0],
            Chrome::Split => [140.0, 104.0, 64.0],
        }
    }

    /// What the unified bar keeps free after its cells. It has no Preferences button, like the
    /// reference app's: Preferences is in the menus and on its shortcut.
    const UNIFIED_END_W: f32 = TOOLBAR_EDGE;

    /// The cells a window this wide has room for. The unified bar gives them up as it narrows, the
    /// size first and then the zoom, so that Copy, Save, Pin, the drag handle and the "More" menu
    /// always fit. The status bar always shows the three.
    pub fn status_cells(self, window_width: f32) -> StatusCells {
        match self {
            Chrome::Split => StatusCells::ALL,
            Chrome::Unified(_) => {
                let room = window_width - self.lead() - ToolbarFit { visible: 0 }.cluster_width() - TOOLBAR_GROUP_GAP - Self::UNIFIED_END_W;
                [StatusCells::ALL, StatusCells { size: false, ..StatusCells::ALL }, StatusCells { size: false, zoom: false, color: true }]
                    .into_iter()
                    .find(|c| c.width(self) <= room)
                    .unwrap_or(StatusCells { color: false, size: false, zoom: false })
            }
        }
    }

    /// Width the groups of tools may take in a toolbar of `window_width`.
    fn cluster_room(self, window_width: f32) -> f32 {
        match self {
            // Centred in the window: between two margins of the same width (the Preferences
            // button sits in the right one).
            Chrome::Split => window_width - 2.0 * self.lead(),
            Chrome::Unified(_) => window_width - self.lead() - TOOLBAR_GROUP_GAP - self.status_cells(window_width).width(self) - Self::UNIFIED_END_W,
        }
    }

    /// Most segments that fit in a toolbar of `window_width`: Copy/Save/Pin (and Preferences, in the split arrangement) stay, the
    /// selection actions then the last tools move into the overflow menu as the window narrows.
    pub fn toolbar_fit(self, window_width: f32) -> ToolbarFit {
        let room = self.cluster_room(window_width);
        (0..=ToolbarFit::ALL)
            .rev()
            .map(|visible| ToolbarFit { visible })
            .find(|f| f.cluster_width() <= room)
            .unwrap_or(ToolbarFit { visible: 0 })
    }

    /// Where the groups of tools start, from the left edge of the toolbar. Split: centred in the
    /// window, never into the Preferences button. Unified: right after its lead.
    pub fn cluster_left(self, window_width: f32, fit: ToolbarFit) -> f32 {
        match self {
            Chrome::Split => {
                let last_left = window_width - TOOLBAR_EDGE - seg_group_w(1) - TOOLBAR_GROUP_GAP - fit.cluster_width();
                ((window_width - fit.cluster_width()) / 2.0).min(last_left).max(self.lead())
            }
            Chrome::Unified(_) => self.lead(),
        }
    }

    /// Narrowest window that shows every tool (and the selection actions) in the toolbar.
    pub fn full_toolbar_width(self) -> f32 {
        let cluster = ToolbarFit { visible: ToolbarFit::ALL }.cluster_width();
        match self {
            Chrome::Split => cluster + 2.0 * self.lead(),
            Chrome::Unified(_) => self.lead() + cluster + TOOLBAR_GROUP_GAP + StatusCells::ALL.width(self) + Self::UNIFIED_END_W,
        }
    }
}

/// `Chrome::toolbar_fit` for this system's arrangement.
pub fn toolbar_fit(window_width: f32) -> ToolbarFit {
    CHROME.toolbar_fit(window_width)
}

/// `Chrome::cluster_left` for this system's arrangement.
pub fn toolbar_cluster_left(window_width: f32, fit: ToolbarFit) -> f32 {
    CHROME.cluster_left(window_width, fit)
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
    fn the_unified_window_is_the_picture_under_its_bar() {
        // The reference app's windows, measured on a screen with 1512 x 949 points free.
        let (screen, bar) = ((1512.0, 949.0), 52.0);
        let open = |w: f32, h: f32| MAC.editor_window((w, h), bar, screen);
        assert_eq!(open(1000.0, 600.0), (1000.0, 652.0));
        assert_eq!(open(1200.0, 700.0), (1200.0, 752.0));
        assert_eq!(open(800.0, 500.0), (836.0, 552.0), "narrower than the smallest window");
        for small in [(200.0, 100.0), (400.0, 300.0), (640.0, 200.0), (120.0, 120.0)] {
            assert_eq!(open(small.0, small.1), UNIFIED_OPEN_MIN, "{small:?}");
        }
        for large in [(1400.0, 860.0), (1500.0, 950.0), (2000.0, 1200.0)] {
            assert_eq!(open(large.0, large.1), (1210.0, 837.0), "{large:?}");
        }
        assert_eq!(open(300.0, 1500.0), (836.0, 837.0), "a tall one");
        assert_eq!(open(3000.0, 400.0).0, 1210.0, "a wide one");
        // Windows opens the same windows: the lead changes the bar, not the window.
        for picture in [(1000.0, 600.0), (200.0, 100.0), (2000.0, 1200.0), (300.0, 1500.0)] {
            assert_eq!(WINDOWS.editor_window(picture, bar, screen), open(picture.0, picture.1), "{picture:?}");
        }
    }

    #[test]
    fn the_unified_window_fits_a_screen_smaller_than_its_smallest_size() {
        let (w, h) = MAC.editor_window((300.0, 200.0), 52.0, (800.0, 500.0));
        assert!(w <= 800.0 * 0.8 + 1.0 && h <= 500.0 - 112.0, "{w} x {h}");
    }

    #[test]
    fn the_split_window_is_as_it_was() {
        let screen = (1440.0, 900.0);
        let min = Chrome::Split.full_toolbar_width();
        assert_eq!(Chrome::Split.editor_window((300.0, 200.0), 120.0, screen), (min, min * 0.9));
        assert_eq!(Chrome::Split.editor_window((6000.0, 6000.0), 120.0, screen), screen);
    }

    #[test]
    fn a_picture_that_fits_touches_the_edges_only_in_the_unified_arrangement() {
        for unified in [MAC, WINDOWS] {
            assert_eq!(unified.fit_margin(true), 0.0);
            assert_eq!(unified.fit_margin(false), UNIFIED_FIT_MARGIN);
        }
        assert_eq!((Chrome::Split.fit_margin(true), Chrome::Split.fit_margin(false)), (SPLIT_FIT_MARGIN, SPLIT_FIT_MARGIN));
    }

    #[test]
    fn a_window_opens_centred_a_little_above_the_middle() {
        // A screen of 1920 x 1040 free under a 40 point bar at its top.
        let at = open_origin((0.0, 40.0), (1920.0, 1040.0), (920.0, 600.0));
        assert_eq!(at.0, 500.0, "as much free on the left as on the right");
        assert_eq!(at.1, 40.0 + 440.0 * OPEN_SPACE_ABOVE, "less free above than below");
        assert!(at.1 - 40.0 < 220.0);
        // A second screen to the left of the main one.
        assert_eq!(open_origin((-1920.0, 0.0), (1920.0, 1080.0), (920.0, 580.0)).0, -1420.0);
        assert_eq!(open_origin((0.0, 0.0), (800.0, 600.0), (900.0, 700.0)), (0.0, 0.0), "larger than the screen");
    }

    #[test]
    fn a_window_grows_at_its_bottom_edge_and_stays_on_its_screen() {
        // A screen with 1000 points free from the top; the window is 600 tall and at least 300.
        let grow = |top: f32, height: f32, dy: f32| grow_at_bottom(top, height, dy, 300.0, 0.0, 1000.0);
        assert_eq!(grow(100.0, 600.0, 48.0), (100.0, 648.0), "room below: the top stays");
        assert_eq!(grow(100.0, 648.0, -48.0), (100.0, 600.0), "and the points come back");
        assert_eq!(grow(380.0, 600.0, 48.0), (352.0, 648.0), "it would end below the screen: it moves up");
        assert_eq!(grow(0.0, 980.0, 48.0), (0.0, 1000.0), "no taller than the screen");
        assert_eq!(grow(0.0, 1000.0, 48.0), (0.0, 1000.0), "no room at all");
        assert_eq!(grow(500.0, 600.0, 48.0), (452.0, 648.0), "already below the edge: not pulled up, only kept from going lower");
        assert_eq!(grow(100.0, 320.0, -48.0), (100.0, 300.0), "no shorter than the smallest window");
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

    // The split arrangement (Linux): the tools centred, the cells in the status bar.
    const SPLIT: Chrome = Chrome::Split;
    // The unified arrangement: everything in one bar, after the window buttons (macOS) or the
    // menu button (Windows).
    const MAC: Chrome = Chrome::Unified(Lead::WindowButtons);
    const WINDOWS: Chrome = Chrome::Unified(Lead::Menu);

    #[test]
    fn toolbar_shows_every_tool_when_the_window_is_wide_enough() {
        let full = SPLIT.full_toolbar_width();
        assert!(SPLIT.toolbar_fit(full).visible == ToolbarFit::ALL && !SPLIT.toolbar_fit(full).overflows());
        assert!(SPLIT.toolbar_fit(full + 400.0).visible == ToolbarFit::ALL);
        assert!(SPLIT.toolbar_fit(full - 1.0).overflows());
        // The default window and a typical screenshot window show everything.
        assert!(SPLIT.toolbar_fit(1180.0).visible == ToolbarFit::ALL);
        assert!(full <= 940.0, "the window a picture opens in shows every tool: {full}");
    }

    #[test]
    fn toolbar_drops_actions_first_then_tools_from_the_end() {
        let full = SPLIT.full_toolbar_width();
        let f = SPLIT.toolbar_fit(full - 1.0);
        assert!(f.overflows() && f.tools() == TOOLBAR_TOOL_ITEMS && f.actions() < TOOLBAR_ACTION_ITEMS);
        let narrow = SPLIT.toolbar_fit(EDITOR_MIN_SIZE.0);
        assert!(narrow.actions() == 0 && narrow.tools() >= 4 && narrow.tools() < TOOLBAR_TOOL_ITEMS, "{narrow:?}");
    }

    #[test]
    fn toolbar_never_needs_more_than_the_window_and_only_shrinks_with_it() {
        let mut last = ToolbarFit::ALL;
        let mut w = 2000.0;
        while w >= EDITOR_MIN_SIZE.0 {
            let f = SPLIT.toolbar_fit(w);
            assert!(f.visible <= last, "monotonic at {w}");
            assert!(f.width() + SPLIT.lead() + TOOLBAR_EDGE <= w, "fits at {w}: {f:?}");
            last = f.visible;
            w -= 7.0;
        }
        assert_eq!(SPLIT.toolbar_fit(0.0).visible, 0);
    }

    #[test]
    fn the_unified_bar_lays_out_from_its_lead_to_the_cells_without_overlap() {
        assert_eq!(MAC.lead(), 86.0, "the window buttons");
        assert_eq!(WINDOWS.lead(), TOOLBAR_EDGE + seg_group_w(1) + TOOLBAR_GROUP_GAP, "the menu button, set like a group of its own");
        for unified in [MAC, WINDOWS] {
            let mut w = 2000.0;
            while w >= EDITOR_MIN_SIZE.0 {
                let (fit, cells) = (unified.toolbar_fit(w), unified.status_cells(w));
                let left = unified.cluster_left(w, fit);
                assert_eq!(left, unified.lead(), "the tools start right after the lead");
                let needed = left + fit.cluster_width() + TOOLBAR_GROUP_GAP + cells.width(unified) + TOOLBAR_EDGE;
                assert!(needed <= w + 0.01, "{unified:?} fits at {w}: {fit:?} {cells:?} need {needed}");
                assert!(cells.color, "the colour cell stays down to the smallest window ({w})");
                w -= 7.0;
            }
        }
    }

    #[test]
    fn the_unified_bar_gives_up_the_size_cell_first_and_then_the_zoom() {
        // A cell that goes leaves its room to the tools: the bar shows more of them just under
        // the width where the cell went.
        assert!(MAC.toolbar_fit(565.0).tools() > MAC.toolbar_fit(570.0).tools());
        let cells = |w: f32| MAC.status_cells(w);
        assert_eq!(cells(1180.0), StatusCells::ALL);
        assert_eq!(cells(UNIFIED_OPEN_MIN.0), StatusCells::ALL);
        assert_eq!(cells(568.0), StatusCells::ALL);
        assert_eq!(cells(567.0), StatusCells { color: true, size: false, zoom: true });
        assert_eq!(cells(EDITOR_MIN_SIZE.0), StatusCells { color: true, size: false, zoom: true });
        assert_eq!(cells(461.0), StatusCells { color: true, size: false, zoom: false });
        assert_eq!(StatusCells::ALL.width(MAC), 272.0);
        // Windows has 30 points more before the cells, so each one goes 30 points later: the
        // smallest window still shows the zoom.
        assert_eq!(WINDOWS.status_cells(538.0), StatusCells::ALL);
        assert_eq!(WINDOWS.status_cells(537.0), StatusCells { color: true, size: false, zoom: true });
        assert_eq!(WINDOWS.status_cells(EDITOR_MIN_SIZE.0), StatusCells { color: true, size: false, zoom: true });
        assert_eq!(StatusCells::ALL.width(Chrome::Split), 320.0);
        assert_eq!(Chrome::Split.status_cells(EDITOR_MIN_SIZE.0), StatusCells::ALL, "a status bar always has room");
    }

    #[test]
    fn the_unified_bar_shows_every_tool_from_1152_points_and_eight_in_the_smallest_window_it_opens() {
        let unified = MAC;
        assert_eq!(unified.full_toolbar_width(), 1152.0);
        assert!(!unified.toolbar_fit(1152.0).overflows() && unified.toolbar_fit(1151.0).overflows());
        // The reference app shows seven tools and its More button next to its three cells in a
        // window 836 points wide.
        let smallest = unified.toolbar_fit(UNIFIED_OPEN_MIN.0);
        assert_eq!((smallest.tools(), smallest.actions()), (8, 0));
        assert_eq!(unified.toolbar_fit(EDITOR_MIN_SIZE.0).visible, 0, "the smallest window keeps Copy, Save, Pin, the handle and More");
    }

    #[test]
    fn the_windows_bar_has_room_for_one_more_tool() {
        assert_eq!(WINDOWS.full_toolbar_width(), 1122.0);
        assert!(!WINDOWS.toolbar_fit(1122.0).overflows() && WINDOWS.toolbar_fit(1121.0).overflows());
        let smallest = WINDOWS.toolbar_fit(UNIFIED_OPEN_MIN.0);
        assert_eq!((smallest.tools(), smallest.actions()), (9, 0));
    }

    #[test]
    fn toolbar_groups_are_centred_in_the_window_at_every_width() {
        let mut w = 2000.0;
        while w >= EDITOR_MIN_SIZE.0 {
            let fit = SPLIT.toolbar_fit(w);
            let left = SPLIT.cluster_left(w, fit);
            let right_gap = w - left - fit.cluster_width();
            assert!(left >= SPLIT.lead(), "clear of the left margin at {w}");
            // The Preferences button (and its edge margin) fits in the space at the right.
            assert!(right_gap >= TOOLBAR_GROUP_GAP + seg_group_w(1) + TOOLBAR_EDGE - 0.01, "room for Preferences at {w}");
            assert!((left - right_gap).abs() < 0.01, "centred at {w}: {left} left, {right_gap} right");
            w -= 5.0;
        }
    }

    #[test]
    fn drag_handle_takes_room_next_to_copy_save_pin_and_still_fits() {
        let before = seg_group_w(TOOLBAR_COPY_ITEMS) + seg_group_w(1) + TOOLBAR_GROUP_GAP;
        let all = ToolbarFit { visible: ToolbarFit::ALL };
        assert!(all.width() >= before + TOOLBAR_DRAG_W + TOOLBAR_GROUP_GAP, "the handle and its gap are counted");
        // The handle is never the thing that moves into the overflow menu: even the narrowest
        // window lays Copy/Save/Pin, the handle and Preferences out.
        let narrow = SPLIT.toolbar_fit(EDITOR_MIN_SIZE.0);
        assert!(narrow.width() + SPLIT.lead() + TOOLBAR_EDGE <= EDITOR_MIN_SIZE.0);
        assert!(narrow.tools() >= 4, "four tools still show at the minimum size");
    }
}
