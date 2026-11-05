//! The frozen area grabber, the logic: every display is photographed first, the user draws the
//! area on those still pictures, and the result is cut from the picture. No screen, no window and
//! no egui in here, so all of it runs on any OS (the overlay that shows it is `frozen_ui.rs`).
//!
//! Coordinates are global points with the origin at the top left of the main display, like
//! `targets.rs`: a display left of or above the main one has negative coordinates. A picture of a
//! display has `scale` pixels per point (2 on Retina), found from its size, never assumed.

use crate::model::Image;
use crate::targets::Rect;
use std::sync::Arc;

/// A selection thinner than this (points) is a click, not an area: the grab goes on waiting.
pub const MIN_SIDE: f64 = 3.0;

/// More displays than this and the grab is not offered (each gets a window title of its own).
pub const MAX_DISPLAYS: usize = 8;

/// Whether Area Capture takes the frozen path: the option is on and the displays are known.
pub fn wanted(enabled: bool, displays: &[Rect]) -> bool {
    enabled && !displays.is_empty() && displays.len() <= MAX_DISPLAYS
}

/// One display as it looked when the grab began.
#[derive(Clone)]
pub struct Shot {
    pub frame: Rect,
    pub img: Image,
}

impl Shot {
    /// The picture is one a display of this size can give: not empty, between half a pixel and
    /// eight pixels per point.
    pub fn looks_right(&self) -> bool {
        let (kx, ky) = self.px_per_point();
        self.img.w > 0 && self.img.h > 0 && (0.5..=8.0).contains(&kx) && (0.5..=8.0).contains(&ky)
    }

    /// Pixels per point, horizontally and vertically. A 3456 px wide picture of a 1728 pt wide
    /// display is 2; a display scaled to a fractional size gives a fractional one.
    pub fn px_per_point(&self) -> (f64, f64) {
        (self.img.w as f64 / self.frame.w.max(1.0), self.img.h as f64 / self.frame.h.max(1.0))
    }
}

/// The rectangle between `anchor` (where the button went down) and `current`, kept inside
/// `bounds` (the display) and on whole points. With `square` the longer side wins and the shorter
/// follows, as far as the display has room in the direction of the drag.
pub fn selection_rect(anchor: (f64, f64), current: (f64, f64), bounds: Rect, square: bool) -> Rect {
    let (bx1, by1) = (bounds.x + bounds.w, bounds.y + bounds.h);
    // Whole points counted from the display's own corner.
    let snap = |p: (f64, f64)| (p.0.clamp(bounds.x, bx1).round(), p.1.clamp(bounds.y, by1).round());
    let (a, mut c) = (snap(anchor), snap(current));
    if square {
        let (dx, dy) = (c.0 - a.0, c.1 - a.1);
        let (sx, sy) = (if dx < 0.0 { -1.0 } else { 1.0 }, if dy < 0.0 { -1.0 } else { 1.0 });
        let room_x = if sx < 0.0 { a.0 - bounds.x } else { bx1 - a.0 };
        let room_y = if sy < 0.0 { a.1 - bounds.y } else { by1 - a.1 };
        let side = dx.abs().max(dy.abs()).min(room_x).min(room_y);
        c = (a.0 + sx * side, a.1 + sy * side);
    }
    Rect::new(a.0.min(c.0), a.1.min(c.1), (a.0 - c.0).abs(), (a.1 - c.1).abs())
}

/// The pixels of `shot` that hold `sel`: left, top, right and bottom edges, rounded to whole
/// pixels and inside the picture. `None` when nothing of the selection is on this display.
pub fn crop_box(shot: &Shot, sel: Rect) -> Option<(u32, u32, u32, u32)> {
    let (kx, ky) = shot.px_per_point();
    let edge = |v: f64, k: f64, max: u32| ((v * k).round().max(0.0) as u32).min(max);
    let (x0, x1) = (edge(sel.x - shot.frame.x, kx, shot.img.w), edge(sel.x + sel.w - shot.frame.x, kx, shot.img.w));
    let (y0, y1) = (edge(sel.y - shot.frame.y, ky, shot.img.h), edge(sel.y + sel.h - shot.frame.y, ky, shot.img.h));
    // A selection of at least a point is never a picture of nothing.
    let x1 = if x1 <= x0 { (x0 + 1).min(shot.img.w) } else { x1 };
    let y1 = if y1 <= y0 { (y0 + 1).min(shot.img.h) } else { y1 };
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

/// The part of the frozen picture inside `sel`, at the picture's own resolution.
pub fn crop(shot: &Shot, sel: Rect) -> Option<Image> {
    let (x0, y0, x1, y1) = crop_box(shot, sel)?;
    let (w, h) = (x1 - x0, y1 - y0);
    let stride = shot.img.w as usize * 4;
    let mut px = Vec::with_capacity(w as usize * h as usize * 4);
    for y in y0..y1 {
        let start = y as usize * stride + x0 as usize * 4;
        px.extend_from_slice(shot.img.px.get(start..start + w as usize * 4)?);
    }
    Some(Image { w, h, px: Arc::new(px) })
}

/// How a grab ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// The area, and the display it is on.
    Done(usize, Rect),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    display: usize,
    anchor: (f64, f64),
    current: (f64, f64),
    square: bool,
}

/// What the user is doing on the frozen displays: nothing yet, dragging an area, or finished.
pub struct Grab {
    frames: Vec<Rect>,
    drag: Option<Drag>,
    pub outcome: Option<Outcome>,
}

impl Grab {
    pub fn new(frames: Vec<Rect>) -> Grab {
        Grab { frames, drag: None, outcome: None }
    }

    pub fn frames(&self) -> &[Rect] {
        &self.frames
    }

    /// The button went down on `display` at `at` (global points). The area stays on that display.
    pub fn press(&mut self, display: usize, at: (f64, f64), square: bool) {
        if self.outcome.is_some() || self.drag.is_some() || display >= self.frames.len() {
            return;
        }
        self.drag = Some(Drag { display, anchor: at, current: at, square });
    }

    /// The pointer moved with the button down.
    pub fn drag_to(&mut self, at: (f64, f64), square: bool) {
        if let Some(d) = &mut self.drag {
            d.current = at;
            d.square = square;
        }
    }

    /// The button went up: a real area ends the grab, a click is forgotten.
    pub fn release(&mut self, at: (f64, f64), square: bool) {
        self.drag_to(at, square);
        self.confirm();
    }

    /// Return, or the button going up: finishes with the area as drawn when it is big enough.
    pub fn confirm(&mut self) {
        if self.outcome.is_some() {
            return;
        }
        let Some((display, sel)) = self.selection() else { return };
        if sel.w >= MIN_SIDE && sel.h >= MIN_SIDE {
            self.outcome = Some(Outcome::Done(display, sel));
        } else {
            self.drag = None;
        }
    }

    pub fn cancel(&mut self) {
        if self.outcome.is_none() {
            self.outcome = Some(Outcome::Cancelled);
        }
    }

    /// The area being drawn, and the display it is on.
    pub fn selection(&self) -> Option<(usize, Rect)> {
        let d = self.drag?;
        Some((d.display, selection_rect(d.anchor, d.current, *self.frames.get(d.display)?, d.square)))
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

/// The text of the live size label, in points like the native selector: `640 × 480`.
pub fn size_label(sel: Rect) -> String {
    format!("{} × {}", sel.w.round() as i64, sel.h.round() as i64)
}

/// Where the size label goes: a little below and to the right of the pointer, moved to the other
/// side of it when that would leave the display. Both are in points; the result is the label's
/// top left corner.
pub fn label_origin(pointer: (f64, f64), label: (f64, f64), bounds: Rect) -> (f64, f64) {
    const GAP: f64 = 14.0;
    let (right, bottom) = (bounds.x + bounds.w, bounds.y + bounds.h);
    let x = if pointer.0 + GAP + label.0 > right { pointer.0 - GAP - label.0 } else { pointer.0 + GAP };
    let y = if pointer.1 + GAP + label.1 > bottom { pointer.1 - GAP - label.1 } else { pointer.1 + GAP };
    (x.max(bounds.x), y.max(bounds.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(frame: Rect, scale: f64) -> Shot {
        let (w, h) = ((frame.w * scale) as u32, (frame.h * scale) as u32);
        // Every pixel tells where it is: red = x mod 256, green = y mod 256.
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 7, 255]);
            }
        }
        Shot { frame, img: Image { w, h, px: Arc::new(px) } }
    }

    const MAIN: Rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };

    #[test]
    fn the_selection_is_the_box_between_the_corners_on_whole_points() {
        let r = selection_rect((10.4, 20.6), (50.2, 30.4), MAIN, false);
        assert_eq!(r, Rect::new(10.0, 21.0, 40.0, 9.0));
        // Dragging up and to the left gives the same box.
        let r2 = selection_rect((50.2, 30.4), (10.4, 20.6), MAIN, false);
        assert_eq!(r, r2);
    }

    #[test]
    fn the_selection_stays_inside_the_display() {
        let r = selection_rect((90.0, 50.0), (250.0, 400.0), MAIN, false);
        assert_eq!(r, Rect::new(90.0, 50.0, 10.0, 10.0));
        let r = selection_rect((10.0, 10.0), (-30.0, -5.0), MAIN, false);
        assert_eq!(r, Rect::new(0.0, 0.0, 10.0, 10.0));
    }

    #[test]
    fn a_display_left_of_the_main_one_has_negative_coordinates() {
        let left = Rect::new(-100.0, 0.0, 100.0, 60.0);
        let r = selection_rect((-20.0, 10.0), (-70.0, 30.0), left, false);
        assert_eq!(r, Rect::new(-70.0, 10.0, 50.0, 20.0));
        let r = selection_rect((-20.0, 10.0), (30.0, 30.0), left, false);
        assert_eq!(r, Rect::new(-20.0, 10.0, 20.0, 20.0), "stops at the seam");
    }

    #[test]
    fn shift_makes_it_square_on_the_longer_side() {
        let r = selection_rect((10.0, 10.0), (40.0, 20.0), MAIN, true);
        assert_eq!((r.w, r.h), (30.0, 30.0));
        assert_eq!((r.x, r.y), (10.0, 10.0));
        let r = selection_rect((50.0, 40.0), (45.0, 10.0), MAIN, true);
        assert_eq!((r.x, r.y, r.w, r.h), (20.0, 10.0, 30.0, 30.0), "grows up and left from the anchor");
    }

    #[test]
    fn a_square_stops_where_the_display_ends() {
        // Room: 10 points down, 90 to the right.
        let r = selection_rect((10.0, 50.0), (90.0, 55.0), MAIN, true);
        assert_eq!((r.w, r.h), (10.0, 10.0));
        let r = selection_rect((95.0, 5.0), (0.0, 20.0), MAIN, true);
        assert_eq!((r.w, r.h), (55.0, 55.0), "limited by the room below: {r:?}");
    }

    #[test]
    fn the_crop_of_a_retina_picture_has_twice_the_points_in_pixels() {
        let s = shot(MAIN, 2.0);
        assert_eq!(s.px_per_point(), (2.0, 2.0));
        let sel = Rect::new(10.0, 20.0, 30.0, 5.0);
        assert_eq!(crop_box(&s, sel), Some((20, 40, 80, 50)));
        let img = crop(&s, sel).unwrap();
        assert_eq!((img.w, img.h), (60, 10));
        // The first pixel of the crop is the pixel at (20, 40) of the display.
        assert_eq!(&img.px[0..4], &[20, 40, 7, 255]);
        // The last one is at (79, 49).
        let n = img.px.len();
        assert_eq!(&img.px[n - 4..], &[79, 49, 7, 255]);
    }

    #[test]
    fn the_crop_of_a_1x_picture_is_one_pixel_per_point() {
        let s = shot(MAIN, 1.0);
        let img = crop(&s, Rect::new(5.0, 6.0, 20.0, 10.0)).unwrap();
        assert_eq!((img.w, img.h), (20, 10));
        assert_eq!(&img.px[0..4], &[5, 6, 7, 255]);
    }

    #[test]
    fn the_crop_on_a_second_display_uses_that_display_s_own_origin() {
        let right = Rect::new(100.0, -20.0, 100.0, 60.0);
        let s = shot(right, 2.0);
        let img = crop(&s, Rect::new(110.0, -10.0, 10.0, 10.0)).unwrap();
        assert_eq!((img.w, img.h), (20, 20));
        assert_eq!(&img.px[0..4], &[20, 20, 7, 255], "(110-100)*2, (-10+20)*2");
    }

    #[test]
    fn displays_with_different_densities_each_crop_at_their_own_scale() {
        let main = shot(MAIN, 2.0);
        let ext = shot(Rect::new(100.0, 0.0, 100.0, 60.0), 1.0);
        let sel = Rect::new(120.0, 10.0, 40.0, 40.0);
        assert_eq!(crop(&main, Rect::new(10.0, 10.0, 40.0, 40.0)).map(|i| i.w), Some(80));
        assert_eq!(crop(&ext, sel).map(|i| (i.w, i.h)), Some((40, 40)));
    }

    #[test]
    fn a_fractional_density_rounds_to_whole_pixels_inside_the_picture() {
        // 1512 x 982 points shown from 3024 x 1964 pixels is exactly 2; a 1710 x 1107 one at 3420 x 2214 too.
        // A scaled mode gives 1.5: 150 x 90 px for 100 x 60 pt.
        let s = shot(MAIN, 1.5);
        assert_eq!(s.px_per_point(), (1.5, 1.5));
        assert_eq!(crop_box(&s, Rect::new(1.0, 1.0, 99.0, 59.0)), Some((2, 2, 150, 90)), "ends at the picture's edge");
    }

    #[test]
    fn a_selection_off_the_display_gives_a_single_edge_pixel_at_most() {
        let s = shot(MAIN, 1.0);
        assert_eq!(crop_box(&s, Rect::new(300.0, 300.0, 10.0, 10.0)), None);
    }

    #[test]
    fn only_a_known_layout_with_the_option_on_takes_the_frozen_path() {
        assert!(wanted(true, &[MAIN]));
        assert!(!wanted(false, &[MAIN]));
        assert!(!wanted(true, &[]));
        assert!(!wanted(true, &[MAIN; MAX_DISPLAYS + 1]));
    }

    #[test]
    fn a_picture_of_the_wrong_size_is_refused() {
        assert!(shot(MAIN, 2.0).looks_right());
        let bad = Shot { frame: MAIN, img: Image { w: 0, h: 0, px: Arc::new(vec![]) } };
        assert!(!bad.looks_right());
        let tiny = Shot { frame: MAIN, img: Image { w: 10, h: 6, px: Arc::new(vec![0; 240]) } };
        assert!(!tiny.looks_right());
    }

    #[test]
    fn a_click_is_not_an_area_and_keeps_the_grab_going() {
        let mut g = Grab::new(vec![MAIN]);
        g.press(0, (10.0, 10.0), false);
        g.release((11.0, 11.0), false);
        assert_eq!(g.outcome, None);
        assert!(!g.dragging());
        g.press(0, (10.0, 10.0), false);
        g.release((60.0, 40.0), false);
        assert_eq!(g.outcome, Some(Outcome::Done(0, Rect::new(10.0, 10.0, 50.0, 30.0))));
    }

    #[test]
    fn releasing_the_button_or_pressing_return_confirms() {
        let mut g = Grab::new(vec![MAIN]);
        g.press(0, (10.0, 10.0), false);
        g.drag_to((30.0, 30.0), false);
        assert_eq!(g.selection(), Some((0, Rect::new(10.0, 10.0, 20.0, 20.0))));
        assert_eq!(g.outcome, None);
        g.confirm();
        assert_eq!(g.outcome, Some(Outcome::Done(0, Rect::new(10.0, 10.0, 20.0, 20.0))));
        // Return with nothing drawn does nothing.
        let mut g = Grab::new(vec![MAIN]);
        g.confirm();
        assert_eq!(g.outcome, None);
    }

    #[test]
    fn escape_cancels_and_nothing_changes_the_outcome_after() {
        let mut g = Grab::new(vec![MAIN]);
        g.press(0, (10.0, 10.0), false);
        g.cancel();
        assert_eq!(g.outcome, Some(Outcome::Cancelled));
        g.release((60.0, 40.0), false);
        g.confirm();
        assert_eq!(g.outcome, Some(Outcome::Cancelled));
    }

    #[test]
    fn the_area_stays_on_the_display_where_the_button_went_down() {
        let right = Rect::new(100.0, 0.0, 100.0, 60.0);
        let mut g = Grab::new(vec![MAIN, right]);
        g.press(1, (150.0, 20.0), false);
        g.release((20.0, 50.0), false);
        assert_eq!(g.outcome, Some(Outcome::Done(1, Rect::new(100.0, 20.0, 50.0, 30.0))));
    }

    #[test]
    fn the_shift_key_at_release_decides_the_shape() {
        let mut g = Grab::new(vec![MAIN]);
        g.press(0, (10.0, 10.0), false);
        g.release((40.0, 20.0), true);
        assert_eq!(g.outcome, Some(Outcome::Done(0, Rect::new(10.0, 10.0, 30.0, 30.0))));
    }

    #[test]
    fn the_label_reads_the_size_in_points() {
        assert_eq!(size_label(Rect::new(0.0, 0.0, 640.0, 480.0)), "640 × 480");
    }

    #[test]
    fn the_label_flips_to_the_other_side_at_the_display_edge() {
        let l = (60.0, 20.0);
        assert_eq!(label_origin((10.0, 10.0), l, MAIN), (24.0, 24.0));
        assert_eq!(label_origin((90.0, 10.0), l, MAIN), (16.0, 24.0));
        assert_eq!(label_origin((90.0, 55.0), l, MAIN), (16.0, 21.0));
        // A display too small for the label on either side keeps it on the display.
        assert_eq!(label_origin((5.0, 5.0), l, Rect::new(0.0, 0.0, 40.0, 30.0)).0, 0.0);
    }
}
