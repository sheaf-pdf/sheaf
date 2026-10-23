//! Annotations flattened (M10.5 §12.2): what each draws, as the
//! appearance a viewer shows, placed on its page in a layer of its own, and
//! the annotation taken off. Forms are drawn by reference, never merged
//! into the page's content or decoded: another tool's comment's and a
//! widget's appearance as its source holds it, a sheaf mark's pinned by
//! what it's drawn from, which a save draws again (`PinnedAp`). Each placed
//! through the matrix that takes the box its form's matrix makes of its
//! bounding box onto the annotation's `/Rect` (ISO 32000-1 12.5.5), marked
//! as an artifact on a page of a tagged file.
//!
//! What a flatten takes: annotations shown (neither Hidden nor NoView),
//! printed too (Print) when only the printable are; never a link, which
//! stays, nor a pop-up, which goes with what opens it. A widget draws the
//! appearance its state (`/AS`) names, and one with states and none named
//! draws nothing (M10 bug 10); another tool's comment with none named draws
//! what PDFium, the page view's engine, shows: the state its `/V` names,
//! else its `/Off`, else nothing, never its first. One whose appearance
//! can't be had draws nothing. Each goes as
//! a delete takes it: a mark or a comment with its thread, a widget from its
//! field, which goes with its last (the cascades). A page a flatten draws
//! on must be read, its content drawn around ([`EditError::Content`]).
//! Annotations the records don't hold (a printer's mark, media) stay as the
//! page has them.

use std::collections::HashSet;
use std::sync::Arc;

use sheaf_edit::FlattenSet;

use super::marks::drop_annots;
use super::{EditError, Lower};
use crate::annot::{Box2, Pt};
use crate::forms::field::annot_flags::{HIDDEN, NO_VIEW, PRINT};
use crate::geom::Affine;
use crate::model::{
    AnnotBody, AnnotRec, FormRef, Layer, MarkAp, PageDef, PageId, PinnedAp, PlaceTag, Placement,
    Static,
};
use crate::prim::{Key, Val};
use crate::source::ObjRef;
use crate::source::appearance::{Shows, frame};
use crate::store::Src;

/// The annotations on `pages` that `which` says flattened, a page at a
/// time with progress and a way to cancel (§6.3): how many.
pub(crate) fn flatten(
    lw: &mut Lower,
    pages: &[PageId],
    which: FlattenSet,
) -> Result<usize, EditError> {
    let mut flattened = 0;
    for (i, &page) in pages.iter().enumerate() {
        lw.pages_done(i, pages.len())?;
        flattened += flatten_page(lw, page, which)?;
    }
    Ok(flattened)
}

/// Page `page`'s annotations that `which` says flattened, in their order
/// there: what they draw a layer over the rest, which a page that's
/// flattened again has another of, and they go. How many went.
fn flatten_page(lw: &mut Lower, page: PageId, which: FlattenSet) -> Result<usize, EditError> {
    lw.read(Key::Page(page));
    lw.read(Key::PageAnnots(page));
    let def = lw.rec.def(page).expect("resolved").clone();
    let tag = match tagged(&def.base.src) {
        true => PlaceTag::Artifact,
        false => PlaceTag::Untagged,
    };
    let ids: Vec<_> = (lw.rec.annot_ids_on(page).into_iter().flatten())
        .copied()
        .collect();
    let mut gone = HashSet::new();
    let mut placed = Vec::new();
    for id in ids {
        lw.read(Key::Annot(id));
        let rec = lw.rec.annot(id).expect("listed on its page");
        let Some(drawn) = drawn(rec, &def, which) else {
            continue;
        };
        gone.insert(id);
        placed.extend(
            drawn
                .into_iter()
                .map(|(form, matrix)| Placement { form, matrix, tag }),
        );
    }
    if !placed.is_empty() {
        let (src, index) = (&def.base.src, def.base.index);
        src.blocks(index).map_err(|e| lw.content(e, page))?;
        let layer = Layer {
            id: lw.block_id(),
            body: Static::Placements(placed.into()),
        };
        let def = PageDef::new(def.base.clone(), def.copy, def.content.with_layer(layer));
        lw.rec.put(Key::Page(page), Some(Val::Page(Arc::new(def))));
    }
    drop_annots(&mut lw.rec, page, |id, _| gone.contains(&id));
    Ok(gone.len())
}

/// Whether `src`'s file is tagged: it has a structure tree, which what's
/// drawn on its pages that isn't in it is an artifact of.
fn tagged(src: &Src) -> bool {
    (src.pdf().catalog()).is_ok_and(|c| c.has(b"StructTreeRoot"))
}

/// What annotation `rec`, on a page showing `def`, draws flattened, each
/// form with the matrix that places it: None when `which` doesn't flatten
/// it.
fn drawn(rec: &AnnotRec, def: &PageDef, which: FlattenSet) -> Option<Vec<(FormRef, Affine)>> {
    let flattens = |flags: u32| {
        flags & (HIDDEN | NO_VIEW) == 0 && (which == FlattenSet::All || flags & PRINT != 0)
    };
    match &rec.body {
        // Its appearance's box is its `/Rect`: it's drawn where it is.
        AnnotBody::Mark(m) => flattens(m.annot.kind.flags()).then(|| {
            let pinned = |ap| FormRef::Pinned(Arc::new(PinnedAp::new(ap)));
            (MarkAp::of(m).into_iter())
                .map(|ap| (pinned(ap), Affine::IDENTITY))
                .collect()
        }),
        AnnotBody::Foreign(f) if f.summary.is_link() => None,
        AnnotBody::Foreign(f) => {
            let (src, read) = match &f.src {
                Some(r) => (&r.src, Shows::read(r)?),
                None => {
                    let (src, index) = (&def.base.src, def.base.index);
                    (src, Shows::in_slot(src, index, f.summary.slot)?)
                }
            };
            if !flattens(read.flags) {
                return None;
            }
            let form = read.form(src, read.state.as_deref(), true);
            Some(placed(form, f.summary.rect))
        }
        AnnotBody::Widget(w) => {
            if !flattens(w.flags) {
                return None;
            }
            let state = w.state.as_deref().map(str::as_bytes);
            let form = (w.src.as_ref()).and_then(|r| Shows::read(r)?.form(&r.src, state, false));
            let [l, b, r, t] = w.rect;
            let rect = Box2::from_corners(Pt { x: l, y: b }, Pt { x: r, y: t });
            Some(placed(form, rect))
        }
    }
}

/// Form `form`, when there's one, placed on `rect`: none when it can't
/// be read, or its box has no area, which draws nothing.
fn placed(form: Option<ObjRef>, rect: Box2) -> Vec<(FormRef, Affine)> {
    let Some(form) = form else {
        return Vec::new();
    };
    let Some((bbox, matrix)) = frame(&form) else {
        return Vec::new();
    };
    let Some(onto) = Affine::onto(matrix.bounds(bbox), rect) else {
        return Vec::new();
    };
    vec![(FormRef::Base(form), onto)]
}
