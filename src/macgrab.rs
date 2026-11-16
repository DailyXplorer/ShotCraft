//! macOS: one look at a part of the screen, taken inside the process.
//!
//! `screencapture` needs about 150 ms per picture: a process to start, a PNG to write and to read
//! back. A scrolling capture looks at the screen several times per scroll, so it asks Core Graphics
//! itself, which takes about 16 ms for a 900 x 700 point area on a Retina display.
//!
//! `CGDisplayCreateImageForRect` is deprecated: Apple's headers hide it from new code, but Core
//! Graphics still has it (macOS 27 included). It is looked up when first needed, so on a system
//! that no longer has it `area` returns `None` and the caller uses `screencapture`, as before.
use crate::scroll::GrabRect;
use image::RgbaImage;
use std::ffi::{c_char, c_void};
use std::sync::OnceLock;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    w: f64,
    h: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

type CGImageRef = *const c_void;
type CreateImageForRect = unsafe extern "C" fn(display: u32, rect: CGRect) -> CGImageRef;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetDisplaysWithRect(rect: CGRect, max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGImageGetWidth(image: CGImageRef) -> usize;
    fn CGImageGetHeight(image: CGImageRef) -> usize;
    fn CGImageGetBytesPerRow(image: CGImageRef) -> usize;
    fn CGImageGetBitsPerPixel(image: CGImageRef) -> usize;
    fn CGImageGetBitmapInfo(image: CGImageRef) -> u32;
    fn CGImageGetDataProvider(image: CGImageRef) -> *const c_void;
    fn CGDataProviderCopyData(provider: *const c_void) -> *const c_void;
    fn CGImageRelease(image: CGImageRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFDataGetLength(data: *const c_void) -> isize;
    fn CFRelease(cf: *const c_void);
}

extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// `RTLD_DEFAULT`: look in every image the process has loaded.
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

/// `kCGBitmapByteOrderMask` and `kCGBitmapByteOrder32Little`: with alpha first, the bytes of a pixel are B, G, R, A.
const BYTE_ORDER_MASK: u32 = 0x7000;
const BYTE_ORDER_32_LITTLE: u32 = 0x2000;
/// `kCGBitmapAlphaInfoMask`, and the alpha layouts that put alpha (or padding) first: premultiplied first, first, skip first.
const ALPHA_MASK: u32 = 0x1f;
const ALPHA_FIRST: [u32; 3] = [2, 4, 6];

fn create_image_for_rect() -> Option<CreateImageForRect> {
    static FOUND: OnceLock<Option<CreateImageForRect>> = OnceLock::new();
    *FOUND.get_or_init(|| {
        // SAFETY: dlsym only reads the name. A non-null result is the address of the C function of
        // that name, whose signature has been the same since macOS 10.6.
        let p = unsafe { dlsym(RTLD_DEFAULT, c"CGDisplayCreateImageForRect".as_ptr()) };
        (!p.is_null()).then(|| unsafe { std::mem::transmute::<*mut c_void, CreateImageForRect>(p) })
    })
}

/// The pixels of `rect` (points from the top left of the main display), or `None` when this way of
/// looking is not available: the function is gone, the area is not inside one display, or the
/// picture comes back in a layout this code does not read.
pub fn area(rect: &GrabRect) -> Option<RgbaImage> {
    let create = create_image_for_rect()?;
    let global = CGRect { origin: CGPoint { x: rect.x as f64, y: rect.y as f64 }, size: CGSize { w: rect.w as f64, h: rect.h as f64 } };
    let (mut ids, mut n) = ([0u32; 8], 0u32);
    // SAFETY: Core Graphics writes at most 8 ids into `ids` and their number into `n`.
    if unsafe { CGGetDisplaysWithRect(global, ids.len() as u32, ids.as_mut_ptr(), &mut n) } != 0 || n != 1 {
        return None;
    }
    // SAFETY: `ids[0]` is a display Core Graphics just named.
    let bounds = unsafe { CGDisplayBounds(ids[0]) };
    let local = CGRect { origin: CGPoint { x: global.origin.x - bounds.origin.x, y: global.origin.y - bounds.origin.y }, size: global.size };
    if !inside(&local, &bounds.size) {
        return None;
    }
    // SAFETY: the image is released below, on every path.
    let image = unsafe { create(ids[0], local) };
    if image.is_null() {
        return None;
    }
    let frame = unsafe { read_bgra(image) };
    // SAFETY: `image` came from a Create function and is not used after this.
    unsafe { CGImageRelease(image) };
    frame
}

/// Is the area, in points from the display's own top left, wholly on a display of this size?
fn inside(local: &CGRect, display: &CGSize) -> bool {
    local.origin.x >= 0.0 && local.origin.y >= 0.0 && local.origin.x + local.size.w <= display.w && local.origin.y + local.size.h <= display.h
}

/// Copies a 32-bit, alpha-first, little-endian picture (what the window server hands out) into RGBA.
///
/// SAFETY: `image` must be a live `CGImageRef`.
unsafe fn read_bgra(image: CGImageRef) -> Option<RgbaImage> {
    let (w, h, stride) = (CGImageGetWidth(image), CGImageGetHeight(image), CGImageGetBytesPerRow(image));
    let info = CGImageGetBitmapInfo(image);
    if w == 0 || h == 0 || CGImageGetBitsPerPixel(image) != 32 || info & BYTE_ORDER_MASK != BYTE_ORDER_32_LITTLE || !ALPHA_FIRST.contains(&(info & ALPHA_MASK)) {
        return None;
    }
    let data = CGDataProviderCopyData(CGImageGetDataProvider(image));
    if data.is_null() {
        return None;
    }
    let len = CFDataGetLength(data) as usize;
    let frame = if stride >= w * 4 && len >= stride * (h - 1) + w * 4 {
        let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data), len);
        Some(bgra_rows_to_rgba(bytes, w, h, stride))
    } else {
        None
    };
    CFRelease(data);
    frame
}

/// Rows of B, G, R, A pixels, `stride` bytes apart, as one opaque RGBA picture.
fn bgra_rows_to_rgba(bytes: &[u8], w: usize, h: usize, stride: usize) -> RgbaImage {
    let mut out = Vec::with_capacity(w * h * 4);
    for row in bytes.chunks(stride).take(h) {
        for p in row[..w * 4].chunks_exact(4) {
            out.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    RgbaImage::from_raw(w as u32, h as u32, out).expect("w * h pixels were written")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_bgra_rows_become_opaque_rgba() {
        // Two pixels per row, three rows, each row padded to 12 bytes as Core Graphics pads them.
        let mut bytes = Vec::new();
        for y in 0..3u8 {
            bytes.extend_from_slice(&[10 + y, 20 + y, 30 + y, 0, 40 + y, 50 + y, 60 + y, 7, 99, 99, 99, 99]);
        }
        let img = bgra_rows_to_rgba(&bytes, 2, 3, 12);
        assert_eq!(img.dimensions(), (2, 3));
        assert_eq!(img.get_pixel(0, 0).0, [30, 20, 10, 255]);
        assert_eq!(img.get_pixel(1, 2).0, [62, 52, 42, 255]);
    }

    #[test]
    fn the_last_row_may_come_without_its_padding() {
        let bytes = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8];
        let img = bgra_rows_to_rgba(&bytes, 1, 2, 8);
        assert_eq!(img.get_pixel(0, 1).0, [7, 6, 5, 255]);
    }

    #[test]
    fn an_area_that_leaves_its_display_is_not_taken_here() {
        let display = CGSize { w: 1512.0, h: 982.0 };
        let rect = |x: f64, y: f64, w: f64, h: f64| CGRect { origin: CGPoint { x, y }, size: CGSize { w, h } };
        assert!(inside(&rect(0.0, 0.0, 1512.0, 982.0), &display));
        assert!(inside(&rect(200.0, 150.0, 900.0, 700.0), &display));
        assert!(!inside(&rect(-1.0, 150.0, 900.0, 700.0), &display));
        assert!(!inside(&rect(700.0, 150.0, 900.0, 700.0), &display));
        assert!(!inside(&rect(200.0, 300.0, 900.0, 700.0), &display));
    }
}
