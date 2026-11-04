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

/// Editor window for a picture of `content` points: close to a square whatever the shape of the
/// picture, at least `min_width` wide, never larger than the screen.
pub fn editor_window(content: Size, chrome: f32, margin: f32, min_width: f32, min_height_ratio: f32, screen: Size) -> Size {
    let width = (content.0 + margin).max(min_width).min(screen.0);
    let height = (content.1 + chrome + margin).max(width * min_height_ratio).min(screen.1);
    (width, height)
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
}
