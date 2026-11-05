//! The menu bar icon's confirmation: after a copy or a save it shows a check mark for a moment and
//! then goes back. This is only the timing; `shell` swaps the actual image.

/// How long the confirmation stays on.
pub const FLASH_SECS: f64 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayImage {
    Normal,
    Done,
}

/// Wanted icon at a time; `tick` says when the real icon has to change.
#[derive(Debug, Default)]
pub struct TrayFlash {
    /// When the confirmation ends, in seconds on the caller's clock.
    until: Option<f64>,
    shown: Option<TrayImage>,
}

impl TrayFlash {
    /// A copy or save just succeeded. A second one while the first is on extends it.
    pub fn start(&mut self, now: f64) {
        self.until = Some(now + FLASH_SECS);
    }

    /// The icon to show at `now`.
    pub fn wanted(&self, now: f64) -> TrayImage {
        match self.until {
            Some(t) if now < t => TrayImage::Done,
            _ => TrayImage::Normal,
        }
    }

    /// The image to put in the menu bar now, or `None` when it already is the wanted one.
    pub fn tick(&mut self, now: f64) -> Option<TrayImage> {
        let wanted = self.wanted(now);
        if wanted == TrayImage::Normal {
            self.until = None;
        }
        let shown = self.shown.unwrap_or(TrayImage::Normal);
        if shown == wanted {
            return None;
        }
        self.shown = Some(wanted);
        Some(wanted)
    }

    /// Seconds until the next `tick` has something to do, while a confirmation is on.
    pub fn next_check(&self, now: f64) -> Option<f64> {
        self.until.map(|t| (t - now).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_happens_without_a_copy() {
        let mut f = TrayFlash::default();
        assert_eq!(f.tick(0.0), None);
        assert_eq!(f.tick(100.0), None);
        assert_eq!(f.next_check(1.0), None);
    }

    #[test]
    fn the_check_shows_at_once_and_goes_after_the_flash() {
        let mut f = TrayFlash::default();
        f.start(10.0);
        assert_eq!(f.tick(10.0), Some(TrayImage::Done));
        assert_eq!(f.tick(10.3), None, "already on");
        assert_eq!(f.tick(10.0 + FLASH_SECS - 0.01), None);
        assert_eq!(f.tick(10.0 + FLASH_SECS), Some(TrayImage::Normal));
        assert_eq!(f.tick(20.0), None, "and stays back");
        assert_eq!(f.next_check(20.0), None);
    }

    #[test]
    fn a_second_copy_while_on_extends_without_blinking() {
        let mut f = TrayFlash::default();
        f.start(0.0);
        assert_eq!(f.tick(0.0), Some(TrayImage::Done));
        f.start(0.4);
        assert_eq!(f.tick(0.7), None, "still on: the first would have ended at 0.6");
        assert_eq!(f.tick(1.0), Some(TrayImage::Normal));
    }

    #[test]
    fn it_can_flash_again_after_going_back() {
        let mut f = TrayFlash::default();
        f.start(0.0);
        f.tick(0.0);
        f.tick(1.0);
        f.start(5.0);
        assert_eq!(f.tick(5.0), Some(TrayImage::Done));
    }

    #[test]
    fn a_late_first_tick_never_shows_a_stale_check() {
        let mut f = TrayFlash::default();
        f.start(0.0);
        assert_eq!(f.tick(3.0), None, "the flash is over before it was ever drawn");
    }

    #[test]
    fn next_check_counts_down_to_the_end() {
        let mut f = TrayFlash::default();
        f.start(2.0);
        let left = f.next_check(2.2).unwrap();
        assert!((left - (FLASH_SECS - 0.2)).abs() < 1e-9);
    }

    #[test]
    fn the_done_icon_is_a_template_like_the_normal_one() {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/menubar-done.png")).unwrap();
        assert_eq!((icon.width, icon.height), (36, 36));
        assert!(icon.rgba.chunks(4).all(|p| p[..3] == [0, 0, 0]), "a template image only uses alpha");
        let normal = eframe::icon_data::from_png_bytes(include_bytes!("../assets/menubar.png")).unwrap();
        assert_ne!(icon.rgba, normal.rgba);
    }
}
