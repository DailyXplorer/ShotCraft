//! Screen capture on Windows and Linux: what `/usr/sbin/screencapture` does for the macOS build.
//!
//! Fullscreen, Delayed and Repeat Area capture straight away. Area and Window first freeze every
//! screen (and, for Window, list the windows on it); `picker` then lets the user choose on that
//! frozen picture, the way macOS shows its crosshair. Coordinates are desktop pixels.
use crate::model::{Image, R};
use crate::system::{CaptureMode, CaptureOptions, Captured};
use crate::targets::{self, Rect, Snapshot, Target};
use std::sync::Arc;

/// One screen as it looked when the capture started.
pub struct Shot {
    /// Top-left corner on the desktop, in pixels.
    pub x: i32,
    pub y: i32,
    /// Pixels per point on this screen (1.5 at 150 % scaling).
    pub scale: f32,
    pub primary: bool,
    pub img: Image,
}

/// A window that can be picked, in desktop pixels.
pub struct WinInfo {
    pub id: u64,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl WinInfo {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x as f32 && y >= self.y as f32 && x < self.x as f32 + self.w as f32 && y < self.y as f32 + self.h as f32
    }
}

pub struct Frozen {
    pub shots: Vec<Shot>,
    /// Topmost first.
    pub windows: Vec<WinInfo>,
}

/// The mouse pointer as the screen shows it, for captures that include it (Preferences ▸ Mouse
/// pointer). `x`, `y` is its top-left corner on the desktop, in pixels (the position minus the
/// hot spot).
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// Straight (not premultiplied) RGBA, row by row.
    pub px: Vec<u8>,
    /// Pixels that invert what is under them (the text cursor's I-beam on Windows), one per pixel.
    pub invert: Vec<bool>,
}

/// Draws the pointer into a picture whose top-left corner is at `(ox, oy)` on the desktop. The
/// parts outside the picture are left out.
pub fn draw_pointer(img: &mut Image, ox: i32, oy: i32, p: &Pointer) {
    let px = Arc::make_mut(&mut img.px);
    for py in 0..p.h {
        for qx in 0..p.w {
            let (dx, dy) = (p.x as i64 + qx as i64 - ox as i64, p.y as i64 + py as i64 - oy as i64);
            if dx < 0 || dy < 0 || dx >= img.w as i64 || dy >= img.h as i64 {
                continue;
            }
            let (s, d) = ((py * p.w + qx) as usize, ((dy as u64 * img.w as u64 + dx as u64) * 4) as usize);
            let (Some(dst), Some(src)) = (px.get_mut(d..d + 3), p.px.get(s * 4..s * 4 + 4)) else { continue };
            if p.invert.get(s).copied().unwrap_or(false) {
                for c in dst.iter_mut() {
                    *c = 255 - *c;
                }
                continue;
            }
            let a = src[3] as u32;
            for (c, &v) in dst.iter_mut().zip(src) {
                *c = ((v as u32 * a + *c as u32 * (255 - a) + 127) / 255) as u8;
            }
        }
    }
}

/// The screens as they are now, with the pointer drawn in when the preference asks for it.
fn screens_now(opt: &CaptureOptions) -> Result<Vec<Shot>, String> {
    let mut shots = backend::monitors(opt)?;
    if opt.cursor {
        if let Some(p) = backend::pointer() {
            for s in &mut shots {
                draw_pointer(&mut s.img, s.x, s.y, &p);
            }
        }
    }
    Ok(shots)
}

pub enum Started {
    Done(Result<Option<Captured>, String>),
    /// The screens are frozen for the user to choose on, in this mode (the active-window capture
    /// falls back to picking a window when no window has the focus).
    Pick(Frozen, CaptureMode),
}

/// Runs off the UI thread.
pub fn start(mode: CaptureMode, opt: &CaptureOptions) -> Started {
    match mode {
        CaptureMode::Area | CaptureMode::Window => match freeze(mode == CaptureMode::Window, opt) {
            Ok(f) => Started::Pick(f, mode),
            Err(e) => Started::Done(Err(e)),
        },
        CaptureMode::Fullscreen | CaptureMode::Delayed | CaptureMode::ActiveWindow | CaptureMode::AllDisplays => targeted(mode, opt),
        CaptureMode::RepeatArea => Started::Done(repeat_area(opt)),
    }
}

/// What the machine looked like when the capture was asked for: the displays (in pixels, so a
/// point is a pixel here), the cursor, and for the active window the focused one. Read on the UI
/// thread before the editor hides, so the capture points at what was in front of it.
pub fn snapshot(mode: CaptureMode) -> Snapshot {
    let displays: Vec<Rect> = backend::screens().into_iter().map(|(x, y, w, h)| Rect::new(x as f64, y as f64, w as f64, h as f64)).collect();
    let scales = vec![1.0; displays.len()];
    let mouse = backend::cursor().map(|(x, y)| (x as f64, y as f64));
    let (frontmost_pid, windows) = if mode == CaptureMode::ActiveWindow { backend::focused_window() } else { (None, vec![]) };
    let wallpapers = vec![None; displays.len()];
    Snapshot { own_pid: std::process::id() as i32, frontmost_pid, displays, scales, mouse, windows, wallpapers }
}

/// Fullscreen and Delayed: the display under the cursor. All Displays: every display in one
/// picture. Active Window: the window that had the focus, with no click. The decision is
/// `targets::resolve`'s, shared with macOS.
fn targeted(mode: CaptureMode, opt: &CaptureOptions) -> Started {
    let Some(target) = targets::resolve(mode, &opt.snapshot) else { return Started::Done(Err("This capture needs a selection.".into())) };
    if mode == CaptureMode::Delayed {
        std::thread::sleep(std::time::Duration::from_secs(u64::from(opt.delay_secs)));
    }
    match target {
        // No window has the focus (or ShotCraft itself does): the user picks one.
        Target::PickWindow => match freeze(true, opt) {
            Ok(f) => Started::Pick(f, CaptureMode::Window),
            Err(e) => Started::Done(Err(e)),
        },
        Target::MainDisplay => Started::Done(main_screen(mode, opt)),
        Target::Window(id, _) => Started::Done(window_by_id(id, mode, opt)),
        Target::Displays(rects) => Started::Done(displays(&rects, mode, opt)),
    }
}

fn window_by_id(id: u32, mode: CaptureMode, opt: &CaptureOptions) -> Result<Option<Captured>, String> {
    let Some(w) = opt.snapshot.windows.iter().find(|w| w.id == id) else { return Err("Can't take a screenshot: the window is gone.".into()) };
    let win = WinInfo { id: id as u64, title: String::new(), x: w.bounds.x.round() as i32, y: w.bounds.y.round() as i32, w: w.bounds.w.round() as u32, h: w.bounds.h.round() as u32 };
    // The window's own pixels get the pointer drawn in; a crop of the screens already has it.
    let own = backend::window_image(&win).map(|mut img| {
        if let Some(p) = opt.cursor.then(backend::pointer).flatten() {
            draw_pointer(&mut img, win.x, win.y, &p);
        }
        img
    });
    let shots = screens_now(opt)?;
    let Some(img) = own.or_else(|| window_image(&shots, &win)) else { return Err("Can't take a screenshot: the window is no longer on screen.".into()) };
    let scale = shot_for(&shots, win.x as f32 + win.w as f32 / 2.0, win.y as f32 + win.h as f32 / 2.0).map_or(opt.scale, |s| s.scale);
    let img = finish_window(img, &win, scale, opt.window_look());
    Ok(Some(Captured { img, scale, area: None, mode }))
}

/// The screen holding a desktop point, in pixels.
fn shot_for(shots: &[Shot], x: f32, y: f32) -> Option<&Shot> {
    shots.iter().find(|s| s.x as f32 <= x && s.y as f32 <= y && x < s.x as f32 + s.img.w as f32 && y < s.y as f32 + s.img.h as f32)
}

/// The screens at these rectangles (pixels): one is returned as it is, several are put in one
/// picture laid out like the desktop.
fn displays(rects: &[Rect], mode: CaptureMode, opt: &CaptureOptions) -> Result<Option<Captured>, String> {
    let shots = screens_now(opt)?;
    let mut imgs = vec![];
    let mut scale = 1.0f32;
    for r in rects {
        let Some(s) = shot_for(&shots, r.x as f32 + 1.0, r.y as f32 + 1.0) else { return Err("A screen changed while the screenshot was being taken.".into()) };
        scale = scale.max(s.scale);
        imgs.push(s.img.clone());
    }
    let img = match imgs.len() {
        0 => return Err("Can't take a screenshot: no screen found.".into()),
        1 => imgs.remove(0),
        _ => targets::compose(rects, &imgs).ok_or("Can't put the screens together in one picture.")?,
    };
    Ok(Some(Captured { img, scale, area: None, mode }))
}

/// The screen containing a desktop point, as (x, y, width, height) in pixels, without capturing it.
pub fn screen_at(x: f32, y: f32) -> Option<(i32, i32, u32, u32)> {
    backend::screens().into_iter().find(|&(sx, sy, w, h)| x >= sx as f32 && y >= sy as f32 && x < sx as f32 + w as f32 && y < sy as f32 + h as f32)
}

fn freeze(with_windows: bool, opt: &CaptureOptions) -> Result<Frozen, String> {
    let shots = screens_now(opt)?;
    let windows = if with_windows { backend::windows() } else { vec![] };
    Ok(Frozen { shots, windows })
}

/// Fullscreen and Delayed take the main screen, as `screencapture -m` does.
fn main_screen(mode: CaptureMode, opt: &CaptureOptions) -> Result<Option<Captured>, String> {
    let shots = screens_now(opt)?;
    let primary = shots.iter().position(|s| s.primary).unwrap_or(0);
    let Some(shot) = shots.into_iter().nth(primary) else {
        return Err("Can't take a screenshot: no screen found.".into());
    };
    Ok(Some(Captured { img: shot.img, scale: shot.scale, area: None, mode }))
}

fn repeat_area(opt: &CaptureOptions) -> Result<Option<Captured>, String> {
    let Some(r) = opt.last_area else {
        return Err("No area to repeat. Repeat Area Capture takes a screenshot of the previously captured area. You need to perform the area capture first.".into());
    };
    let shots = screens_now(opt)?;
    let (cx, cy) = ((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0);
    let Some(shot) = shots.iter().find(|s| s.x as f32 <= cx && s.y as f32 <= cy && cx < s.x as f32 + s.img.w as f32 && cy < s.y as f32 + s.img.h as f32) else {
        return Err("The area captured last time is no longer on any screen. Capture an area again.".into());
    };
    let local = R { x0: r.x0 - shot.x as f32, y0: r.y0 - shot.y as f32, x1: r.x1 - shot.x as f32, y1: r.y1 - shot.y as f32 };
    let Some(img) = crop(&shot.img, local) else {
        return Err("The area captured last time is no longer on any screen. Capture an area again.".into());
    };
    Ok(Some(Captured { img, scale: shot.scale, area: Some(r), mode: CaptureMode::RepeatArea }))
}

/// The part of `img` inside `r` (pixels, clamped to the image); None when nothing is left.
pub fn crop(img: &Image, r: R) -> Option<Image> {
    let x0 = r.x0.min(r.x1).round().clamp(0.0, img.w as f32) as u32;
    let y0 = r.y0.min(r.y1).round().clamp(0.0, img.h as f32) as u32;
    let x1 = r.x0.max(r.x1).round().clamp(0.0, img.w as f32) as u32;
    let y1 = r.y0.max(r.y1).round().clamp(0.0, img.h as f32) as u32;
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        return None;
    }
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        let start = ((y * img.w + x0) * 4) as usize;
        px.extend_from_slice(img.px.get(start..start + (w * 4) as usize)?);
    }
    Some(Image { w, h, px: Arc::new(px) })
}

/// A window's picture: its own pixels where the system can give them (Windows), else what the
/// frozen screens show inside its frame.
pub fn window_image(shots: &[Shot], win: &WinInfo) -> Option<Image> {
    if let Some(img) = backend::window_image(win) {
        return Some(img);
    }
    let (cx, cy) = (win.x as f32 + win.w as f32 / 2.0, win.y as f32 + win.h as f32 / 2.0);
    let shot = shots.iter().find(|s| s.x as f32 <= cx && s.y as f32 <= cy && cx < s.x as f32 + s.img.w as f32 && cy < s.y as f32 + s.img.h as f32).or_else(|| shots.first())?;
    let r = R { x0: (win.x - shot.x) as f32, y0: (win.y - shot.y) as f32, x1: (win.x - shot.x) as f32 + win.w as f32, y1: (win.y - shot.y) as f32 + win.h as f32 };
    crop(&shot.img, r)
}

// ---- window captures: Preferences ▸ Window Screenshot Background ----

/// How a window capture is finished on Windows and Linux.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowLook {
    /// The window as captured (Transparent).
    Plain,
    /// Laid on a colour.
    Solid([u8; 4]),
    /// With a shadow, on the desktop picture behind it; the colour if there is no picture.
    Wallpaper([u8; 4]),
}

/// A window capture finished as `look` asks. `win` is where the window is on the desktop (pixels),
/// `scale` its screen's pixels per point.
pub fn finish_window(img: Image, win: &WinInfo, scale: f32, look: WindowLook) -> Image {
    match look {
        WindowLook::Plain => img,
        WindowLook::Solid(bg) => crate::system::flatten_on(&img, bg),
        WindowLook::Wallpaper(bg) => {
            let shadowed = with_shadow(&img, scale);
            match backend::wallpaper(win.x + (win.w / 2) as i32, win.y + (win.h / 2) as i32).and_then(|path| crate::system::load_image_file(&path)) {
                Ok(wall) => on_wallpaper(&shadowed, (win.x, win.y, win.w, win.h), &wall, screen_at(win.x as f32 + win.w as f32 / 2.0, win.y as f32 + win.h as f32 / 2.0)),
                Err(why) => {
                    crate::system::notify("ShotCraft", &format!("Can't read the desktop picture ({why}). The window is on the solid colour instead."));
                    crate::windowbg::on_color(&shadowed, [bg[0], bg[1], bg[2]])
                }
            }
        }
    }
}

/// The window with a soft shadow around it, as macOS hands a window capture over: a margin of 30
/// points left and right, 20 above and 46 below, a blurred shadow falling a little down. The
/// window's own pixels are kept as they are.
pub fn with_shadow(img: &Image, scale: f32) -> Image {
    let s = scale.clamp(1.0, 4.0);
    let px = |pt: f32| (pt * s).round() as u32;
    let (l, r, t, b) = (px(30.0), px(30.0), px(20.0), px(46.0));
    let (w, h) = (img.w + l + r, img.h + t + b);
    let (ww, hh) = (w as usize, h as usize);
    // The window's shape, a little lower, blurred three times: close to a gaussian.
    let mut mask = vec![0f32; ww * hh];
    let drop = px(12.0) as usize;
    for y in 0..img.h as usize {
        let row = (y + t as usize + drop) * ww + l as usize;
        for x in 0..img.w as usize {
            if let (Some(m), Some(a)) = (mask.get_mut(row + x), img.px.get((y * img.w as usize + x) * 4 + 3)) {
                *m = *a as f32 / 255.0;
            }
        }
    }
    let radius = px(9.0) as usize;
    for _ in 0..3 {
        box_blur(&mut mask, ww, hh, radius);
    }
    let mut out = vec![0u8; ww * hh * 4];
    for (i, m) in mask.iter().enumerate() {
        out[i * 4 + 3] = (m * 0.45 * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    // The window over its shadow.
    for y in 0..img.h as usize {
        for x in 0..img.w as usize {
            let (si, di) = ((y * img.w as usize + x) * 4, ((y + t as usize) * ww + x + l as usize) * 4);
            let (Some(src), Some(dst)) = (img.px.get(si..si + 4), out.get(di..di + 4)) else { continue };
            let (sa, da) = (src[3] as f32 / 255.0, dst[3] as f32 / 255.0);
            let a = sa + da * (1.0 - sa);
            let mut p = [0u8; 4];
            for c in 0..3 {
                // The shadow is black: only the window brings colour.
                p[c] = if a > 0.0 { (src[c] as f32 * sa / a).round().clamp(0.0, 255.0) as u8 } else { 0 };
            }
            p[3] = (a * 255.0).round() as u8;
            out[di..di + 4].copy_from_slice(&p);
        }
    }
    Image { w, h, px: Arc::new(out) }
}

/// One box blur pass of `radius` over a `w` × `h` plane, across then down.
fn box_blur(v: &mut [f32], w: usize, h: usize, radius: usize) {
    if radius == 0 || w == 0 || h == 0 {
        return;
    }
    let mut line = Vec::new();
    let mut pass = |v: &mut [f32], len: usize, count: usize, at: &dyn Fn(usize, usize) -> usize| {
        let n = (2 * radius + 1) as f32;
        for k in 0..count {
            line.clear();
            line.extend((0..len).map(|i| v[at(k, i)]));
            let get = |i: isize| if i < 0 || i >= len as isize { 0.0 } else { line[i as usize] };
            let mut sum: f32 = (-(radius as isize)..=radius as isize).map(get).sum();
            for i in 0..len {
                v[at(k, i)] = sum / n;
                sum += get(i as isize + radius as isize + 1) - get(i as isize - radius as isize);
            }
        }
    };
    pass(v, w, h, &|row, i| row * w + i);
    pass(v, h, w, &|col, i| i * w + col);
}

/// `shadowed` (from `with_shadow`) on the part of the wallpaper that lies behind it on its screen,
/// the wallpaper filling the screen as Windows' "Fill" lays it out. `win` is the window's place on the
/// desktop and `screen` its screen, in pixels.
pub fn on_wallpaper(shadowed: &Image, win: (i32, i32, u32, u32), wall: &Image, screen: Option<(i32, i32, u32, u32)>) -> Image {
    use crate::windowbg::{canvas_origin, layout, on_picture, wallpaper_canvas, DisplayPlace};
    let lay = layout(shadowed);
    let origin = canvas_origin((win.0 as f64, win.1 as f64, win.2 as f64, win.3 as f64), 1.0, shadowed, &lay);
    let display = screen.map(|(x, y, w, h)| DisplayPlace { frame: (x as f64, y as f64, w as f64, h as f64), px_per_point: 1.0 });
    on_picture(shadowed, &wallpaper_canvas(wall, &lay, origin, display))
}
// ---- scrolling capture (`scroll`), Windows and Linux ----

/// The screens frozen for choosing the area of a scrolling capture: no windows, no pointer.
pub fn freeze_for_scrolling(scale: f32) -> Result<Frozen, String> {
    let opt = CaptureOptions { last_area: None, window_bg: crate::settings::WindowBg::Transparent, solid_color: [0, 0, 0], scale, cursor: false, delay_secs: 0, snapshot: Snapshot::default() };
    freeze(false, &opt)
}

/// The area as it looks now (desktop pixels), cut to the screen that holds its centre. The pointer
/// is parked inside it and left out, as `screencapture` leaves it out.
pub fn grab_area(rect: &crate::scroll::GrabRect) -> Result<image::RgbaImage, String> {
    backend::grab_region(rect.x, rect.y, rect.w, rect.h)
}

/// Pixels per point of the screen holding a desktop point (1.5 at 150 % scaling).
pub fn scale_at(x: i32, y: i32) -> f32 {
    backend::scale_at(x, y).clamp(0.5, 8.0)
}

fn rgba(w: u32, h: u32, px: Vec<u8>) -> Image {
    Image { w, h, px: Arc::new(px) }
}

/// A placeholder until the Windows and Linux backends arrive; every capture says it isn't available yet.
#[cfg(not(target_os = "macos"))]
mod backend {
    use super::{Pointer, Shot, WinInfo};
    use crate::model::Image;
    use crate::system::CaptureOptions;
    use crate::targets::WindowInfo;

    const NOT_YET: &str = "Screen capture isn't available on this system yet.";

    pub fn monitors(_opt: &CaptureOptions) -> Result<Vec<Shot>, String> {
        Err(NOT_YET.into())
    }
    pub fn grab_region(_x: i32, _y: i32, _w: u32, _h: u32) -> Result<image::RgbaImage, String> {
        Err(NOT_YET.into())
    }
    pub fn scale_at(_x: i32, _y: i32) -> f32 {
        1.0
    }
    pub fn wallpaper(_x: i32, _y: i32) -> Result<std::path::PathBuf, String> {
        Err(NOT_YET.into())
    }
    pub fn screens() -> Vec<(i32, i32, u32, u32)> {
        vec![]
    }
    pub fn windows() -> Vec<WinInfo> {
        vec![]
    }
    pub fn cursor() -> Option<(i32, i32)> {
        None
    }
    pub fn pointer() -> Option<Pointer> {
        None
    }
    pub fn focused_window() -> (Option<i32>, Vec<WindowInfo>) {
        (None, vec![])
    }
    pub fn window_image(_win: &WinInfo) -> Option<Image> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> Image {
        let px = (0..w * h).flat_map(|i| [(i % 256) as u8, (i / 256) as u8, 7, 255]).collect();
        Image { w, h, px: Arc::new(px) }
    }

    #[test]
    fn the_pointer_is_drawn_where_it_was_and_cut_at_the_edges() {
        let mut screen = Image { w: 4, h: 2, px: Arc::new(vec![200; 4 * 2 * 4]) };
        // 2×2 at desktop (101, -1) on a screen whose corner is (100, 0): one row is above it.
        let p = Pointer { x: 101, y: -1, w: 2, h: 2, px: vec![0, 0, 0, 255, 10, 20, 30, 255, 255, 255, 255, 128, 0, 0, 0, 0], invert: vec![false, false, false, false] };
        draw_pointer(&mut screen, 100, 0, &p);
        fn at(img: &Image, x: usize, y: usize) -> &[u8] {
            &img.px[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4]
        }
        assert_eq!(at(&screen, 1, 0), &[228, 228, 228, 200], "half-transparent white over grey");
        assert_eq!(at(&screen, 2, 0), &[200, 200, 200, 200], "a transparent pixel leaves the screen");
        assert_eq!(at(&screen, 0, 0), &[200, 200, 200, 200], "left of the pointer");
        assert_eq!(at(&screen, 1, 1), &[200, 200, 200, 200], "below it");
        // An inverting pixel flips what is under it; off the picture nothing happens.
        let q = Pointer { x: 103, y: 1, w: 2, h: 1, px: vec![0; 8], invert: vec![true, true] };
        draw_pointer(&mut screen, 100, 0, &q);
        assert_eq!(at(&screen, 3, 1), &[55, 55, 55, 200]);
    }

    #[test]
    fn a_window_gets_a_shadow_like_the_macos_capture() {
        let win = Image { w: 200, h: 100, px: Arc::new([30u8, 90, 160, 255].repeat(200 * 100)) };
        let s = with_shadow(&win, 1.0);
        assert_eq!((s.w, s.h), (260, 166));
        // Upstream's own measure finds the macOS margins.
        assert_eq!(crate::windowbg::shadow_paddings(&s), crate::windowbg::Margins { top: 20, right: 30, bottom: 46, left: 30 });
        // The window is untouched; below it the shadow is dark and soft; the far corner is clear.
        assert_eq!(s.get(30 + 50, 20 + 50), [30, 90, 160, 255]);
        let under = s.get(130, 20 + 100 + 10);
        assert!(under[3] > 40 && under[3] < 200 && under[..3] == [0, 0, 0], "{under:?}");
        assert_eq!(s.get(0, 0)[3], 0);
        // At 2× everything is twice as big.
        let s2 = with_shadow(&win, 2.0);
        assert_eq!((s2.w, s2.h), (200 + 120, 100 + 132));
    }

    #[test]
    fn the_wallpaper_behind_the_window_is_what_is_behind_it_on_screen() {
        // A 400 × 300 screen at (100, 50) whose wallpaper is a gradient of the same size.
        let wall = Image { w: 400, h: 300, px: Arc::new((0..300u32).flat_map(|y| (0..400u32).flat_map(move |x| [(x / 2) as u8, (y / 2) as u8, 99, 255])).collect()) };
        let win = Image { w: 100, h: 60, px: Arc::new([250u8, 250, 250, 255].repeat(100 * 60)) };
        let shadowed = with_shadow(&win, 1.0);
        let out = on_wallpaper(&shadowed, (100 + 150, 50 + 120, 100, 60), &wall, Some((100, 50, 400, 300)));
        let lay = crate::windowbg::layout(&shadowed);
        assert_eq!((out.w, out.h), (lay.w, lay.h));
        // The canvas's top left corner shows the wallpaper where the canvas begins on the screen.
        let (cx, cy) = (150 - (lay.x + lay.margins.left), 120 - (lay.y + lay.margins.top));
        let corner = out.get(0, 0);
        let expect = wall.get(cx as i32, cy as i32);
        assert!(corner.iter().zip(expect).all(|(a, b)| a.abs_diff(b) <= 2), "{corner:?} vs {expect:?}");
        assert_eq!(out.get((lay.x + lay.margins.left + 50) as i32, (lay.y + lay.margins.top + 30) as i32), [250, 250, 250, 255], "the window");
        assert!(out.px.chunks_exact(4).all(|p| p[3] == 255), "an opaque picture");
    }

    #[test]
    fn plain_and_solid_window_looks() {
        let win = Image { w: 2, h: 1, px: Arc::new(vec![10, 20, 30, 255, 0, 0, 0, 0]) };
        let w = WinInfo { id: 1, title: String::new(), x: 0, y: 0, w: 2, h: 1 };
        assert_eq!(finish_window(win.clone(), &w, 1.0, WindowLook::Plain).px, win.px);
        let solid = finish_window(win, &w, 1.0, WindowLook::Solid([200, 100, 0, 255]));
        assert_eq!(solid.get(1, 0), [200, 100, 0, 255], "see-through parts take the colour");
    }

    /// To look at it: SHOTCRAFT_SHOTS=<dir> saves a window on a made-up wallpaper.
    #[test]
    fn wallpaper_look_picture_for_the_eyes() {
        let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") else { return };
        let wall = Image { w: 1600, h: 900, px: Arc::new((0..900u32).flat_map(|y| (0..1600u32).flat_map(move |x| [(40 + x / 12) as u8, (70 + y / 8) as u8, 160, 255])).collect()) };
        let win = crate::system::load_image_file(std::path::Path::new(&std::env::var("SHOTCRAFT_SAMPLE").unwrap_or_default())).unwrap_or(Image {
            w: 600,
            h: 400,
            px: Arc::new([245u8, 245, 245, 255].repeat(600 * 400)),
        });
        let out = on_wallpaper(&with_shadow(&win, 1.0), (200, 150, win.w, win.h), &wall, Some((0, 0, 1600, 900)));
        crate::headless::save_png(&out, &std::path::Path::new(&dir).join("window-on-wallpaper.png"));
    }

    #[test]
    fn crop_takes_the_pixels_inside() {
        let src = img(10, 8);
        let out = crop(&src, R { x0: 2.0, y0: 3.0, x1: 5.0, y1: 7.0 }).expect("crop");
        assert_eq!((out.w, out.h), (3, 4));
        assert_eq!(&out.px[0..4], &[(3 * 10 + 2) as u8, 0, 7, 255]);
    }

    #[test]
    fn crop_clamps_and_refuses_empty_areas() {
        let src = img(10, 8);
        let out = crop(&src, R { x0: -5.0, y0: 6.0, x1: 4.0, y1: 50.0 }).expect("crop");
        assert_eq!((out.w, out.h), (4, 2));
        assert!(crop(&src, R { x0: 12.0, y0: 0.0, x1: 20.0, y1: 5.0 }).is_none());
        assert!(crop(&src, R { x0: 3.0, y0: 3.0, x1: 3.0, y1: 6.0 }).is_none());
    }

    #[test]
    fn a_window_contains_points_inside_its_frame() {
        let win = WinInfo { id: 1, title: "t".into(), x: 110, y: 10, w: 20, h: 30 };
        assert!(win.contains(115.0, 20.0) && win.contains(110.0, 10.0));
        assert!(!win.contains(105.0, 20.0) && !win.contains(130.0, 20.0) && !win.contains(115.0, 40.0));
    }

    fn shot(x: i32, y: i32, w: u32, h: u32, scale: f32, shade: u8) -> Shot {
        let px = (0..w * h).flat_map(|_| [shade, shade, shade, 255]).collect();
        Shot { x, y, scale, primary: x == 0 && y == 0, img: Image { w, h, px: Arc::new(px) } }
    }

    #[test]
    fn a_screen_is_found_by_a_point_inside_it() {
        let shots = [shot(0, 0, 100, 50, 1.0, 10), shot(100, 0, 80, 50, 2.0, 20)];
        assert_eq!(shot_for(&shots, 99.0, 10.0).map(|s| s.img.px[0]), Some(10));
        assert_eq!(shot_for(&shots, 100.0, 10.0).map(|s| s.img.px[0]), Some(20));
        assert!(shot_for(&shots, 180.0, 10.0).is_none());
    }

    #[test]
    fn the_snapshot_asks_resolve_for_the_display_under_the_cursor() {
        let displays = vec![Rect::new(0.0, 0.0, 100.0, 50.0), Rect::new(100.0, 0.0, 80.0, 50.0)];
        let snap = Snapshot { own_pid: 1, displays, scales: vec![1.0, 1.0], mouse: Some((120.0, 10.0)), ..Snapshot::default() };
        assert_eq!(targets::resolve(CaptureMode::Fullscreen, &snap), Some(Target::Displays(vec![Rect::new(100.0, 0.0, 80.0, 50.0)])));
        assert!(matches!(targets::resolve(CaptureMode::AllDisplays, &snap), Some(Target::Displays(d)) if d.len() == 2));
    }

    #[test]
    fn the_active_window_is_the_focused_one_unless_it_is_ours() {
        let win = targets::WindowInfo { id: 7, pid: 42, layer: 0, alpha: 1.0, bounds: Rect::new(10.0, 10.0, 300.0, 200.0) };
        let snap = Snapshot { own_pid: 1, frontmost_pid: Some(42), windows: vec![win], ..Snapshot::default() };
        assert_eq!(targets::resolve(CaptureMode::ActiveWindow, &snap), Some(Target::Window(7, 1.0)));
        // ShotCraft in front, or nothing focused: the user picks.
        let ours = Snapshot { frontmost_pid: Some(1), ..snap.clone() };
        assert_eq!(targets::resolve(CaptureMode::ActiveWindow, &ours), Some(Target::PickWindow));
        let none = Snapshot { frontmost_pid: None, ..snap };
        assert_eq!(targets::resolve(CaptureMode::ActiveWindow, &none), Some(Target::PickWindow));
    }

    #[test]
    fn several_displays_land_in_one_picture_laid_out_like_the_desktop() {
        // `displays` reads the real screens, so its two halves are checked apart: compose here.
        let rects = [Rect::new(-80.0, 0.0, 80.0, 50.0), Rect::new(0.0, 10.0, 100.0, 50.0)];
        let imgs = [shot(0, 0, 80, 50, 1.0, 10).img, shot(0, 0, 100, 50, 1.0, 200).img];
        let out = targets::compose(&rects, &imgs).expect("compose");
        assert_eq!((out.w, out.h), (180, 60));
        let at = |x: usize, y: usize| out.px[(y * 180 + x) * 4];
        assert_eq!((at(5, 5), at(120, 30)), (10, 200), "each display where it sits");
        assert_eq!(out.px[(55 * 180 + 5) * 4 + 3], 0, "left of the lower display, below the left one: transparent");
    }
}
