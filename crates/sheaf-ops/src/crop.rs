//! Trim White (Document > Crop Pages…, "Trim white margins"): each page
//! cropped down to its content. A crop is geometry, a box edit of the page's
//! `Geom` ([`Op::SetPageBoxes`]): nothing stored moves, and marks,
//! bookmarks and OCR text, all in PageSpace, stay on the same content.
//!
//! Trim White ([`trim_white`]) is a resolve stage (M10.5 §6.3): it reads
//! each page's content through the document's views and the pool, a
//! planner's in the background's budget and a dialog's preview in the
//! screen's ([`Urgency`]), so a page is measured as it's drawn and a source
//! PDFium refuses is measured from its groups. Its crops are ops a
//! planner's transaction commits ([`Trimmed::lower`]).

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::executor::block_on;
use in_progress::{NoProgress, Progress};
use sheaf_doc::geom::{self, Rect};
use sheaf_doc::prim::Key;
use sheaf_doc::realize::ViewInputs;
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{Box2, Document, EditError, PAGES, PageId, Snapshot, Tx};
use sheaf_edit::{BoxSpec, Op, PageSel, Patch, Rotation};
use sheaf_image::raster::{PixelFormat, content_bounds};
use sheaf_render::{Budget, Job};

use crate::views::Views;

/// Near-white: Trim White takes anything darker as a page's content.
const WHITE: u8 = 235;

/// What Trim White keeps around a page's content as the app trims it, in
/// points: an eighth of an inch.
pub const PAD: f32 = 9.0;

/// Whom Trim White draws pages for, which says their priority and budget
/// in the pool (M10.5 §10.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// A planner's: after everything else, in the background's budget.
    Background,
    /// A dialog's preview, which the user waits on: first, in the
    /// screen's budget.
    Preview,
}

impl Urgency {
    /// The priority and budget its renders are asked at.
    fn render(self) -> (u8, Budget) {
        match self {
            Urgency::Background => (u8::MAX, Budget::Background),
            Urgency::Preview => (0, Budget::Screen),
        }
    }
}

/// What Trim White found on the pages it was given.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trimmed {
    /// Each page whose crop changes, with its new crop box, in PageSpace.
    pub crops: Vec<(PageId, Box2)>,
    /// The pages whose content was measured, in order, those it leaves as
    /// they were included: what the crops were decided by.
    pub measured: Vec<PageId>,
    /// The pages it can't crop, left as they were: blank, with too little
    /// content to crop to once padded, or whose file shows less than
    /// [`MIN_SIDE`](geom::MIN_SIDE) of them a side.
    pub left: Vec<PageId>,
}

impl Trimmed {
    /// The crops, as ops of `tx`, a transaction made at the snapshot they
    /// were measured in, which commits under `Stale::Validate` (M10.5
    /// §8.5): what each measured page draws with its content alone, read
    /// ([`Tx::read_render`]), a page left as it was too, since its content
    /// decided that; the boxes of each page it found a crop for, since
    /// whether the page has that crop already decided whether it takes an
    /// op; and an op for each page cropped. All of them, or none and why.
    pub fn lower(&self, tx: &mut Tx) -> Result<(), EditError> {
        let content = ViewInputs::content_only();
        let left: HashSet<PageId> = self.left.iter().copied().collect();
        for &page in &self.measured {
            tx.read_render(page, &content);
            if !left.contains(&page) {
                tx.read(Key::Geom(page));
            }
        }
        let ops = (self.crops.iter()).map(|&(page, to)| crop_to(page, to));
        tx.apply_batch(ops.collect(), &NoProgress)?;
        Ok(())
    }
}

/// Trim White (M10.5 §6.3's resolve stage): pages `pages` of `snap`, each
/// to be cropped down to its content, anything darker than near-white,
/// with `pad` points (at least 0) around it, within the box its file
/// crops it to. Each page is measured as its file crops it, whatever crop
/// sheaf gave it, with its content alone ([`ViewInputs::content_only`]):
/// drawn at 72 dpi through `views`, as `urgency` asks, so a page of a
/// source PDFium refuses is drawn from its group. `lower` is the
/// planner's, whose transaction made at `snap` commits the crops
/// ([`Trimmed::lower`]).
pub fn trim_white(
    views: &Views,
    snap: &Snapshot,
    lower: Arc<LowerCx>,
    pages: &[PageId],
    pad: f32,
    urgency: Urgency,
    progress: &dyn Progress,
) -> Result<Trimmed> {
    progress.set_total(Some(pages.len() as u64), PAGES);
    let pad = pad.max(0.0);
    let (measure, owns) = measuring(snap, lower, pages)?;
    let st = measure.state();
    let content = ViewInputs::content_only();
    let (priority, budget) = urgency.render();
    let mut out = Trimmed::default();
    for (done, (&id, own)) in pages.iter().zip(owns).enumerate() {
        progress.check()?;
        progress.set(done as u64 + 1);
        let Some(own) = own else {
            out.left.push(id);
            continue;
        };
        // The page as its file crops it, shown as the document turns it.
        let g = **st.geom(id).context("page not found")?;
        let shown = g.shown();
        let job = Job::Page {
            width: shown.width.round().max(1.0) as u32,
            rotation: Rotation::R0,
        };
        let rx = views.render(st, id, &content, job, PixelFormat::Rgba, priority, budget);
        let raster = block_on(rx).context("the render thread is gone")??.raster;
        let (rw, rh) = (raster.width, raster.height);
        let gray = raster.to_gray().context("short render")?;
        out.measured.push(id);
        let Some((x0, y0, x1, y1)) = content_bounds(&gray, WHITE) else {
            out.left.push(id);
            continue;
        };
        // The content as the page shows, padded, back in PageSpace.
        let (sx, sy) = (shown.width / rw as f32, shown.height / rh as f32);
        let found = Box2 {
            x: x0 as f32 * sx - pad,
            y: y0 as f32 * sy - pad,
            w: (x1 - x0) as f32 * sx + 2.0 * pad,
            h: (y1 - y0) as f32 * sy + 2.0 * pad,
        };
        let kept = rect(g.from_view().apply_box(found)).intersect(own);
        if kept.width() < geom::MIN_SIDE || kept.height() < geom::MIN_SIDE {
            out.left.push(id);
            continue;
        }
        let to = box_of(kept);
        let now = snap.geom(id);
        if rect(to).intersect(now.media) != now.crop {
            out.crops.push((id, to));
        }
    }
    Ok(out)
}

/// The state Trim White measures pages `pages` of `snap` in, lowered with
/// `lower`, and each page's own crop box, its base page's in its file
/// (none when that shows less than [`MIN_SIDE`](geom::MIN_SIDE) a side,
/// and the page isn't measured): `snap`'s, each page cropped as its file
/// crops it, so a crop made in sheaf never changes what Trim White reads
/// or finds. A page whose only change is its crop is untouched there, and
/// is drawn from its source's own bytes; but for a box whose far corner
/// isn't exactly its near one plus its size in `f32`, as an op's box
/// carries it, which is drawn from its group, alike.
///
/// Within the file's own crop box, not the whole media box, so what a
/// file's `/CropBox` hides (printer's marks, bleed) is never taken for
/// content: a decision for Greg, the whole media box being `geom.media`
/// in place of the own box.
fn measuring(
    snap: &Snapshot,
    lower: Arc<LowerCx>,
    pages: &[PageId],
) -> Result<(Tx, Vec<Option<Rect>>)> {
    let mut measure = Document::tx_at(snap, lower);
    let mut owns = Vec::with_capacity(pages.len());
    for &id in pages {
        let base = &snap.state.def(id).context("page not found")?.base;
        let own = base.src.geom(base.index).crop;
        let own = (own.width() >= geom::MIN_SIDE && own.height() >= geom::MIN_SIDE).then_some(own);
        if let Some(own) = own
            && snap.geom(id).crop != own
        {
            measure.apply(crop_to(id, box_of(own)), &NoProgress)?;
        }
        owns.push(own);
    }
    Ok((measure, owns))
}

/// Page `page` cropped to `to`, in PageSpace.
fn crop_to(page: PageId, to: Box2) -> Op {
    Op::SetPageBoxes {
        pages: PageSel::ids([page]),
        crop: Patch::Set(BoxSpec::Rect(to)),
        trim: Patch::Keep,
        bleed: Patch::Keep,
        art: Patch::Keep,
    }
}

/// Box `b` as a rectangle, as an op setting it reads it.
fn rect(b: Box2) -> Rect {
    Rect::new(b.x, b.y, b.right(), b.bottom())
}

/// Rectangle `r` as a box.
fn box_of(r: Rect) -> Box2 {
    Box2 {
        x: r.x0,
        y: r.y0,
        w: r.width(),
        h: r.height(),
    }
}
