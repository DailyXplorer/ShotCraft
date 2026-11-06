//! Small windows of their own (About, Keyboard Shortcuts), so they move like Preferences instead
//! of sitting fixed in the middle of the editor.
use crate::editor::{th, Th};
use eframe::egui::{self, Key, Pos2, ViewportBuilder, ViewportCommand, ViewportId};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[derive(Default)]
pub struct Dialog {
    /// The user closed the window; the app hears it on its next pass.
    closed: bool,
    /// Frames drawn since it opened, and where it moves once drawn (Windows and Linux).
    drawn: u32,
    place: Option<Pos2>,
}

pub type Shared = Arc<Mutex<Dialog>>;

fn lock(d: &Shared) -> MutexGuard<'_, Dialog> {
    d.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Windows and Linux show a new window before its first frame is drawn, which flashes white: it
/// opens out of sight and moves into place once drawn.
#[cfg(not(target_os = "macos"))]
pub const OUT_OF_SIGHT: Pos2 = egui::pos2(-20_000.0, -20_000.0);

/// Where a window of `size` points sits in the middle of the editor's screen; None where windows
/// can't place themselves (Wayland, macOS here), which keeps the system's own placement.
pub fn centre_place(ctx: &egui::Context, size: [f32; 2]) -> Option<Pos2> {
    #[cfg(target_os = "macos")]
    {
        let _ = (ctx, size);
        None
    }
    #[cfg(not(target_os = "macos"))]
    {
        let ppp = ctx.pixels_per_point().max(0.5);
        let centre = ctx.input(|i| i.viewport().outer_rect).map(|r| r.center()).unwrap_or(egui::pos2(1.0, 1.0));
        let (x, y, w, h) = crate::capture::screen_at(centre.x * ppp, centre.y * ppp).or_else(|| crate::capture::screen_at(1.0, 1.0))?;
        let (x, y, w, h) = (x as f32 / ppp, y as f32 / ppp, w as f32 / ppp, h as f32 / ppp);
        Some(egui::pos2(x + ((w - size[0]) / 2.0).max(0.0), y + ((h - size[1]) / 2.0).max(0.0)))
    }
}

/// Shows the window while `open`; returns false once the user has closed it (close button or Esc).
pub fn show(ctx: &egui::Context, key: &'static str, title: &str, size: [f32; 2], open: bool, state: &Shared, body: impl Fn(&mut egui::Ui, &Th) + Send + Sync + 'static) -> bool {
    let mut st = lock(state);
    if !open || st.closed {
        *st = Dialog::default();
        return false;
    }
    if st.drawn == 0 {
        st.place = centre_place(ctx, size);
    }
    let builder = ViewportBuilder::default().with_title(title).with_inner_size(size).with_resizable(false).with_minimize_button(false).with_maximize_button(false);
    #[cfg(not(target_os = "macos"))]
    let builder = if st.place.is_some() { builder.with_position(OUT_OF_SIGHT).with_visible(false) } else { builder };
    drop(st);
    let shared = state.clone();
    crate::show_window(ctx, ViewportId::from_hash_of(key), builder, move |ui| {
        {
            let mut st = lock(&shared);
            if ui.input(|i| i.viewport().close_requested() || i.key_pressed(Key::Escape)) {
                st.closed = true;
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
            st.drawn = st.drawn.saturating_add(1);
            match st.drawn {
                1 => ui.ctx().request_repaint(),
                2 => {
                    if let Some(pos) = st.place {
                        ui.ctx().send_viewport_cmd(ViewportCommand::OuterPosition(pos));
                        ui.ctx().send_viewport_cmd(ViewportCommand::Visible(true));
                    }
                }
                _ => {}
            }
        }
        let t = th(ui.visuals().dark_mode);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.bar2).inner_margin(20)).show(ui, |ui| body(ui, &t));
    });
    true
}
