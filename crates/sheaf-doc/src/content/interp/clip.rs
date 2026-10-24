//! The clip: what drawing is cut to, as PDFium keeps it (`CPDF_ClipPath`).
//! Each part cuts it: a path, by the rule it's filled by, or the glyphs
//! text shown in a clipping mode drew between a `BT` and its `ET`. It's
//! in the graphics state, so it's a list each part is added to the front
//! of, which `q` saves as a clone.

use std::sync::Arc;

use super::path::Seg;
use super::text::Quad;
use crate::content::FillRule;
use crate::geom::Rect;

/// The parts that cut what's drawn, the last added first; none where
/// nothing does.
#[derive(Debug, Clone, Default)]
pub struct Clip(Option<Arc<Node>>);

#[derive(Debug)]
struct Node {
    part: ClipPart,
    /// What it and the parts outside it leave: their boxes' intersection.
    bounds: Rect,
    /// Where its part is a path that's a rectangle on the axes, the
    /// rectangle.
    rect: Option<Rect>,
    /// The text in its parts and theirs, as PDFium counts it: an object
    /// a string shown, and one each part.
    texts: usize,
    outer: Clip,
}

/// What cuts what's drawn.
#[derive(Debug, Clone)]
pub enum ClipPart {
    /// A path in PageSpace: what its rule says is inside it, and the box
    /// its points span.
    Path {
        segments: Arc<[Seg]>,
        rule: FillRule,
        bounds: Rect,
    },
    /// What text shown in a clipping mode (`Tr` 4 to 7) between a `BT`
    /// and its `ET` drew: its glyphs' boxes, and the box they span. A
    /// glyph's outline, which only its font's program says, may reach past
    /// its box ([`Glyph::quad`](super::Glyph::quad)).
    Text { glyphs: Arc<[Quad]>, bounds: Rect },
}

impl ClipPart {
    pub fn bounds(&self) -> Rect {
        match self {
            ClipPart::Path { bounds, .. } | ClipPart::Text { bounds, .. } => *bounds,
        }
    }
}

/// PDFium's cap on the text in a clip (`CPDF_ClipPath::AppendTexts`): text
/// past it cuts nothing.
pub(super) const TEXTS: usize = 1024;

impl Clip {
    /// Its parts, the last added first.
    pub fn parts(&self) -> impl Iterator<Item = &ClipPart> {
        std::iter::successors(self.0.as_deref(), |node| node.outer.0.as_deref())
            .map(|node| &node.part)
    }

    /// A box that holds what it leaves, as PDFium bounds it
    /// (`CPDF_ClipPath::GetClipBox`): its parts' boxes' intersection, the
    /// zero rectangle where they don't meet. None where nothing cuts
    /// what's drawn.
    pub fn bounds(&self) -> Option<Rect> {
        self.0.as_ref().map(|node| node.bounds)
    }

    /// It cut by `segments` too, filled by `rule`, `bounds` the box their
    /// points span. A rectangle on the axes added last that holds them
    /// cuts nothing they don't, so they take its place, as in PDFium
    /// (`AppendPathWithAutoMerge`).
    pub(super) fn with_path(&self, segments: Vec<Seg>, rule: FillRule, bounds: Rect) -> Clip {
        let outer = match self.0.as_deref() {
            Some(Node {
                rect: Some(r),
                outer,
                ..
            }) if contains(*r, bounds) => outer,
            _ => self,
        };
        let rect = rectangle(&segments);
        let part = ClipPart::Path {
            segments: segments.into(),
            rule,
            bounds,
        };
        outer.with(part, rect, 0)
    }

    /// It cut by the glyphs `glyphs` of `objects` strings shown, as PDFium
    /// cuts it: not at all where that's past its cap on the text in a
    /// clip.
    pub(super) fn with_text(&self, glyphs: Vec<Quad>, objects: usize) -> Clip {
        let texts = self.0.as_ref().map_or(0, |node| node.texts);
        if texts + objects > TEXTS {
            return self.clone();
        }
        let mut quads = glyphs.iter().map(Quad::bounds);
        let first = quads.next().unwrap_or_default();
        let bounds = quads.fold(first, |r, q| Rect {
            x0: r.x0.min(q.x0),
            y0: r.y0.min(q.y0),
            x1: r.x1.max(q.x1),
            y1: r.y1.max(q.y1),
        });
        let part = ClipPart::Text {
            glyphs: glyphs.into(),
            bounds,
        };
        self.with(part, None, objects + 1)
    }

    fn with(&self, part: ClipPart, rect: Option<Rect>, texts: usize) -> Clip {
        let outer = self.0.as_ref();
        let bounds = outer.map_or(part.bounds(), |node| node.bounds.intersect(part.bounds()));
        Clip(Some(Arc::new(Node {
            part,
            bounds,
            rect,
            texts: outer.map_or(0, |node| node.texts) + texts,
            outer: self.clone(),
        })))
    }
}

/// A list as long as content can make it is dropped a part at a time,
/// not by recursion.
impl Drop for Clip {
    fn drop(&mut self) {
        let mut next = self.0.take();
        while let Some(node) = next {
            next = Arc::try_unwrap(node)
                .ok()
                .and_then(|mut node| node.outer.0.take());
        }
    }
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.x0 <= inner.x0 && outer.y0 <= inner.y0 && inner.x1 <= outer.x1 && inner.y1 <= outer.y1
}

/// The rectangle `segs` are, where they're one on the axes: four corners
/// joined by lines, back to the first, each side along an axis.
fn rectangle(segs: &[Seg]) -> Option<Rect> {
    let mut corners = Vec::with_capacity(5);
    for seg in segs {
        match *seg {
            Seg::Move(p) if corners.is_empty() => corners.push(p),
            Seg::Line(p) if !corners.is_empty() => corners.push(p),
            Seg::Close => {}
            _ => return None,
        }
    }
    if corners.len() == 5 && corners[4] == corners[0] {
        corners.pop();
    }
    let [a, b, c, d] = corners[..] else {
        return None;
    };
    let across = a.x == b.x && b.y == c.y && c.x == d.x && d.y == a.y;
    let up = a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x;
    (across || up).then(|| Rect::new(a.x, a.y, c.x, c.y))
}
