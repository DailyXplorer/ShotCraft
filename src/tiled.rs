//! Pictures larger than the GPU's texture limit, shown as a grid of textures.
//!
//! A texture side can't exceed `max_texture_side` (egui-wgpu asks the device for 8192, egui_glow
//! asserts against `GL_MAX_TEXTURE_SIZE`), and a scrolling capture is easily taller than that.
//! `plan` cuts a picture into tiles that fit; each tile carries one extra pixel of its neighbours so
//! that smooth scaling shows no seam. The full-resolution pixels stay in the document: this only
//! affects what is drawn on screen.
use crate::model::R;
use eframe::egui::{self, pos2, Color32, ColorImage, Context, Painter, Rect, TextureHandle, TextureOptions};

/// Pixels of a neighbouring tile each tile also holds.
const MARGIN: u32 = 1;

/// A half-open pixel rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PxRect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl PxRect {
    fn w(&self) -> u32 {
        self.x1 - self.x0
    }
    fn h(&self) -> u32 {
        self.y1 - self.y0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tile {
    /// The pixels this tile is responsible for. The cores of all tiles partition the picture.
    pub core: PxRect,
    /// What the texture holds: the core plus the margin, clamped to the picture.
    pub pad: PxRect,
}

/// Cuts a `w × h` picture into tiles whose textures are at most `max_side` pixels on a side.
pub fn plan(w: u32, h: u32, max_side: u32) -> Vec<Tile> {
    if w == 0 || h == 0 {
        return vec![];
    }
    let step = max_side.saturating_sub(2 * MARGIN).max(1);
    let (step_x, step_y) = if w <= max_side { (w, h.min(step).max(1)) } else { (step, step) };
    // A picture that fits in one texture needs no margin.
    if w <= max_side && h <= max_side {
        let all = PxRect { x0: 0, y0: 0, x1: w, y1: h };
        return vec![Tile { core: all, pad: all }];
    }
    let mut tiles = vec![];
    let mut y = 0;
    while y < h {
        let y1 = (y + step_y).min(h);
        let mut x = 0;
        while x < w {
            let x1 = (x + step_x).min(w);
            let core = PxRect { x0: x, y0: y, x1, y1 };
            let pad = PxRect { x0: x.saturating_sub(MARGIN), y0: y.saturating_sub(MARGIN), x1: (x1 + MARGIN).min(w), y1: (y1 + MARGIN).min(h) };
            tiles.push(Tile { core, pad });
            x = x1;
        }
        y = y1;
    }
    tiles
}

/// The part of `src` (picture pixels) that `tile` draws, with its texture coordinates (u0, v0, u1, v1).
pub fn visible(tile: &Tile, src: &R) -> Option<(R, [f32; 4])> {
    let c = tile.core;
    let sub = R { x0: src.x0.max(c.x0 as f32), y0: src.y0.max(c.y0 as f32), x1: src.x1.min(c.x1 as f32), y1: src.y1.min(c.y1 as f32) };
    if sub.x1 <= sub.x0 || sub.y1 <= sub.y0 {
        return None;
    }
    let p = tile.pad;
    let (pw, ph) = (p.w() as f32, p.h() as f32);
    Some((sub, [(sub.x0 - p.x0 as f32) / pw, (sub.y0 - p.y0 as f32) / ph, (sub.x1 - p.x0 as f32) / pw, (sub.y1 - p.y0 as f32) / ph]))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Alpha {
    Straight,
    Premultiplied,
}

/// The device's texture side limit as egui reports it.
pub fn max_side(ctx: &Context) -> u32 {
    ctx.input(|i| i.max_texture_side).clamp(64, u32::MAX as usize) as u32
}

pub struct TiledTexture {
    size: (u32, u32),
    max_side: u32,
    tiles: Vec<(Tile, TextureHandle)>,
}

fn tile_image(px: &[u8], width: u32, pad: PxRect, alpha: Alpha) -> ColorImage {
    let (pw, ph) = (pad.w() as usize, pad.h() as usize);
    let stride = width as usize * 4;
    let mut buf = Vec::with_capacity(pw * ph * 4);
    for y in pad.y0..pad.y1 {
        let start = y as usize * stride + pad.x0 as usize * 4;
        buf.extend_from_slice(&px[start..start + pw * 4]);
    }
    match alpha {
        Alpha::Straight => ColorImage::from_rgba_unmultiplied([pw, ph], &buf),
        Alpha::Premultiplied => ColorImage::from_rgba_premultiplied([pw, ph], &buf),
    }
}

/// True when the `pad` rectangle holds the same bytes in both buffers.
fn same_pixels(a: &[u8], b: &[u8], width: u32, pad: PxRect) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let stride = width as usize * 4;
    (pad.y0..pad.y1).all(|y| {
        let start = y as usize * stride + pad.x0 as usize * 4;
        let end = start + pad.w() as usize * 4;
        a[start..end] == b[start..end]
    })
}

impl TiledTexture {
    pub fn new(ctx: &Context, name: &str, size: (u32, u32), px: &[u8], alpha: Alpha, options: TextureOptions) -> TiledTexture {
        let max_side = max_side(ctx);
        let tiles = plan(size.0, size.1, max_side)
            .into_iter()
            .enumerate()
            .map(|(i, tile)| {
                let img = tile_image(px, size.0, tile.pad, alpha);
                (tile, ctx.load_texture(format!("{name}-{i}"), img, options))
            })
            .collect();
        TiledTexture { size, max_side, tiles }
    }

    /// True when new pixels of this size can be written into the existing textures.
    pub fn fits(&self, ctx: &Context, size: (u32, u32)) -> bool {
        self.size == size && self.max_side == max_side(ctx)
    }

    /// Writes new pixels into the textures. Tiles whose pixels equal `old` (the previous contents)
    /// are not uploaded again, so editing a tall picture only touches the tiles that changed.
    pub fn update(&mut self, old: Option<&[u8]>, px: &[u8], alpha: Alpha, options: TextureOptions) {
        for (tile, handle) in &mut self.tiles {
            if old.is_some_and(|o| same_pixels(o, px, self.size.0, tile.pad)) {
                continue;
            }
            handle.set(tile_image(px, self.size.0, tile.pad, alpha), options);
        }
    }

    /// Draws the part of the picture inside `src` (picture pixels); `to_screen` places picture
    /// rectangles on the screen.
    pub fn paint(&self, painter: &Painter, src: &R, to_screen: impl Fn(&R) -> Rect, tint: Color32) {
        for (tile, handle) in &self.tiles {
            if let Some((sub, uv)) = visible(tile, src) {
                let uv = egui::Rect::from_min_max(pos2(uv[0], uv[1]), pos2(uv[2], uv[3]));
                painter.image(handle.id(), to_screen(&sub), uv, tint);
            }
        }
    }

    /// Draws the whole picture into `rect`.
    pub fn paint_in(&self, painter: &Painter, rect: Rect, tint: Color32) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let all = R { x0: 0.0, y0: 0.0, x1: w, y1: h };
        self.paint(
            painter,
            &all,
            |r| Rect::from_min_max(pos2(rect.min.x + r.x0 / w * rect.width(), rect.min.y + r.y0 / h * rect.height()), pos2(rect.min.x + r.x1 / w * rect.width(), rect.min.y + r.y1 / h * rect.height())),
            tint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_partition(w: u32, h: u32, max_side: u32) -> Vec<Tile> {
        let tiles = plan(w, h, max_side);
        // Every pixel belongs to exactly one core; every texture fits.
        let mut covered = vec![0u8; (w * h) as usize];
        for t in &tiles {
            assert!(t.pad.w() <= max_side && t.pad.h() <= max_side, "{t:?} exceeds {max_side}");
            assert!(t.pad.x0 <= t.core.x0 && t.pad.y0 <= t.core.y0 && t.pad.x1 >= t.core.x1 && t.pad.y1 >= t.core.y1);
            assert!(t.core.x1 <= w && t.core.y1 <= h);
            for y in t.core.y0..t.core.y1 {
                for x in t.core.x0..t.core.x1 {
                    covered[(y * w + x) as usize] += 1;
                }
            }
        }
        assert!(covered.iter().all(|c| *c == 1), "cores must partition the picture");
        tiles
    }

    #[test]
    fn a_picture_that_fits_is_one_tile() {
        let tiles = plan(1200, 800, 2048);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].core, tiles[0].pad);
        assert_eq!(plan(2048, 2048, 2048).len(), 1);
    }

    #[test]
    fn nothing_to_draw_gives_no_tiles() {
        assert!(plan(0, 100, 2048).is_empty());
        assert!(plan(100, 0, 2048).is_empty());
    }

    #[test]
    fn a_tall_picture_is_cut_into_bands_within_the_limit() {
        // The default scrolling capture limit, on a device limited to 8192 like egui-wgpu asks for.
        let tiles = plan(1000, crate::stitch::MAX_HEIGHT, 8192);
        assert_eq!(tiles.len(), 3);
        assert!(tiles.iter().all(|t| t.pad.h() <= 8192 && t.pad.w() == 1000));
        // And on a small limit (2048) the same picture is still cut to fit.
        let tiles = plan(1000, crate::stitch::MAX_HEIGHT, 2048);
        assert_eq!(tiles.len(), 10);
        assert!(tiles.iter().all(|t| t.pad.h() <= 2048));
    }

    #[test]
    fn cores_partition_and_textures_fit() {
        for (w, h, max) in [(100, 100, 64), (130, 70, 64), (64, 200, 64), (65, 65, 64), (7, 300, 16), (300, 7, 16), (33, 129, 8), (1, 1000, 16)] {
            let tiles = check_partition(w, h, max);
            assert!(tiles.len() > 1 || (w <= max && h <= max), "{w}x{h} at {max}");
        }
    }

    #[test]
    fn a_wide_and_tall_picture_gets_a_grid() {
        let tiles = check_partition(150, 150, 64);
        // 62 pixels per core → 3 × 3 tiles.
        assert_eq!(tiles.len(), 9);
    }

    #[test]
    fn the_single_texture_path_is_over_the_limit_exactly_when_a_side_exceeds_it() {
        // What the editor used to upload: one texture of the whole picture. With the default wgpu
        // backend egui requests a device whose `max_texture_dimension_2d` is 8192 (egui-wgpu
        // `setup.rs`), so a 20 000 px tall capture fails texture creation; the glow backend
        // asserts `w <= max_texture_side && h <= max_texture_side` in `egui_glow::Painter::set_texture`.
        let single_texture_fits = |w: u32, h: u32, max: u32| w <= max && h <= max;
        assert!(single_texture_fits(1440, 900, 8192));
        assert!(!single_texture_fits(1000, crate::stitch::MAX_HEIGHT, 8192));
        assert!(!single_texture_fits(1000, 2049, 2048));
        assert!(plan(1000, crate::stitch::MAX_HEIGHT, 8192).iter().all(|t| single_texture_fits(t.pad.w(), t.pad.h(), 8192)));
    }

    #[test]
    fn visible_parts_cover_the_view_exactly_once() {
        let (w, h) = (100u32, 230u32);
        let tiles = plan(w, h, 64);
        for view in [R { x0: 0.0, y0: 0.0, x1: 100.0, y1: 230.0 }, R { x0: 13.5, y0: 40.0, x1: 77.25, y1: 190.5 }, R { x0: 0.0, y0: 62.0, x1: 100.0, y1: 124.0 }] {
            let mut area = 0.0;
            for t in &tiles {
                if let Some((sub, uv)) = visible(t, &view) {
                    area += sub.w() * sub.h();
                    assert!(uv.iter().all(|c| (0.0..=1.0).contains(c)), "{uv:?}");
                    assert!(uv[2] > uv[0] && uv[3] > uv[1]);
                }
            }
            assert!((area - view.w() * view.h()).abs() < 1e-3, "{area} vs {}", view.w() * view.h());
        }
    }

    #[test]
    fn texture_coordinates_point_at_the_same_pixels_as_the_picture() {
        let tiles = plan(100, 230, 64);
        let tile = tiles.iter().find(|t| t.core.y0 == 62 && t.core.x0 == 0).unwrap();
        // The tile starts one pixel above its core, so the core's top edge is 1 / pad height down.
        let (sub, uv) = visible(tile, &R { x0: 0.0, y0: 0.0, x1: 100.0, y1: 230.0 }).unwrap();
        assert_eq!((sub.y0, sub.y1), (62.0, 124.0));
        assert!((uv[1] - 1.0 / tile.pad.h() as f32).abs() < 1e-6);
        assert!((uv[3] - 63.0 / tile.pad.h() as f32).abs() < 1e-6);
    }

    #[test]
    fn only_tiles_with_changed_pixels_are_reuploaded() {
        let (w, h) = (10u32, 20u32);
        let a = vec![0u8; (w * h * 4) as usize];
        let mut b = a.clone();
        b[(15 * w * 4 + 3 * 4) as usize] = 9; // pixel (3, 15)
        let tiles = plan(w, h, 8);
        let changed: Vec<bool> = tiles.iter().map(|t| !same_pixels(&a, &b, w, t.pad)).collect();
        // Cores are 6 px; the pixel sits in one core, and in the margin of its neighbours only.
        for (t, c) in tiles.iter().zip(&changed) {
            let inside = t.pad.x0 <= 3 && 3 < t.pad.x1 && t.pad.y0 <= 15 && 15 < t.pad.y1;
            assert_eq!(*c, inside, "{t:?}");
        }
        assert!(changed.iter().any(|c| *c) && changed.iter().any(|c| !*c));
        assert!(same_pixels(&a, &a, w, tiles[0].pad));
    }

    #[test]
    fn tile_pixels_come_from_the_right_place() {
        let (w, h) = (10u32, 12u32);
        let px: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, (i / 251) as u8, 7, 255]).collect();
        let tiles = plan(w, h, 6);
        for t in &tiles {
            let img = tile_image(&px, w, t.pad, Alpha::Straight);
            assert_eq!(img.size, [t.pad.w() as usize, t.pad.h() as usize]);
            let first = img.pixels[0];
            let i = t.pad.y0 * w + t.pad.x0;
            assert_eq!(first.r(), (i % 251) as u8);
        }
    }
}
