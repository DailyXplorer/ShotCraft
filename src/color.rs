//! Colour formats and contrast measures of the colour inspector. Pure functions, no UI.
//!
//! Everything is written from the public specifications:
//! * sRGB transfer curve: IEC 61966-2-1 (the 0.04045 breakpoint).
//! * WCAG 2.x contrast ratio and relative luminance: W3C WCAG 2.x, "contrast ratio" and
//!   "relative luminance" definitions.
//! * OKLCH: Björn Ottosson, "A perceptual color space for image processing" (2020), the
//!   linear-sRGB → LMS → Oklab matrices, then polar form (CSS Color Module 4 uses the same).
//! * APCA Lc: APCA-W3 0.0.98G (SAPC, the constants below are the published ones).
use crate::model::{Image, Rgba, R};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Which contrast measure the status bar shows first.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ContrastType {
    #[default]
    Wcag2,
    Apca,
}

impl ContrastType {
    pub fn toggled(self) -> ContrastType {
        match self {
            ContrastType::Wcag2 => ContrastType::Apca,
            ContrastType::Apca => ContrastType::Wcag2,
        }
    }
}

// ---------------------------------------------------------------- formats

/// Names shown in the colour format list. The index is what `Settings::color_format` stores:
/// new formats are only ever appended so saved settings keep their meaning.
pub const COLOR_FORMATS: [&str; 9] = [
    "HEX #FFC200",
    "HEX FFC200",
    "RGB 255, 194, 0",
    "RGB (1.00, 0.76, 0.00)",
    "UIColor (Swift)",
    "CSS rgba()",
    "C# Color",
    "Android 0xFFFFC200",
    "OKLCH",
];

pub fn format_color(c: Rgba, fmt: usize) -> String {
    let f = |v: u8| v as f32 / 255.0;
    match fmt {
        1 => format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
        2 => format!("{}, {}, {}", c[0], c[1], c[2]),
        3 => format!("({:.2}, {:.2}, {:.2})", f(c[0]), f(c[1]), f(c[2])),
        4 => format!("UIColor(red: {:.2}, green: {:.2}, blue: {:.2}, alpha: 1)", f(c[0]), f(c[1]), f(c[2])),
        5 => format!("rgba({}, {}, {}, 1)", c[0], c[1], c[2]),
        6 => format!("Color.FromArgb({}, {}, {});", c[0], c[1], c[2]),
        7 => format!("0xFF{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
        8 => {
            let (l, ch, h) = oklch(c);
            format!("oklch({}% {} {})", trim(l * 100.0, 1), trim(ch, 3), trim(h, 1))
        }
        _ => format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
    }
}

/// `v` with `digits` decimals and no trailing zeros ("0.250" → "0.25", "100.0" → "100").
fn trim(v: f64, digits: usize) -> String {
    let s = format!("{v:.digits$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

// ------------------------------------------------------------- conversions

/// sRGB 8-bit channel → linear light 0..1.
fn linear(v: u8) -> f64 {
    let c = v as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB colour → OKLCH: lightness 0..1, chroma ≥ 0, hue in degrees 0..360.
/// Chroma below 1e-4 is rounding noise of the matrices: it is reported as 0 with hue 0.
pub fn oklch(c: Rgba) -> (f64, f64, f64) {
    let (r, g, b) = (linear(c[0]), linear(c[1]), linear(c[2]));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let ll = 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let bb = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    let chroma = a.hypot(bb);
    if chroma < 1e-4 {
        return (ll, 0.0, 0.0);
    }
    (ll, chroma, bb.atan2(a).to_degrees().rem_euclid(360.0))
}

// ---------------------------------------------------------------- contrast

/// WCAG 2.x relative luminance (0 = black, 1 = white).
pub fn wcag_luminance(c: Rgba) -> f64 {
    0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
}

/// WCAG 2.x contrast ratio, 1..21. Symmetric: which colour is the text does not matter.
pub fn contrast_wcag(a: Rgba, b: Rgba) -> f64 {
    let (la, lb) = (wcag_luminance(a), wcag_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

// APCA-W3 0.0.98G constants.
const APCA_TRC: f64 = 2.4;
const APCA_COEF: [f64; 3] = [0.2126729, 0.7151522, 0.0721750];
const APCA_NORM_BG: f64 = 0.56;
const APCA_NORM_TXT: f64 = 0.57;
const APCA_REV_TXT: f64 = 0.62;
const APCA_REV_BG: f64 = 0.65;
const APCA_BLK_THRS: f64 = 0.022;
const APCA_BLK_CLMP: f64 = 1.414;
const APCA_SCALE: f64 = 1.14;
const APCA_LO_OFFSET: f64 = 0.027;
const APCA_DELTA_Y_MIN: f64 = 0.0005;
const APCA_LO_CLIP: f64 = 0.1;

/// APCA "screen luminance" Y of an sRGB colour (plain 2.4 power curve, not the piecewise one).
fn apca_y(c: Rgba) -> f64 {
    let ch = |v: u8| (v as f64 / 255.0).powf(APCA_TRC);
    APCA_COEF[0] * ch(c[0]) + APCA_COEF[1] * ch(c[1]) + APCA_COEF[2] * ch(c[2])
}

/// APCA lightness contrast Lc of `text` on `bg`, about -108..106. Positive = dark text on a
/// lighter background, negative = light text on a darker one, 0 = too close to read.
pub fn contrast_apca(text: Rgba, bg: Rgba) -> f64 {
    let soft = |y: f64| if y >= APCA_BLK_THRS { y } else { y + (APCA_BLK_THRS - y).powf(APCA_BLK_CLMP) };
    let (yt, yb) = (soft(apca_y(text)), soft(apca_y(bg)));
    if (yb - yt).abs() < APCA_DELTA_Y_MIN {
        return 0.0;
    }
    let out = if yb > yt {
        let sapc = (yb.powf(APCA_NORM_BG) - yt.powf(APCA_NORM_TXT)) * APCA_SCALE;
        if sapc < APCA_LO_CLIP { 0.0 } else { sapc - APCA_LO_OFFSET }
    } else {
        let sapc = (yb.powf(APCA_REV_BG) - yt.powf(APCA_REV_TXT)) * APCA_SCALE;
        if sapc > -APCA_LO_CLIP { 0.0 } else { sapc + APCA_LO_OFFSET }
    };
    out * 100.0
}

/// The two colours a selection is about: `(text, background)`. The background is the most
/// frequent colour of the selection, the text is the frequent colour farthest from it (the
/// anti-aliased edge pixels of text are in between and rarer than its core). A flat selection
/// gives the same colour twice. At most ~40 000 pixels are looked at, on an even grid.
pub fn contrast_pair(img: &Image, r: &R) -> Option<(Rgba, Rgba)> {
    let r = r.clamp_to(img.w as f32, img.h as f32);
    let (x0, y0, x1, y1) = (r.x0 as i32, r.y0 as i32, r.x1.ceil() as i32, r.y1.ceil() as i32);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let area = (x1 - x0) as i64 * (y1 - y0) as i64;
    let step = ((area as f64 / 40_000.0).sqrt().ceil() as i32).max(1);
    let mut hist: HashMap<[u8; 3], u32> = HashMap::new();
    let mut total = 0u32;
    for y in (y0..y1).step_by(step as usize) {
        for x in (x0..x1).step_by(step as usize) {
            let c = img.get(x, y);
            *hist.entry([c[0], c[1], c[2]]).or_insert(0) += 1;
            total += 1;
        }
    }
    // Ties are broken by colour value so the answer does not depend on hash order.
    let bg = hist.iter().max_by_key(|(c, n)| (**n, **c)).map(|(c, _)| *c)?;
    let min_count = (total / 200).max(1);
    let dist = |c: &[u8; 3]| (0..3).map(|i| (c[i] as i32 - bg[i] as i32).pow(2)).sum::<i32>();
    let fg = hist.iter().filter(|(_, n)| **n >= min_count).max_by_key(|(c, _)| (dist(c), **c)).map(|(c, _)| *c).unwrap_or(bg);
    Some(([fg[0], fg[1], fg[2], 255], [bg[0], bg[1], bg[2], 255]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const BLACK: Rgba = [0, 0, 0, 255];
    const WHITE: Rgba = [255, 255, 255, 255];

    fn close(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() <= eps, "{a} vs {b} (eps {eps})");
    }

    #[test]
    fn existing_formats_keep_their_text() {
        let c = [255, 194, 0, 255];
        assert_eq!(format_color(c, 0), "#FFC200");
        assert_eq!(format_color(c, 1), "FFC200");
        assert_eq!(format_color(c, 2), "255, 194, 0");
        assert_eq!(format_color(c, 3), "(1.00, 0.76, 0.00)");
        assert_eq!(format_color(c, 4), "UIColor(red: 1.00, green: 0.76, blue: 0.00, alpha: 1)");
        assert_eq!(format_color(c, 5), "rgba(255, 194, 0, 1)");
        assert_eq!(format_color(c, 6), "Color.FromArgb(255, 194, 0);");
        assert_eq!(format_color(c, 7), "0xFFFFC200");
        assert_eq!(format_color(c, 99), "#FFC200", "unknown index falls back to hex");
        assert_eq!(COLOR_FORMATS.len(), 9);
    }

    /// Ottosson's reference table (bottom of "A perceptual color space for image processing",
    /// also the oklch() values of the CSS named colours red/lime/blue in CSS Color 4):
    /// red 0.627955 0.257683 29.2339, lime 0.866440 0.294827 142.4953, blue 0.452014 0.313214 264.0520.
    #[test]
    fn oklch_matches_published_primaries() {
        for (rgb, l, c, h) in [
            ([255, 0, 0, 255], 0.627955, 0.257683, 29.2339),
            ([0, 255, 0, 255], 0.866440, 0.294827, 142.4953),
            ([0, 0, 255, 255], 0.452014, 0.313214, 264.0520),
        ] {
            let (ol, oc, oh) = oklch(rgb);
            close(ol, l, 5e-5);
            close(oc, c, 5e-5);
            close(oh, h, 5e-2);
        }
    }

    /// White is oklch(1 0 0), black oklch(0 0 0), mid greys have no chroma and hue 0.
    #[test]
    fn achromatic_colours_have_no_chroma_or_hue() {
        assert_eq!(format_color(WHITE, 8), "oklch(100% 0 0)");
        assert_eq!(format_color(BLACK, 8), "oklch(0% 0 0)");
        assert_eq!(oklch([128, 128, 128, 255]).1, 0.0);
        assert_eq!(oklch([128, 128, 128, 255]).2, 0.0);
    }

    #[test]
    fn oklch_text_uses_percent_lightness_and_short_numbers() {
        assert_eq!(format_color([255, 0, 0, 255], 8), "oklch(62.8% 0.258 29.2)");
        assert_eq!(format_color([0, 0, 255, 255], 8), "oklch(45.2% 0.313 264.1)");
    }

    #[test]
    fn hue_stays_in_zero_to_360() {
        for c in [[255, 0, 128, 255], [10, 200, 30, 255], [250, 10, 10, 255], [0, 40, 255, 255]] {
            let h = oklch(c).2;
            assert!((0.0..360.0).contains(&h), "{h}");
        }
    }

    /// WCAG 2.x: black on white is 21:1 (W3C understanding-contrast-minimum), white on white 1:1.
    #[test]
    fn wcag_extremes() {
        close(contrast_wcag(BLACK, WHITE), 21.0, 1e-9);
        close(contrast_wcag(WHITE, BLACK), 21.0, 1e-9);
        close(contrast_wcag(WHITE, WHITE), 1.0, 1e-12);
    }

    /// WebAIM contrast checker values on white: #767676 → 4.54:1 (the lightest grey that passes
    /// AA for body text), #777777 → 4.48:1 (fails), #595959 → 7.00:1 (AAA).
    #[test]
    fn wcag_matches_webaim_greys() {
        close(contrast_wcag([0x76, 0x76, 0x76, 255], WHITE), 4.54, 5e-3);
        close(contrast_wcag([0x77, 0x77, 0x77, 255], WHITE), 4.48, 5e-3);
        close(contrast_wcag([0x59, 0x59, 0x59, 255], WHITE), 7.00, 5e-3);
    }

    /// W3C relative luminance definition: pure red 0.2126, green 0.7152, blue 0.0722.
    #[test]
    fn wcag_luminance_uses_the_published_weights() {
        close(wcag_luminance([255, 0, 0, 255]), 0.2126, 1e-9);
        close(wcag_luminance([0, 255, 0, 255]), 0.7152, 1e-9);
        close(wcag_luminance([0, 0, 255, 255]), 0.0722, 1e-9);
    }

    /// APCA-W3 0.0.98G reference outputs (SAPC / apca-w3 README and independent ports):
    /// black text on white = 106.04067321268862, white text on black = -107.88473318309848.
    #[test]
    fn apca_matches_published_black_white() {
        close(contrast_apca(BLACK, WHITE), 106.04067321268862, 1e-9);
        close(contrast_apca(WHITE, BLACK), -107.88473318309848, 1e-9);
    }

    #[test]
    fn apca_is_zero_for_identical_or_barely_different_colours() {
        assert_eq!(contrast_apca(WHITE, WHITE), 0.0);
        assert_eq!(contrast_apca([200, 200, 200, 255], [201, 201, 201, 255]), 0.0);
    }

    #[test]
    fn apca_is_not_symmetric_and_signs_follow_polarity() {
        let grey = [0x88, 0x88, 0x88, 255];
        let dark_on_light = contrast_apca(grey, WHITE);
        let light_on_dark = contrast_apca(WHITE, grey);
        assert!(dark_on_light > 0.0 && light_on_dark < 0.0);
        assert!((dark_on_light + light_on_dark).abs() > 1.0);
    }

    fn two_tone(bg: Rgba, fg: Rgba) -> Image {
        // 40×20, mostly bg, a 6×6 block of fg and a 1 px anti-aliased fringe of the mix.
        let (w, h) = (40u32, 20u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let c = if (10..16).contains(&x) && (5..11).contains(&y) {
                    fg
                } else if x == 16 && (5..11).contains(&y) {
                    [(bg[0] / 2 + fg[0] / 2), (bg[1] / 2 + fg[1] / 2), (bg[2] / 2 + fg[2] / 2), 255]
                } else {
                    bg
                };
                px.extend_from_slice(&c);
            }
        }
        Image { w, h, px: Arc::new(px) }
    }

    #[test]
    fn pair_is_the_common_colour_and_the_farthest_frequent_one() {
        let img = two_tone([250, 250, 250, 255], [20, 30, 40, 255]);
        let all = R { x0: 0.0, y0: 0.0, x1: 40.0, y1: 20.0 };
        assert_eq!(contrast_pair(&img, &all), Some(([20, 30, 40, 255], [250, 250, 250, 255])));
        let dark = two_tone([20, 30, 40, 255], [250, 250, 250, 255]);
        assert_eq!(contrast_pair(&dark, &all), Some(([250, 250, 250, 255], [20, 30, 40, 255])));
        // Part of the selection that holds only the background: same colour twice.
        let flat = R { x0: 20.0, y0: 0.0, x1: 40.0, y1: 20.0 };
        assert_eq!(contrast_pair(&img, &flat), Some(([250, 250, 250, 255], [250, 250, 250, 255])));
    }

    #[test]
    fn pair_of_an_empty_or_outside_selection_is_none() {
        let img = two_tone(WHITE, BLACK);
        assert_eq!(contrast_pair(&img, &R { x0: 5.0, y0: 5.0, x1: 5.0, y1: 9.0 }), None);
        assert_eq!(contrast_pair(&img, &R { x0: 100.0, y0: 100.0, x1: 120.0, y1: 120.0 }), None);
    }

    #[test]
    fn contrast_type_toggles_and_old_settings_default_to_wcag() {
        assert_eq!(ContrastType::default(), ContrastType::Wcag2);
        assert_eq!(ContrastType::Wcag2.toggled(), ContrastType::Apca);
        assert_eq!(ContrastType::Apca.toggled(), ContrastType::Wcag2);
    }

    #[test]
    fn settings_saved_before_contrast_existed_still_load() {
        let s: crate::settings::Settings = serde_json::from_str(r#"{"color_format": 3}"#).unwrap();
        assert_eq!((s.color_format, s.contrast_type), (3, ContrastType::Wcag2));
        let back: crate::settings::Settings = serde_json::from_str(&serde_json::to_string(&{
            let mut s = s;
            s.contrast_type = ContrastType::Apca;
            s
        })
        .unwrap())
        .unwrap();
        assert_eq!(back.contrast_type, ContrastType::Apca);
    }

    /// Optional: `SHOTCRAFT_TEST_OUT=dir SHOTCRAFT_TEST_TAG=after cargo test color_inspector_for_the_eyes`
    /// draws the status bar with a selection (and the format popover) for a visual check.
    #[test]
    fn color_inspector_for_the_eyes() {
        use crate::editor::Popover;
        let Some(dir) = std::env::var_os("SHOTCRAFT_TEST_OUT").map(std::path::PathBuf::from) else { return };
        let tag = std::env::var("SHOTCRAFT_TEST_TAG").unwrap_or_else(|_| "shot".into());
        // White page with mid-grey #767676 "text" bars: 4.54:1 / Lc 71.
        let (w, h) = (480u32, 200u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let bar = (40..440).contains(&x) && (0..8).any(|i| (30 + i * 18..38 + i * 18).contains(&y));
                px.extend_from_slice(if bar { &[0x76, 0x76, 0x76, 255] } else { &[255, 255, 255, 255] });
            }
        }
        let img = Image { w, h, px: Arc::new(px) };
        let size = (1100.0, 560.0);
        for (name, fmt, ct, popover) in [
            ("wcag", 8usize, ContrastType::Wcag2, false),
            ("apca", 8, ContrastType::Apca, false),
            ("popover", 8, ContrastType::Wcag2, true),
        ] {
            crate::headless::with_rig(|rig| {
                let mut app = rig.app();
                app.doc = Some(crate::model::Doc::new(img.clone(), 2.0));
                app.settings.color_format = fmt;
                app.settings.contrast_type = ct;
                app.ed.hover_color = [255, 194, 0, 255];
                app.ed.selection = Some(R { x0: 0.0, y0: 0.0, x1: w as f32, y1: h as f32 });
                if popover {
                    let y = size.1 - crate::editor::STATUS_H;
                    app.ed.popover = Popover::Color(eframe::egui::Rect::from_min_size(eframe::egui::pos2(8.0, y), eframe::egui::vec2(190.0, crate::editor::STATUS_H)));
                }
                let shot = rig.render_editor(&mut app, size, 2.0, false);
                crate::headless::save_png(&shot, &dir.join(format!("{tag}-{name}.png")));
            });
        }
    }
}
