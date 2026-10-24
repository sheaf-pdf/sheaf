//! Paths: built a segment at a time and ended by a painting operator, as
//! PDFium builds them (`CPDF_StreamContentParser`'s `AddPathPoint` and
//! `AddPathObject`). A path is built in user space and placed by the CTM
//! where it ends, and what's built isn't saved by `q`: it's the content's,
//! not the graphics state's.
//!
//! After an `m` of two operands PDFium reads the path operators that
//! follow its own way (`ParsePathObject`), and so does this: their
//! numbers, up to six, go in a buffer each fills from its start and none
//! clears, so an operator given too few or too many takes what's there.

use std::f32::consts::SQRT_2;

use super::state::{State, number, nums};
use crate::annot::Pt;
use crate::content::lex::{Step, Tok, is_number};
use crate::content::{FillRule, LineCap, LineJoin, Paint};
use crate::geom::Rect;

/// A part of a path, from where the one before ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    /// A subpath starts here.
    Move(Pt),
    Line(Pt),
    /// A cubic Bézier curve: its two control points, then where it ends.
    Curve(Pt, Pt, Pt),
    /// The subpath ends closed, where it started: the segment before it
    /// ends there, PDFium having drawn a line back where the pen wasn't.
    Close,
}

impl Seg {
    /// Its points, the control points among them.
    fn points(&self) -> impl Iterator<Item = Pt> {
        let points = match *self {
            Seg::Move(p) | Seg::Line(p) => [Some(p), None, None],
            Seg::Curve(a, b, p) => [Some(a), Some(b), Some(p)],
            Seg::Close => [None; 3],
        };
        points.into_iter().flatten()
    }
}

/// A path painted, or made the clip, by the operator that ends it.
#[derive(Debug, Clone, Copy)]
pub struct PathDrawn<'e> {
    /// Its segments in PageSpace, placed by the CTM where it ends; for a
    /// path of one point that clips, PDFium's empty rectangle at
    /// PageSpace's origin, which clips everything away.
    pub segments: &'e [Seg],
    /// How it's painted: [`Paint::None`] where it only clips.
    pub paint: Paint,
    /// The rule it clips by, where `W` or `W*` came before it; the clip it
    /// makes is in the state of the events after it.
    pub clip: Option<FillRule>,
    /// A box that holds all it paints: its points', and for a stroke as
    /// far past them as the pen reaches, at a square cap's corner and a
    /// miter join's point too. Where it only clips, its points'.
    pub bounds: Rect,
}

/// A path being built, in user space: PDFium's, and where its subpath
/// started and the pen is, both at the origin before any.
#[derive(Debug)]
pub(super) struct Builder {
    segs: Vec<Seg>,
    start: Pt,
    current: Pt,
    /// `W` or `W*`, for the operator that ends the path.
    clip: Option<FillRule>,
    /// The numbers PDFium's own reading of path operators has, while it
    /// reads them so.
    fast: Option<[f32; 6]>,
}

/// A path ended: what PDFium paints and clips by.
pub(super) struct Ended {
    /// Its segments in PageSpace.
    pub segs: Vec<Seg>,
    pub paint: Paint,
    pub clip: Option<FillRule>,
    /// Its points' box, and what it paints'.
    pub points: Rect,
    pub bounds: Rect,
}

impl Default for Builder {
    fn default() -> Builder {
        let origin = Pt { x: 0.0, y: 0.0 };
        Builder {
            segs: Vec::new(),
            start: origin,
            current: origin,
            clip: None,
            fast: None,
        }
    }
}

impl Builder {
    /// How many segments it has.
    pub(super) fn len(&self) -> usize {
        self.segs.len()
    }

    /// Whether `op`, with `step`'s operands, is read PDFium's own way, as
    /// a path operator after an `m`: once one isn't, none after it is till
    /// the next `m`.
    fn keeps(&mut self, op: &[u8], step: &Step<'_, '_>) -> bool {
        let path_op = matches!(op, b"m" | b"l" | b"c" | b"v" | b"y" | b"h" | b"re");
        if self.fast.is_some() && !(path_op && step.args().all(is_number)) {
            self.fast = None;
        }
        self.fast.is_some()
    }

    /// What `step`, an operator that builds the path, does to it; false
    /// for any other step. Every step is handed to it, as each but a path
    /// operator ends PDFium's own reading of them.
    pub(super) fn build(&mut self, step: &Step<'_, '_>) -> bool {
        let Tok::Op(op) = step.tok else {
            self.fast = None;
            return false;
        };
        if self.keeps(op, step) {
            self.fast_op(op, step);
            return true;
        }
        let point = |x: f32, y: f32| Pt { x, y };
        match op {
            b"m" if step.operands.len() == 2 => {
                if let Some([x, y]) = nums(step) {
                    self.move_to(point(x, y));
                }
                self.fast = Some([0.0; 6]);
            }
            b"l" if step.operands.len() == 2 => {
                if let Some([x, y]) = nums(step) {
                    self.line_to(point(x, y));
                }
            }
            b"m" | b"l" => {}
            b"c" => {
                if let Some([x1, y1, x2, y2, x3, y3]) = nums(step) {
                    self.curve(point(x1, y1), point(x2, y2), point(x3, y3));
                }
            }
            b"v" => {
                if let Some([x2, y2, x3, y3]) = nums(step) {
                    self.curve(self.current, point(x2, y2), point(x3, y3));
                }
            }
            b"y" => {
                if let Some([x1, y1, x3, y3]) = nums(step) {
                    self.curve(point(x1, y1), point(x3, y3), point(x3, y3));
                }
            }
            b"h" => self.close(),
            b"re" => {
                if let Some([x, y, w, h]) = nums(step) {
                    self.rect(x, y, w, h);
                }
            }
            b"W" => self.clip = Some(FillRule::NonZero),
            b"W*" => self.clip = Some(FillRule::EvenOdd),
            _ => return false,
        }
        true
    }

    /// A path operator read PDFium's own way: its numbers, up to six, put
    /// in the buffer from its start, and each point it takes read from
    /// there. One that isn't finite builds nothing.
    fn fast_op(&mut self, op: &[u8], step: &Step<'_, '_>) {
        let Some(n) = self.fast.as_mut() else {
            return;
        };
        for (slot, operand) in n.iter_mut().zip(step.args()) {
            *slot = number(operand);
        }
        let n = *n;
        let used = match op {
            b"m" | b"l" => 2,
            b"c" => 6,
            b"v" | b"y" | b"re" => 4,
            _ => 0,
        };
        if !n[..used].iter().all(|v| v.is_finite()) {
            return;
        }
        let at = |i: usize| Pt {
            x: n[i],
            y: n[i + 1],
        };
        match op {
            b"m" => self.move_to(at(0)),
            b"l" => self.line_to(at(0)),
            b"c" => self.curve(at(0), at(2), at(4)),
            b"v" => self.curve(self.current, at(0), at(2)),
            b"y" => self.curve(at(0), at(2), at(2)),
            b"h" => self.close(),
            _ => self.rect(n[0], n[1], n[2], n[3]),
        }
    }

    /// PDFium's `AddPathPoint` for a move: a move just after another
    /// takes its place.
    fn move_to(&mut self, p: Pt) {
        self.current = p;
        self.start = p;
        match self.segs.last_mut() {
            Some(Seg::Move(last)) => *last = p,
            _ => self.segs.push(Seg::Move(p)),
        }
    }

    /// A line, or a curve, where there's a path to add it to.
    fn line_to(&mut self, p: Pt) {
        self.current = p;
        if !self.segs.is_empty() {
            self.segs.push(Seg::Line(p));
        }
    }

    fn curve(&mut self, a: Pt, b: Pt, p: Pt) {
        self.current = p;
        if !self.segs.is_empty() {
            self.segs.push(Seg::Curve(a, b, p));
        }
    }

    /// `h`: a line back to the start where the pen isn't there, and the
    /// subpath closed.
    fn close(&mut self) {
        if self.segs.is_empty() {
            return;
        }
        if self.current != self.start {
            self.close_to(self.start);
        } else if self.segs.last() != Some(&Seg::Close) {
            self.segs.push(Seg::Close);
        }
    }

    /// PDFium's `AddPathPointAndClose`: a line to `p`, and the subpath
    /// closed.
    fn close_to(&mut self, p: Pt) {
        self.current = p;
        if !self.segs.is_empty() {
            self.segs.extend([Seg::Line(p), Seg::Close]);
        }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let point = |x: f32, y: f32| Pt { x, y };
        self.move_to(point(x, y));
        self.line_to(point(x + w, y));
        self.line_to(point(x + w, y + h));
        self.line_to(point(x, y + h));
        self.close_to(point(x, y));
    }

    /// The path, ended by `paint` in `state`, as PDFium's `AddPathObject`
    /// ends it: None where it draws and clips nothing. A path of one point
    /// clips everything away; it's painted only where it's closed and
    /// stroked with round caps, a dot. A move it ends with is dropped.
    pub(super) fn end(&mut self, paint: Paint, state: &State) -> Option<Ended> {
        match paint {
            Paint::CloseStroke | Paint::CloseFillStroke => self.close(),
            // PDFium closes `b*` with a line back even where the pen's
            // there.
            Paint::CloseFillStrokeEvenOdd => self.close_to(self.start),
            _ => {}
        }
        let mut segs = std::mem::take(&mut self.segs);
        let clip = self.clip.take();
        if segs.is_empty() {
            return None;
        }
        if segs.iter().map(|s| s.points().count()).sum::<usize>() == 1 {
            if clip.is_some() {
                let origin = Pt { x: 0.0, y: 0.0 };
                let mut segs = vec![Seg::Move(origin)];
                segs.extend([Seg::Line(origin); 4]);
                segs.push(Seg::Close);
                return Some(Ended {
                    segs,
                    paint: Paint::None,
                    clip: Some(FillRule::NonZero),
                    points: Rect::default(),
                    bounds: Rect::default(),
                });
            }
            let [Seg::Move(p), Seg::Close] = segs[..] else {
                return None;
            };
            if state.line_cap != LineCap::Round {
                return None;
            }
            segs = vec![Seg::Move(p), Seg::Line(p), Seg::Close];
        }
        if let Some(Seg::Move(_)) = segs.last() {
            segs.pop();
        }
        if !paint.strokes() && paint.fills().is_none() && clip.is_none() {
            return None;
        }
        let m = state.ctm;
        for seg in &mut segs {
            *seg = match *seg {
                Seg::Move(p) => Seg::Move(m.apply(p)),
                Seg::Line(p) => Seg::Line(m.apply(p)),
                Seg::Curve(a, b, p) => Seg::Curve(m.apply(a), m.apply(b), m.apply(p)),
                Seg::Close => Seg::Close,
            };
        }
        let points = bounds(&segs);
        let bounds = match paint.strokes() {
            true => stroked(points, &segs, state),
            false => points,
        };
        Some(Ended {
            segs,
            paint,
            clip,
            points,
            bounds,
        })
    }
}

/// Whether `segs`' points are all finite.
pub(super) fn finite(segs: &[Seg]) -> bool {
    (segs.iter().flat_map(Seg::points)).all(|p| p.x.is_finite() && p.y.is_finite())
}

/// The box `segs`' points span.
pub(super) fn bounds(segs: &[Seg]) -> Rect {
    let mut points = segs.iter().flat_map(Seg::points);
    let Some(first) = points.next() else {
        return Rect::default();
    };
    points.fold(Rect::new(first.x, first.y, first.x, first.y), |r, p| Rect {
        x0: r.x0.min(p.x),
        y0: r.y0.min(p.y),
        x1: r.x1.max(p.x),
        y1: r.y1.max(p.y),
    })
}

/// `points`, the box of `segs`' points in PageSpace, widened by what
/// stroking them in `state` paints past them: the pen's reach, through
/// the CTM; for a line width of 0, the thinnest line, half a point, as
/// PDFium bounds one.
fn stroked(points: Rect, segs: &[Seg], state: &State) -> Rect {
    let width = state.line_width.abs();
    let (dx, dy) = if width == 0.0 {
        (0.5, 0.5)
    } else {
        let r = reach(width, segs, state);
        let m = state.ctm;
        (r * m.a.hypot(m.c), r * m.b.hypot(m.d))
    };
    Rect {
        x0: points.x0 - dx,
        y0: points.y0 - dy,
        x1: points.x1 + dx,
        y1: points.y1 + dy,
    }
}

/// How far from the path, in user space, a stroke of `width` paints: half
/// the width; at a square cap's corner √2 times that; and at a miter
/// join up to the miter limit times it, where the path has a join, a
/// curve (drawn as lines joined) among them.
fn reach(width: f32, segs: &[Seg], state: &State) -> f32 {
    let cap = match state.line_cap {
        LineCap::Square => SQRT_2,
        LineCap::Butt | LineCap::Round => 1.0,
    };
    let join = match state.line_join {
        LineJoin::Miter if joins(segs) => state.miter_limit.max(1.0),
        LineJoin::Miter | LineJoin::Round | LineJoin::Bevel => 1.0,
    };
    width / 2.0 * cap.max(join)
}

/// Whether a stroke of `segs` joins segments: a subpath of two or more,
/// one closed, or a curve.
fn joins(segs: &[Seg]) -> bool {
    let mut drawn = 0;
    for seg in segs {
        match seg {
            Seg::Move(_) => drawn = 0,
            Seg::Line(_) => drawn += 1,
            Seg::Curve(..) | Seg::Close => return true,
        }
        if drawn > 1 {
            return true;
        }
    }
    false
}
