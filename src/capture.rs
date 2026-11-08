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

/// The pointer from two renders of it, on black and on white (BGRA, as GDI gives them). Where
/// the two differ by `d`, the pointer lets `d` of the background through; a pixel that is light
/// on black and dark on white inverts the screen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn unblend(w: u32, h: u32, black: &[u8], white: &[u8]) -> Option<Pointer> {
    let n = (w as usize).checked_mul(h as usize)?;
    if black.len() < n * 4 || white.len() < n * 4 {
        return None;
    }
    let mut px = vec![0u8; n * 4];
    let mut invert = vec![false; n];
    for i in 0..n {
        let (b, wt) = (&black[i * 4..i * 4 + 3], &white[i * 4..i * 4 + 3]);
        let (sb, sw): (i32, i32) = (b.iter().map(|&c| c as i32).sum(), wt.iter().map(|&c| c as i32).sum());
        if sb > sw + 3 * 64 {
            invert[i] = true;
            continue;
        }
        let through = ((sw - sb).max(0) / 3).min(255);
        let a = 255 - through;
        if a == 0 {
            continue;
        }
        // BGRA on black is the colour times its alpha: divide it back out.
        for (c, src) in [2usize, 1, 0].into_iter().enumerate() {
            px[i * 4 + c] = ((b[src] as i32 * 255 + a / 2) / a).min(255) as u8;
        }
        px[i * 4 + 3] = a as u8;
    }
    Some(Pointer { x: 0, y: 0, w, h, px, invert })
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

/// Wayland: Area (and the text-recognition area) is chosen with ShotCraft's own crosshair on a
/// plain screenshot of the desktop, as on X11, and Repeat Area works on one too. Window and Active
/// Window keep the desktop's own tool (apps can't learn where windows are); so does everything
/// when the plain screenshot can't be had.
#[cfg(target_os = "linux")]
fn wayland_start(mode: CaptureMode, opt: &CaptureOptions) -> Started {
    let own_picker = matches!(mode, CaptureMode::Area | CaptureMode::RepeatArea);
    if own_picker {
        match backend::portal_picture(false) {
            Ok(Some(img)) => {
                let shots = shots_from_picture(img, &backend::xwayland_screens(), opt.scale);
                return match (mode, opt.last_area) {
                    (CaptureMode::RepeatArea, Some(r)) => Started::Done(repeat_area_in(r, shots)),
                    (CaptureMode::RepeatArea, None) => Started::Done(repeat_area(opt)),
                    _ => Started::Pick(Frozen { shots, windows: vec![] }, mode),
                };
            }
            Ok(None) => {}
            Err(e) => eprintln!("wayland: no plain screenshot, using the desktop's tool: {e}"),
        }
    }
    if mode == CaptureMode::RepeatArea {
        return Started::Done(Err("Repeat Area Capture couldn't take a screenshot of the desktop.".into()));
    }
    Started::Done(backend::portal(mode, opt))
}

/// A desktop picture as the screens the picker works on. One screen per monitor when the monitors
/// tile the picture exactly (a window per screen covers it); otherwise the picture is one screen.
#[cfg(target_os = "linux")]
pub fn shots_from_picture(img: Image, monitors: &[(i32, i32, u32, u32)], scale: f32) -> Vec<Shot> {
    let inside = |&(x, y, w, h): &(i32, i32, u32, u32)| x >= 0 && y >= 0 && w > 0 && h > 0 && x as i64 + w as i64 <= img.w as i64 && y as i64 + h as i64 <= img.h as i64;
    let area: u64 = monitors.iter().map(|&(_, _, w, h)| u64::from(w) * u64::from(h)).sum();
    if monitors.len() > 1 && monitors.iter().all(inside) && area == u64::from(img.w) * u64::from(img.h) {
        let mut out: Vec<Shot> = monitors
            .iter()
            .filter_map(|&(x, y, w, h)| {
                let r = R { x0: x as f32, y0: y as f32, x1: (x as u32 + w) as f32, y1: (y as u32 + h) as f32 };
                crop(&img, r).map(|img| Shot { x, y, scale, primary: false, img })
            })
            .collect();
        if out.len() == monitors.len() {
            // The one at the desktop's origin is the main one, as far as can be told.
            let main = out.iter().position(|s| s.x == 0 && s.y == 0).unwrap_or(0);
            out[main].primary = true;
            return out;
        }
    }
    vec![Shot { x: 0, y: 0, scale, primary: true, img }]
}

pub enum Started {
    Done(Result<Option<Captured>, String>),
    /// The screens are frozen for the user to choose on, in this mode (the active-window capture
    /// falls back to picking a window when no window has the focus).
    Pick(Frozen, CaptureMode),
}

/// Runs off the UI thread.
pub fn start(mode: CaptureMode, opt: &CaptureOptions) -> Started {
    #[cfg(target_os = "linux")]
    if backend::wayland() {
        return wayland_start(mode, opt);
    }
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
    repeat_area_in(r, screens_now(opt)?)
}

/// Repeat Area on screens already taken (Wayland takes them from a portal screenshot).
fn repeat_area_in(r: R, shots: Vec<Shot>) -> Result<Option<Captured>, String> {
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

/// Why scrolling capture can't run in this session, if it can't: on Wayland apps can neither look
/// at the screen nor scroll other apps.
#[cfg(target_os = "linux")]
pub fn scrolling_unavailable(wayland: bool) -> Option<&'static str> {
    wayland.then_some("Scrolling capture needs an X11 session: on Wayland, apps can't look at the screen.")
}

/// The screens frozen for choosing the area of a scrolling capture: no windows, no pointer.
pub fn freeze_for_scrolling(scale: f32) -> Result<Frozen, String> {
    #[cfg(target_os = "linux")]
    if let Some(why) = scrolling_unavailable(backend::wayland()) {
        return Err(why.into());
    }
    let opt =CaptureOptions { last_area: None, window_bg: crate::settings::WindowBg::Transparent, solid_color: [0, 0, 0], scale, cursor: false, delay_secs: 0, snapshot: Snapshot::default() };
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

/// A Wayland session, where apps can neither look at the screen nor scroll other apps.
#[cfg(target_os = "linux")]
pub fn wayland() -> bool {
    backend::wayland()
}
fn rgba(w: u32, h: u32, px: Vec<u8>) -> Image {
    Image { w, h, px: Arc::new(px) }
}

/// True when every sampled pixel is black or transparent: what PrintWindow returns for some
/// GPU-drawn windows.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn blank(img: &Image) -> bool {
    img.px.chunks_exact(4).step_by(97).all(|p| p[3] == 0 || (p[0] == 0 && p[1] == 0 && p[2] == 0))
}

#[cfg(target_os = "windows")]
mod backend {
    use super::{blank, rgba, Shot, WinInfo};
    use crate::model::Image;
    use crate::system::CaptureOptions;
    use crate::targets::{Rect, WindowInfo};

    fn err(e: xcap::XCapError) -> String {
        format!("Can't take a screenshot: {e}")
    }

    pub fn monitors(_opt: &CaptureOptions) -> Result<Vec<Shot>, String> {
        let mut out = vec![];
        for m in xcap::Monitor::all().map_err(err)? {
            let img = m.capture_image().map_err(err)?;
            let (w, h) = img.dimensions();
            out.push(Shot {
                x: m.x().unwrap_or(0),
                y: m.y().unwrap_or(0),
                scale: m.scale_factor().unwrap_or(1.0).max(0.5),
                primary: m.is_primary().unwrap_or(false),
                img: opaque(rgba(w, h, img.into_raw())),
            });
        }
        if out.is_empty() {
            return Err("Can't take a screenshot: no screen found.".into());
        }
        Ok(out)
    }

    fn monitor_at(x: i32, y: i32) -> Option<xcap::Monitor> {
        xcap::Monitor::all().ok()?.into_iter().find(|m| {
            let (Ok(mx), Ok(my), Ok(mw), Ok(mh)) = (m.x(), m.y(), m.width(), m.height()) else { return false };
            x >= mx && y >= my && x < mx + mw as i32 && y < my + mh as i32
        })
    }

    /// The pixels of a desktop rectangle, from the screen holding its centre.
    pub fn grab_region(x: i32, y: i32, w: u32, h: u32) -> Result<image::RgbaImage, String> {
        let m = monitor_at(x + (w / 2) as i32, y + (h / 2) as i32).ok_or("The scrolling area is no longer on a screen.")?;
        let (mx, my, mw, mh) = (m.x().map_err(err)?, m.y().map_err(err)?, m.width().map_err(err)?, m.height().map_err(err)?);
        let (lx, ly) = ((x - mx).max(0) as u32, (y - my).max(0) as u32);
        let (rw, rh) = (w.min(mw.saturating_sub(lx)), h.min(mh.saturating_sub(ly)));
        if rw == 0 || rh == 0 {
            return Err("The scrolling area is no longer on a screen.".into());
        }
        let mut img = m.capture_region(lx, ly, rw, rh).map_err(err)?;
        // As for whole screens: some drivers give alpha 0.
        for p in img.pixels_mut() {
            p.0[3] = 255;
        }
        Ok(img)
    }

    pub fn scale_at(x: i32, y: i32) -> f32 {
        monitor_at(x, y).and_then(|m| m.scale_factor().ok()).unwrap_or(1.0)
    }

    /// The desktop picture's file. Windows keeps one for the desktop (several screens share it).
    pub fn wallpaper(_x: i32, _y: i32) -> Result<std::path::PathBuf, String> {
        use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
        let mut buf = [0u16; 1024];
        // SAFETY: the buffer is as long as the length given; Windows writes a NUL-terminated path into it.
        unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr() as *mut std::ffi::c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.map_err(|e| e.message())?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len]));
        if len == 0 || !path.is_file() {
            return Err("the desktop has no picture".into());
        }
        Ok(path)
    }
    /// Screen pixels come back with alpha 0 from some drivers; a screenshot is always opaque.
    fn opaque(img: Image) -> Image {
        if img.px.chunks_exact(4).all(|p| p[3] == 255) {
            return img;
        }
        let mut px = img.px.to_vec();
        for p in px.chunks_exact_mut(4) {
            p[3] = 255;
        }
        rgba(img.w, img.h, px)
    }

    pub fn screens() -> Vec<(i32, i32, u32, u32)> {
        xcap::Monitor::all().unwrap_or_default().iter().filter_map(|m| Some((m.x().ok()?, m.y().ok()?, m.width().ok()?, m.height().ok()?))).collect()
    }

    pub fn windows() -> Vec<WinInfo> {
        let Ok(all) = xcap::Window::all() else { return vec![] };
        all.into_iter()
            .filter_map(|w| {
                if w.is_minimized().unwrap_or(true) {
                    return None;
                }
                let (width, height) = (w.width().ok()?, w.height().ok()?);
                if width < 4 || height < 4 {
                    return None;
                }
                Some(WinInfo { id: w.id().ok()? as u64, title: w.title().unwrap_or_default(), x: w.x().ok()?, y: w.y().ok()?, w: width, h: height })
            })
            .collect()
    }

    /// The mouse pointer, in desktop pixels.
    pub fn cursor() -> Option<(i32, i32)> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT::default();
        // SAFETY: GetCursorPos only writes the pointer position into the POINT it is given.
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x, p.y))
    }

    /// The mouse pointer as it is drawn now; None when it is hidden (a game, a video) or can't be read.
    pub fn pointer() -> Option<super::Pointer> {
        use windows::Win32::Graphics::Gdi::DeleteObject;
        use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, HICON, ICONINFO};
        let mut info = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        // SAFETY: GetCursorInfo fills the CURSORINFO it is given, whose size cbSize states.
        unsafe { GetCursorInfo(&mut info) }.ok()?;
        if info.flags.0 & CURSOR_SHOWING.0 == 0 || info.hCursor.is_invalid() {
            return None;
        }
        // The cursor is shared and owned by the system: it is only drawn here, never destroyed.
        let icon = HICON(info.hCursor.0);
        let mut ii = ICONINFO::default();
        // SAFETY: GetIconInfo fills the ICONINFO; the bitmaps it creates are deleted just below.
        unsafe { GetIconInfo(icon, &mut ii) }.ok()?;
        let monochrome = ii.hbmColor.is_invalid();
        let size = bitmap_size(if monochrome { ii.hbmMask } else { ii.hbmColor });
        for b in [ii.hbmColor, ii.hbmMask] {
            if !b.is_invalid() {
                // SAFETY: GetIconInfo made these bitmaps for the caller to delete, and nothing else holds them.
                let _ = unsafe { DeleteObject(b.into()) };
            }
        }
        let (w, h) = size?;
        // A monochrome cursor's mask is its AND mask above its XOR mask.
        let h = if monochrome { h / 2 } else { h };
        if w == 0 || h == 0 || w > 512 || h > 512 {
            return None;
        }
        let mut p = super::unblend(w, h, &render(icon, w, h, 0x00)?, &render(icon, w, h, 0xFF)?)?;
        p.x = info.ptScreenPos.x - ii.xHotspot as i32;
        p.y = info.ptScreenPos.y - ii.yHotspot as i32;
        Some(p)
    }

    fn bitmap_size(bmp: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<(u32, u32)> {
        use windows::Win32::Graphics::Gdi::{GetObjectW, BITMAP};
        let mut bm = BITMAP::default();
        // SAFETY: GetObjectW writes at most the size it is given into the BITMAP.
        let n = unsafe { GetObjectW(bmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut std::ffi::c_void)) };
        (n > 0 && bm.bmWidth > 0 && bm.bmHeight > 0).then_some((bm.bmWidth as u32, bm.bmHeight as u32))
    }

    /// The cursor drawn on a background of `bg` (every byte), as top-down BGRA.
    fn render(icon: windows::Win32::UI::WindowsAndMessaging::HICON, w: u32, h: u32, bg: u8) -> Option<Vec<u8>> {
        use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
        use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, DI_NORMAL};
        let len = (w as usize) * (h as usize) * 4;
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: a memory DC, deleted below on every path.
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return None;
        }
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: bmi describes a w×h 32-bit top-down DIB; `bits` receives its memory, valid while the DIB lives.
        let out = match unsafe { CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) } {
            Ok(dib) if !bits.is_null() => {
                // SAFETY: the DIB is selected into our DC only for this drawing and selected out before it is deleted.
                let old = unsafe { SelectObject(dc, dib.into()) };
                // SAFETY: `bits` points to the DIB's w*h*4 bytes, which nothing else touches while we hold it.
                let pixels = unsafe { std::slice::from_raw_parts_mut(bits as *mut u8, len) };
                pixels.fill(bg);
                // SAFETY: draws the cursor into our DC at its own size; GdiFlush makes the pixels final.
                let drawn = unsafe { DrawIconEx(dc, 0, 0, icon, w as i32, h as i32, 0, None, DI_NORMAL) }.is_ok();
                // SAFETY: takes no arguments; finishes this thread's pending GDI drawing.
                let _ = unsafe { GdiFlush() };
                let copy = drawn.then(|| pixels.to_vec());
                // SAFETY: restores the DC's own bitmap, then deletes ours, which is no longer selected.
                unsafe {
                    SelectObject(dc, old);
                    let _ = DeleteObject(dib.into());
                }
                copy
            }
            _ => None,
        };
        // SAFETY: the DC made above, with its original bitmap back in.
        let _ = unsafe { DeleteDC(dc) };
        out
    }

    /// The window that has the focus and its process, when it is a real, visible window.
    pub fn focused_window() -> (Option<i32>, Vec<WindowInfo>) {
        let Ok(all) = xcap::Window::all() else { return (None, vec![]) };
        for w in all {
            if !w.is_focused().unwrap_or(false) || w.is_minimized().unwrap_or(true) {
                continue;
            }
            let (Ok(id), Ok(pid), Ok(x), Ok(y), Ok(width), Ok(height)) = (w.id(), w.pid(), w.x(), w.y(), w.width(), w.height()) else { continue };
            let info = WindowInfo { id, pid: pid as i32, layer: 0, alpha: 1.0, bounds: Rect::new(x as f64, y as f64, width as f64, height as f64) };
            return (Some(pid as i32), vec![info]);
        }
        (None, vec![])
    }

    pub fn window_image(win: &WinInfo) -> Option<Image> {
        let w = xcap::Window::all().ok()?.into_iter().find(|w| w.id().ok().map(u64::from) == Some(win.id))?;
        let img = w.capture_image().ok()?;
        let (width, height) = img.dimensions();
        let img = opaque(rgba(width, height, img.into_raw()));
        (!blank(&img)).then_some(img)
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::{rgba, Shot, WinInfo};
    use crate::model::Image;
    use crate::system::{CaptureMode, CaptureOptions, Captured};
    use crate::targets::{Rect, WindowInfo};
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xfixes::ConnectionExt as _;
    use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, ImageFormat, ImageOrder, Window};
    use x11rb::rust_connection::RustConnection;

    /// On Wayland, apps can't read the screen or other windows themselves; the desktop's
    /// screenshot portal does it and shows its own picker.
    pub fn wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland")
    }

    pub fn portal(mode: CaptureMode, opt: &CaptureOptions) -> Result<Option<Captured>, String> {
        if mode == CaptureMode::RepeatArea {
            return Err("Repeat Area Capture isn't available on Wayland: the desktop doesn't tell apps where an area was.".into());
        }
        if mode == CaptureMode::Delayed {
            std::thread::sleep(std::time::Duration::from_secs(u64::from(opt.delay_secs)));
        }
        // The desktop has no "active window" for apps to read, so it asks; All Displays is its
        // plain full-desktop screenshot.
        let interactive = matches!(mode, CaptureMode::Area | CaptureMode::Window | CaptureMode::ActiveWindow);
        let Some(img) = portal_picture(interactive)? else { return Ok(None) };
        Ok(Some(Captured { img, scale: opt.scale, area: None, mode }))
    }

    /// A screenshot from the desktop's portal: the whole desktop, or (`interactive`) what the user
    /// picks in the desktop's own tool. `Ok(None)`: cancelled. The desktop's copy is deleted.
    pub fn portal_picture(interactive: bool) -> Result<Option<Image>, String> {
        use ashpd::desktop::screenshot::Screenshot;
        let result = async_io::block_on(async { Screenshot::request().interactive(interactive).modal(true).send().await?.response() });
        let shot = match result {
            Ok(shot) => shot,
            Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => return Ok(None),
            Err(e) => return Err(format!("Can't take a screenshot: {e}")),
        };
        let Some(path) = file_path(shot.uri().as_str()) else {
            return Err(format!("Can't take a screenshot: the desktop returned {}", shot.uri().as_str()));
        };
        read_and_remove(&path).map(Some)
    }

    /// Read the screenshot the desktop saved for this request, then delete that file. The desktop
    /// leaves a copy of every shot in ~/Pictures, and ShotCraft keeps the picture in its own
    /// window (the user saves what they want). The file goes whether or not it could be read, so
    /// an unreadable one doesn't pile up either.
    pub fn read_and_remove(path: &std::path::Path) -> Result<Image, String> {
        let img = crate::system::load_image_file(path);
        let _ = std::fs::remove_file(path);
        img
    }

    /// `file:///home/me/Pictures/Screenshot%20from%20….png` → a path.
    pub fn file_path(uri: &str) -> Option<std::path::PathBuf> {
        let rest = uri.strip_prefix("file://")?;
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        let bytes = rest.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        use std::os::unix::ffi::OsStringExt;
        Some(std::path::PathBuf::from(std::ffi::OsString::from_vec(out)))
    }

    fn connect() -> Result<(RustConnection, usize), String> {
        x11rb::connect(None).map_err(|e| format!("Can't take a screenshot: no X11 display ({e})"))
    }

    pub fn monitors(opt: &CaptureOptions) -> Result<Vec<Shot>, String> {
        let (conn, screen_num) = connect()?;
        let screen = conn.setup().roots.get(screen_num).ok_or("Can't take a screenshot: no X11 screen")?.clone();
        let full = get_image(&conn, screen.root, 0, 0, screen.width_in_pixels, screen.height_in_pixels)?;
        let monitors = conn.randr_get_monitors(screen.root, true).ok().and_then(|c| c.reply().ok()).map(|r| r.monitors).unwrap_or_default();
        let mut out = vec![];
        for m in &monitors {
            let r = crate::model::R { x0: m.x as f32, y0: m.y as f32, x1: m.x as f32 + m.width as f32, y1: m.y as f32 + m.height as f32 };
            if let Some(img) = super::crop(&full, r) {
                out.push(Shot { x: m.x as i32, y: m.y as i32, scale: opt.scale, primary: m.primary, img });
            }
        }
        if out.is_empty() {
            out.push(Shot { x: 0, y: 0, scale: opt.scale, primary: true, img: full });
        } else if !out.iter().any(|s| s.primary) {
            out[0].primary = true;
        }
        Ok(out)
    }

    /// The pixels of a desktop rectangle (scrolling capture), cut to the X screen.
    pub fn grab_region(x: i32, y: i32, w: u32, h: u32) -> Result<image::RgbaImage, String> {
        let (conn, screen_num) = connect()?;
        let screen = conn.setup().roots.get(screen_num).ok_or("Can't take a screenshot: no X11 screen")?.clone();
        let (x0, y0) = (x.clamp(0, screen.width_in_pixels as i32), y.clamp(0, screen.height_in_pixels as i32));
        let w = (x + w as i32).min(screen.width_in_pixels as i32) - x0;
        let h = (y + h as i32).min(screen.height_in_pixels as i32) - y0;
        if w <= 0 || h <= 0 {
            return Err("The scrolling area is no longer on a screen.".into());
        }
        let img = get_image(&conn, screen.root, x0 as i16, y0 as i16, w as u16, h as u16)?;
        image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).ok_or_else(|| "Can't read the screen.".to_string())
    }

    /// The desktop picture's file, from GNOME's settings (also used by Cinnamon, Budgie and others);
    /// other desktops keep theirs elsewhere.
    pub fn wallpaper(_x: i32, _y: i32) -> Result<std::path::PathBuf, String> {
        let get = |schema: &str, key: &str| {
            let out = std::process::Command::new("gsettings").args(["get", schema, key]).output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string())
        };
        let read = |key: &str| get("org.gnome.desktop.background", key).and_then(|uri| file_path(&uri)).filter(|p| p.is_file());
        // GNOME keeps a light and a dark picture and shows the one of the current style.
        let dark = get("org.gnome.desktop.interface", "color-scheme").is_some_and(|s| s == "prefer-dark");
        let (first, second) = if dark { ("picture-uri-dark", "picture-uri") } else { ("picture-uri", "picture-uri-dark") };
        read(first).or_else(|| read(second)).ok_or_else(|| "the desktop picture isn't one ShotCraft can find here".to_string())
    }
    /// X11 reports pixels: one per point.
    pub fn scale_at(_x: i32, _y: i32) -> f32 {
        1.0
    }
    /// Screen rectangles from RandR; none on Wayland, where windows can't place themselves.
    pub fn screens() -> Vec<(i32, i32, u32, u32)> {
        if wayland() {
            return vec![];
        }
        randr_screens()
    }

    /// The monitors XWayland reports, for splitting a portal screenshot per monitor
    /// (`shots_from_picture` checks that they tile the picture before trusting them).
    pub fn xwayland_screens() -> Vec<(i32, i32, u32, u32)> {
        randr_screens()
    }

    fn randr_screens() -> Vec<(i32, i32, u32, u32)> {
        let Ok((conn, screen_num)) = connect() else { return vec![] };
        let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else { return vec![] };
        conn.randr_get_monitors(root, true)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.monitors.iter().map(|m| (m.x as i32, m.y as i32, m.width as u32, m.height as u32)).collect())
            .unwrap_or_default()
    }

    fn get_image(conn: &RustConnection, drawable: Window, x: i16, y: i16, w: u16, h: u16) -> Result<Image, String> {
        let fail = |e: String| format!("Can't take a screenshot: {e}");
        let reply = conn.get_image(ImageFormat::Z_PIXMAP, drawable, x, y, w, h, !0).map_err(|e| fail(e.to_string()))?.reply().map_err(|e| fail(e.to_string()))?;
        let bpp = conn.setup().pixmap_formats.iter().find(|f| f.depth == reply.depth).map(|f| f.bits_per_pixel).unwrap_or(0);
        let n = w as usize * h as usize;
        if bpp != 32 || reply.data.len() < n * 4 {
            return Err(fail(format!("the screen uses {} bits per pixel at depth {}; only 24- and 32-bit screens are supported", bpp, reply.depth)));
        }
        let lsb = conn.setup().image_byte_order == ImageOrder::LSB_FIRST;
        let mut px = Vec::with_capacity(n * 4);
        for p in reply.data.chunks_exact(4).take(n) {
            let (r, g, b) = if lsb { (p[2], p[1], p[0]) } else { (p[1], p[2], p[3]) };
            px.extend_from_slice(&[r, g, b, 255]);
        }
        Ok(rgba(w as u32, h as u32, px))
    }

    fn atom(conn: &RustConnection, name: &str) -> Option<Atom> {
        conn.intern_atom(false, name.as_bytes()).ok()?.reply().ok().map(|r| r.atom)
    }

    fn prop32(conn: &RustConnection, win: Window, prop: Atom, ty: impl Into<Atom>, len: u32) -> Vec<u32> {
        conn.get_property(false, win, prop, ty.into(), 0, len).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|v| v.collect())).unwrap_or_default()
    }

    fn title(conn: &RustConnection, win: Window, net_wm_name: Option<Atom>, utf8: Option<Atom>) -> String {
        if let (Some(name), Some(utf8)) = (net_wm_name, utf8) {
            if let Some(r) = conn.get_property(false, win, name, utf8, 0, 1024).ok().and_then(|c| c.reply().ok()) {
                if !r.value.is_empty() {
                    return String::from_utf8_lossy(&r.value).into_owned();
                }
            }
        }
        conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024).ok().and_then(|c| c.reply().ok()).map(|r| String::from_utf8_lossy(&r.value).into_owned()).unwrap_or_default()
    }

    /// The desktop's windows, topmost first, with their frames (title bar and borders).
    pub fn windows() -> Vec<WinInfo> {
        let Ok((conn, screen_num)) = connect() else { return vec![] };
        let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else { return vec![] };
        let (Some(stacking), Some(frame), Some(state), Some(hidden), Some(pid), Some(desktop), Some(current)) = (
            atom(&conn, "_NET_CLIENT_LIST_STACKING"),
            atom(&conn, "_NET_FRAME_EXTENTS"),
            atom(&conn, "_NET_WM_STATE"),
            atom(&conn, "_NET_WM_STATE_HIDDEN"),
            atom(&conn, "_NET_WM_PID"),
            atom(&conn, "_NET_WM_DESKTOP"),
            atom(&conn, "_NET_CURRENT_DESKTOP"),
        ) else {
            return vec![];
        };
        let (net_wm_name, utf8) = (atom(&conn, "_NET_WM_NAME"), atom(&conn, "UTF8_STRING"));
        let me = std::process::id();
        let current_desktop = prop32(&conn, root, current, AtomEnum::CARDINAL, 1).first().copied();
        let mut out = vec![];
        for win in prop32(&conn, root, stacking, AtomEnum::WINDOW, 4096).into_iter().rev() {
            if prop32(&conn, win, pid, AtomEnum::CARDINAL, 1).first() == Some(&me) {
                continue;
            }
            if prop32(&conn, win, state, AtomEnum::ATOM, 64).contains(&hidden) {
                continue;
            }
            let on = prop32(&conn, win, desktop, AtomEnum::CARDINAL, 1).first().copied();
            if let (Some(on), Some(cur)) = (on, current_desktop) {
                if on != cur && on != u32::MAX {
                    continue;
                }
            }
            let Some(geo) = conn.get_geometry(win).ok().and_then(|c| c.reply().ok()) else { continue };
            let Some(pos) = conn.translate_coordinates(win, root, 0, 0).ok().and_then(|c| c.reply().ok()) else { continue };
            let ext = prop32(&conn, win, frame, AtomEnum::CARDINAL, 4);
            let [l, r, t, b] = <[u32; 4]>::try_from(ext.as_slice()).unwrap_or([0; 4]);
            let (w, h) = (geo.width as u32 + l + r, geo.height as u32 + t + b);
            if w < 4 || h < 4 {
                continue;
            }
            out.push(WinInfo { id: win as u64, title: title(&conn, win, net_wm_name, utf8), x: pos.dst_x as i32 - l as i32, y: pos.dst_y as i32 - t as i32, w, h });
        }
        out
    }

    /// The mouse pointer, in desktop pixels; unknown on Wayland.
    pub fn cursor() -> Option<(i32, i32)> {
        if wayland() {
            return None;
        }
        let (conn, screen_num) = connect().ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;
        let p = conn.query_pointer(root).ok()?.reply().ok()?;
        Some((p.root_x as i32, p.root_y as i32))
    }

    /// The mouse pointer as it is drawn now (XFixes); unknown on Wayland, whose portal draws it or not.
    pub fn pointer() -> Option<super::Pointer> {
        if wayland() {
            return None;
        }
        let (conn, _) = connect().ok()?;
        // XFixes answers only a client that said which version it speaks.
        conn.xfixes_query_version(5, 0).ok()?.reply().ok()?;
        let c = conn.xfixes_get_cursor_image().ok()?.reply().ok()?;
        let (w, h) = (c.width as u32, c.height as u32);
        if w == 0 || h == 0 || w > 512 || h > 512 || c.cursor_image.len() < (w * h) as usize {
            return None;
        }
        // Premultiplied ARGB, one u32 per pixel: divide the colour back out.
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for &argb in c.cursor_image.iter().take((w * h) as usize) {
            let a = argb >> 24;
            let un = |c: u32| if a == 0 { 0 } else { ((c * 255 + a / 2) / a).min(255) as u8 };
            px.extend_from_slice(&[un((argb >> 16) & 0xff), un((argb >> 8) & 0xff), un(argb & 0xff), a as u8]);
        }
        let invert = vec![false; (w * h) as usize];
        Some(super::Pointer { x: c.x as i32 - c.xhot as i32, y: c.y as i32 - c.yhot as i32, w, h, px, invert })
    }
    /// The window the window manager says is active. `windows()` leaves out ShotCraft's own, so
    /// when ShotCraft has the focus nothing is found and the user picks a window instead.
    pub fn focused_window() -> (Option<i32>, Vec<WindowInfo>) {
        let Ok((conn, screen_num)) = connect() else { return (None, vec![]) };
        let Some(root) = conn.setup().roots.get(screen_num).map(|s| s.root) else { return (None, vec![]) };
        let (Some(active), Some(pid)) = (atom(&conn, "_NET_ACTIVE_WINDOW"), atom(&conn, "_NET_WM_PID")) else { return (None, vec![]) };
        let Some(id) = prop32(&conn, root, active, AtomEnum::WINDOW, 1).first().copied().filter(|id| *id != 0) else { return (None, vec![]) };
        let Some(w) = windows().into_iter().find(|w| w.id == id as u64) else { return (None, vec![]) };
        // Without a process id the window's own id stands in: it only has to differ from ShotCraft's.
        let pid = prop32(&conn, id, pid, AtomEnum::CARDINAL, 1).first().map_or(id as i32, |p| *p as i32);
        let info = WindowInfo { id, pid, layer: 0, alpha: 1.0, bounds: Rect::new(w.x as f64, w.y as f64, w.w as f64, w.h as f64) };
        (Some(pid), vec![info])
    }

    /// X11 has no picture of a covered window without a compositor's help; the frozen screen is used.
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

    #[cfg(target_os = "linux")]
    #[test]
    fn a_portal_picture_is_split_per_monitor_only_when_the_monitors_tile_it() {
        // Two 4x2 monitors side by side in an 8x2 picture: one screen each, the one at 0,0 primary.
        let two = shots_from_picture(img(8, 2), &[(4, 0, 4, 2), (0, 0, 4, 2)], 1.25);
        assert_eq!(two.len(), 2);
        assert!(two[0].x == 4 && !two[0].primary && two[1].x == 0 && two[1].primary);
        assert!(two.iter().all(|s| s.img.w == 4 && s.img.h == 2 && s.scale == 1.25));
        // Monitors that don't add up to the picture (XWayland reports other units): one screen.
        let one = shots_from_picture(img(8, 2), &[(0, 0, 12, 3), (12, 0, 12, 3)], 1.0);
        assert_eq!((one.len(), one[0].x, one[0].y, one[0].img.w, one[0].primary), (1, 0, 0, 8, true));
        assert_eq!(shots_from_picture(img(8, 2), &[], 1.0).len(), 1, "no monitors known");
        assert_eq!(shots_from_picture(img(8, 2), &[(0, 0, 8, 2)], 1.0).len(), 1, "one monitor");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn repeat_area_crops_the_last_area_out_of_a_given_picture() {
        let shots = shots_from_picture(img(10, 8), &[], 1.0);
        let r = R { x0: 2.0, y0: 3.0, x1: 6.0, y1: 7.0 };
        let got = repeat_area_in(r, shots).expect("ok").expect("a capture");
        assert_eq!((got.img.w, got.img.h, got.mode), (4, 4, CaptureMode::RepeatArea));
        assert!(repeat_area_in(R { x0: 50.0, y0: 50.0, x1: 60.0, y1: 60.0 }, shots_from_picture(img(10, 8), &[], 1.0)).is_err(), "off every screen");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn scrolling_capture_is_refused_with_a_reason_on_wayland_only() {
        let why = scrolling_unavailable(true).expect("Wayland can't scroll other apps");
        assert!(why.contains("X11"), "the message says what to use instead: {why}");
        assert_eq!(scrolling_unavailable(false), None);
    }

    #[test]
    fn a_pointer_comes_out_of_its_two_renders() {
        // BGRA renders of a 4×1 pointer: opaque red, half-transparent black (a shadow), nothing,
        // and an inverting pixel (white on black, black on white).
        let black = [0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 0];
        let white = [0, 0, 255, 0, 128, 128, 128, 0, 255, 255, 255, 0, 0, 0, 0, 0];
        let p = unblend(4, 1, &black, &white).expect("pointer");
        assert_eq!(&p.px[0..4], &[255, 0, 0, 255], "opaque red");
        assert_eq!(p.px[7], 127, "the shadow lets half the background through");
        assert_eq!(&p.px[4..7], &[0, 0, 0], "and is black");
        assert_eq!(p.px[11], 0, "nothing there");
        assert_eq!(p.invert, vec![false, false, false, true]);
        assert!(unblend(4, 1, &black[..8], &white).is_none(), "short renders are refused");
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

    /// The real cursor, read through GDI (only its picture and position, never the screen). Skips
    /// when no cursor is shown, as in a session without a mouse.
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_reads_the_pointer_it_shows() {
        let Some(p) = backend::pointer() else {
            eprintln!("no pointer shown: skipped");
            return;
        };
        assert!(p.w >= 16 && p.h >= 16 && p.w <= 512 && p.h <= 512, "{}x{}", p.w, p.h);
        assert_eq!(p.px.len(), (p.w * p.h * 4) as usize);
        let opaque = p.px.chunks_exact(4).filter(|c| c[3] == 255).count();
        let clear = p.px.chunks_exact(4).zip(&p.invert).filter(|(c, inv)| c[3] == 0 && !**inv).count();
        assert!(opaque > 0 || p.invert.iter().any(|i| *i), "something is drawn");
        assert!(clear > 0, "the pointer doesn't fill its square");
        eprintln!("pointer {}x{} at ({}, {}): {opaque} opaque, {} inverting", p.w, p.h, p.x, p.y, p.invert.iter().filter(|i| **i).count());
        // To look at it: SHOTCRAFT_SHOTS=<dir> saves it drawn on grey, four times its size.
        if let Ok(dir) = std::env::var("SHOTCRAFT_SHOTS") {
            let mut img = Image { w: p.w + 8, h: p.h + 8, px: Arc::new(vec![150; ((p.w + 8) * (p.h + 8) * 4) as usize]) };
            draw_pointer(&mut img, p.x - 4, p.y - 4, &p);
            let big = image::imageops::resize(&image::RgbaImage::from_raw(img.w, img.h, img.px.to_vec()).expect("image"), img.w * 4, img.h * 4, image::imageops::FilterType::Nearest);
            let _ = big.save(std::path::Path::new(&dir).join("pointer.png"));
        }
    }

    /// The real grab of a scrolling capture, on an 8×8 corner of the main screen: only its size and
    /// opacity are checked, nothing is kept.
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_grabs_just_the_area_asked_for() {
        let Some(&(x, y, _, _)) = backend::screens().first() else {
            eprintln!("no screen: skipped");
            return;
        };
        let img = grab_area(&crate::scroll::GrabRect { x, y, w: 8, h: 8 }).expect("grab");
        assert_eq!(img.dimensions(), (8, 8));
        assert!(img.pixels().all(|p| p.0[3] == 255), "opaque");
        assert!(grab_area(&crate::scroll::GrabRect { x: -1_000_000, y: -1_000_000, w: 8, h: 8 }).is_err(), "off every screen");
        assert!(scale_at(x, y) >= 0.5);
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

    /// Windows says where the desktop picture is (or that there is none). Only that the file exists
    /// is checked: its path and picture stay private.
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_finds_the_desktop_picture_or_says_there_is_none() {
        match backend::wallpaper(0, 0) {
            Ok(path) => assert!(path.is_file()),
            Err(why) => assert!(why.contains("no picture") || !why.is_empty()),
        }
    }

    /// The real X11 code under a display (CI runs the tests under Xvfb): the screens, a frozen
    /// picture of them, an 8×8 grab and the pointer. Skips without an X11 display.
    #[cfg(target_os = "linux")]
    #[test]
    fn x11_reads_the_screens_grabs_an_area_and_the_pointer() {
        if backend::wayland() || std::env::var_os("DISPLAY").is_none() {
            eprintln!("no X11 display: skipped");
            return;
        }
        let screens = backend::screens();
        assert!(!screens.is_empty(), "RandR names at least one screen");
        let frozen = freeze_for_scrolling(1.0).expect("the screens are photographed");
        assert!(!frozen.shots.is_empty() && frozen.shots.iter().all(|s| s.img.w > 0 && s.img.h > 0));
        let img = grab_area(&crate::scroll::GrabRect { x: 0, y: 0, w: 8, h: 8 }).expect("grab");
        assert_eq!(img.dimensions(), (8, 8));
        assert!(img.pixels().all(|p| p.0[3] == 255), "opaque");
        assert!(grab_area(&crate::scroll::GrabRect { x: 1_000_000, y: 0, w: 8, h: 8 }).is_err(), "off the screen");
        match backend::pointer() {
            Some(p) => {
                assert!(p.w > 0 && p.h > 0 && p.px.len() == (p.w * p.h * 4) as usize);
                eprintln!("pointer {}x{} at ({}, {}), {} screens", p.w, p.h, p.x, p.y, screens.len());
            }
            None => eprintln!("XFixes gave no pointer, {} screens", screens.len()),
        }
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

    #[test]
    fn blank_spots_black_and_transparent_pictures() {
        assert!(blank(&Image { w: 2, h: 1, px: Arc::new(vec![0, 0, 0, 255, 0, 0, 0, 0]) }));
        assert!(!blank(&img(20, 20)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn portal_uris_become_paths() {
        assert_eq!(backend::file_path("file:///home/a/Pictures/Screenshot%20from%202026.png"), Some("/home/a/Pictures/Screenshot from 2026.png".into()));
        assert_eq!(backend::file_path("https://example.com/x.png"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_desktops_copy_of_a_screenshot_is_deleted_after_reading() {
        let dir = std::env::temp_dir().join(format!("shotcraft-portal-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let good = dir.join("Screenshot from 2026.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255])).save(&good).expect("write a png");
        let img = backend::read_and_remove(&good).expect("a readable picture");
        assert_eq!((img.w, img.h), (3, 2));
        assert!(!good.exists(), "the copy in Pictures is gone");

        let bad = dir.join("not-a-picture.png");
        std::fs::write(&bad, b"not a png").expect("write a file");
        assert!(backend::read_and_remove(&bad).is_err());
        assert!(!bad.exists(), "an unreadable file doesn't pile up either");

        assert!(backend::read_and_remove(&dir.join("missing.png")).is_err(), "a missing file is an error, not a panic");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Captures this computer's screens into memory and checks their shape; nothing is saved or shown.
#[cfg(all(test, target_os = "windows"))]
mod real_screen {
    use super::*;

    #[test]
    #[ignore = "reads this computer's real screens"]
    fn captures_every_screen_and_lists_windows() {
        let opt = CaptureOptions { last_area: None, window_bg: crate::settings::WindowBg::Transparent, solid_color: [0, 0, 0], scale: 1.0, cursor: false, delay_secs: 3, snapshot: Default::default() };
        let shots = backend::monitors(&opt).expect("capture");
        assert!(!shots.is_empty());
        assert_eq!(shots.iter().filter(|s| s.primary).count(), 1, "one primary screen");
        for s in &shots {
            assert!(s.img.w >= 640 && s.img.h >= 480);
            assert_eq!(s.img.px.len(), (s.img.w * s.img.h * 4) as usize);
            assert!(s.img.px.chunks_exact(4).all(|p| p[3] == 255), "opaque");
            assert_eq!(screen_at(s.x as f32 + 1.0, s.y as f32 + 1.0), Some((s.x, s.y, s.img.w, s.img.h)));
            eprintln!("screen at {},{}: {}x{}, scale {}, primary {}", s.x, s.y, s.img.w, s.img.h, s.scale, s.primary);
        }
        let windows = backend::windows();
        eprintln!("{} windows listed", windows.len());
        assert!(windows.iter().all(|w| w.w >= 4 && w.h >= 4));
        let Ok(Some(c)) = main_screen(CaptureMode::Fullscreen, &opt) else { panic!("fullscreen") };
        assert_eq!(c.mode, CaptureMode::Fullscreen);

        // The modes that point at something: the display under the cursor, all displays, the focused window.
        let with =
            |mode| CaptureOptions { last_area: None, window_bg: crate::settings::WindowBg::Transparent, solid_color: [0, 0, 0], scale: 1.0, cursor: false, delay_secs: 3, snapshot: snapshot(mode) };
        let snap = snapshot(CaptureMode::ActiveWindow);
        assert_eq!(snap.displays.len(), shots.len());
        let (cx, cy) = backend::cursor().expect("a cursor position");
        assert!(screen_at(cx as f32, cy as f32).is_some(), "the cursor is on a screen");
        eprintln!("cursor at {cx},{cy}; focused window: {:?}", snap.windows.first().map(|w| (w.pid, w.bounds)));
        let Started::Done(Ok(Some(under_cursor))) = targeted(CaptureMode::Fullscreen, &with(CaptureMode::Fullscreen)) else { panic!("fullscreen under the cursor") };
        assert!(shots.iter().any(|s| (s.img.w, s.img.h) == (under_cursor.img.w, under_cursor.img.h)), "one whole screen");
        let Started::Done(Ok(Some(all))) = targeted(CaptureMode::AllDisplays, &with(CaptureMode::AllDisplays)) else { panic!("all displays") };
        let (min_x, max_x) = (shots.iter().map(|s| s.x).min().unwrap_or(0), shots.iter().map(|s| s.x + s.img.w as i32).max().unwrap_or(0));
        let (min_y, max_y) = (shots.iter().map(|s| s.y).min().unwrap_or(0), shots.iter().map(|s| s.y + s.img.h as i32).max().unwrap_or(0));
        assert_eq!((all.img.w as i32, all.img.h as i32), (max_x - min_x, max_y - min_y), "the desktop's size");
        match targeted(CaptureMode::ActiveWindow, &with(CaptureMode::ActiveWindow)) {
            Started::Done(Ok(Some(w))) => eprintln!("active window: {}x{}", w.img.w, w.img.h),
            Started::Pick(..) => eprintln!("no usable focused window: the picker would open"),
            Started::Done(Err(e)) => panic!("active window: {e}"),
            Started::Done(Ok(None)) => panic!("active window: cancelled"),
        }
    }
}
