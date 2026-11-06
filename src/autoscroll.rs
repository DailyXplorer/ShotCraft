//! Scrolls the page under the capture area by posting scroll-wheel events, like a mouse wheel would.
//!
//! macOS delivers a wheel event to the window under the pointer, and only lets a process post input
//! events when it has the Accessibility permission. So the pointer is parked in the middle of the
//! area for the length of the capture (and put back at the end), and the permission is checked, and
//! asked for, when a capture starts.
use crate::scroll::GrabRect;

/// One wheel event moves the page by at most this many points; a longer scroll is sent as several
/// events, the way a trackpad or a wheel delivers it, so that pages with smooth scrolling follow.
pub const CHUNK: i32 = 48;

/// Splits a scroll of `dy` points into wheel events of at most `CHUNK` points, same sign, same total.
pub fn chunks(dy: i32) -> Vec<i32> {
    let mut left = dy;
    let mut out = Vec::new();
    while left != 0 {
        let c = left.clamp(-CHUNK, CHUNK);
        out.push(c);
        left -= c;
    }
    out
}

/// Where the pointer is parked: the middle of the area, in points from the top left of the main display.
pub fn park_point(rect: &GrabRect) -> (f64, f64) {
    (rect.x as f64 + rect.w as f64 / 2.0, rect.y as f64 + rect.h as f64 / 2.0)
}

/// True when the pointer at (x, y) is inside the area, with a margin so it never sits on the edge.
pub fn pointer_inside(rect: &GrabRect, x: f64, y: f64) -> bool {
    let m = 8.0;
    x >= rect.x as f64 + m && x <= (rect.x + rect.w as i32) as f64 - m && y >= rect.y as f64 + m && y <= (rect.y + rect.h as i32) as f64 - m
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use std::ffi::{c_char, c_void, CString};

    type CFTypeRef = *const c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceCreate(state: i32) -> CFTypeRef;
        fn CGEventCreateScrollWheelEvent2(source: CFTypeRef, units: u32, wheel_count: u32, wheel1: i32, wheel2: i32, wheel3: i32) -> CFTypeRef;
        fn CGEventPost(tap: u32, event: CFTypeRef);
        fn CGEventSetLocation(event: CFTypeRef, location: CGPoint);
        fn CGEventCreate(source: CFTypeRef) -> CFTypeRef;
        fn CGEventGetLocation(event: CFTypeRef) -> CGPoint;
        fn CGWarpMouseCursorPosition(location: CGPoint) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFAllocatorDefault: CFTypeRef;
        static kCFBooleanTrue: CFTypeRef;
        static kCFTypeDictionaryKeyCallBacks: [u8; 0];
        static kCFTypeDictionaryValueCallBacks: [u8; 0];
        fn CFStringCreateWithCString(alloc: CFTypeRef, c_str: *const c_char, encoding: u32) -> CFTypeRef;
        fn CFDictionaryCreate(alloc: CFTypeRef, keys: *const CFTypeRef, values: *const CFTypeRef, count: isize, key_callbacks: *const c_void, value_callbacks: *const c_void) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
    }

    const HID_SYSTEM_STATE: i32 = 1;
    const HID_EVENT_TAP: u32 = 0;
    const UNIT_PIXEL: u32 = 0;
    const UTF8: u32 = 0x0800_0100;

    /// Does this process have the Accessibility permission? With `prompt`, macOS asks the user for it
    /// when it is missing (the answer is still `false` until they grant it).
    pub fn trusted(prompt: bool) -> bool {
        unsafe {
            if !prompt {
                return AXIsProcessTrustedWithOptions(std::ptr::null());
            }
            let key = CString::new("AXTrustedCheckOptionPrompt").unwrap();
            let key = CFStringCreateWithCString(kCFAllocatorDefault, key.as_ptr(), UTF8);
            let (keys, values) = ([key], [kCFBooleanTrue]);
            let options = CFDictionaryCreate(kCFAllocatorDefault, keys.as_ptr(), values.as_ptr(), 1, kCFTypeDictionaryKeyCallBacks.as_ptr().cast(), kCFTypeDictionaryValueCallBacks.as_ptr().cast());
            let ok = AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
            CFRelease(key);
            ok
        }
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }

    /// Does this process have the Screen Recording permission? Without it `screencapture` hands back
    /// pictures without the other apps' windows, which never change while the page scrolls.
    pub fn screen_recording_ok() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    fn pointer() -> (f64, f64) {
        unsafe {
            let e = CGEventCreate(std::ptr::null());
            if e.is_null() {
                return (0.0, 0.0);
            }
            let p = CGEventGetLocation(e);
            CFRelease(e);
            (p.x, p.y)
        }
    }

    fn warp(x: f64, y: f64) {
        unsafe {
            CGWarpMouseCursorPosition(CGPoint { x, y });
        }
    }

    /// Posts wheel events over one area. Puts the pointer back where it was when dropped.
    pub struct Scroller {
        rect: GrabRect,
        saved: Option<(f64, f64)>,
    }

    impl Scroller {
        pub fn new(rect: GrabRect) -> Scroller {
            Scroller { rect, saved: None }
        }

        /// Scrolls the page down by `dy` points (up when negative).
        pub fn scroll(&mut self, dy: i32) -> Result<(), String> {
            let (px, py) = pointer();
            if !pointer_inside(&self.rect, px, py) {
                self.saved.get_or_insert((px, py));
                let (cx, cy) = park_point(&self.rect);
                warp(cx, cy);
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            let (x, y) = pointer();
            unsafe {
                let source = CGEventSourceCreate(HID_SYSTEM_STATE);
                for c in chunks(dy) {
                    // A positive wheel value scrolls the content down (towards the top of the page).
                    let ev = CGEventCreateScrollWheelEvent2(source, UNIT_PIXEL, 1, -c, 0, 0);
                    if ev.is_null() {
                        if !source.is_null() {
                            CFRelease(source);
                        }
                        return Err("Can't post scroll events.".into());
                    }
                    CGEventSetLocation(ev, CGPoint { x, y });
                    CGEventPost(HID_EVENT_TAP, ev);
                    CFRelease(ev);
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
                if !source.is_null() {
                    CFRelease(source);
                }
            }
            Ok(())
        }
    }

    impl Drop for Scroller {
        fn drop(&mut self) {
            if let Some((x, y)) = self.saved {
                warp(x, y);
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::{screen_recording_ok, trusted, Scroller};

/// Windows: wheel events through `SendInput`, to the window under the pointer, which is parked in
/// the area as on macOS. Windows asks no permission for it.
#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    pub fn trusted(_prompt: bool) -> bool {
        true
    }

    pub fn screen_recording_ok() -> bool {
        true
    }

    fn pointer() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        // SAFETY: GetCursorPos only writes the pointer position into the POINT it is given.
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x, p.y))
    }

    fn warp(x: i32, y: i32) {
        // SAFETY: moves the pointer; no memory is involved.
        let _ = unsafe { SetCursorPos(x, y) };
    }

    pub struct Scroller {
        rect: GrabRect,
        saved: Option<(i32, i32)>,
        /// Pixels per point of the screen the area is on: apps scroll in points.
        scale: f32,
    }

    impl Scroller {
        pub fn new(rect: GrabRect) -> Scroller {
            let scale = crate::capture::scale_at(rect.x + (rect.w / 2) as i32, rect.y + (rect.h / 2) as i32);
            Scroller { rect, saved: None, scale }
        }

        pub fn scroll(&mut self, dy: i32) -> Result<(), String> {
            let (px, py) = pointer().unwrap_or((i32::MIN, i32::MIN));
            if !pointer_inside(&self.rect, px as f64, py as f64) {
                self.saved.get_or_insert((px, py));
                let (cx, cy) = park_point(&self.rect);
                warp(cx.round() as i32, cy.round() as i32);
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            for c in chunks(dy) {
                let units = wheel_units(c, self.scale);
                if units == 0 {
                    continue;
                }
                let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: units as u32, dwFlags: MOUSEEVENTF_WHEEL, time: 0, dwExtraInfo: 0 } } };
                // SAFETY: one INPUT, of the size given; SendInput only reads it.
                let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
                if sent != 1 {
                    return Err("Can't scroll: Windows refused the wheel events. A window run as administrator takes none from ShotCraft: scroll it yourself (Preferences, Advanced, Scroll by hand).".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            Ok(())
        }
    }

    impl Drop for Scroller {
        fn drop(&mut self) {
            if let Some((x, y)) = self.saved.filter(|&(x, _)| x != i32::MIN) {
                warp(x, y);
            }
        }
    }
}

/// Wheel units for a scroll of `dy` pixels on a screen of `scale` pixels per point. A wheel notch
/// (120 units) moves most pages about 100 points; down is a negative delta.
#[cfg(target_os = "windows")]
pub fn wheel_units(dy: i32, scale: f32) -> i32 {
    -((dy as f32 / scale.max(0.5)) * 1.2).round() as i32
}

/// Linux: a placeholder until the Linux backend arrives; the user scrolls.
#[cfg(target_os = "linux")]
pub fn trusted(_prompt: bool) -> bool {
    false
}

#[cfg(target_os = "linux")]
pub fn screen_recording_ok() -> bool {
    true
}

#[cfg(target_os = "linux")]
pub struct Scroller;

#[cfg(target_os = "linux")]
impl Scroller {
    pub fn new(_rect: GrabRect) -> Scroller {
        Scroller
    }

    pub fn scroll(&mut self, _dy: i32) -> Result<(), String> {
        Err("Scrolling for you isn't available on Linux yet.".into())
    }
}

#[cfg(target_os = "windows")]
pub use win::{screen_recording_ok, trusted, Scroller};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scroll_is_cut_into_wheel_events_that_add_up() {
        for dy in [0, 1, 47, 48, 49, 300, -300, -1, 1000] {
            let parts = chunks(dy);
            assert_eq!(parts.iter().sum::<i32>(), dy);
            assert!(parts.iter().all(|c| c.abs() <= CHUNK && c.signum() == dy.signum()), "{dy}: {parts:?}");
        }
        assert!(chunks(0).is_empty());
    }

    #[test]
    fn the_pointer_is_parked_in_the_middle_and_counts_as_inside() {
        let r = GrabRect { x: 100, y: 200, w: 800, h: 400 };
        let (x, y) = park_point(&r);
        assert_eq!((x, y), (500.0, 400.0));
        assert!(pointer_inside(&r, x, y));
        assert!(!pointer_inside(&r, 99.0, 400.0));
        assert!(!pointer_inside(&r, 500.0, 601.0));
        // On the edge counts as outside: a wheel event there could go to the window next door.
        assert!(!pointer_inside(&r, 100.0, 400.0));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_wheel_units_follow_points_not_pixels() {
        // A notch (120) for about 100 points, down negative; at 150 % the same scroll is 150 pixels.
        assert_eq!(wheel_units(100, 1.0), -120);
        assert_eq!(wheel_units(150, 1.5), -120);
        assert_eq!(wheel_units(-48, 1.0), 58);
        assert_eq!(wheel_units(0, 2.0), 0);
        // Every chunk of a long scroll is sent, and they add up to about the whole.
        let total: i32 = chunks(700).iter().map(|&c| wheel_units(c, 1.0)).sum();
        assert!((total + 840).abs() <= chunks(700).len() as i32, "{total}");
    }
}
