//! Joins the frames of a scrolling page into one tall image.
//!
//! Pure Rust on top of the `image` crate and deterministic: the same frames always give the same
//! result, and nothing here knows about the screen.
//!
//! How it works:
//! * Every frame is reduced to one signature per pixel row: the grayscale average of 64 column blocks.
//! * The vertical offset between the previous frame and a new one is the shift that minimises the mean
//!   row difference over the rows both frames show (a coarse search on a subset of rows, then a check
//!   on all of them). A shift is only accepted when it clearly beats every unrelated shift, so flat or
//!   repetitive content gives "lost track" and never a wrong seam.
//! * Rows that are identical in the first two distinct frames at the top and the bottom are a fixed
//!   header and footer. They are kept once: the header stays on top, the footer goes at the very end.
//! * Only rows below everything already stitched are appended, so frames that do not move, or that
//!   scroll back up over known content, add nothing.
use image::RgbaImage;

/// The stitched image never grows past this many pixel rows.
pub const MAX_HEIGHT: u32 = 20_000;

/// Column blocks per row signature.
const BLOCKS: usize = 64;
/// Two rows are the same when their blocks differ by at most this much on average (0-255 gray levels).
const ROW_TOLERANCE: u32 = 3;
/// A shift is accepted when the mean difference of its overlap is at most this (coarse search).
const MAX_COARSE_COST: f32 = 6.0;
/// The best shift must beat every shift further than `NEIGHBOURHOOD` rows away by this much.
const MIN_MARGIN: f32 = 0.6;
const NEIGHBOURHOOD: i32 = 2;
/// After the search, at least this share of the overlapping rows must match.
const MIN_MATCHED: f32 = 0.85;
/// The overlap must contain at least this many rows with something to match on.
const MIN_INFORMATIVE_ROWS: usize = 4;
/// A row is textured when its brightest and darkest blocks are this far apart.
const TEXTURE_RANGE: u8 = 20;
/// Frames need at least this many rows between the fixed bands to be matched.
const MIN_BODY: usize = 16;
/// The header and the footer may each take at most this share of the frame height (percent).
const MAX_BAND_PERCENT: usize = 40;

/// What `Stitcher::push` did with a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Push {
    /// The first frame, taken as the start of the image.
    First,
    /// `rows` new rows were appended; the page moved `offset` rows down since the previous frame.
    Appended { rows: u32, offset: i32 },
    /// The frame matches the previous one: nothing moved.
    Unchanged,
    /// The page moved `offset` rows but everything the frame shows is already stitched (scrolled back up).
    Revisited { offset: i32 },
    /// The offset could not be found with confidence (scrolled too fast, flat or repetitive content).
    /// The frame is dropped and the previous one is still the reference.
    Lost,
    /// The frame has another size than the first one and is dropped.
    SizeMismatch,
    /// The height limit is reached; the frame is dropped.
    Capped,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StitchError {
    NoFrames,
}

/// One small signature per pixel row of a frame.
struct Sig {
    blocks: usize,
    rows: Vec<u8>,
}

impl Sig {
    fn new(img: &RgbaImage) -> Sig {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let blocks = w.clamp(1, BLOCKS);
        let raw = img.as_raw();
        let mut rows = vec![0u8; blocks * h];
        for y in 0..h {
            let row = &raw[y * w * 4..(y + 1) * w * 4];
            for b in 0..blocks {
                let (x0, x1) = (b * w / blocks, (b + 1) * w / blocks);
                let sum: u32 = row[x0 * 4..x1 * 4].chunks_exact(4).map(|p| (p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8).sum();
                rows[y * blocks + b] = (sum / (x1 - x0) as u32) as u8;
            }
        }
        Sig { blocks, rows }
    }

    fn row(&self, y: usize) -> &[u8] {
        &self.rows[y * self.blocks..(y + 1) * self.blocks]
    }

    /// Sum of the block differences between a row here and a row of `other`.
    fn diff(&self, y: usize, other: &Sig, other_y: usize) -> u32 {
        self.row(y).iter().zip(other.row(other_y)).map(|(a, b)| a.abs_diff(*b) as u32).sum()
    }

    fn same_row(&self, y: usize, other: &Sig, other_y: usize) -> bool {
        self.diff(y, other, other_y) <= ROW_TOLERANCE * self.blocks as u32
    }

    /// A row that helps to tell shifts apart: it has texture, or it differs from the row above it.
    fn informative(&self, y: usize) -> bool {
        let r = self.row(y);
        let range = r.iter().max().unwrap_or(&0) - r.iter().min().unwrap_or(&0);
        range >= TEXTURE_RANGE || (y > 0 && !self.same_row(y, self, y - 1))
    }
}

enum Estimate {
    /// The new frame shows the previous one moved by this many rows: `new[y] == prev[y + offset]`.
    Offset(i32),
    Same,
    Lost,
}

/// Finds how far the page moved between `prev` and `cur`, looking only at rows `top..bot`.
fn estimate(prev: &Sig, cur: &Sig, top: usize, bot: usize) -> Estimate {
    let body = bot.saturating_sub(top);
    if body < MIN_BODY {
        return Estimate::Lost;
    }
    let blocks = cur.blocks;
    let mismatches = (top..bot).filter(|&y| !cur.same_row(y, prev, y)).count();
    if mismatches * 200 <= body {
        return Estimate::Same;
    }
    let min_overlap = (body / 8).max(24).min(body - 1);
    let dmax = (body - min_overlap) as i32;
    let step = (body / 160).max(1);
    // Rows that carry information, counted per range: an overlap of blank rows matches anything and
    // proves nothing, so shifts with too few of them are not candidates.
    let mut informative = vec![0usize; cur.rows.len() / blocks + 1];
    for y in 0..informative.len() - 1 {
        informative[y + 1] = informative[y] + cur.informative(y) as usize;
    }
    let mut costs = Vec::with_capacity(2 * dmax as usize + 1);
    for d in -dmax..=dmax {
        let lo = top + (-d).max(0) as usize;
        let hi = bot - d.max(0) as usize;
        let (mut sum, mut n) = (0u64, 0u64);
        let mut y = lo;
        while y < hi {
            sum += cur.diff(y, prev, (y as i32 + d) as usize) as u64;
            n += 1;
            y += step;
        }
        let evidence = informative[hi] - informative[lo];
        costs.push(if evidence >= MIN_INFORMATIVE_ROWS { sum as f32 / (n.max(1) * blocks as u64) as f32 } else { f32::MAX });
    }
    let at = |i: usize| i as i32 - dmax;
    let mut best = 0usize;
    for i in 1..costs.len() {
        if costs[i] < costs[best] || (costs[i] == costs[best] && at(i).abs() < at(best).abs()) {
            best = i;
        }
    }
    let d = at(best);
    if costs[best] > MAX_COARSE_COST {
        return Estimate::Lost;
    }
    let runner_up = (0..costs.len()).filter(|&i| (at(i) - d).abs() > NEIGHBOURHOOD).map(|i| costs[i]).fold(f32::MAX, f32::min);
    if runner_up < costs[best] + MIN_MARGIN {
        return Estimate::Lost;
    }
    // Check the winner on every row of the overlap.
    let lo = top + (-d).max(0) as usize;
    let hi = bot - d.max(0) as usize;
    let rows = hi - lo;
    let matched = (lo..hi).filter(|&y| cur.same_row(y, prev, (y as i32 + d) as usize)).count();
    if (matched as f32) < MIN_MATCHED * rows as f32 {
        return Estimate::Lost;
    }
    if d == 0 {
        Estimate::Same
    } else {
        Estimate::Offset(d)
    }
}

/// Builds the image frame by frame. Feed it every captured frame in order, then call `finish`.
pub struct Stitcher {
    max_height: u32,
    size: Option<(u32, u32)>,
    /// The first frame, kept until a second one shows how far the page moved and which bands are fixed.
    first: Vec<u8>,
    first_sig: Option<Sig>,
    bands_known: bool,
    header: usize,
    footer: usize,
    /// Rows stitched so far: header and body, without the footer.
    out: Vec<u8>,
    out_rows: u32,
    prev_sig: Option<Sig>,
    /// Row of the stitched image where the previous frame starts. Negative above the first frame.
    pos: i64,
    footer_px: Vec<u8>,
    capped: bool,
    frames_used: u32,
}

impl Stitcher {
    pub fn new() -> Stitcher {
        Stitcher::with_max_height(MAX_HEIGHT)
    }

    pub fn with_max_height(max_height: u32) -> Stitcher {
        Stitcher {
            max_height,
            size: None,
            first: vec![],
            first_sig: None,
            bands_known: false,
            header: 0,
            footer: 0,
            out: vec![],
            out_rows: 0,
            prev_sig: None,
            pos: 0,
            footer_px: vec![],
            capped: false,
            frames_used: 0,
        }
    }

    /// Height of the image `finish` would return now.
    pub fn height(&self) -> u32 {
        match self.size {
            None => 0,
            Some((_, h)) if !self.bands_known => h.min(self.max_height),
            Some(_) => self.out_rows + self.footer as u32,
        }
    }

    /// Frames that counted: the first one and the ones that moved or confirmed the position.
    pub fn frames_used(&self) -> u32 {
        self.frames_used
    }

    pub fn is_capped(&self) -> bool {
        self.capped
    }

    pub fn push(&mut self, frame: &RgbaImage) -> Push {
        if self.capped {
            return Push::Capped;
        }
        let (w, h) = frame.dimensions();
        let Some((fw, fh)) = self.size else {
            if w == 0 || h == 0 {
                return Push::SizeMismatch;
            }
            self.size = Some((w, h));
            self.first = frame.as_raw().clone();
            self.first_sig = Some(Sig::new(frame));
            self.frames_used = 1;
            return Push::First;
        };
        if (w, h) != (fw, fh) {
            return Push::SizeMismatch;
        }
        let sig = Sig::new(frame);
        if self.bands_known {
            return self.push_known(frame, sig);
        }
        let first = self.first_sig.take().expect("the first frame is kept until the bands are known");
        let result = self.push_second(frame, sig, &first);
        if !self.bands_known {
            self.first_sig = Some(first);
        }
        result
    }

    /// The second distinct frame: find the fixed bands and the first offset.
    fn push_second(&mut self, frame: &RgbaImage, sig: Sig, first: &Sig) -> Push {
        let (_, h) = frame.dimensions();
        let h = h as usize;
        let cap = h * MAX_BAND_PERCENT / 100;
        let header = (0..cap).take_while(|&y| first.same_row(y, &sig, y)).count();
        let footer = (0..cap).take_while(|&i| first.same_row(h - 1 - i, &sig, h - 1 - i)).count();
        match estimate(first, &sig, header, h - footer) {
            Estimate::Same => Push::Unchanged,
            Estimate::Lost => Push::Lost,
            Estimate::Offset(d) => {
                self.bands_known = true;
                self.header = header;
                self.footer = footer;
                let w = frame.width() as usize;
                self.out = self.first[..(h - footer) * w * 4].to_vec();
                self.out.reserve(w * 4 * 256);
                self.out_rows = (h - footer) as u32;
                self.first = vec![];
                self.pos = 0;
                self.prev_sig = Some(Sig { blocks: first.blocks, rows: first.rows.clone() });
                self.apply_offset(frame, sig, d)
            }
        }
    }

    fn push_known(&mut self, frame: &RgbaImage, sig: Sig) -> Push {
        let h = frame.height() as usize;
        let prev = self.prev_sig.as_ref().expect("set with the bands");
        match estimate(prev, &sig, self.header, h - self.footer) {
            Estimate::Same => {
                self.frames_used += 1;
                self.remember_footer(frame);
                Push::Unchanged
            }
            Estimate::Lost => Push::Lost,
            Estimate::Offset(d) => self.apply_offset(frame, sig, d),
        }
    }

    fn remember_footer(&mut self, frame: &RgbaImage) {
        if self.footer > 0 {
            let (w, h) = (frame.width() as usize, frame.height() as usize);
            self.footer_px = frame.as_raw()[(h - self.footer) * w * 4..].to_vec();
        }
    }

    /// The page moved `d` rows since the previous frame: append what lies below the stitched image.
    fn apply_offset(&mut self, frame: &RgbaImage, sig: Sig, d: i32) -> Push {
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let new_pos = self.pos + d as i64;
        let body_start = new_pos + self.header as i64;
        let body_end = new_pos + (h - self.footer) as i64;
        let covered = self.out_rows as i64;
        if body_start > covered {
            // A gap between what is stitched and what the frame shows: the match cannot be right.
            return Push::Lost;
        }
        self.frames_used += 1;
        self.pos = new_pos;
        self.prev_sig = Some(sig);
        self.remember_footer(frame);
        if body_end <= covered {
            return Push::Revisited { offset: d };
        }
        let room = (self.max_height as usize).saturating_sub(self.footer).saturating_sub(self.out_rows as usize);
        let wanted = (body_end - covered) as usize;
        let rows = wanted.min(room);
        let from = (covered - new_pos) as usize;
        self.out.extend_from_slice(&frame.as_raw()[from * w * 4..(from + rows) * w * 4]);
        self.out_rows += rows as u32;
        if rows < wanted || room == rows {
            self.capped = true;
        }
        Push::Appended { rows: rows as u32, offset: d }
    }

    /// The stitched image: the first frame with everything the page showed below it, the fixed header
    /// on top and the fixed footer at the bottom.
    pub fn finish(self) -> Result<RgbaImage, StitchError> {
        let Some((w, h)) = self.size else { return Err(StitchError::NoFrames) };
        if !self.bands_known {
            let rows = h.min(self.max_height);
            let mut px = self.first;
            px.truncate(w as usize * rows as usize * 4);
            return Ok(RgbaImage::from_raw(w, rows, px).expect("rows of the first frame"));
        }
        let mut px = self.out;
        px.extend_from_slice(&self.footer_px);
        let rows = self.out_rows + self.footer as u32;
        Ok(RgbaImage::from_raw(w, rows, px).expect("rows of the stitched frames"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// Small deterministic generator (xorshift), so every run sees the same pictures.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 16) as u32
        }
    }

    fn hash(a: u32, b: u32, c: u32) -> u32 {
        let mut x = a.wrapping_mul(0x9e37_79b1) ^ b.wrapping_mul(0x85eb_ca6b) ^ c.wrapping_mul(0xc2b2_ae35);
        x ^= x >> 15;
        x = x.wrapping_mul(0x2c1b_3c6d);
        x ^= x >> 12;
        x
    }

    const DIGITS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b001, 0b001, 0b001],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];

    const LINE: u32 = 20;

    /// A page that looks like text: numbered lines of words, a picture-like block now and then.
    fn text_page(w: u32, h: u32, seed: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba([250, 250, 247, 255]));
        let mut rng = Rng(seed as u64 * 7919 + 17);
        let mut y = 0;
        let mut n = 0u32;
        while y + LINE <= h {
            n += 1;
            if n % 17 == 0 {
                // A block with a colour per band and noise, standing for an image.
                let tint = [(rng.next() % 200) as u8, (rng.next() % 200) as u8, (rng.next() % 200) as u8];
                for yy in y..(y + 3 * LINE).min(h) {
                    for x in 24..w - 24 {
                        let v = (hash(seed, x, yy) % 60) as u8;
                        img.put_pixel(x, yy, Rgba([tint[0].saturating_add(v), tint[1].saturating_add(v), tint[2].saturating_add(v), 255]));
                    }
                }
                y += 3 * LINE;
                n += 2;
                continue;
            }
            // Line number on the left, in 2x2 pixels per dot.
            for (i, digit) in format!("{n:04}").bytes().enumerate() {
                for (r, bits) in DIGITS[(digit - b'0') as usize].iter().enumerate() {
                    for c in 0..3u32 {
                        if bits >> (2 - c) & 1 == 1 {
                            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                                let (px, py) = (8 + i as u32 * 9 + c * 2 + dx, y + 4 + r as u32 * 2 + dy);
                                if px < w && py < h {
                                    img.put_pixel(px, py, Rgba([20, 20, 20, 255]));
                                }
                            }
                        }
                    }
                }
            }
            // Words of random width with glyph-like noise.
            let mut x = 60;
            while x + 12 < w {
                let word = 14 + rng.next() % 70;
                for xx in x..(x + word).min(w - 8) {
                    for yy in y + 4..y + 14 {
                        if hash(seed, xx / 2 + n * 1000, yy) % 100 < 55 {
                            img.put_pixel(xx, yy, Rgba([45, 45, 52, 255]));
                        }
                    }
                }
                x += word + 9;
            }
            y += LINE;
        }
        img
    }

    fn crop(page: &RgbaImage, y: u32, h: u32) -> RgbaImage {
        image::imageops::crop_imm(page, 0, y, page.width(), h).to_image()
    }

    fn paste(dst: &mut RgbaImage, src: &RgbaImage, y: u32) {
        image::imageops::replace(dst, src, 0, y as i64);
    }

    /// A browser-like bar: solid colour with a little pattern, the same in every frame.
    fn bar(w: u32, h: u32, color: [u8; 3]) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba([color[0], color[1], color[2], 255]));
        for y in 0..h {
            for x in 0..w {
                if hash(99, x / 3, y) % 100 < 6 {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                }
            }
        }
        img
    }

    fn feed(s: &mut Stitcher, page: &RgbaImage, ys: &[u32], h: u32) -> Vec<Push> {
        ys.iter().map(|&y| s.push(&crop(page, y, h))).collect()
    }

    fn rows(img: &RgbaImage, from: u32, to: u32) -> RgbaImage {
        crop(img, from, to - from)
    }

    /// Writes one PNG per case when SHOTCRAFT_STITCH_OUT names a folder: the input frames side by side
    /// on the left, the stitched result on the right.
    fn export_pngs(name: &str, frames: &[RgbaImage], result: &RgbaImage) {
        let Some(dir) = std::env::var_os("SHOTCRAFT_STITCH_OUT") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gap = 16;
        let (fw, fh) = frames[0].dimensions();
        let strip_w = frames.len() as u32 * (fw + gap) + gap;
        let mut sheet = RgbaImage::from_pixel(strip_w + result.width() + 2 * gap, result.height().max(fh) + 2 * gap, Rgba([70, 74, 82, 255]));
        for (i, f) in frames.iter().enumerate() {
            image::imageops::replace(&mut sheet, f, (gap + i as u32 * (fw + gap)) as i64, gap as i64);
        }
        image::imageops::replace(&mut sheet, result, (strip_w + gap) as i64, gap as i64);
        sheet.save(dir.join(format!("{name}.png"))).unwrap();
    }

    const W: u32 = 360;
    const FRAME_H: u32 = 300;

    #[test]
    fn known_offsets_give_the_original_page() {
        let page = text_page(W, 1500, 1);
        let ys = [0, 120, 240, 360, 480, 600, 720, 840, 960, 1080, 1200];
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &ys, FRAME_H);
        assert_eq!(pushes[0], Push::First);
        for p in &pushes[1..] {
            assert!(matches!(p, Push::Appended { rows: 120, offset: 120 }), "{p:?}");
        }
        let frames: Vec<RgbaImage> = ys.iter().map(|&y| crop(&page, y, FRAME_H)).collect();
        let out = s.finish().unwrap();
        assert_eq!(out, crop(&page, 0, 1500));
        export_pngs("stitch-scroll", &frames[..4], &out);
    }

    #[test]
    fn varying_offsets_including_tiny_and_large_steps() {
        let page = text_page(W, 2200, 2);
        let ys = [0, 3, 40, 41, 190, 200, 330, 331, 335, 560, 561, 700, 702, 940, 1100, 1340, 1341, 1580, 1830, 1900];
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &ys, FRAME_H);
        assert!(pushes.iter().all(|p| !matches!(p, Push::Lost)), "{pushes:?}");
        assert_eq!(s.height(), 1900 + FRAME_H);
        assert_eq!(s.finish().unwrap(), crop(&page, 0, 1900 + FRAME_H));
    }

    #[test]
    fn fixed_header_and_footer_are_kept_once() {
        let (header_h, footer_h) = (56, 40);
        let page = text_page(W, 1800, 3);
        let header = bar(W, header_h, [30, 90, 200]);
        let footer = bar(W, footer_h, [200, 120, 30]);
        let ys = [0, 90, 200, 330, 450, 600, 740, 880, 1000];
        let frames: Vec<RgbaImage> = ys
            .iter()
            .map(|&y| {
                let mut f = crop(&page, y, FRAME_H);
                paste(&mut f, &header, 0);
                paste(&mut f, &footer, FRAME_H - footer_h);
                f
            })
            .collect();
        let mut s = Stitcher::new();
        let pushes: Vec<Push> = frames.iter().map(|f| s.push(f)).collect();
        assert!(pushes.iter().all(|p| !matches!(p, Push::Lost | Push::SizeMismatch)), "{pushes:?}");
        let last = *ys.last().unwrap();
        let mut expected = RgbaImage::new(W, header_h + (last + FRAME_H - footer_h - header_h) + footer_h);
        paste(&mut expected, &header, 0);
        paste(&mut expected, &rows(&page, header_h, last + FRAME_H - footer_h), header_h);
        let footer_y = expected.height() - footer_h;
        paste(&mut expected, &footer, footer_y);
        let out = s.finish().unwrap();
        assert_eq!(out.dimensions(), expected.dimensions());
        assert!(out == expected, "stitched image differs from the page with one header and one footer");
        export_pngs("stitch-fixed-header-footer", &frames[..4], &out);
    }

    #[test]
    fn frames_without_movement_add_nothing() {
        let page = text_page(W, 900, 4);
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &[100, 100, 100], FRAME_H);
        assert_eq!(pushes, vec![Push::First, Push::Unchanged, Push::Unchanged]);
        assert_eq!(s.height(), FRAME_H);
        // Still before the first move, then it moves, then it stops again.
        let more = feed(&mut s, &page, &[100, 220, 220, 220, 340], FRAME_H);
        assert_eq!(
            more,
            vec![Push::Unchanged, Push::Appended { rows: 120, offset: 120 }, Push::Unchanged, Push::Unchanged, Push::Appended { rows: 120, offset: 120 }]
        );
        assert_eq!(s.finish().unwrap(), crop(&page, 100, 340 + FRAME_H - 100));
    }

    #[test]
    fn a_single_frame_comes_back_unchanged() {
        let page = text_page(W, 400, 5);
        let mut s = Stitcher::new();
        s.push(&crop(&page, 0, FRAME_H));
        assert_eq!(s.finish().unwrap(), crop(&page, 0, FRAME_H));
        assert_eq!(Stitcher::new().finish(), Err(StitchError::NoFrames));
    }

    #[test]
    fn scrolling_back_up_does_not_duplicate_rows() {
        let page = text_page(W, 1500, 6);
        let ys = [0, 100, 200, 300, 200, 100, 150, 300, 420, 560, 700];
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &ys, FRAME_H);
        assert!(matches!(pushes[4], Push::Revisited { offset: -100 }), "{:?}", pushes[4]);
        assert!(matches!(pushes[5], Push::Revisited { offset: -100 }));
        assert!(matches!(pushes[6], Push::Revisited { offset: 50 }));
        assert!(matches!(pushes[7], Push::Revisited { offset: 150 }));
        assert!(matches!(pushes[8], Push::Appended { rows: 120, .. }), "{:?}", pushes[8]);
        let frames: Vec<RgbaImage> = ys.iter().map(|&y| crop(&page, y, FRAME_H)).collect();
        let out = s.finish().unwrap();
        assert_eq!(out, crop(&page, 0, 700 + FRAME_H));
        export_pngs("stitch-scroll-back-up", &frames[2..6], &out);
    }

    #[test]
    fn a_jump_beyond_the_overlap_is_reported_and_leaves_no_seam() {
        let page = text_page(W, 1800, 7);
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &[0, 100, 900, 1000, 150, 250], FRAME_H);
        assert!(matches!(pushes[2], Push::Lost), "{:?}", pushes[2]);
        assert!(matches!(pushes[3], Push::Lost), "{:?}", pushes[3]);
        // Coming back within reach of the last good frame picks the thread up again.
        assert!(matches!(pushes[4], Push::Appended { rows: 50, offset: 50 }), "{:?}", pushes[4]);
        assert!(matches!(pushes[5], Push::Appended { rows: 100, .. }), "{:?}", pushes[5]);
        assert_eq!(s.finish().unwrap(), crop(&page, 0, 250 + FRAME_H));
    }

    #[test]
    fn flat_or_repetitive_content_is_not_guessed() {
        // A smooth vertical gradient: every shift looks nearly right.
        let gradient = |y0: u32| RgbaImage::from_fn(W, FRAME_H, |_, y| {
            let v = ((y0 + y) / 8).min(255) as u8;
            Rgba([v, v, v, 255])
        });
        let mut s = Stitcher::new();
        s.push(&gradient(0));
        assert_eq!(s.push(&gradient(40)), Push::Lost);

        // Evenly spaced stripes: the shift is only known up to the period.
        let stripes = |y0: u32| RgbaImage::from_fn(W, FRAME_H, |_, y| {
            let v = if (y0 + y) % 20 < 10 { 30 } else { 230 };
            Rgba([v, v, v, 255])
        });
        let mut s = Stitcher::new();
        s.push(&stripes(0));
        assert_eq!(s.push(&stripes(7)), Push::Lost);

        // A blank page with one thin line: nothing to anchor on.
        let mostly_blank = |y0: u32| RgbaImage::from_fn(W, FRAME_H, |_, y| if y0 + y == 500 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        let mut s = Stitcher::new();
        s.push(&mostly_blank(300));
        assert_eq!(s.push(&mostly_blank(340)), Push::Lost);
    }

    #[test]
    fn an_empty_page_does_not_look_like_movement() {
        let blank = RgbaImage::from_pixel(W, FRAME_H, Rgba([255, 255, 255, 255]));
        let mut s = Stitcher::new();
        assert_eq!(s.push(&blank), Push::First);
        assert_eq!(s.push(&blank), Push::Unchanged);
        assert_eq!(s.height(), FRAME_H);
    }

    #[test]
    fn small_changes_between_frames_are_tolerated() {
        // A cursor-sized patch and a little noise change from frame to frame.
        let page = text_page(W, 1000, 8);
        let ys = [0, 110, 230, 350, 470, 600];
        let mut s = Stitcher::new();
        for (i, &y) in ys.iter().enumerate() {
            let mut f = crop(&page, y, FRAME_H);
            for dy in 0..14 {
                for dx in 0..10 {
                    f.put_pixel(200 + dx, 120 + dy, Rgba([(i * 40) as u8, 0, 200, 255]));
                }
            }
            let p = s.push(&f);
            assert!(!matches!(p, Push::Lost), "frame {i}: {p:?}");
        }
        assert_eq!(s.height(), 600 + FRAME_H);
    }

    #[test]
    fn height_is_capped() {
        let page = text_page(W, 2000, 9);
        let ys: Vec<u32> = (0..=17).map(|i| i * 100).collect();
        let mut s = Stitcher::with_max_height(700);
        let pushes = feed(&mut s, &page, &ys, FRAME_H);
        assert!(s.is_capped());
        assert_eq!(s.height(), 700);
        assert!(matches!(pushes[3], Push::Appended { rows: 100, .. }));
        assert!(matches!(pushes[4], Push::Appended { rows: 100, .. }));
        assert_eq!(pushes[5], Push::Capped);
        assert_eq!(*pushes.last().unwrap(), Push::Capped);
        assert_eq!(s.finish().unwrap(), crop(&page, 0, 700));
    }

    #[test]
    fn the_cap_leaves_room_for_the_footer() {
        let footer_h = 30;
        let page = text_page(W, 2000, 10);
        let footer = bar(W, footer_h, [10, 140, 90]);
        let mut s = Stitcher::with_max_height(650);
        for y in (0..1500).step_by(100) {
            let mut f = crop(&page, y, FRAME_H);
            paste(&mut f, &footer, FRAME_H - footer_h);
            s.push(&f);
        }
        assert!(s.is_capped());
        let out = s.finish().unwrap();
        assert_eq!(out.height(), 650);
        assert_eq!(rows(&out, 650 - footer_h, 650), footer);
        assert_eq!(rows(&out, 0, 650 - footer_h), rows(&page, 0, 650 - footer_h));
    }

    #[test]
    fn the_default_limit_is_twenty_thousand_rows() {
        assert_eq!(MAX_HEIGHT, 20_000);
        assert_eq!(Stitcher::new().max_height, 20_000);
    }

    #[test]
    fn frames_of_another_size_are_ignored() {
        let page = text_page(W, 800, 11);
        let mut s = Stitcher::new();
        s.push(&crop(&page, 0, FRAME_H));
        assert_eq!(s.push(&crop(&page, 0, FRAME_H - 1)), Push::SizeMismatch);
        assert!(matches!(s.push(&crop(&page, 100, FRAME_H)), Push::Appended { .. }));
    }

    #[test]
    fn retina_sized_frames_stitch_in_reasonable_time() {
        // 1600 × 1200 frames scrolled by 180 rows, 20 of them: what a few seconds of scrolling looks like.
        let page = text_page(1600, 5000, 12);
        let mut s = Stitcher::new();
        let started = std::time::Instant::now();
        for i in 0..20 {
            let p = s.push(&crop(&page, i * 180, 1200));
            assert!(!matches!(p, Push::Lost), "frame {i}: {p:?}");
        }
        let per_frame = started.elapsed() / 20;
        assert_eq!(s.finish().unwrap(), crop(&page, 0, 19 * 180 + 1200));
        assert!(per_frame < std::time::Duration::from_millis(400), "{per_frame:?} per frame");
    }
}
