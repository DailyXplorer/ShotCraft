//! The crosshair for Area and Window capture on Windows and Linux (X11), which macOS draws itself
//! for `screencapture -i`. Each screen gets a borderless full-screen window showing it frozen:
//! drag to take an area, Space to switch between area and window, click a window to take it,
//! Esc to cancel.
use crate::capture::{self, Frozen, Shot, WinInfo};
use crate::model::R;
use crate::system::{CaptureMode, Captured};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Key, Pos2, Rect, Stroke, StrokeKind, TextureOptions, ViewportBuilder, ViewportId};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

const LOUPE: f32 = 120.0;
const LOUPE_PX: f32 = 15.0;
const MIN_AREA: f32 = 3.0;

/// The crosshair while it's up. Its windows are deferred viewports: eframe can't always create
/// immediate ones while the editor is hidden (it panics that the callback was never called), which
/// is also why the macOS build's pins and Preferences are deferred.
pub struct Picker {
    state: Arc<Mutex<State>>,
    pub for_ocr: bool,
    /// The area chosen starts a scrolling capture; the picture of it is not used.
    pub for_scrolling: bool,
}

#[cfg(target_os = "linux")]
impl Drop for Picker {
    fn drop(&mut self) {
        crate::wayland_bridge::PICKER_UP.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Picker {
    pub fn new(frozen: Frozen, mode: CaptureMode, for_ocr: bool, look: crate::capture::WindowLook) -> Picker {
        #[cfg(target_os = "linux")]
        crate::wayland_bridge::PICKER_UP.store(true, std::sync::atomic::Ordering::SeqCst);
        Picker { state: Arc::new(Mutex::new(State::new(frozen, mode, look))), for_ocr, for_scrolling: false }
    }

    /// For the area of a scrolling capture.
    pub fn for_scrolling(mut self) -> Picker {
        self.for_scrolling = true;
        self
    }

    fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
        state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Shows one window per screen; returns the outcome once the user has picked or cancelled.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<Result<Option<Captured>, String>> {
        let n = {
            let mut st = Self::lock(&self.state);
            if let Some(r) = st.result.take() {
                return Some(r);
            }
            st.shots.len()
        };
        for i in 0..n {
            let state = self.state.clone();
            crate::show_window(ctx, ViewportId::from_hash_of(("shotcraft-picker", i)), Self::builder(i), move |ui| {
                let mut st = Self::lock(&state);
                if st.result.is_none() {
                    st.screen_ui(ui, i);
                }
                if st.result.is_some() {
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }
            });
        }
        None
    }

    /// Borderless full screen on the i-th monitor. No size or position: egui applies those after
    /// creating the window, which would shrink it back out of full screen.
    fn builder(i: usize) -> ViewportBuilder {
        let builder = ViewportBuilder::default().with_title("ShotCraft Capture").with_decorations(false).with_always_on_top().with_taskbar(false).with_fullscreen(true).with_monitor(i);

        // Not on Linux: there winit pins a non-resizable window's size hints (min = max = 800x600),
        // and GNOME's mutter then refuses to make it full screen, leaving a small window.
        #[cfg(not(target_os = "linux"))]
        let builder = builder.with_resizable(false);
        #[cfg(target_os = "linux")]
        let builder = if capture::wayland() { builder } else { builder.with_inner_size([2.0, 2.0]) };
        builder
    }

    /// Test hook: see `State::preview`.
    pub fn preview(&mut self, ui: &mut egui::Ui, pointer: Pos2, from: Option<Pos2>) {
        Self::lock(&self.state).preview(ui, pointer, from);
    }
}

/// X11: how many frames a crosshair window may wait for the window manager to make it full screen.
#[cfg(target_os = "linux")]
const SETTLE_FRAMES: u32 = 90;

/// What the crosshair windows share; each draws from its own callback.
struct State {
    shots: Vec<Shot>,
    windows: Vec<WinInfo>,
    mode: CaptureMode,
    window_mode: bool,
    /// How a picked window is finished (Preferences ▸ Window Screenshot Background).
    look: crate::capture::WindowLook,
    textures: Vec<Option<egui::TextureHandle>>,
    /// Screen index and start of a drag, in desktop pixels.
    drag: Option<(usize, Pos2)>,
    /// Screen index and pointer, in desktop pixels.
    pointer: Option<(usize, Pos2)>,
    /// Frames each window has drawn.
    frames: Vec<u32>,
    /// For each window, the frozen screen it turned out to cover (found from its position).
    covers: Vec<Option<usize>>,
    result: Option<Result<Option<Captured>, String>>,
}

impl State {
    fn new(frozen: Frozen, mode: CaptureMode, look: crate::capture::WindowLook) -> State {
        let n = frozen.shots.len();
        State {
            shots: frozen.shots,
            windows: frozen.windows,
            window_mode: mode == CaptureMode::Window,
            mode,
            look,
            textures: vec![None; n],
            drag: None,
            pointer: None,
            frames: vec![0; n],
            covers: vec![None; n],
            result: None,
        }
    }

    /// The frozen screen this window covers. The system's monitor order needn't match the capture's,
    /// so each window looks up the screen at its own position (falling back to the same index).
    fn covered(&mut self, ctx: &egui::Context, i: usize) -> usize {
        if let Some(j) = self.covers[i] {
            return j;
        }
        let ppp = ctx.pixels_per_point();
        let found = ctx.input(|inp| inp.viewport().outer_rect).and_then(|r| {
            let screens: Vec<(i32, i32, u32, u32)> = self.shots.iter().map(|s| (s.x, s.y, s.img.w, s.img.h)).collect();
            screen_at_window(&screens, (r.min.x * ppp, r.min.y * ppp, r.width() * ppp, r.height() * ppp))
        });
        if let Some(j) = found {
            self.covers[i] = Some(j);
        }
        found.unwrap_or(i)
    }

    fn screen_ui(&mut self, ui: &mut egui::Ui, own: usize) {
        let ctx = ui.ctx().clone();
        let rect = ctx.content_rect();
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return;
        }
        let i = self.covered(&ctx, own);
        self.frames[own] = self.frames[own].saturating_add(1);
        if self.frames[own] <= 2 && self.shots[i].primary {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        let (hover, pressed, released, esc, space, close) = ui.input(|inp| {
            (inp.pointer.hover_pos(), inp.pointer.primary_pressed(), inp.pointer.primary_released(), inp.key_pressed(Key::Escape), inp.key_pressed(Key::Space), inp.viewport().close_requested())
        });
        // The window under the pointer is the only one that draws a crosshair.
        let hover = if self.pointer_on(i) { hover } else { None };
        if esc || close {
            self.result = Some(Ok(None));
            return;
        }
        if space && self.can_pick_window() {
            self.window_mode = !self.window_mode;
            self.drag = None;
        }
        // X11: the window first shows at a default place and size (a small box in the middle of a
        // monitor) before the window manager makes it full screen. Until it covers its screen
        // nothing is drawn in it, so that box is empty and not a second, tiny picture of the
        // screen. A window manager that never makes it full screen gets the picture after
        // `SETTLE_FRAMES` anyway.
        #[cfg(target_os = "linux")]
        if !capture::wayland() && self.covers[own].is_none() && self.frames[own] < SETTLE_FRAMES {
            ctx.request_repaint();
            return;
        }
        self.paint(ui, i, rect, hover, pressed, released);
    }

    /// Whether Space has windows to offer. Wayland apps can't list other windows, so there the
    /// crosshair has none (Window capture there goes through the desktop's own picker).
    /// Whether the pointer is on screen `i`. On X11 a new window can be told a pointer position that
    /// isn't its own (the other monitor's, as the window opens), and it never hears that the pointer
    /// is elsewhere, so it drew a second crosshair and magnifier for good; the system's own answer
    /// settles it. Where an app can't ask (Wayland), the window's events are right and this is true.
    fn pointer_on(&self, i: usize) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(p) = crate::capture::desktop_cursor() {
            let s = &self.shots[i];
            return point_on_screen((s.x, s.y, s.img.w, s.img.h), p);
        }
        let _ = i;
        true
    }

    fn can_pick_window(&self) -> bool {
        !self.windows.is_empty() || !cfg!(target_os = "linux")
    }

    /// Test hook (`SHOTCRAFT_PICKER` with `--screenshot`): draws the first screen into `ui` with the
    /// pointer at `pointer` and, for an area, a drag from `from` (desktop pixels).
    pub fn preview(&mut self, ui: &mut egui::Ui, pointer: Pos2, from: Option<Pos2>) {
        let rect = ui.max_rect();
        let Some(shot) = self.shots.first() else { return };
        let (sx, sy, sw, sh) = (shot.x as f32, shot.y as f32, shot.img.w as f32, shot.img.h as f32);
        let hover = pos2(rect.min.x + (pointer.x - sx) / sw * rect.width(), rect.min.y + (pointer.y - sy) / sh * rect.height());
        self.drag = from.map(|f| (0, f));
        self.paint(ui, 0, rect, Some(hover), false, false);
    }

    fn paint(&mut self, ui: &mut egui::Ui, i: usize, rect: Rect, hover: Option<Pos2>, pressed: bool, released: bool) {
        let ctx = ui.ctx().clone();
        let shot = &self.shots[i];
        let (sx, sy, sw, sh) = (shot.x as f32, shot.y as f32, shot.img.w as f32, shot.img.h as f32);
        let to_px = |p: Pos2| pos2(sx + (p.x - rect.min.x) / rect.width() * sw, sy + (p.y - rect.min.y) / rect.height() * sh);
        let to_pt = |q: Pos2| pos2(rect.min.x + (q.x - sx) / sw * rect.width(), rect.min.y + (q.y - sy) / sh * rect.height());
        let tex = self.textures[i]
            .get_or_insert_with(|| {
                let img = egui::ColorImage::from_rgba_unmultiplied([shot.img.w as usize, shot.img.h as usize], &shot.img.px);
                ctx.load_texture(format!("shotcraft-picker-{i}"), img, TextureOptions::NEAREST)
            })
            .id();
        if let Some(p) = hover {
            let q = to_px(p);
            self.pointer = Some((i, pos2(q.x.clamp(sx, sx + sw - 1.0), q.y.clamp(sy, sy + sh - 1.0))));
        }
        let here = self.pointer.filter(|(s, _)| *s == i).map(|(_, q)| q);

        let painter = ui.painter();
        painter.image(tex, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        let dim = Color32::from_black_alpha(80);

        if self.window_mode {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            let target = here.and_then(|q| self.windows.iter().position(|w| w.contains(q.x, q.y)));
            match target {
                Some(t) => {
                    let w = &self.windows[t];
                    let r = Rect::from_min_max(to_pt(pos2(w.x as f32, w.y as f32)), to_pt(pos2((w.x + w.w as i32) as f32, (w.y + w.h as i32) as f32)));
                    dim_outside(painter, rect, r, dim);
                    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(64, 140, 255, 50));
                    painter.rect_stroke(r, 0.0, Stroke::new(2.0, Color32::from_rgb(64, 140, 255)), StrokeKind::Inside);
                    if !w.title.is_empty() {
                        label(painter, r.left_top() + vec2(8.0, 8.0), Align2::LEFT_TOP, &w.title);
                    }
                }
                None => {
                    painter.rect_filled(rect, 0.0, dim);
                }
            }
            if released && self.drag.is_none() {
                if let Some(t) = target {
                    self.finish_window(t, i);
                }
            }
            if pressed {
                self.drag = None;
            }
            hint(painter, rect, "Click a window to capture it   ·   Space: select an area   ·   Esc: cancel");
            return;
        }

        ctx.set_cursor_icon(CursorIcon::Crosshair);
        if pressed {
            if let Some(q) = here {
                self.drag = Some((i, q));
            }
        }
        let selection = match (self.drag, here) {
            (Some((s, a)), Some(b)) if s == i => Some(Rect::from_two_pos(a, b)),
            _ => None,
        };
        match selection {
            Some(sel) => {
                let r = Rect::from_two_pos(to_pt(sel.min), to_pt(sel.max));
                dim_outside(painter, rect, r, dim);
                painter.rect_stroke(r.expand(1.0), 0.0, Stroke::new(1.0, Color32::from_black_alpha(140)), StrokeKind::Inside);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
                if let Some(p) = hover {
                    label(painter, p + vec2(16.0, 16.0), Align2::LEFT_TOP, &format!("{} × {}", sel.width().round(), sel.height().round()));
                }
            }
            None => {
                painter.rect_filled(rect, 0.0, dim);
                if let (Some(p), Some(q)) = (hover, here) {
                    let line = Stroke::new(1.0, Color32::from_white_alpha(170));
                    painter.line_segment([pos2(rect.left(), p.y), pos2(rect.right(), p.y)], line);
                    painter.line_segment([pos2(p.x, rect.top()), pos2(p.x, rect.bottom())], line);
                    loupe(painter, rect, tex, p, (q.x - sx, q.y - sy), (sw, sh));
                    label(painter, p + vec2(16.0, 16.0 + LOUPE + 8.0), Align2::LEFT_TOP, &format!("{}, {}", (q.x - sx).round(), (q.y - sy).round()));
                }
            }
        }
        if released {
            if let (Some((s, _)), Some(sel)) = (self.drag.take(), selection) {
                if s == i && sel.width() >= MIN_AREA && sel.height() >= MIN_AREA {
                    self.finish_area(i, sel);
                    return;
                }
            }
        }
        if self.can_pick_window() {
            hint(painter, rect, "Drag to capture an area   ·   Space: pick a window   ·   Esc: cancel");
        } else {
            hint(painter, rect, "Drag to capture an area   ·   Esc: cancel");
        }
    }

    fn finish_area(&mut self, i: usize, sel: Rect) {
        let shot = &self.shots[i];
        let local = R { x0: sel.min.x - shot.x as f32, y0: sel.min.y - shot.y as f32, x1: sel.max.x - shot.x as f32, y1: sel.max.y - shot.y as f32 };
        self.result = Some(match capture::crop(&shot.img, local) {
            Some(img) => Ok(Some(Captured { img, scale: shot.scale, area: Some(R { x0: sel.min.x, y0: sel.min.y, x1: sel.max.x, y1: sel.max.y }), mode: self.mode })),
            None => Ok(None),
        });
    }

    fn finish_window(&mut self, t: usize, i: usize) {
        let w = &self.windows[t];
        let scale = self.shots[i].scale;
        self.result = Some(match capture::window_image(&self.shots, w) {
            Some(img) => Ok(Some(Captured { img: capture::finish_window(img, w, scale, self.look), scale, area: None, mode: CaptureMode::Window })),
            None => Err("Can't capture that window.".into()),
        });
    }
}

/// The screen (x, y, width, height in pixels) a window covers, or none while it hasn't settled: a new
/// window can show its first frames at a default place and size (the LG's window at the Dell's
/// position, on X11), and matching that place alone gave the wrong screen for good. A window that
/// covers a screen is at its corner and as big as it.
fn screen_at_window(screens: &[(i32, i32, u32, u32)], window: (f32, f32, f32, f32)) -> Option<usize> {
    let (x, y, w, h) = window;
    screens.iter().position(|&(sx, sy, sw, sh)| (sx as f32 - x).abs() <= 2.0 && (sy as f32 - y).abs() <= 2.0 && (sw as f32 - w).abs() <= 4.0 && (sh as f32 - h).abs() <= 4.0)
}

/// Whether a desktop point (pixels) is on a screen (x, y, width, height).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn point_on_screen(screen: (i32, i32, u32, u32), p: (i32, i32)) -> bool {
    p.0 >= screen.0 && p.1 >= screen.1 && i64::from(p.0) < i64::from(screen.0) + i64::from(screen.2) && i64::from(p.1) < i64::from(screen.1) + i64::from(screen.3)
}

/// Darkens everything in `all` except `keep`.
fn dim_outside(painter: &egui::Painter, all: Rect, keep: Rect, dim: Color32) {
    let keep = keep.intersect(all);
    for r in [
        Rect::from_min_max(all.min, pos2(all.max.x, keep.min.y)),
        Rect::from_min_max(pos2(all.min.x, keep.max.y), all.max),
        Rect::from_min_max(pos2(all.min.x, keep.min.y), pos2(keep.min.x, keep.max.y)),
        Rect::from_min_max(pos2(keep.max.x, keep.min.y), pos2(all.max.x, keep.max.y)),
    ] {
        if r.is_positive() {
            painter.rect_filled(r, 0.0, dim);
        }
    }
}

fn label(painter: &egui::Painter, at: Pos2, align: Align2, text: &str) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(12.0), Color32::WHITE);
    let r = align.anchor_size(at, galley.size()).expand2(vec2(6.0, 3.0));
    let clip = painter.clip_rect();
    let shift = vec2((clip.right() - r.right()).min(0.0), (clip.bottom() - r.bottom()).min(0.0));
    let r = r.translate(shift);
    painter.rect_filled(r, 4.0, Color32::from_black_alpha(190));
    painter.galley(r.min + vec2(6.0, 3.0), galley, Color32::WHITE);
}

fn hint(painter: &egui::Painter, all: Rect, text: &str) {
    label(painter, pos2(all.center().x, all.top() + 18.0), Align2::CENTER_TOP, text);
}

/// A magnified view of the pixels around the pointer, so an edge can be hit exactly.
fn loupe(painter: &egui::Painter, all: Rect, tex: egui::TextureId, p: Pos2, q: (f32, f32), size: (f32, f32)) {
    let mut at = p + vec2(16.0, 16.0);
    if at.x + LOUPE > all.right() {
        at.x = p.x - 16.0 - LOUPE;
    }
    if at.y + LOUPE + 30.0 > all.bottom() {
        at.y = p.y - 16.0 - LOUPE - 30.0;
    }
    let r = Rect::from_min_size(at, vec2(LOUPE, LOUPE));
    let half = LOUPE_PX / 2.0;
    let (cx, cy) = (q.0.floor() + 0.5, q.1.floor() + 0.5);
    let uv = Rect::from_min_max(pos2((cx - half) / size.0, (cy - half) / size.1), pos2((cx + half) / size.0, (cy + half) / size.1));
    painter.rect_filled(r, 0.0, Color32::BLACK);
    painter.image(tex, r, uv, Color32::WHITE);
    let cell = LOUPE / LOUPE_PX;
    let centre = Rect::from_center_size(r.center(), vec2(cell, cell));
    painter.rect_stroke(centre, 0.0, Stroke::new(1.0, Color32::from_rgb(255, 59, 48)), StrokeKind::Outside);
    painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_white_alpha(220)), StrokeKind::Outside);
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// GNOME (mutter) won't make a window full screen once winit has pinned its size hints, which
    /// a non-resizable window gets: the picker stayed an 800x600 window.
    #[test]
    fn the_picker_stays_resizable_so_that_it_can_go_full_screen() {
        let builder = Picker::builder(0);
        assert_ne!(builder.resizable, Some(false));
        assert_eq!(builder.fullscreen, Some(true));
    }

    /// Wayland lists no windows, so Space has nothing to pick there; where windows are listed it works.
    #[test]
    fn space_picks_a_window_only_when_there_are_windows_to_pick() {
        let win = crate::capture::WinInfo { id: 1, title: "t".into(), x: 0, y: 0, w: 10, h: 10 };
        let with = State::new(crate::capture::Frozen { shots: vec![], windows: vec![win] }, CaptureMode::Area, crate::capture::WindowLook::Plain);
        let without = State::new(crate::capture::Frozen { shots: vec![], windows: vec![] }, CaptureMode::Area, crate::capture::WindowLook::Plain);
        assert!(with.can_pick_window());
        assert_eq!(without.can_pick_window(), !cfg!(target_os = "linux"));
    }

    #[test]
    fn a_window_counts_for_a_screen_only_when_it_sits_there_at_its_size() {
        let screens = [(0, 0, 3840, 2160), (3840, 0, 1680, 1050)];
        assert_eq!(screen_at_window(&screens, (0.0, 0.0, 3840.0, 2160.0)), Some(0));
        assert_eq!(screen_at_window(&screens, (3840.0, 0.0, 1680.0, 1050.0)), Some(1));
        assert_eq!(screen_at_window(&screens, (3840.0, 0.0, 800.0, 600.0)), None, "the first frames, at the Dell's corner but not its size");
        assert_eq!(screen_at_window(&screens, (3841.0, 1.0, 1679.0, 1051.0)), Some(1), "a pixel of slack");
        assert_eq!(screen_at_window(&screens, (500.0, 500.0, 3840.0, 2160.0)), None);
    }

    #[test]
    fn only_the_screen_under_the_pointer_takes_it() {
        let (lg, dell) = ((0, 0, 3840, 2160), (3840, 0, 1680, 1050));
        assert!(point_on_screen(lg, (1500, 900)) && !point_on_screen(dell, (1500, 900)));
        assert!(point_on_screen(dell, (4300, 500)) && !point_on_screen(lg, (4300, 500)));
        assert!(point_on_screen(lg, (3839, 2159)) && !point_on_screen(lg, (3840, 0)) && !point_on_screen(dell, (3840, 1050)));
    }
}
