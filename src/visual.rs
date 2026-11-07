//! Visual regression checks: a picture the app produced is compared with a saved reference.
//!
//! A reference is a PNG under `tests/reference/`. A check passes when almost every pixel is within
//! a small tolerance of it (rasterizers differ by a level or two between processors). A failure
//! leaves the new picture and a red-marked difference under `target/visual-diffs/`, to look at.
//! Set `SHOTCRAFT_UPDATE_REFERENCES=1` to write the references instead of checking them, then
//! look at what was written before committing it.
//!
//! This file is also compiled into the window tests (`tests/ui_snapshots.rs`), so it only needs
//! the `image` crate.
#![allow(dead_code)]
use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};

/// How different two same-sized pictures are.
#[derive(Debug, PartialEq)]
pub struct Diff {
    /// The largest difference of any colour channel, 0 to 255.
    pub max: u8,
    /// Pixels with a channel off by more than the tolerance, and all pixels.
    pub differing: usize,
    pub total: usize,
}

impl Diff {
    pub fn fraction(&self) -> f64 {
        self.differing as f64 / self.total.max(1) as f64
    }
}

/// None when the sizes differ (then nothing can be compared pixel by pixel).
pub fn diff(a: &RgbaImage, b: &RgbaImage, tolerance: u8) -> Option<Diff> {
    if a.dimensions() != b.dimensions() {
        return None;
    }
    let (mut max, mut differing) = (0u8, 0usize);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let d = p.0.iter().zip(q.0.iter()).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        max = max.max(d);
        if d > tolerance {
            differing += 1;
        }
    }
    Some(Diff { max, differing, total: a.pixels().len() })
}

/// The new picture with the pixels that differ marked red and the rest dimmed.
fn highlight(a: &RgbaImage, b: &RgbaImage, tolerance: u8) -> RgbaImage {
    let mut out = RgbaImage::new(a.width().max(b.width()), a.height().max(b.height()));
    for (x, y, px) in out.enumerate_pixels_mut() {
        *px = match (a.get_pixel_checked(x, y), b.get_pixel_checked(x, y)) {
            (Some(p), Some(q)) if p.0.iter().zip(q.0.iter()).all(|(i, j)| i.abs_diff(*j) <= tolerance) => Rgba([p[0] / 3 + 170, p[1] / 3 + 170, p[2] / 3 + 170, 255]),
            _ => Rgba([255, 0, 0, 255]),
        };
    }
    out
}

fn updating() -> bool {
    std::env::var_os("SHOTCRAFT_UPDATE_REFERENCES").is_some_and(|v| !v.is_empty())
}

/// Where failures are written: the build's `target/visual-diffs`.
fn diffs_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("visual-diffs")
}

/// Compares `actual` with `dir/name.png`. `tolerance` is the channel difference below which a
/// pixel counts as the same; `allowed` the fraction of pixels that may still differ.
pub fn check(name: &str, actual: &RgbaImage, dir: &Path, tolerance: u8, allowed: f64) -> Result<(), String> {
    let reference = dir.join(format!("{name}.png"));
    if updating() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        actual.save(&reference).map_err(|e| format!("{}: {e}", reference.display()))?;
        return Ok(());
    }
    let Ok(want) = image::open(&reference) else {
        return Err(format!("{name}: no reference at {}; run with SHOTCRAFT_UPDATE_REFERENCES=1 to create it", reference.display()));
    };
    let want = want.to_rgba8();
    let fail = |why: String, marked: Option<RgbaImage>| {
        let out = diffs_dir();
        let _ = std::fs::create_dir_all(&out);
        let _ = actual.save(out.join(format!("{name}.actual.png")));
        if let Some(m) = marked {
            let _ = m.save(out.join(format!("{name}.diff.png")));
        }
        Err(format!("{name}: {why} (see {})", out.display()))
    };
    match diff(actual, &want, tolerance) {
        None => fail(format!("the picture is {}x{} but the reference is {}x{} (references are for 100% display scaling)", actual.width(), actual.height(), want.width(), want.height()), None),
        Some(d) if d.fraction() > allowed => fail(
            format!("{} of {} pixels differ ({:.3}%, allowed {:.3}%), the largest by {}", d.differing, d.total, d.fraction() * 100.0, allowed * 100.0, d.max),
            Some(highlight(actual, &want, tolerance)),
        ),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shotcraft-visual-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn same_pictures_do_not_differ_and_small_steps_stay_within_tolerance() {
        let a = flat(10, 10, [100, 100, 100, 255]);
        assert_eq!(diff(&a, &a, 0), Some(Diff { max: 0, differing: 0, total: 100 }));
        let b = flat(10, 10, [103, 98, 100, 255]);
        let d = diff(&a, &b, 3).unwrap();
        assert_eq!((d.max, d.differing), (3, 0), "within tolerance");
        assert_eq!(diff(&a, &b, 2).unwrap().differing, 100, "just past it");
    }

    #[test]
    fn pictures_of_other_sizes_cannot_be_compared() {
        assert_eq!(diff(&flat(10, 10, [0; 4]), &flat(10, 11, [0; 4]), 0), None);
    }

    #[test]
    fn a_check_passes_against_its_reference_and_fails_on_a_change() {
        let dir = scratch("check");
        let good = flat(20, 20, [10, 20, 30, 255]);
        good.save(dir.join("pic.png")).unwrap();
        assert!(check("pic", &good, &dir, 2, 0.0).is_ok());

        let mut changed = good.clone();
        for x in 0..10 {
            changed.put_pixel(x, 0, Rgba([255, 255, 255, 255]));
        }
        let e = check("pic", &changed, &dir, 2, 0.01).unwrap_err();
        assert!(e.contains("10 of 400 pixels differ"), "{e}");
        assert!(check("pic", &changed, &dir, 2, 0.05).is_ok(), "a change under the allowed fraction passes");
        let e = check("pic", &flat(21, 20, [10, 20, 30, 255]), &dir, 2, 1.0).unwrap_err();
        assert!(e.contains("21x20") && e.contains("20x20"), "{e}");
        let e = check("missing", &good, &dir, 2, 0.0).unwrap_err();
        assert!(e.contains("SHOTCRAFT_UPDATE_REFERENCES"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_difference_picture_marks_changed_pixels_red() {
        let a = flat(4, 1, [0, 0, 0, 255]);
        let mut b = a.clone();
        b.put_pixel(2, 0, Rgba([200, 0, 0, 255]));
        let m = highlight(&a, &b, 5);
        assert_eq!(*m.get_pixel(2, 0), Rgba([255, 0, 0, 255]));
        assert_ne!(*m.get_pixel(0, 0), Rgba([255, 0, 0, 255]));
    }
}
