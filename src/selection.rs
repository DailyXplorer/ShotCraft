//! The rules behind the editor's keyboard and pointer handling that do not need a window: how a
//! pixel selection moves, resizes and grows, how it snaps to content, which region a ⌘-click picks,
//! where a zoom to the selection lands, and how the Shift constraints bend a drag.
//!
//! The editor (`editor.rs`) reads the events and calls these; every function here is plain maths
//! on image pixels, so each rule is unit tested without egui.
use crate::model::{p, Image, P, R, ZOOM_MAX};
use eframe::egui::Key;
use std::collections::HashMap;

/// Arrow keys move or resize a selection by one pixel, ten with Shift.
pub const STEP: f32 = 1.0;
pub const STEP_BIG: f32 = 10.0;
/// Colours closer than this (sum of the channel differences) count as the same when a selection
/// snaps to content, and when ⌘-click picks a flat region.
pub const ADJUST_TOLERANCE: i32 = 24;
pub const FLOOD_TOLERANCE: i32 = 12;
/// Zoom (physical pixels per image pixel) of Q and W: close enough to see single pixels.
pub const CORNER_ZOOM: f32 = 8.0;
/// Space kept around a selection that ⌘2 zooms to, and around a Z-dragged area, in points.
pub const ZOOM_MARGIN: f32 = 48.0;
/// A Z-drag shorter than this many points is a click.
pub const ZOOM_DRAG_MIN: f32 = 6.0;

pub fn step(shift: bool) -> f32 {
    if shift {
        STEP_BIG
    } else {
        STEP
    }
}

/// Shift while dragging a line: the end snaps to the nearest 45° from the start.
pub fn snap45(a: P, b: P) -> P {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    let ang = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
    p(a.x + len * ang.cos(), a.y + len * ang.sin())
}

/// Shift while dragging a box (or a selection): the opposite corner moves to make a square, as
/// large as the longer side, towards the side the pointer is on.
pub fn square(a: P, b: P) -> P {
    let s = (b.x - a.x).abs().max((b.y - a.y).abs());
    p(a.x + s * (b.x - a.x).signum(), a.y + s * (b.y - a.y).signum())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

impl Arrow {
    pub fn from_key(key: Key) -> Option<Arrow> {
        Some(match key {
            Key::ArrowLeft => Arrow::Left,
            Key::ArrowRight => Arrow::Right,
            Key::ArrowUp => Arrow::Up,
            Key::ArrowDown => Arrow::Down,
            _ => return None,
        })
    }

    pub fn delta(self, step: f32) -> (f32, f32) {
        match self {
            Arrow::Left => (-step, 0.0),
            Arrow::Right => (step, 0.0),
            Arrow::Up => (0.0, -step),
            Arrow::Down => (0.0, step),
        }
    }
}

/// The selection moved by (dx, dy) on whole pixels. It keeps its size and stops at the edge of
/// `bounds`.
pub fn nudge(r: R, dx: f32, dy: f32, bounds: R) -> R {
    let r = r.rounded();
    let (w, h) = (r.w().min(bounds.w()), r.h().min(bounds.h()));
    let x0 = (r.x0 + dx).clamp(bounds.x0, (bounds.x1 - w).max(bounds.x0));
    let y0 = (r.y0 + dy).clamp(bounds.y0, (bounds.y1 - h).max(bounds.y0));
    R { x0, y0, x1: x0 + w, y1: y0 + h }
}

/// The selection with its right and bottom edges moved by (dw, dh), at least one pixel wide and
/// high and not past `bounds`. The top left corner stays.
pub fn resize(r: R, dw: f32, dh: f32, bounds: R) -> R {
    let r = r.rounded();
    let x1 = (r.x1 + dw).min(bounds.x1).max(r.x0 + 1.0);
    let y1 = (r.y1 + dh).min(bounds.y1).max(r.y0 + 1.0);
    R { x0: r.x0, y0: r.y0, x1, y1 }
}

/// The selection with every side moved out by `n` pixels (in when `n` is negative), inside
/// `bounds`, never thinner than a pixel.
pub fn grow(r: R, n: f32, bounds: R) -> R {
    let r = r.rounded();
    let side = |lo: f32, hi: f32, min: f32, max: f32| {
        let (mut a, mut b) = (lo - n, hi + n);
        if b - a < 1.0 {
            a = ((lo + hi) / 2.0).floor();
            b = a + 1.0;
        }
        (a.max(min), b.min(max).max(a.max(min) + 1.0))
    };
    let (x0, x1) = side(r.x0, r.x1, bounds.x0, bounds.x1);
    let (y0, y1) = side(r.y0, r.y1, bounds.y0, bounds.y1);
    R { x0, y0, x1, y1 }
}

/// What an arrow key does to a selection: it moves it, or with ⌘ it resizes it; Shift makes the
/// step ten pixels.
pub fn arrow_edit(r: R, dir: Arrow, cmd: bool, shift: bool, bounds: R) -> R {
    let (dx, dy) = dir.delta(step(shift));
    if cmd {
        resize(r, dx, dy, bounds)
    } else {
        nudge(r, dx, dy, bounds)
    }
}

/// How far `[` and `]` change the selection, in pixels per side: `]` grows and `[` shrinks by
/// one, ten with Shift. Shift+bracket may arrive as the key `{` or `}` (what the keyboard types)
/// or as the bracket key with the Shift flag, depending on the layout: both mean ten.
pub fn bracket_delta(key: Key, shift: bool) -> Option<f32> {
    Some(match key {
        Key::CloseBracket => step(shift),
        Key::OpenBracket => -step(shift),
        Key::CloseCurlyBracket => STEP_BIG,
        Key::OpenCurlyBracket => -STEP_BIG,
        _ => return None,
    })
}

/// Number keys 1 to 9 (a Spotlight's darkness); 0 is not one.
pub fn digit(key: Key) -> Option<u8> {
    Some(match key {
        Key::Num1 => 1,
        Key::Num2 => 2,
        Key::Num3 => 3,
        Key::Num4 => 4,
        Key::Num5 => 5,
        Key::Num6 => 6,
        Key::Num7 => 7,
        Key::Num8 => 8,
        Key::Num9 => 9,
        _ => return None,
    })
}

fn far(a: &[u8], b: &[u8; 4], tol: i32) -> bool {
    (0..4).map(|c| (a[c] as i32 - b[c] as i32).abs()).sum::<i32>() > tol
}

fn px(img: &Image, x: i32, y: i32) -> &[u8] {
    let i = (y as usize * img.w as usize + x as usize) * 4;
    &img.px[i..i + 4]
}

/// The colour that is most common along the edge of the area: the background to cut away.
fn border_color(img: &Image, x0: i32, y0: i32, x1: i32, y1: i32) -> [u8; 4] {
    let mut counts: HashMap<[u8; 4], (u32, [u8; 4])> = HashMap::new();
    let mut add = |x: i32, y: i32| {
        let c = px(img, x, y);
        let key = [c[0] >> 3, c[1] >> 3, c[2] >> 3, c[3] >> 3];
        counts.entry(key).or_insert((0, [c[0], c[1], c[2], c[3]])).0 += 1;
    };
    for x in x0..x1 {
        add(x, y0);
        add(x, y1 - 1);
    }
    for y in y0 + 1..y1 - 1 {
        add(x0, y);
        add(x1 - 1, y);
    }
    // Ties go to the colour with the smaller value so that the result does not depend on the
    // hash map's order.
    counts.into_iter().max_by_key(|(k, (n, _))| (*n, std::cmp::Reverse(*k))).map(|(_, (_, c))| c).unwrap_or([0; 4])
}

/// The selection pulled in to the content inside it: the box around everything that differs from
/// the background (the most common colour along the selection's edge). Edges that already touch
/// content stay, and a selection with nothing but background is returned as it is.
pub fn smart_adjust(img: &Image, r: R) -> R {
    let x0 = (r.x0.floor().max(0.0) as i32).min(img.w as i32);
    let y0 = (r.y0.floor().max(0.0) as i32).min(img.h as i32);
    let x1 = (r.x1.ceil().max(0.0) as i32).min(img.w as i32);
    let y1 = (r.y1.ceil().max(0.0) as i32).min(img.h as i32);
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return r;
    }
    let bg = border_color(img, x0, y0, x1, y1);
    let row = |y: i32, from: i32, to: i32| (from..to).any(|x| far(px(img, x, y), &bg, ADJUST_TOLERANCE));
    let col = |x: i32, from: i32, to: i32| (from..to).any(|y| far(px(img, x, y), &bg, ADJUST_TOLERANCE));
    let Some(top) = (y0..y1).find(|y| row(*y, x0, x1)) else { return r };
    let bottom = (y0..y1).rev().find(|y| row(*y, x0, x1)).unwrap_or(top);
    let left = (x0..x1).find(|x| col(*x, top, bottom + 1)).unwrap_or(x0);
    let right = (x0..x1).rev().find(|x| col(*x, top, bottom + 1)).unwrap_or(x1 - 1);
    R { x0: left as f32, y0: top as f32, x1: right as f32 + 1.0, y1: bottom as f32 + 1.0 }
}

/// ⌘-click: the box around the flat region under the pixel, that is the connected pixels whose
/// colour is within `tol` of the clicked one. `None` outside the picture.
pub fn flood_select(img: &Image, x: i32, y: i32, tol: i32) -> Option<R> {
    if x < 0 || y < 0 || x >= img.w as i32 || y >= img.h as i32 {
        return None;
    }
    let (w, h) = (img.w as i32, img.h as i32);
    let seed = {
        let c = px(img, x, y);
        [c[0], c[1], c[2], c[3]]
    };
    let mut seen = vec![false; (w * h) as usize];
    let mut stack = vec![(x, y)];
    seen[(y * w + x) as usize] = true;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (x, y, x, y);
    while let Some((cx, cy)) = stack.pop() {
        min_x = min_x.min(cx);
        max_x = max_x.max(cx);
        min_y = min_y.min(cy);
        max_y = max_y.max(cy);
        for (nx, ny) in [(cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)] {
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let i = (ny * w + nx) as usize;
            if !seen[i] && !far(px(img, nx, ny), &seed, tol) {
                seen[i] = true;
                stack.push((nx, ny));
            }
        }
    }
    Some(R { x0: min_x as f32, y0: min_y as f32, x1: max_x as f32 + 1.0, y1: max_y as f32 + 1.0 })
}

/// Which corner Q and W zoom to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Corner {
    TopLeft,
    BottomRight,
}

pub fn corner_point(r: R, c: Corner) -> P {
    match c {
        Corner::TopLeft => p(r.x0, r.y0),
        Corner::BottomRight => p(r.x1, r.y1),
    }
}

/// Zoom (points per image pixel) at which `r` fills a canvas of `canvas` points with `margin` all
/// around, not beyond `max_zoom`.
pub fn fit_zoom(canvas: (f32, f32), r: R, margin: f32, max_zoom: f32) -> f32 {
    let (aw, ah) = ((canvas.0 - 2.0 * margin).max(1.0), (canvas.1 - 2.0 * margin).max(1.0));
    (aw / r.w().max(1.0)).min(ah / r.h().max(1.0)).min(max_zoom)
}

/// Pan, from the canvas's top left, that puts the image point `at` in the middle of the canvas.
pub fn pan_to_center(canvas: (f32, f32), zoom: f32, at: P) -> (f32, f32) {
    (canvas.0 / 2.0 - at.x * zoom, canvas.1 / 2.0 - at.y * zoom)
}

/// ⌘2: zoom and pan that show the whole selection. `ppp` is points per physical pixel.
pub fn zoom_to_rect(canvas: (f32, f32), r: R, ppp: f32) -> (f32, (f32, f32)) {
    let zoom = fit_zoom(canvas, r, ZOOM_MARGIN, ZOOM_MAX / ppp);
    (zoom, pan_to_center(canvas, zoom, r.center()))
}

/// Q / W: zoom and pan that put a corner of the selection in the middle of the canvas. A view
/// that is already closer than `CORNER_ZOOM` stays at its zoom.
pub fn zoom_to_corner(canvas: (f32, f32), current: f32, r: R, c: Corner, ppp: f32) -> (f32, (f32, f32)) {
    let zoom = current.max(CORNER_ZOOM / ppp).min(ZOOM_MAX / ppp);
    (zoom, pan_to_center(canvas, zoom, corner_point(r, c)))
}

/// Z + click: zoom `target` (already stepped by the caller) centred on the clicked point.
pub fn zoom_to_point(canvas: (f32, f32), zoom: f32, at: P, ppp: f32) -> (f32, (f32, f32)) {
    let zoom = zoom.min(ZOOM_MAX / ppp);
    (zoom, pan_to_center(canvas, zoom, at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const B: R = R { x0: 0.0, y0: 0.0, x1: 100.0, y1: 80.0 };
    const SEL: R = R { x0: 10.0, y0: 20.0, x1: 40.0, y1: 50.0 };

    fn img(w: u32, h: u32, fill: [u8; 4]) -> Image {
        Image { w, h, px: Arc::new(fill.iter().copied().cycle().take((w * h * 4) as usize).collect()) }
    }

    fn paint(im: &mut Image, r: (u32, u32, u32, u32), c: [u8; 4]) {
        let px = Arc::make_mut(&mut im.px);
        for y in r.1..r.3 {
            for x in r.0..r.2 {
                let i = ((y * im.w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&c);
            }
        }
    }

    #[test]
    fn arrows_move_the_selection_one_pixel_and_ten_with_shift() {
        assert_eq!(arrow_edit(SEL, Arrow::Right, false, false, B), SEL.translate(1.0, 0.0));
        assert_eq!(arrow_edit(SEL, Arrow::Left, false, false, B), SEL.translate(-1.0, 0.0));
        assert_eq!(arrow_edit(SEL, Arrow::Up, false, false, B), SEL.translate(0.0, -1.0));
        assert_eq!(arrow_edit(SEL, Arrow::Down, false, false, B), SEL.translate(0.0, 1.0));
        assert_eq!(arrow_edit(SEL, Arrow::Right, false, true, B), SEL.translate(10.0, 0.0));
        assert_eq!(arrow_edit(SEL, Arrow::Up, false, true, B), SEL.translate(0.0, -10.0));
    }

    #[test]
    fn a_moved_selection_keeps_its_size_and_stops_at_the_picture_edge() {
        let at_left = R { x0: 3.0, y0: 20.0, x1: 33.0, y1: 50.0 };
        let moved = arrow_edit(at_left, Arrow::Left, false, true, B);
        assert_eq!((moved.x0, moved.w(), moved.h()), (0.0, 30.0, 30.0));
        let at_bottom = R { x0: 10.0, y0: 45.0, x1: 40.0, y1: 78.0 };
        let moved = arrow_edit(at_bottom, Arrow::Down, false, true, B);
        assert_eq!((moved.y1, moved.h()), (80.0, 33.0));
        let moved = arrow_edit(R { x0: 70.0, y0: 0.0, x1: 100.0, y1: 10.0 }, Arrow::Right, false, false, B);
        assert_eq!(moved.x1, 100.0);
    }

    #[test]
    fn keyboard_edits_work_on_whole_pixels() {
        let loose = R { x0: 10.4, y0: 20.6, x1: 40.2, y1: 50.7 };
        assert_eq!(arrow_edit(loose, Arrow::Right, false, false, B), R { x0: 11.0, y0: 21.0, x1: 41.0, y1: 51.0 });
    }

    #[test]
    fn command_arrows_resize_from_the_top_left_corner() {
        assert_eq!(arrow_edit(SEL, Arrow::Right, true, false, B), R { x1: 41.0, ..SEL });
        assert_eq!(arrow_edit(SEL, Arrow::Left, true, false, B), R { x1: 39.0, ..SEL });
        assert_eq!(arrow_edit(SEL, Arrow::Down, true, false, B), R { y1: 51.0, ..SEL });
        assert_eq!(arrow_edit(SEL, Arrow::Up, true, false, B), R { y1: 49.0, ..SEL });
        assert_eq!(arrow_edit(SEL, Arrow::Right, true, true, B), R { x1: 50.0, ..SEL });
        assert_eq!(arrow_edit(SEL, Arrow::Up, true, true, B), R { y1: 40.0, ..SEL });
    }

    #[test]
    fn resizing_stops_at_one_pixel_and_at_the_picture_edge() {
        let thin = R { x0: 10.0, y0: 20.0, x1: 14.0, y1: 50.0 };
        assert_eq!(arrow_edit(thin, Arrow::Left, true, true, B).w(), 1.0);
        assert_eq!(arrow_edit(SEL, Arrow::Right, true, true, R { x1: 45.0, ..B }).x1, 45.0);
        assert_eq!(arrow_edit(SEL, Arrow::Up, true, true, B).h(), 20.0);
        assert_eq!(resize(SEL, 0.0, -1000.0, B).h(), 1.0);
    }

    #[test]
    fn brackets_grow_and_shrink_every_side() {
        assert_eq!(grow(SEL, 1.0, B), SEL.expand(1.0));
        assert_eq!(grow(SEL, -1.0, B), SEL.expand(-1.0));
        assert_eq!(grow(SEL, 10.0, B), SEL.expand(10.0));
        assert_eq!(grow(SEL, -10.0, B), SEL.expand(-10.0));
    }

    #[test]
    fn growing_stays_inside_the_picture_and_shrinking_keeps_a_pixel() {
        let corner = R { x0: 4.0, y0: 3.0, x1: 96.0, y1: 78.0 };
        let g = grow(corner, 10.0, B);
        assert_eq!(g, B);
        let small = R { x0: 10.0, y0: 10.0, x1: 14.0, y1: 12.0 };
        let s = grow(small, -10.0, B);
        assert_eq!((s.w(), s.h()), (1.0, 1.0));
        assert!(s.x0 >= small.x0 && s.x1 <= small.x1 && s.y0 >= small.y0 && s.y1 <= small.y1);
    }

    #[test]
    fn bracket_keys_map_to_steps() {
        assert_eq!(bracket_delta(Key::CloseBracket, false), Some(1.0));
        assert_eq!(bracket_delta(Key::OpenBracket, false), Some(-1.0));
        assert_eq!(bracket_delta(Key::CloseBracket, true), Some(10.0));
        assert_eq!(bracket_delta(Key::OpenBracket, true), Some(-10.0));
        assert_eq!(bracket_delta(Key::CloseCurlyBracket, false), Some(10.0));
        assert_eq!(bracket_delta(Key::OpenCurlyBracket, true), Some(-10.0));
        assert_eq!(bracket_delta(Key::A, false), None);
    }

    #[test]
    fn arrow_and_digit_keys_map() {
        assert_eq!(Arrow::from_key(Key::ArrowUp), Some(Arrow::Up));
        assert_eq!(Arrow::from_key(Key::Enter), None);
        assert_eq!(digit(Key::Num1), Some(1));
        assert_eq!(digit(Key::Num9), Some(9));
        assert_eq!(digit(Key::Num0), None);
    }

    #[test]
    fn shift_makes_a_square_towards_the_pointer() {
        let a = p(10.0, 10.0);
        assert_eq!(square(a, p(40.0, 25.0)), p(40.0, 40.0));
        assert_eq!(square(a, p(25.0, 40.0)), p(40.0, 40.0));
        assert_eq!(square(a, p(-20.0, 15.0)), p(-20.0, 40.0));
        assert_eq!(square(a, p(15.0, -30.0)), p(50.0, -30.0));
        assert_eq!(square(a, a), a);
    }

    #[test]
    fn shift_snaps_a_line_to_45_degrees() {
        let a = p(0.0, 0.0);
        let near = |q: P, x: f32, y: f32| assert!((q.x - x).abs() < 1e-3 && (q.y - y).abs() < 1e-3, "{q:?} vs ({x}, {y})");
        let len = (100.0f32 * 100.0 + 10.0 * 10.0).sqrt();
        near(snap45(a, p(100.0, 10.0)), len, 0.0);
        near(snap45(a, p(10.0, 100.0)), 0.0, len);
        let diag = (100.0f32 * 100.0 + 90.0 * 90.0).sqrt() / 2.0f32.sqrt();
        near(snap45(a, p(100.0, 90.0)), diag, diag);
        near(snap45(a, p(-70.0, 72.0)), -(70.0f32 * 70.0 + 72.0 * 72.0).sqrt() / 2.0f32.sqrt(), (70.0f32 * 70.0 + 72.0 * 72.0).sqrt() / 2.0f32.sqrt());
        near(snap45(p(5.0, 5.0), p(5.0, 5.0)), 5.0, 5.0);
    }

    #[test]
    fn smart_adjust_snaps_to_the_content_inside() {
        let mut im = img(120, 90, [255, 255, 255, 255]);
        paint(&mut im, (40, 30, 70, 50), [20, 40, 200, 255]);
        let loose = R { x0: 25.0, y0: 15.0, x1: 95.0, y1: 75.0 };
        assert_eq!(smart_adjust(&im, loose), R { x0: 40.0, y0: 30.0, x1: 70.0, y1: 50.0 });
    }

    #[test]
    fn smart_adjust_takes_every_island_into_the_box() {
        let mut im = img(120, 90, [250, 250, 250, 255]);
        paint(&mut im, (30, 20, 40, 30), [0, 0, 0, 255]);
        paint(&mut im, (80, 60, 90, 70), [0, 0, 0, 255]);
        let adj = smart_adjust(&im, R { x0: 10.0, y0: 5.0, x1: 110.0, y1: 85.0 });
        assert_eq!(adj, R { x0: 30.0, y0: 20.0, x1: 90.0, y1: 70.0 });
    }

    #[test]
    fn smart_adjust_works_on_a_dark_background_and_ignores_faint_noise() {
        let mut im = img(100, 100, [20, 20, 24, 255]);
        paint(&mut im, (10, 10, 12, 12), [22, 21, 25, 255]);
        paint(&mut im, (50, 40, 60, 60), [240, 240, 240, 255]);
        let adj = smart_adjust(&im, R { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0 });
        assert_eq!(adj, R { x0: 50.0, y0: 40.0, x1: 60.0, y1: 60.0 });
    }

    #[test]
    fn smart_adjust_leaves_a_flat_area_and_a_tiny_one_alone() {
        let im = img(50, 50, [9, 9, 9, 255]);
        let r = R { x0: 5.0, y0: 5.0, x1: 40.0, y1: 40.0 };
        assert_eq!(smart_adjust(&im, r), r);
        let tiny = R { x0: 5.0, y0: 5.0, x1: 6.0, y1: 40.0 };
        assert_eq!(smart_adjust(&im, tiny), tiny);
    }

    #[test]
    fn smart_adjust_clips_a_selection_that_hangs_over_the_picture() {
        let mut im = img(60, 40, [255, 255, 255, 255]);
        paint(&mut im, (20, 10, 30, 20), [0, 0, 0, 255]);
        let adj = smart_adjust(&im, R { x0: -30.0, y0: -10.0, x1: 200.0, y1: 90.0 });
        assert_eq!(adj, R { x0: 20.0, y0: 10.0, x1: 30.0, y1: 20.0 });
    }

    #[test]
    fn flood_select_returns_the_box_of_a_flat_region() {
        let mut im = img(100, 80, [255, 255, 255, 255]);
        paint(&mut im, (20, 10, 60, 40), [30, 120, 220, 255]);
        paint(&mut im, (30, 20, 35, 25), [255, 255, 255, 255]);
        assert_eq!(flood_select(&im, 25, 15, FLOOD_TOLERANCE), Some(R { x0: 20.0, y0: 10.0, x1: 60.0, y1: 40.0 }));
        assert_eq!(flood_select(&im, 32, 22, FLOOD_TOLERANCE), Some(R { x0: 30.0, y0: 20.0, x1: 35.0, y1: 25.0 }));
    }

    #[test]
    fn flood_select_tolerates_small_differences_but_not_big_ones() {
        let mut im = img(60, 40, [200, 200, 200, 255]);
        paint(&mut im, (10, 10, 30, 30), [203, 199, 200, 255]);
        paint(&mut im, (30, 10, 45, 30), [140, 200, 200, 255]);
        let r = flood_select(&im, 5, 5, FLOOD_TOLERANCE).unwrap();
        assert_eq!((r.x0, r.y0, r.x1, r.y1), (0.0, 0.0, 60.0, 40.0), "the faint patch joins the background");
        let r = flood_select(&im, 35, 20, FLOOD_TOLERANCE).unwrap();
        assert_eq!(r, R { x0: 30.0, y0: 10.0, x1: 45.0, y1: 30.0 }, "the strong patch is its own region");
    }

    #[test]
    fn flood_select_does_not_cross_diagonals_and_ignores_clicks_outside() {
        let mut im = img(10, 10, [255, 255, 255, 255]);
        paint(&mut im, (2, 2, 3, 3), [0, 0, 0, 255]);
        paint(&mut im, (3, 3, 4, 4), [0, 0, 0, 255]);
        assert_eq!(flood_select(&im, 2, 2, 0), Some(R { x0: 2.0, y0: 2.0, x1: 3.0, y1: 3.0 }));
        assert_eq!(flood_select(&im, -1, 2, 0), None);
        assert_eq!(flood_select(&im, 2, 10, 0), None);
    }

    #[test]
    fn zoom_to_selection_fits_it_with_a_margin_and_centres_it() {
        let canvas = (1000.0, 700.0);
        let r = R { x0: 100.0, y0: 200.0, x1: 300.0, y1: 300.0 };
        let (zoom, pan) = zoom_to_rect(canvas, r, 2.0);
        assert!((zoom - (1000.0 - 96.0) / 200.0).abs() < 1e-4, "width bound: {zoom}");
        let centre = (r.center().x * zoom + pan.0, r.center().y * zoom + pan.1);
        assert!((centre.0 - 500.0).abs() < 1e-3 && (centre.1 - 350.0).abs() < 1e-3);
        let tall = R { x0: 0.0, y0: 0.0, x1: 50.0, y1: 400.0 };
        assert!((zoom_to_rect(canvas, tall, 2.0).0 - (700.0 - 96.0) / 400.0).abs() < 1e-4, "height bound");
    }

    #[test]
    fn zoom_to_a_tiny_selection_stops_at_the_maximum() {
        let (zoom, _) = zoom_to_rect((1000.0, 700.0), R { x0: 5.0, y0: 5.0, x1: 7.0, y1: 7.0 }, 2.0);
        assert_eq!(zoom * 2.0, ZOOM_MAX);
    }

    #[test]
    fn q_and_w_centre_a_corner_of_the_selection() {
        let canvas = (800.0, 600.0);
        let (zoom, pan) = zoom_to_corner(canvas, 0.5, SEL, Corner::TopLeft, 2.0);
        assert_eq!(zoom, CORNER_ZOOM / 2.0);
        assert_eq!((10.0 * zoom + pan.0, 20.0 * zoom + pan.1), (400.0, 300.0));
        let (zoom, pan) = zoom_to_corner(canvas, 0.5, SEL, Corner::BottomRight, 2.0);
        assert_eq!((40.0 * zoom + pan.0, 50.0 * zoom + pan.1), (400.0, 300.0));
    }

    #[test]
    fn q_keeps_a_view_that_is_already_closer() {
        let (zoom, _) = zoom_to_corner((800.0, 600.0), 10.0, SEL, Corner::TopLeft, 2.0);
        assert_eq!(zoom, 10.0);
        let (zoom, _) = zoom_to_corner((800.0, 600.0), 100.0, SEL, Corner::TopLeft, 2.0);
        assert_eq!(zoom * 2.0, ZOOM_MAX);
    }

    #[test]
    fn z_click_centres_the_spot() {
        let (zoom, pan) = zoom_to_point((800.0, 600.0), 2.0, p(30.0, 40.0), 2.0);
        assert_eq!(zoom, 2.0);
        assert_eq!((30.0 * zoom + pan.0, 40.0 * zoom + pan.1), (400.0, 300.0));
    }
}
