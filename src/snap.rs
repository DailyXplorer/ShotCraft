//! Object snapping while an annotation is dragged, resized or drawn.
//!
//! "Snap to Objects": edges and centres line up with the edges and centres of the other
//! annotations. "Snap to Similar Objects" adds, among annotations of the same kind: the same width
//! or height, and equal spacing (a row of counters, a stack of boxes). Everything here is pure:
//! rectangles in, a correction and the guide lines to draw out. The threshold is given in screen
//! points and turned into image pixels by the caller's zoom.
use crate::model::{p, Kind, Obj, P, R};
use crate::render;

/// How close (screen points) an edge must come to a line before it jumps onto it.
pub const SNAP_POINTS: f32 = 6.0;

/// Candidates this close to the best one (image pixels) are snapped to together, and their guides shown.
const SAME: f32 = 0.01;
/// With many objects the pairwise spacing search is skipped.
const MAX_SPACING_OBJECTS: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Opts {
    /// Snap to Objects.
    pub objects: bool,
    /// Snap to Similar Objects.
    pub similar: bool,
}

impl Opts {
    pub fn any(self) -> bool {
        self.objects || self.similar
    }
}

/// Another annotation: where it is, and whether it is of the same kind as the one being moved.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Other {
    pub r: R,
    pub same_kind: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuideKind {
    /// A line through two aligned edges.
    Edge,
    /// A line through an aligned centre.
    Centre,
    /// The gap between two objects, equal to another gap.
    Gap,
    /// A width or height equal to another object's.
    Size,
}

/// A segment to draw, in image pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Guide {
    pub a: P,
    pub b: P,
    pub kind: GuideKind,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Snap {
    /// What to add to the dragged position.
    pub dx: f32,
    pub dy: f32,
    pub guides: Vec<Guide>,
}

#[derive(Clone, Copy)]
enum Ax {
    X,
    Y,
}

impl Ax {
    /// The extent of `r` along this axis.
    fn span(self, r: &R) -> (f32, f32) {
        match self {
            Ax::X => (r.x0, r.x1),
            Ax::Y => (r.y0, r.y1),
        }
    }
    /// The extent of `r` across this axis.
    fn cross(self, r: &R) -> (f32, f32) {
        match self {
            Ax::X => (r.y0, r.y1),
            Ax::Y => (r.x0, r.x1),
        }
    }
    fn point(self, along: f32, across: f32) -> P {
        match self {
            Ax::X => p(along, across),
            Ax::Y => p(across, along),
        }
    }
}

/// What a candidate lines up with; turned into guides once the final position is known.
#[derive(Clone, Copy, Debug)]
enum Match {
    /// The position `pos` along the axis, shared with object `other`.
    Align { pos: f32, other: usize, centre: bool },
    /// Equal gaps: (left, right) rectangles whose gap equals the others'.
    Gaps { a: usize, b: usize, between: Option<usize>, after: bool },
    /// The same size as object `other`.
    Size { other: usize },
}

#[derive(Clone, Copy, Debug)]
struct Cand {
    delta: f32,
    m: Match,
}

fn overlap(a: (f32, f32), b: (f32, f32)) -> Option<(f32, f32)> {
    let (lo, hi) = (a.0.max(b.0), a.1.min(b.1));
    (hi > lo).then_some((lo, hi))
}

/// The smallest corrections within `tol`, all of them equally small.
fn best(cands: Vec<Cand>, tol: f32) -> (f32, Vec<Match>) {
    let Some(min) = cands.iter().map(|c| c.delta.abs()).filter(|d| *d <= tol).min_by(|a, b| a.total_cmp(b)) else {
        return (0.0, vec![]);
    };
    let pick = cands.iter().find(|c| c.delta.abs() <= min + SAME).map(|c| c.delta).unwrap_or(0.0);
    let ms = cands.iter().filter(|c| (c.delta - pick).abs() <= SAME).map(|c| c.m).collect();
    (pick, ms)
}

/// Edge-to-edge, edge-to-centre and centre-to-centre alignment candidates for the positions `mine`.
fn align_cands(mine: &[(f32, bool)], others: &[Other], ax: Ax, out: &mut Vec<Cand>) {
    for (i, o) in others.iter().enumerate() {
        let (lo, hi) = ax.span(&o.r);
        for (pos, c_other) in [(lo, false), ((lo + hi) / 2.0, true), (hi, false)] {
            for (m, c_mine) in mine {
                out.push(Cand { delta: pos - m, m: Match::Align { pos, other: i, centre: c_other || *c_mine } });
            }
        }
    }
}

/// Moving a whole box along one axis.
fn move_axis(m: &R, others: &[Other], o: Opts, ax: Ax, tol: f32) -> (f32, Vec<Match>) {
    let (lo, hi) = ax.span(m);
    let mut cands = vec![];
    if o.objects {
        align_cands(&[(lo, false), ((lo + hi) / 2.0, true), (hi, false)], others, ax, &mut cands);
    }
    if o.similar && others.len() <= MAX_SPACING_OBJECTS {
        let size = hi - lo;
        let same: Vec<usize> = (0..others.len()).filter(|i| others[*i].same_kind).collect();
        let on_row = |i: usize| overlap(ax.cross(m), ax.cross(&others[i].r)).is_some();
        for &a in &same {
            for &b in &same {
                if a == b {
                    continue;
                }
                let (sa, sb) = (ax.span(&others[a].r), ax.span(&others[b].r));
                let gap = sb.0 - sa.1;
                if gap <= 0.0 || overlap(ax.cross(&others[a].r), ax.cross(&others[b].r)).is_none() {
                    continue;
                }
                // The same gap after `b`, and before `a`.
                if on_row(b) {
                    cands.push(Cand { delta: sb.1 + gap - lo, m: Match::Gaps { a, b, between: None, after: true } });
                }
                if on_row(a) {
                    cands.push(Cand { delta: sa.0 - gap - hi, m: Match::Gaps { a, b, between: None, after: false } });
                }
            }
        }
        // Centred between two objects, so that both gaps are equal.
        for (ai, &a) in same.iter().enumerate() {
            for &b in &same[ai + 1..] {
                let (first, second) = if ax.span(&others[a].r).1 <= ax.span(&others[b].r).0 { (a, b) } else { (b, a) };
                let (sa, sb) = (ax.span(&others[first].r), ax.span(&others[second].r));
                let room = sb.0 - sa.1 - size;
                if room >= 0.0 && on_row(first) && on_row(second) {
                    cands.push(Cand { delta: sa.1 + room / 2.0 - lo, m: Match::Gaps { a: first, b: second, between: Some(first), after: true } });
                }
            }
        }
    }
    best(cands, tol)
}

/// Moving one coordinate `v` (an edge being dragged or a point) along one axis. `fixed` is the
/// opposite edge, when there is one, so that sizes can match.
fn point_axis(v: f32, fixed: Option<f32>, others: &[Other], o: Opts, ax: Ax, tol: f32) -> (f32, Vec<Match>) {
    let mut cands = vec![];
    if o.objects {
        align_cands(&[(v, false)], others, ax, &mut cands);
    }
    if let (true, Some(f)) = (o.similar, fixed) {
        let dir = if v >= f { 1.0 } else { -1.0 };
        for (i, other) in others.iter().enumerate().filter(|(_, x)| x.same_kind) {
            let (lo, hi) = ax.span(&other.r);
            if hi - lo > 0.0 {
                cands.push(Cand { delta: f + dir * (hi - lo) - v, m: Match::Size { other: i } });
            }
        }
    }
    best(cands, tol)
}

fn line(a: P, b: P, centre: bool) -> Guide {
    Guide { a, b, kind: if centre { GuideKind::Centre } else { GuideKind::Edge } }
}

/// The segment across the gap between two boxes, in the middle of where their rows overlap.
fn gap_guide(first: &R, second: &R, ax: Ax) -> Option<Guide> {
    let across = overlap(ax.cross(first), ax.cross(second))?;
    let mid = (across.0 + across.1) / 2.0;
    let (g0, g1) = (ax.span(first).1, ax.span(second).0);
    (g1 > g0).then(|| Guide { a: ax.point(g0, mid), b: ax.point(g1, mid), kind: GuideKind::Gap })
}

/// A dimension line through the middle of a box.
fn size_guide(r: &R, ax: Ax) -> Guide {
    let (lo, hi) = ax.span(r);
    let (c0, c1) = ax.cross(r);
    let mid = (c0 + c1) / 2.0;
    Guide { a: ax.point(lo, mid), b: ax.point(hi, mid), kind: GuideKind::Size }
}

/// The guides for the matches of one axis, for a box `m` that has been moved to its final place.
fn guides_for(m: &R, others: &[Other], matches: &[Match], ax: Ax, out: &mut Vec<Guide>) {
    for mt in matches {
        match *mt {
            Match::Align { pos, other, centre } => {
                let (c0, c1) = ax.cross(m);
                let (d0, d1) = ax.cross(&others[other].r);
                out.push(line(ax.point(pos, c0.min(d0)), ax.point(pos, c1.max(d1)), centre));
            }
            Match::Gaps { a, b, between, after } => {
                let (ra, rb) = (&others[a].r, &others[b].r);
                if between.is_none() {
                    out.extend(gap_guide(ra, rb, ax));
                }
                match (between, after) {
                    (Some(_), _) => {
                        out.extend(gap_guide(ra, m, ax));
                        out.extend(gap_guide(m, rb, ax));
                    }
                    (None, true) => out.extend(gap_guide(rb, m, ax)),
                    (None, false) => out.extend(gap_guide(m, ra, ax)),
                }
            }
            Match::Size { other } => {
                out.push(size_guide(&others[other].r, ax));
                out.push(size_guide(m, ax));
            }
        }
    }
}

/// Moving a box: how far to shift it so that edges, centres and spacing line up, and the guides.
pub fn snap_move(m: R, others: &[Other], o: Opts, tol: f32) -> Snap {
    if !o.any() || others.is_empty() {
        return Snap::default();
    }
    let (dx, mx) = move_axis(&m, others, o, Ax::X, tol);
    let (dy, my) = move_axis(&m, others, o, Ax::Y, tol);
    let moved = R { x0: m.x0 + dx, x1: m.x1 + dx, y0: m.y0 + dy, y1: m.y1 + dy };
    let mut guides = vec![];
    guides_for(&moved, others, &mx, Ax::X, &mut guides);
    guides_for(&moved, others, &my, Ax::Y, &mut guides);
    Snap { dx, dy, guides }
}

/// Dragging a corner or an end point to `q` (the opposite corner is `anchor`, if any): how far to
/// shift `q`, and the guides.
pub fn snap_point(q: P, anchor: Option<P>, others: &[Other], o: Opts, tol: f32) -> Snap {
    if !o.any() || others.is_empty() {
        return Snap::default();
    }
    let (dx, mx) = point_axis(q.x, anchor.map(|a| a.x), others, o, Ax::X, tol);
    let (dy, my) = point_axis(q.y, anchor.map(|a| a.y), others, o, Ax::Y, tol);
    let q2 = p(q.x + dx, q.y + dy);
    // The dragged box (or just the point) the guides are drawn around.
    let m = match anchor {
        Some(a) => R::from_points(a, q2),
        None => R { x0: q2.x, y0: q2.y, x1: q2.x, y1: q2.y },
    };
    let mut guides = vec![];
    guides_for(&m, others, &mx, Ax::X, &mut guides);
    guides_for(&m, others, &my, Ax::Y, &mut guides);
    Snap { dx, dy, guides }
}

/// Where an annotation is, for snapping: its geometry, without stroke padding.
pub fn object_rect(obj: &Obj, fonts: &render::Fonts) -> R {
    match &obj.kind {
        Kind::Arrow { a, b, .. } | Kind::Line { a, b } | Kind::Highlighter { a, b } | Kind::Ruler { a, b } => R::from_points(*a, *b),
        Kind::Rect { r, .. } | Kind::Oval { r, .. } | Kind::Blur { r, .. } | Kind::Spotlight { r } | Kind::Magnifier { r, .. } => *r,
        Kind::Freehand { pts } => pts.iter().fold(R { x0: f32::MAX, y0: f32::MAX, x1: f32::MIN, y1: f32::MIN }, |r, q| R {
            x0: r.x0.min(q.x),
            y0: r.y0.min(q.y),
            x1: r.x1.max(q.x),
            y1: r.y1.max(q.y),
        }),
        Kind::Counter { at, .. } => {
            let s = obj.style.counter_size() / 2.0;
            R { x0: at.x - s, y0: at.y - s, x1: at.x + s, y1: at.y + s }
        }
        Kind::Text { at, text } => render::text_box(*at, text, &obj.style, fonts),
    }
}

/// The other annotations, as snapping sees them. `skip` is the one being dragged.
pub fn others_of(objs: &[Obj], skip: Option<usize>, kind_of: &Obj, fonts: &render::Fonts) -> Vec<Other> {
    objs.iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != skip)
        .map(|(_, o)| Other { r: object_rect(o, fonts), same_kind: o.tool() == kind_of.tool() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f32 = 6.0;
    const OBJ: Opts = Opts { objects: true, similar: false };
    const SIM: Opts = Opts { objects: false, similar: true };
    const BOTH: Opts = Opts { objects: true, similar: true };

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> R {
        R { x0, y0, x1, y1 }
    }
    fn other(rect: R) -> Other {
        Other { r: rect, same_kind: true }
    }

    #[test]
    fn edges_snap_within_the_threshold() {
        let others = [other(r(100.0, 100.0, 200.0, 160.0))];
        // The left edge is 4 px from the other's left edge: shifts by -4 and nothing else lines up.
        let s = snap_move(r(104.0, 300.0, 144.0, 330.0), &others, OBJ, TOL);
        assert_eq!((s.dx, s.dy), (-4.0, 0.0));
        assert!(s.guides.iter().any(|g| g.kind == GuideKind::Edge && g.a.x == 100.0 && g.b.x == 100.0));
        // The right edge too, to the other's right edge.
        let s = snap_move(r(163.0, 300.0, 203.0, 330.0), &others, OBJ, TOL);
        assert_eq!(s.dx, -3.0);
        // Top edge to the other's bottom edge.
        let s = snap_move(r(400.0, 163.0, 440.0, 190.0), &others, OBJ, TOL);
        assert_eq!((s.dx, s.dy), (0.0, -3.0));
    }

    #[test]
    fn nothing_snaps_beyond_the_threshold_or_when_off() {
        let others = [other(r(100.0, 100.0, 200.0, 160.0))];
        let m = r(107.0, 300.0, 127.0, 330.0);
        assert_eq!(snap_move(m, &others, OBJ, TOL), Snap::default());
        let m = r(104.0, 300.0, 144.0, 330.0);
        assert_eq!(snap_move(m, &others, Opts::default(), TOL), Snap::default());
        assert_eq!(snap_move(m, &[], OBJ, TOL), Snap::default());
    }

    #[test]
    fn centres_snap_to_centres() {
        let others = [other(r(100.0, 100.0, 200.0, 160.0))];
        // Centre x is 150: a 40 wide box at x 133..173 (centre 153) shifts -3.
        let s = snap_move(r(133.0, 300.0, 173.0, 330.0), &others, OBJ, TOL);
        assert_eq!(s.dx, -3.0);
        assert!(s.guides.iter().any(|g| g.kind == GuideKind::Centre && g.a.x == 150.0));
        // Centre y is 130.
        let s = snap_move(r(400.0, 112.0, 440.0, 144.0), &others, OBJ, TOL);
        assert_eq!(s.dy, 0.0 + (130.0 - 128.0));
    }

    #[test]
    fn the_closest_candidate_wins() {
        let others = [other(r(100.0, 0.0, 200.0, 50.0)), other(r(102.0, 200.0, 140.0, 240.0))];
        // Left edge at 105: 100 is 5 away, 102 is 3 away.
        let s = snap_move(r(105.0, 100.0, 145.0, 130.0), &others, OBJ, TOL);
        assert_eq!(s.dx, -3.0);
    }

    #[test]
    fn guides_span_both_objects() {
        let others = [other(r(100.0, 100.0, 200.0, 160.0))];
        let s = snap_move(r(100.0, 300.0, 140.0, 330.0), &others, OBJ, TOL);
        let g = s.guides.iter().find(|g| g.kind == GuideKind::Edge).unwrap();
        assert_eq!((g.a.y, g.b.y), (100.0, 330.0));
    }

    #[test]
    fn similar_mode_matches_equal_spacing_after_and_before() {
        // Two boxes 100 wide with a 20 gap: x 0..100 and 120..220.
        let others = [other(r(0.0, 0.0, 100.0, 50.0)), other(r(120.0, 0.0, 220.0, 50.0))];
        // A third box 60 wide dragged to x 243..303: its gap after the second would be 20 at x 240.
        let s = snap_move(r(243.0, 5.0, 303.0, 45.0), &others, SIM, TOL);
        assert_eq!(s.dx, -3.0);
        assert_eq!(s.dy, 0.0, "similar mode alone does not align edges");
        assert!(s.guides.iter().filter(|g| g.kind == GuideKind::Gap).count() >= 2);
        // Before the first: x 340..? no: before `a` the box would end at -20.
        let s = snap_move(r(-81.0, 5.0, -21.0, 45.0), &others, SIM, TOL);
        assert_eq!(s.dx, 1.0);
        // Not on the same row: no spacing snap.
        let s = snap_move(r(243.0, 200.0, 303.0, 240.0), &others, SIM, TOL);
        assert_eq!(s, Snap::default());
    }

    #[test]
    fn similar_mode_centres_between_two_objects() {
        let others = [other(r(0.0, 0.0, 50.0, 50.0)), other(r(150.0, 0.0, 200.0, 50.0))];
        // 40 wide between 50 and 150: gaps of 30 each, at x 80..120.
        let s = snap_move(r(83.0, 0.0, 123.0, 50.0), &others, SIM, TOL);
        assert_eq!(s.dx, -3.0);
        assert!(s.guides.iter().filter(|g| g.kind == GuideKind::Gap).count() >= 2);
    }

    #[test]
    fn spacing_only_considers_objects_of_the_same_kind() {
        let mut others = [other(r(0.0, 0.0, 100.0, 50.0)), other(r(120.0, 0.0, 220.0, 50.0))];
        others[1].same_kind = false;
        assert_eq!(snap_move(r(243.0, 5.0, 303.0, 45.0), &others, SIM, TOL), Snap::default());
    }

    #[test]
    fn dragging_a_corner_snaps_to_edges() {
        let others = [other(r(100.0, 100.0, 200.0, 160.0))];
        let anchor = p(10.0, 10.0);
        let s = snap_point(p(203.0, 500.0), Some(anchor), &others, OBJ, TOL);
        assert_eq!((s.dx, s.dy), (-3.0, 0.0));
        let s = snap_point(p(500.0, 163.0), Some(anchor), &others, OBJ, TOL);
        assert_eq!((s.dx, s.dy), (0.0, -3.0));
        // A free point (line end) too.
        let s = snap_point(p(98.0, 131.0), None, &others, OBJ, TOL);
        assert_eq!((s.dx, s.dy), (2.0, -1.0));
    }

    #[test]
    fn resizing_snaps_to_the_width_or_height_of_similar_objects() {
        let others = [other(r(300.0, 300.0, 380.0, 330.0))]; // 80 × 30
        let anchor = p(10.0, 10.0);
        // The corner at (93, 38): width 83 → 80, height 28 → 30.
        let s = snap_point(p(93.0, 38.0), Some(anchor), &others, SIM, TOL);
        assert_eq!((s.dx, s.dy), (-3.0, 2.0));
        assert!(s.guides.iter().any(|g| g.kind == GuideKind::Size));
        // Dragged to the left of the anchor, the size is still measured.
        let s = snap_point(p(-72.0, -22.0), Some(anchor), &others, SIM, TOL);
        assert_eq!((s.dx, s.dy), (2.0, 2.0));
        // Objects mode alone does not match sizes.
        assert_eq!(snap_point(p(93.0, 38.0), Some(anchor), &others, OBJ, TOL), Snap::default());
        // A different kind does not count.
        let mut o2 = others;
        o2[0].same_kind = false;
        assert_eq!(snap_point(p(93.0, 38.0), Some(anchor), &o2, SIM, TOL), Snap::default());
    }

    #[test]
    fn both_modes_together_take_the_closest() {
        let others = [other(r(0.0, 0.0, 100.0, 50.0)), other(r(120.0, 0.0, 220.0, 50.0))];
        // Left edge 1 px from the right edge of the second (220): edge snap wins over the 3 px spacing.
        let s = snap_move(r(241.0, 5.0, 301.0, 45.0), &others, BOTH, TOL);
        // The gap after b is 20 → x 240, the same place: both agree.
        assert_eq!(s.dx, -1.0);
        let s = snap_move(r(222.0, 100.0, 282.0, 140.0), &others, BOTH, TOL);
        assert_eq!(s.dx, -2.0, "the edge at 220 is nearer than any spacing");
    }

    #[test]
    fn object_rect_is_the_geometry_without_stroke_padding() {
        use crate::model::{Kind, Style};
        let fonts = render::Fonts::load();
        let st = Style::default();
        let rect = Obj { kind: Kind::Rect { r: r(10.0, 20.0, 110.0, 70.0), seed: 1 }, style: st };
        assert_eq!(object_rect(&rect, &fonts), r(10.0, 20.0, 110.0, 70.0));
        let line = Obj { kind: Kind::Line { a: p(50.0, 90.0), b: p(10.0, 30.0) }, style: st };
        assert_eq!(object_rect(&line, &fonts), r(10.0, 30.0, 50.0, 90.0));
        let free = Obj { kind: Kind::Freehand { pts: vec![p(5.0, 9.0), p(15.0, 2.0), p(8.0, 30.0)] }, style: st };
        assert_eq!(object_rect(&free, &fonts), r(5.0, 2.0, 15.0, 30.0));
        let counter = Obj { kind: Kind::Counter { at: p(100.0, 100.0), n: 1 }, style: st };
        let c = object_rect(&counter, &fonts);
        assert_eq!(c.center(), p(100.0, 100.0));
        assert!(c.w() > 4.0 && (c.w() - c.h()).abs() < 1e-3);
    }

    #[test]
    fn others_skip_the_dragged_object_and_mark_the_same_kind() {
        use crate::model::{Kind, Style};
        let fonts = render::Fonts::load();
        let st = Style::default();
        let objs = vec![
            Obj { kind: Kind::Rect { r: r(0.0, 0.0, 10.0, 10.0), seed: 1 }, style: st },
            Obj { kind: Kind::Rect { r: r(20.0, 0.0, 30.0, 10.0), seed: 2 }, style: st },
            Obj { kind: Kind::Oval { r: r(40.0, 0.0, 50.0, 10.0), seed: 3 }, style: st },
        ];
        let o = others_of(&objs, Some(0), &objs[0], &fonts);
        assert_eq!(o.len(), 2);
        assert!(o[0].same_kind && !o[1].same_kind);
    }
}
