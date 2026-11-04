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

/// A column (of the 64) that has detail counts as fixed when this share of its rows is unchanged...
const FIXED_COLUMN_PERCENT: usize = 98;
/// ... and as moving when less than this share is.
const MOVING_COLUMN_PERCENT: usize = 90;
/// Columns are only blanked when at least this many are fixed and at least this many move.
const MIN_FIXED_COLUMNS: usize = 2;
const MIN_MOVING_COLUMNS: usize = 4;

/// A hint narrows the search to this fraction of the body height (1/n) on each side, at least `HINT_MIN_TOLERANCE` rows.
const HINT_TOLERANCE_DIV: usize = 20;
const HINT_MIN_TOLERANCE: usize = 6;

/// Which way the page grows while it is captured.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Direction {
    /// The capture starts at the top of the page and scrolls down.
    #[default]
    Down,
    /// The capture starts at the bottom of the page and scrolls up (a chat, a feed that loads older items).
    /// The frames are turned upside down on the way in and the result on the way out, so that the
    /// page is always followed in one direction: its rows are compared whole, which is the same
    /// either way up. Offsets, hints and "above the start" then count in the direction of scrolling.
    Up,
}

impl Direction {
    /// The sign of a scroll in this direction: positive scrolls down.
    pub fn sign(self) -> i32 {
        match self {
            Direction::Down => 1,
            Direction::Up => -1,
        }
    }
}

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
    /// Per row: does it help to tell shifts apart (see `informative`)?
    inf: Vec<bool>,
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
        let mut sig = Sig { blocks, rows, inf: vec![] };
        sig.inf = (0..h).map(|y| sig.informative(y)).collect();
        sig
    }

    /// The same signature with some columns set to one grey, in every row: they match any shift and tell none apart.
    fn blanked(&self, columns: &[bool]) -> Sig {
        let mut rows = self.rows.clone();
        for (i, v) in rows.iter_mut().enumerate() {
            if columns[i % self.blocks] {
                *v = 128;
            }
        }
        let mut sig = Sig { blocks: self.blocks, rows, inf: vec![] };
        sig.inf = (0..sig.rows.len() / sig.blocks).map(|y| sig.informative(y)).collect();
        sig
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

/// A column that has detail and does not move at all between two frames belongs to something fixed
/// next to the scrolling part (a side panel, a pane that scrolls on its own): the rows of the whole
/// frame then never line up, whatever the shift. Such columns are blanked in both signatures, so the
/// shift is found on the columns that move. Nothing changes unless some columns move and some are fixed.
fn without_fixed_columns(prev: &Sig, cur: &Sig, top: usize, bot: usize) -> Option<(Sig, Sig)> {
    let (blocks, body) = (cur.blocks, bot - top);
    let mut fixed = vec![false; blocks];
    let (mut n_fixed, mut n_moving) = (0, 0);
    for b in 0..blocks {
        let col = |s: &Sig, y: usize| s.rows[y * blocks + b];
        let (lo, hi) = (top..bot).fold((255u8, 0u8), |(lo, hi), y| (lo.min(col(cur, y)), hi.max(col(cur, y))));
        let detail = (top..bot).filter(|&y| col(cur, y).abs_diff(col(cur, top)) > ROW_TOLERANCE as u8).count() * 20 >= body;
        if hi - lo < TEXTURE_RANGE || !detail {
            continue;
        }
        let same = (top..bot).filter(|&y| col(cur, y).abs_diff(col(prev, y)) <= ROW_TOLERANCE as u8).count();
        if same * 100 >= body * FIXED_COLUMN_PERCENT {
            fixed[b] = true;
            n_fixed += 1;
        } else if same * 100 < body * MOVING_COLUMN_PERCENT {
            n_moving += 1;
        }
    }
    if n_fixed < MIN_FIXED_COLUMNS || n_moving < MIN_MOVING_COLUMNS {
        return None;
    }
    Some((prev.blanked(&fixed), cur.blanked(&fixed)))
}

/// Finds how far the page moved between `prev` and `cur`, looking only at rows `top..bot`.
fn estimate(prev: &Sig, cur: &Sig, top: usize, bot: usize, hint: Option<i32>) -> Estimate {
    if bot.saturating_sub(top) >= MIN_BODY {
        if let Some((p, c)) = without_fixed_columns(prev, cur, top, bot) {
            return estimate_columns(&p, &c, top, bot, hint);
        }
    }
    estimate_columns(prev, cur, top, bot, hint)
}

fn estimate_columns(prev: &Sig, cur: &Sig, top: usize, bot: usize, hint: Option<i32>) -> Estimate {
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
    // An overlap of blank rows matches anything and proves nothing: only rows that carry information
    // in both frames count as evidence, and a shift needs enough of them to be a candidate.
    let mut costs = Vec::with_capacity(2 * dmax as usize + 1);
    for d in -dmax..=dmax {
        let lo = top + (-d).max(0) as usize;
        let hi = bot - d.max(0) as usize;
        let (mut sum, mut n, mut evidence) = (0u64, 0u64, 0usize);
        let mut y = lo;
        while y < hi {
            let py = (y as i32 + d) as usize;
            sum += cur.diff(y, prev, py) as u64;
            evidence += (cur.inf[y] && prev.inf[py]) as usize;
            n += 1;
            y += step;
        }
        costs.push(if evidence >= MIN_INFORMATIVE_ROWS { sum as f32 / (n.max(1) * blocks as u64) as f32 } else { f32::MAX });
    }
    let at = |i: usize| i as i32 - dmax;
    // With a hint (the scroll the caller asked for), only shifts near it are candidates, and among the
    // ones that fit equally well the closest to the hint wins: that is what tells the right row of a
    // list of look-alike rows from its neighbours. When nothing near the hint fits, the search is open.
    if let Some(h) = hint.filter(|h| h.abs() <= dmax) {
        let tol = ((body / HINT_TOLERANCE_DIV).max(HINT_MIN_TOLERANCE)) as i32;
        let near: Vec<usize> = (0..costs.len()).filter(|&i| (at(i) - h).abs() <= tol).collect();
        if let Some(&floor) = near.iter().min_by(|&&a, &&b| costs[a].total_cmp(&costs[b])) {
            if costs[floor] <= MAX_COARSE_COST {
                let pick = near.iter().copied().filter(|&i| costs[i] <= costs[floor] + MIN_MARGIN).min_by_key(|&i| (at(i) - h).abs()).unwrap_or(floor);
                if let Some(e) = verify(prev, cur, top, bot, at(pick)) {
                    return e;
                }
            }
        }
    }
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
    verify(prev, cur, top, bot, d).unwrap_or(Estimate::Lost)
}

/// Checks a candidate shift on every row of the overlap.
fn verify(prev: &Sig, cur: &Sig, top: usize, bot: usize, d: i32) -> Option<Estimate> {
    // Check the winner on every row of the overlap.
    let lo = top + (-d).max(0) as usize;
    let hi = bot - d.max(0) as usize;
    let rows = hi - lo;
    let both = |y: usize| cur.inf[y] && prev.inf[(y as i32 + d) as usize];
    let matched = (lo..hi).filter(|&y| cur.same_row(y, prev, (y as i32 + d) as usize)).count();
    let (matched_inf, total_inf) = (lo..hi).filter(|&y| both(y)).fold((0, 0), |(m, t), y| (m + cur.same_row(y, prev, (y as i32 + d) as usize) as usize, t + 1));
    if (matched as f32) < MIN_MATCHED * rows as f32 || (matched_inf as f32) < MIN_MATCHED * total_inf as f32 {
        return None;
    }
    Some(if d == 0 { Estimate::Same } else { Estimate::Offset(d) })
}

/// True when two frames of the same size show the same picture, give or take a few pixels of noise
/// (a blinking caret, a video corner). Used to tell when a page has stopped moving.
pub fn frames_match(a: &RgbaImage, b: &RgbaImage) -> bool {
    if a.dimensions() != b.dimensions() {
        return false;
    }
    let (w, h) = (a.width() as usize, a.height() as usize);
    let (ra, rb) = (a.as_raw(), b.as_raw());
    let (mut seen, mut differ) = (0usize, 0usize);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let i = (y * w + x) * 4;
            seen += 1;
            differ += (0..3).any(|c| ra[i + c].abs_diff(rb[i + c]) > 8) as usize;
        }
    }
    differ * 500 <= seen
}

/// Builds the image frame by frame (see `Direction` for scrolling up). Feed it every captured frame in order, then call `finish`.
pub struct Stitcher {
    direction: Direction,
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
    above_start: bool,
    frames_used: u32,
}

impl Stitcher {
    pub fn new() -> Stitcher {
        Stitcher::with_max_height(MAX_HEIGHT)
    }

    pub fn with_max_height(max_height: u32) -> Stitcher {
        Stitcher::with_direction(max_height, Direction::Down)
    }

    pub fn with_direction(max_height: u32, direction: Direction) -> Stitcher {
        Stitcher {
            direction,
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
            above_start: false,
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

    /// True when the page was scrolled above the first frame: that part is not part of the image.
    pub fn above_start(&self) -> bool {
        self.above_start
    }

    pub fn is_capped(&self) -> bool {
        self.capped
    }

    pub fn push(&mut self, frame: &RgbaImage) -> Push {
        self.push_hinted(frame, None)
    }

    /// Like `push`, with the number of rows the page is expected to have moved since the previous
    /// frame (positive = down). The hint only picks between shifts that fit the pixels equally well;
    /// it never makes a bad fit acceptable.
    pub fn push_hinted(&mut self, frame: &RgbaImage, hint: Option<i32>) -> Push {
        match self.direction {
            Direction::Down => self.push_down(frame, hint),
            Direction::Up => self.push_down(&image::imageops::flip_vertical(frame), hint),
        }
    }

    fn push_down(&mut self, frame: &RgbaImage, hint: Option<i32>) -> Push {
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
            return self.push_known(frame, sig, hint);
        }
        let first = self.first_sig.take().expect("the first frame is kept until the bands are known");
        let result = self.push_second(frame, sig, &first, hint);
        if !self.bands_known {
            self.first_sig = Some(first);
        }
        result
    }

    /// The second distinct frame: find the fixed bands and the first offset.
    fn push_second(&mut self, frame: &RgbaImage, sig: Sig, first: &Sig, hint: Option<i32>) -> Push {
        let (_, h) = frame.dimensions();
        let h = h as usize;
        let cap = h * MAX_BAND_PERCENT / 100;
        let header = (0..cap).take_while(|&y| first.same_row(y, &sig, y)).count();
        let footer = (0..cap).take_while(|&i| first.same_row(h - 1 - i, &sig, h - 1 - i)).count();
        // A fixed bar ends in an edge. Blank rows that merely look alike in both frames (page margins)
        // continue past the band, so they are not a bar.
        let header = if header > 0 && first.same_row(header - 1, first, header) { 0 } else { header };
        let footer = if footer > 0 && first.same_row(h - footer, first, h - footer - 1) { 0 } else { footer };
        match estimate(first, &sig, header, h - footer, hint) {
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
                self.prev_sig = Some(Sig { blocks: first.blocks, rows: first.rows.clone(), inf: first.inf.clone() });
                self.apply_offset(frame, sig, d)
            }
        }
    }

    fn push_known(&mut self, frame: &RgbaImage, sig: Sig, hint: Option<i32>) -> Push {
        let h = frame.height() as usize;
        let prev = self.prev_sig.as_ref().expect("set with the bands");
        match estimate(prev, &sig, self.header, h - self.footer, hint) {
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
        self.above_start |= new_pos < 0;
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
        let direction = self.direction;
        let img = self.finish_down()?;
        Ok(match direction {
            Direction::Down => img,
            Direction::Up => image::imageops::flip_vertical(&img),
        })
    }

    fn finish_down(self) -> Result<RgbaImage, StitchError> {
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

    /// A mail-list-like page: rows of equal height, each with the same icons at the same places and
    /// different text. A shift by a whole number of rows lines the icons up, which is what makes this
    /// the hard case. Every `repeat`th row is an exact copy of the one before it.
    fn inbox_page(w: u32, rows: u32, row_h: u32, seed: u32, repeat: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, rows * row_h, Rgba([255, 255, 255, 255]));
        for r in 0..rows {
            let id = if repeat > 0 && r % repeat == repeat - 1 { r - 1 } else { r };
            let y0 = r * row_h;
            // Separator line, checkbox and star: identical in every row.
            for x in 0..w {
                img.put_pixel(x, y0 + row_h - 1, Rgba([225, 228, 232, 255]));
            }
            for (x0, x1) in [(14, 26), (40, 52)] {
                for y in y0 + row_h / 2 - 6..y0 + row_h / 2 + 6 {
                    for x in x0..x1 {
                        img.put_pixel(x, y, Rgba([150, 150, 156, 255]));
                    }
                }
            }
            // Sender, subject and date: words of different widths, row by row.
            for (x_from, x_to, dark) in [(70, 150, 30u8), (170, w - 90, 90), (w - 70, w - 12, 120)] {
                let mut x = x_from;
                let mut k = 0;
                while x + 6 < x_to {
                    let word = 12 + hash(seed, id, k) % 46;
                    for xx in x..(x + word).min(x_to) {
                        for yy in y0 + row_h / 2 - 5..y0 + row_h / 2 + 5 {
                            if hash(seed ^ 0x55, xx / 2 + id * 977, yy) % 100 < 58 {
                                img.put_pixel(xx, yy, Rgba([dark, dark, dark.saturating_add(8), 255]));
                            }
                        }
                    }
                    x += word + 8;
                    k += 1;
                }
            }
        }
        img
    }

    fn label(p: &Push) -> String {
        match p {
            Push::First => "first".into(),
            Push::Appended { offset, .. } => format!("+{offset}"),
            other => format!("{other:?}"),
        }
    }

    /// Frames of a list of look-alike rows, scrolled by every step from a few rows to nearly a whole
    /// frame. Every step must be found exactly: a shift by a whole number of rows would line the
    /// icons up as well, which is what makes this the hard case.
    #[test]
    fn look_alike_rows_are_followed_for_every_scroll_step() {
        let page = inbox_page(W, 200, 40, 5, 0);
        for step in [4u32, 10, 20, 39, 40, 41, 60, 80, 100, 120, 150, 200, 240] {
            for hint in [None, Some(step as i32)] {
                let mut s = Stitcher::new();
                let mut got = vec![];
                for k in 0..8u32 {
                    let f = crop(&page, k * step, 492);
                    got.push(if k == 0 { s.push(&f) } else { s.push_hinted(&f, hint) });
                }
                assert!(got[1..].iter().all(|p| matches!(p, Push::Appended { offset, .. } if *offset == step as i32)), "step {step} hint {hint:?}: {:?}", got.iter().map(label).collect::<Vec<_>>());
                assert_eq!(s.finish().unwrap(), crop(&page, 0, 7 * step + 492), "step {step} hint {hint:?}");
            }
        }
    }

    /// The same list where every second row is an exact copy of the one above: only the words around
    /// differ, and rows repeat with period 2. A hint breaks the tie in favour of the real scroll.
    #[test]
    fn a_hint_picks_the_real_shift_among_exact_repeats() {
        let page = inbox_page(W, 120, 40, 7, 2);
        let step = 200;
        let mut s = Stitcher::new();
        s.push(&crop(&page, 0, 492));
        for k in 1..8u32 {
            let r = s.push_hinted(&crop(&page, k * step, 492), Some(step as i32));
            assert!(matches!(r, Push::Appended { offset, .. } if offset == step as i32), "{k}: {r:?}");
        }
        assert_eq!(s.finish().unwrap(), crop(&page, 0, 7 * step + 492));
    }

    /// Web mail looks like this: a bar on top, a side column and a margin on the right that stay put
    /// while only the list moves. Every row of the frame then differs from the one before in the
    /// list columns and matches in the others, so no whole-row shift ever matches.
    #[test]
    fn a_list_that_scrolls_between_fixed_panels_is_followed() {
        let (w, fh) = (1100u32, 984u32);
        let page = inbox_page(w, 140, 80, 5, 0);
        let side = text_page(220, fh, 9);
        let top = bar(w, 120, [240, 242, 245]);
        let right = text_page(70, fh, 11);
        for step in [20u32, 60, 100, 200, 300, 500] {
            let mut s = Stitcher::new();
            let mut got = vec![];
            for k in 0..7u32 {
                let mut f = crop(&page, k * step, fh);
                paste(&mut f, &side, 0);
                paste(&mut f, &right, w - 70);
                paste(&mut f, &top, 0);
                got.push(if k == 0 { s.push(&f) } else { s.push_hinted(&f, Some(step as i32)) });
            }
            assert!(got[1..].iter().all(|p| matches!(p, Push::Appended { offset, .. } if *offset == step as i32)), "step {step}: {:?}", got.iter().map(label).collect::<Vec<_>>());
            assert_eq!(s.frames_used(), 7);
        }
    }

    /// Retina frames, as `screencapture` writes them: twice as many pixel rows as points.
    #[test]
    fn retina_scaled_frames_are_followed_with_a_step_in_points() {
        let page = inbox_page(W * 2, 120, 80, 3, 0);
        let step_px = 2 * 150;
        let mut s = Stitcher::new();
        s.push(&crop(&page, 0, 984));
        for k in 1..6u32 {
            let r = s.push_hinted(&crop(&page, k * step_px, 984), Some(step_px as i32));
            assert!(matches!(r, Push::Appended { offset, .. } if offset == step_px as i32), "{k}: {r:?}");
        }
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
    fn blank_page_margins_are_not_taken_for_a_fixed_header() {
        let mut page = RgbaImage::from_pixel(W, 1700, Rgba([250, 250, 247, 255]));
        paste(&mut page, &text_page(W, 1450, 14), 250);
        let ys = [0, 120, 240, 360, 480, 600, 720];
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &ys, FRAME_H);
        assert!(pushes[1..].iter().all(|p| matches!(p, Push::Appended { rows: 120, .. })), "{pushes:?}");
        assert_eq!(s.finish().unwrap(), crop(&page, 0, 720 + FRAME_H));
    }

    #[test]
    fn a_blank_first_frame_gives_no_anchor() {
        let mut page = RgbaImage::from_pixel(W, 1200, Rgba([250, 250, 247, 255]));
        paste(&mut page, &text_page(W, 900, 15), 300);
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &[0, 20, 250], FRAME_H);
        // Frame 3 shows a few text rows against a blank reference: not enough to place it.
        assert_eq!(pushes[0], Push::First);
        assert!(pushes[2..].iter().all(|p| matches!(p, Push::Lost)), "{pushes:?}");
        assert_eq!(s.height(), FRAME_H);
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
    fn scrolling_above_the_first_frame_is_flagged_and_not_added() {
        let page = text_page(W, 1200, 13);
        let mut s = Stitcher::new();
        let pushes = feed(&mut s, &page, &[300, 400, 250, 150, 300, 420], FRAME_H);
        assert!(s.above_start());
        assert!(matches!(pushes[3], Push::Revisited { offset: -100 }), "{:?}", pushes[3]);
        assert_eq!(s.finish().unwrap(), crop(&page, 300, 120 + FRAME_H));
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

    // Scrolling up: the capture starts at the bottom of the page and the page grows upwards.

    fn up() -> Stitcher {
        Stitcher::with_direction(MAX_HEIGHT, Direction::Up)
    }

    #[test]
    fn scrolling_up_gives_the_original_page() {
        let page = text_page(W, 1500, 21);
        let ys = [1200, 1080, 960, 840, 720, 600, 480, 360, 240, 120, 0];
        let mut s = up();
        let pushes = feed(&mut s, &page, &ys, FRAME_H);
        assert_eq!(pushes[0], Push::First);
        for p in &pushes[1..] {
            assert!(matches!(p, Push::Appended { rows: 120, offset: 120 }), "{p:?}");
        }
        let frames: Vec<RgbaImage> = ys.iter().map(|&y| crop(&page, y, FRAME_H)).collect();
        let out = s.finish().unwrap();
        assert_eq!(out, crop(&page, 0, 1500));
        export_pngs("stitch-scroll-up", &frames[..4], &out);
    }

    #[test]
    fn scrolling_up_by_varying_steps_stops_where_the_user_stopped() {
        let page = text_page(W, 2200, 22);
        let ys = [1900, 1897, 1860, 1859, 1710, 1700, 1570, 1569, 1565, 1340, 1339, 1200, 1198, 960, 800, 560];
        let mut s = up();
        let pushes = feed(&mut s, &page, &ys, FRAME_H);
        assert!(pushes.iter().all(|p| !matches!(p, Push::Lost)), "{pushes:?}");
        assert_eq!(s.height(), 1900 + FRAME_H - 560);
        // The image is the part of the page between the top of the last frame and the bottom of the first.
        assert_eq!(s.finish().unwrap(), crop(&page, 560, 1900 + FRAME_H - 560));
    }

    #[test]
    fn scrolling_up_keeps_a_fixed_header_on_top_and_a_footer_at_the_bottom() {
        let (header_h, footer_h) = (56, 40);
        let page = text_page(W, 1800, 23);
        let header = bar(W, header_h, [30, 90, 200]);
        let footer = bar(W, footer_h, [200, 120, 30]);
        let ys = [1500, 1410, 1300, 1170, 1050, 900, 760, 620, 500];
        let mut s = up();
        for &y in &ys {
            let mut f = crop(&page, y, FRAME_H);
            paste(&mut f, &header, 0);
            paste(&mut f, &footer, FRAME_H - footer_h);
            assert!(!matches!(s.push(&f), Push::Lost | Push::SizeMismatch));
        }
        let first = *ys.last().unwrap();
        let mut expected = RgbaImage::new(W, header_h + (1500 + FRAME_H - footer_h - header_h - first) + footer_h);
        paste(&mut expected, &header, 0);
        paste(&mut expected, &rows(&page, first + header_h, 1500 + FRAME_H - footer_h), header_h);
        let footer_y = expected.height() - footer_h;
        paste(&mut expected, &footer, footer_y);
        let out = s.finish().unwrap();
        assert_eq!(out.dimensions(), expected.dimensions());
        assert!(out == expected, "one header on top, one footer at the bottom");
    }

    #[test]
    fn scrolling_up_adds_nothing_when_the_page_does_not_move_or_goes_back_down() {
        let page = text_page(W, 1500, 24);
        let mut s = up();
        let pushes = feed(&mut s, &page, &[1000, 1000, 1000], FRAME_H);
        assert_eq!(pushes, vec![Push::First, Push::Unchanged, Push::Unchanged]);
        // Up, then back down past the start: that part is flagged and not added.
        let more = feed(&mut s, &page, &[880, 760, 880, 1000, 1100, 1100, 1000, 880, 760, 640], FRAME_H);
        assert!(matches!(more[0], Push::Appended { rows: 120, offset: 120 }), "{more:?}");
        assert!(matches!(more[2], Push::Revisited { offset: -120 }), "{more:?}");
        assert!(s.above_start(), "scrolled back below the first frame");
        assert!(more[6..9].iter().all(|p| matches!(p, Push::Revisited { .. })), "{more:?}");
        assert!(matches!(more[9], Push::Appended { rows: 120, offset: 120 }), "{more:?}");
        assert_eq!(s.finish().unwrap(), crop(&page, 640, 1000 + FRAME_H - 640));
    }

    #[test]
    fn scrolling_up_uses_the_hint_in_the_direction_of_scrolling() {
        // Rows that repeat exactly: only the hint tells the real shift.
        let page = inbox_page(W, 120, 40, 7, 2);
        let (step, top) = (200u32, 7 * 200);
        let mut s = up();
        s.push(&crop(&page, top, 492));
        for k in 1..8u32 {
            let r = s.push_hinted(&crop(&page, top - k * step, 492), Some(step as i32));
            assert!(matches!(r, Push::Appended { offset, .. } if offset == step as i32), "{k}: {r:?}");
        }
        assert_eq!(s.finish().unwrap(), crop(&page, 0, 7 * step + 492));
    }

    #[test]
    fn scrolling_up_reports_a_jump_beyond_the_overlap_and_leaves_no_seam() {
        let page = text_page(W, 1800, 27);
        let mut s = up();
        let pushes = feed(&mut s, &page, &[1500, 1400, 600, 500, 1350, 1250], FRAME_H);
        assert!(matches!(pushes[2], Push::Lost), "{:?}", pushes[2]);
        assert!(matches!(pushes[3], Push::Lost), "{:?}", pushes[3]);
        // Coming back within reach of the last good frame picks the thread up again.
        assert!(matches!(pushes[4], Push::Appended { rows: 50, offset: 50 }), "{:?}", pushes[4]);
        assert!(matches!(pushes[5], Push::Appended { rows: 100, .. }), "{:?}", pushes[5]);
        assert_eq!(s.finish().unwrap(), crop(&page, 1250, 250 + FRAME_H));
    }

    #[test]
    fn an_upward_height_cap_keeps_the_bottom_of_the_page() {
        let page = text_page(W, 1500, 28);
        let mut s = Stitcher::with_direction(700, Direction::Up);
        let pushes = feed(&mut s, &page, &[1200, 1080, 960, 840, 720, 600], FRAME_H);
        assert!(s.is_capped());
        assert!(matches!(pushes[5], Push::Appended { .. } | Push::Capped));
        let out = s.finish().unwrap();
        assert_eq!(out.height(), 700);
        assert_eq!(out, crop(&page, 800, 700));
    }

    #[test]
    fn the_direction_signs_scrolls() {
        assert_eq!((Direction::Down.sign(), Direction::Up.sign()), (1, -1));
        assert_eq!(Direction::default(), Direction::Down);
    }
}
