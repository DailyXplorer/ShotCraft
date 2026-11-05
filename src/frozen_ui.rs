//! The frozen area grabber, the screen side: one borderless window per display, each showing the
//! still picture of its display with guide lines, the area being drawn and its size. The rules
//! (what the selection is, how it maps onto pixels) are in `frozen.rs`; this file draws and
//! reads the mouse and keys.

use crate::frozen::{self, Grab, Outcome, Shot};
use crate::targets::Rect;
use crate::tiled::{Alpha, TiledTexture};
use eframe::egui::{self, pos2, vec2, Color32, CursorIcon, FontId, Key, Stroke, StrokeKind, Ui, ViewportBuilder, ViewportId};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Window titles, one per display: the window code finds them by title.
pub const TITLES: [&str; frozen::MAX_DISPLAYS] =
    ["ShotCraft Area 1", "ShotCraft Area 2", "ShotCraft Area 3", "ShotCraft Area 4", "ShotCraft Area 5", "ShotCraft Area 6", "ShotCraft Area 7", "ShotCraft Area 8"];

/// No overlay window on screen after this long: something is wrong, use the system selector.
pub const SHOW_TIMEOUT: Duration = Duration::from_secs(4);

/// What the screen looks like outside the area being drawn, and before one is.
const DIM_SELECTING: Color32 = Color32::from_black_alpha(120);
const DIM_IDLE: Color32 = Color32::from_black_alpha(36);

pub struct State {
    pub grab: Grab,
    pub shots: Vec<Shot>,
    tex: Vec<Option<TiledTexture>>,
    /// Where the pointer is: display and global point.
    pointer: Option<(usize, (f64, f64))>,
    /// Some overlay window has painted a frame.
    pub shown: bool,
    pub started: Instant,
    pub for_ocr: bool,
}

pub type Shared = Arc<Mutex<State>>;

impl State {
    pub fn new(shots: Vec<Shot>, for_ocr: bool) -> State {
        let grab = Grab::new(shots.iter().map(|s| s.frame).collect());
        let tex = shots.iter().map(|_| None).collect();
        State { grab, shots, tex, pointer: None, shown: false, started: Instant::now(), for_ocr }
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.grab.outcome
    }

    /// True when no window has appeared in time.
    pub fn timed_out(&self) -> bool {
        !self.shown && self.started.elapsed() > SHOW_TIMEOUT
    }
}

pub fn viewport_id(display: usize) -> ViewportId {
    ViewportId::from_hash_of(("frozen-area", display))
}

/// A window that covers display `frame` exactly, above everything else.
pub fn builder(display: usize, frame: Rect) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title(TITLES[display.min(TITLES.len() - 1)])
        .with_inner_size([frame.w as f32, frame.h as f32])
        .with_position([frame.x as f32, frame.y as f32])
        .with_decorations(false)
        .with_resizable(false)
        .with_has_shadow(false)
        .with_taskbar(false)
        .with_always_on_top()
}

pub fn body(shared: Shared, display: usize) -> impl Fn(&mut Ui) + Send + Sync + 'static {
    move |ui| overlay_ui(ui, &mut shared.lock().unwrap(), display)
}

fn egui_rect(r: Rect, origin: (f64, f64)) -> egui::Rect {
    egui::Rect::from_min_size(pos2((r.x - origin.0) as f32, (r.y - origin.1) as f32), vec2(r.w as f32, r.h as f32))
}

/// What the mouse and keys did in one window during one frame.
#[derive(Clone, Copy, Default)]
pub struct Input {
    /// The pointer, in the window's points, while it is over the window.
    pub hover: Option<egui::Pos2>,
    /// Where the pointer last was, also when it has left the window with a button held.
    pub latest: Option<egui::Pos2>,
    pub press_origin: Option<egui::Pos2>,
    pub pressed: bool,
    pub down: bool,
    pub released: bool,
    pub shift: bool,
    pub escape: bool,
    pub enter: bool,
    pub right_click: bool,
}

fn read_input(ctx: &egui::Context) -> Input {
    ctx.input(|i| Input {
        hover: i.pointer.hover_pos(),
        latest: i.pointer.latest_pos(),
        press_origin: i.pointer.press_origin(),
        pressed: i.pointer.primary_pressed(),
        down: i.pointer.primary_down(),
        released: i.pointer.primary_released(),
        shift: i.modifiers.shift,
        escape: i.key_pressed(Key::Escape),
        enter: i.key_pressed(Key::Enter),
        right_click: i.pointer.secondary_pressed(),
    })
}

/// Draws display `display` and feeds its mouse and keys to the grab.
pub fn overlay_ui(ui: &mut Ui, st: &mut State, display: usize) {
    let ctx = ui.ctx().clone();
    ctx.set_cursor_icon(CursorIcon::Crosshair);
    apply_input(st, display, &read_input(&ctx));
    paint(&ctx, st, display);
    if st.grab.outcome.is_some() {
        // The app picks the result up in its next pass.
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

/// Feeds one frame of input from the window of `display` to the grab.
pub fn apply_input(st: &mut State, display: usize, inp: &Input) {
    let Some(shot) = st.shots.get(display) else { return };
    let frame = shot.frame;
    let global = |p: egui::Pos2| (frame.x + p.x as f64, frame.y + p.y as f64);
    if inp.escape || inp.right_click {
        st.grab.cancel();
    }
    if inp.enter {
        st.grab.confirm();
    }
    if let Some(p) = inp.hover {
        st.pointer = Some((display, global(p)));
    } else if st.pointer.is_some_and(|(d, _)| d == display) && !st.grab.dragging() {
        // The pointer went to another display: its guide lines go with it.
        st.pointer = None;
    }
    // The press is where the button went down, even when the pointer has moved on within the same frame.
    if let (true, Some(p)) = (inp.pressed, inp.latest) {
        st.grab.press(display, global(inp.press_origin.unwrap_or(p)), inp.shift);
    }
    if let (Some(p), true) = (inp.latest, st.grab.selection().map(|(d, _)| d) == Some(display)) {
        if inp.released {
            st.grab.release(global(p), inp.shift);
        } else if inp.down {
            st.grab.drag_to(global(p), inp.shift);
            st.pointer = Some((display, global(p)));
        }
    }
    st.shown = true;
}

/// Paints display `display`: its picture, the dimming, the area, the guide lines and the size.
pub fn paint(ctx: &egui::Context, st: &mut State, display: usize) {
    let Some(shot) = st.shots.get(display) else { return };
    let frame = shot.frame;
    let origin = (frame.x, frame.y);
    let screen = ctx.content_rect();

    // Picture.
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new(("frozen-area", display))));
    painter.rect_filled(screen, 0.0, Color32::BLACK);
    let shot = &st.shots[display];
    let tex = st.tex[display].get_or_insert_with(|| TiledTexture::new(ctx, &format!("frozen-{display}"), (shot.img.w, shot.img.h), &shot.img.px, Alpha::Straight, egui::TextureOptions::LINEAR));
    tex.paint_in(&painter, screen, Color32::WHITE);

    // Dimming, and the area in full light.
    let selection = st.grab.selection();
    match selection {
        Some((d, sel)) if d == display => {
            let a = egui_rect(sel, origin);
            for part in [
                egui::Rect::from_min_max(screen.min, pos2(screen.max.x, a.min.y)),
                egui::Rect::from_min_max(pos2(screen.min.x, a.max.y), screen.max),
                egui::Rect::from_min_max(pos2(screen.min.x, a.min.y), pos2(a.min.x, a.max.y)),
                egui::Rect::from_min_max(pos2(a.max.x, a.min.y), pos2(screen.max.x, a.max.y)),
            ] {
                painter.rect_filled(part, 0.0, DIM_SELECTING);
            }
            painter.rect_stroke(a.expand(1.0), 0.0, Stroke::new(1.0, Color32::from_black_alpha(160)), StrokeKind::Outside);
            painter.rect_stroke(a, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
        }
        Some(_) => {
            painter.rect_filled(screen, 0.0, DIM_SELECTING);
        }
        None => {
            painter.rect_filled(screen, 0.0, DIM_IDLE);
        }
    }

    // Guide lines through the pointer, across the whole display, light over dark so they show on any picture.
    if let Some((_, g)) = st.pointer.filter(|(d, _)| *d == display) {
        let at = pos2((g.0 - frame.x) as f32, (g.1 - frame.y) as f32);
        let at = pos2(at.x.clamp(screen.min.x, screen.max.x), at.y.clamp(screen.min.y, screen.max.y));
        let (x, y) = (at.x.floor() + 0.5, at.y.floor() + 0.5);
        for (dark, off) in [(true, 1.0), (false, 0.0)] {
            let c = if dark { Color32::from_black_alpha(110) } else { Color32::from_white_alpha(200) };
            painter.line_segment([pos2(x + off, screen.min.y), pos2(x + off, screen.max.y)], Stroke::new(1.0, c));
            painter.line_segment([pos2(screen.min.x, y + off), pos2(screen.max.x, y + off)], Stroke::new(1.0, c));
        }
        // The size of the area beside the pointer.
        if let Some((_, sel)) = selection.filter(|(sd, _)| *sd == display) {
            let galley = painter.layout_no_wrap(frozen::size_label(sel), FontId::proportional(12.0), Color32::WHITE);
            let pad = vec2(7.0, 4.0);
            let size = galley.size() + pad * 2.0;
            let (lx, ly) = frozen::label_origin((at.x as f64, at.y as f64), (size.x as f64, size.y as f64), Rect::new(screen.min.x as f64, screen.min.y as f64, screen.width() as f64, screen.height() as f64));
            let bg = egui::Rect::from_min_size(pos2(lx as f32, ly as f32), size);
            painter.rect_filled(bg, 5.0, Color32::from_black_alpha(210));
            painter.galley(bg.min + pad, galley, Color32::WHITE);
        }
    }
}

/// The windows go above the menu bar and the Dock, on every Space, and in front of the other
/// apps. Returns how many of the overlay windows exist yet; the windows are made a few frames
/// after they are asked for, so the caller repeats until all are there.
#[cfg(target_os = "macos")]
pub fn raise_windows(count: usize, key: usize) -> usize {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindowCollectionBehavior};
    let Some(mtm) = MainThreadMarker::new() else { return 0 };
    let mut found = 0;
    for w in NSApplication::sharedApplication(mtm).windows().iter() {
        let title = w.title().to_string();
        let Some(i) = TITLES[..count].iter().position(|t| *t == title) else { continue };
        found += 1;
        // The screen saver level: above the menu bar (24) and the Dock (20).
        w.setLevel(1000);
        w.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::FullScreenAuxiliary);
        w.setHasShadow(false);
        w.orderFrontRegardless();
        if i == key {
            w.makeKeyWindow();
        }
    }
    found
}

#[cfg(not(target_os = "macos"))]
pub fn raise_windows(count: usize, _key: usize) -> usize {
    count
}

#[cfg(test)]
mod shots {
    use super::*;
    use crate::headless::{save_png, with_rig};
    use crate::model::Image;

    /// A busy desktop: a menu bar, a window with a title bar and some text-like rows.
    fn desktop(w: u32, h: u32, ppp: u32, tint: [u8; 3]) -> Image {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let (px_x, px_y) = (x / ppp, y / ppp);
                let c = if px_y < 24 {
                    [236, 236, 240]
                } else if (150..=520).contains(&px_x) && (80..=420).contains(&px_y) {
                    if px_y < 108 {
                        [70, 74, 84]
                    } else if px_y % 22 < 8 && px_x > 170 && px_x < 460 {
                        [90, 100, 120]
                    } else {
                        [250, 250, 252]
                    }
                } else {
                    tint
                };
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        Image { w, h, px: Arc::new(px) }
    }

    fn out_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("SHOTCRAFT_TEST_OUT").map(|d| std::path::PathBuf::from(d).join("frozen"))
    }

    fn state(frames: &[(Rect, u32)]) -> State {
        let shots = frames.iter().enumerate().map(|(i, (f, ppp))| Shot { frame: *f, img: desktop((f.w as u32) * ppp, (f.h as u32) * ppp, *ppp, [70 + 60 * i as u8, 110, 170]) }).collect();
        State::new(shots, false)
    }

    /// One frame of input in a window, built by hand: the shared headless rig keeps egui's own
    /// pointer state between tests, so these tests feed `apply_input` and let the rig only paint.
    #[derive(Clone, Copy)]
    enum Ev {
        Move(f32, f32),
        Press(f32, f32),
        Drag(f32, f32),
        Release(f32, f32),
        Key(Key),
        RightClick,
    }

    fn frame_input(ev: Ev, shift: bool) -> Input {
        let at = |x: f32, y: f32| Some(pos2(x, y));
        let base = Input { shift, ..Input::default() };
        match ev {
            Ev::Move(x, y) => Input { hover: at(x, y), latest: at(x, y), ..base },
            Ev::Press(x, y) => Input { hover: at(x, y), latest: at(x, y), press_origin: at(x, y), pressed: true, down: true, ..base },
            Ev::Drag(x, y) => Input { hover: at(x, y), latest: at(x, y), down: true, ..base },
            Ev::Release(x, y) => Input { hover: at(x, y), latest: at(x, y), released: true, ..base },
            Ev::Key(Key::Escape) => Input { escape: true, ..base },
            Ev::Key(_) => Input { enter: true, ..base },
            Ev::RightClick => Input { right_click: true, ..base },
        }
    }

    /// Feeds the events to the window of `display`, one frame each, and paints the result.
    fn draw(st: &mut State, display: usize, size: (f32, f32), ppp: f32, events: &[Ev]) -> Image {
        for ev in events {
            apply_input(st, display, &frame_input(*ev, false));
        }
        with_rig(|rig| rig.render_window(size, ppp, vec![], |ui| paint(&ui.ctx().clone(), st, display)))
    }

    const MAIN: Rect = Rect { x: 0.0, y: 0.0, w: 640.0, h: 400.0 };

    #[test]
    fn idle_with_guide_lines_then_a_drag_with_its_size_then_confirm() {
        let mut st = state(&[(MAIN, 2)]);
        let idle = draw(&mut st, 0, (640.0, 400.0), 2.0, &[Ev::Move(200.0, 150.0)]);
        assert!(st.shown);
        assert!(!st.grab.dragging());
        // Press, drag: the area is drawn and measured.
        let dragging = draw(&mut st, 0, (640.0, 400.0), 2.0, &[Ev::Press(200.0, 150.0), Ev::Drag(420.0, 300.0)]);
        assert!(st.grab.dragging());
        let sel = st.grab.selection().unwrap().1;
        assert_eq!((sel.x, sel.y, sel.w, sel.h), (200.0, 150.0, 220.0, 150.0));
        assert_ne!(idle.px, dragging.px);
        // Release confirms.
        draw(&mut st, 0, (640.0, 400.0), 2.0, &[Ev::Release(420.0, 300.0)]);
        let Some(Outcome::Done(0, done)) = st.outcome() else { panic!("{:?}", st.outcome()) };
        let img = frozen::crop(&st.shots[0], done).unwrap();
        assert_eq!((img.w, img.h), (440, 300), "220 x 150 points at 2x");
        if let Some(dir) = out_dir() {
            save_png(&idle, &dir.join("idle-guides.png"));
            save_png(&dragging, &dir.join("dragging-size-label.png"));
            save_png(&img, &dir.join("cropped-result.png"));
        }
    }

    #[test]
    fn a_press_and_the_move_after_it_in_one_frame_start_where_the_button_went_down() {
        let mut st = state(&[(MAIN, 1)]);
        let both = Input { hover: Some(pos2(150.0, 120.0)), latest: Some(pos2(150.0, 120.0)), press_origin: Some(pos2(50.0, 50.0)), pressed: true, down: true, ..Input::default() };
        apply_input(&mut st, 0, &both);
        assert_eq!(st.grab.selection().unwrap().1, Rect::new(50.0, 50.0, 100.0, 70.0));
    }

    #[test]
    fn escape_cancels_and_return_confirms_what_is_drawn() {
        let mut st = state(&[(MAIN, 1)]);
        draw(&mut st, 0, (640.0, 400.0), 1.0, &[Ev::Press(50.0, 50.0), Ev::Drag(150.0, 120.0), Ev::Key(Key::Enter)]);
        assert_eq!(st.outcome(), Some(Outcome::Done(0, Rect::new(50.0, 50.0, 100.0, 70.0))));

        let mut st = state(&[(MAIN, 1)]);
        draw(&mut st, 0, (640.0, 400.0), 1.0, &[Ev::Press(50.0, 50.0), Ev::Drag(150.0, 120.0), Ev::Key(Key::Escape)]);
        assert_eq!(st.outcome(), Some(Outcome::Cancelled));

        // A right click cancels too: the way out when the keyboard is not reachable.
        let mut st = state(&[(MAIN, 1)]);
        draw(&mut st, 0, (640.0, 400.0), 1.0, &[Ev::Move(50.0, 50.0), Ev::RightClick]);
        assert_eq!(st.outcome(), Some(Outcome::Cancelled));
    }

    #[test]
    fn shift_held_draws_a_square() {
        let mut st = state(&[(MAIN, 1)]);
        apply_input(&mut st, 0, &frame_input(Ev::Press(100.0, 100.0), true));
        apply_input(&mut st, 0, &frame_input(Ev::Drag(300.0, 160.0), true));
        let sel = st.grab.selection().unwrap().1;
        assert_eq!((sel.w, sel.h), (200.0, 200.0));
    }

    #[test]
    fn two_displays_draw_one_area_on_the_display_where_the_button_went_down() {
        let right = Rect::new(640.0, -100.0, 480.0, 300.0);
        let mut st = state(&[(MAIN, 2), (right, 1)]);
        // Press on the right display (its own coordinates 100, 100 = global 740, 0).
        let second = draw(&mut st, 1, (480.0, 300.0), 2.0, &[Ev::Move(100.0, 100.0), Ev::Press(100.0, 100.0), Ev::Drag(300.0, 220.0)]);
        // The first display sees the pointer leave; it only dims.
        apply_input(&mut st, 0, &Input::default());
        let first = draw(&mut st, 0, (640.0, 400.0), 2.0, &[]);
        let sel = st.grab.selection().unwrap();
        assert_eq!(sel.0, 1);
        assert_eq!((sel.1.x, sel.1.y, sel.1.w, sel.1.h), (740.0, 0.0, 200.0, 120.0));
        // The first display's window gets mouse moves too (nothing is pressed there): no effect on the area.
        apply_input(&mut st, 0, &frame_input(Ev::Move(300.0, 300.0), false));
        assert_eq!(st.grab.selection().unwrap().1, sel.1);
        // Drag past the seam to the left: stays on the display of the press, clamped to its edge.
        apply_input(&mut st, 1, &frame_input(Ev::Drag(-80.0, 220.0), false));
        let sel = st.grab.selection().unwrap().1;
        assert_eq!((sel.x, sel.w), (640.0, 100.0));
        apply_input(&mut st, 1, &frame_input(Ev::Release(-80.0, 220.0), false));
        let Some(Outcome::Done(1, area)) = st.outcome() else { panic!() };
        let img = frozen::crop(&st.shots[1], area).unwrap();
        assert_eq!((img.w, img.h), (100, 120), "the right display is 1x");
        if let Some(dir) = out_dir() {
            save_png(&first, &dir.join("two-displays-first.png"));
            save_png(&second, &dir.join("two-displays-second.png"));
        }
    }

    #[test]
    fn the_guide_lines_leave_with_the_pointer() {
        let mut st = state(&[(MAIN, 1), (Rect::new(640.0, 0.0, 480.0, 300.0), 1)]);
        apply_input(&mut st, 0, &frame_input(Ev::Move(100.0, 100.0), false));
        assert_eq!(st.pointer, Some((0, (100.0, 100.0))));
        // The window of the first display reports the pointer gone, as it moves to the second.
        apply_input(&mut st, 0, &Input::default());
        assert_eq!(st.pointer, None);
        apply_input(&mut st, 1, &frame_input(Ev::Move(10.0, 20.0), false));
        assert_eq!(st.pointer, Some((1, (650.0, 20.0))));
    }

    #[test]
    fn the_label_is_beside_the_pointer_and_flips_at_the_corner() {
        let mut st = state(&[(MAIN, 2)]);
        let img = draw(&mut st, 0, (640.0, 400.0), 2.0, &[Ev::Press(500.0, 300.0), Ev::Drag(636.0, 398.0)]);
        if let Some(dir) = out_dir() {
            save_png(&img, &dir.join("label-flipped-at-corner.png"));
        }
        assert_eq!(frozen::size_label(st.grab.selection().unwrap().1), "136 × 98");
    }

    #[test]
    fn no_window_in_time_is_a_timeout() {
        let mut st = state(&[(MAIN, 1)]);
        assert!(!st.timed_out());
        st.started = Instant::now() - SHOW_TIMEOUT - Duration::from_millis(10);
        assert!(st.timed_out());
        st.shown = true;
        assert!(!st.timed_out());
    }
}
