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
//! its first. One whose appearance can't be had draws nothing. Each goes as
//! a delete takes it: a mark or a comment with its thread, a widget from its
//! field, which goes with its last (the cascades). A page a flatten draws
//! on must be read, its content drawn around ([`EditError::Content`]).
//! Annotations the records don't hold (a printer's mark, media) stay as the
//! page has them.

use std::collections::HashSet;
use std::sync::Arc;

use lopdf::{Dictionary, LazyPdf, Object, ObjectId};
use sheaf_edit::FlattenSet;

use super::marks::drop_annots;
use super::{EditError, Lower};
use crate::annot::{Box2, Pt};
use crate::forms::field::annot_flags::{HIDDEN, NO_VIEW, PRINT};
use crate::geom::{Affine, Rect};
use crate::model::{
    AnnotBody, AnnotRec, FormRef, Layer, MarkAp, PageDef, PageId, PinnedAp, PlaceTag, Placement,
    Static,
};
use crate::prim::{Key, Val};
use crate::source::ObjRef;
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
                Some(r) => (&r.src, read(r.src.pdf(), r.id)?),
                None => {
                    let facts = def.base.src.page(def.base.index)?;
                    let Object::Dictionary(dict) = facts.annots.get(f.summary.slot)? else {
                        return None;
                    };
                    (&def.base.src, Shows::of(def.base.src.pdf(), dict))
                }
            };
            if !flattens(read.flags) {
                return None;
            }
            let state = read.state.as_deref();
            let form = read.normal.and_then(|n| shown(src, &n, state, true));
            Some(placed(src, form, f.summary.rect))
        }
        AnnotBody::Widget(w) => {
            if !flattens(w.flags) {
                return None;
            }
            let form = (w.src.as_ref()).and_then(|r| {
                let read = read(r.src.pdf(), r.id)?;
                Some((&r.src, read.normal?))
            });
            let form = form.and_then(|(src, n)| {
                let at = shown(src, &n, w.state.as_deref().map(str::as_bytes), false)?;
                Some((src, at))
            });
            let [l, b, r, t] = w.rect;
            let rect = Box2::from_corners(Pt { x: l, y: b }, Pt { x: r, y: t });
            Some(match form {
                Some((src, at)) => placed(src, Some(at), rect),
                None => Vec::new(),
            })
        }
    }
}

/// Form `form` of `src`, when there's one, placed on `rect`: none when it
/// can't be read, or its box has no area, which draws nothing.
fn placed(src: &Src, form: Option<ObjectId>, rect: Box2) -> Vec<(FormRef, Affine)> {
    let Some(form) = form else {
        return Vec::new();
    };
    let Some((bbox, matrix)) = frame(src.pdf(), form) else {
        return Vec::new();
    };
    let Some(onto) = Affine::onto(matrix.bounds(bbox), rect) else {
        return Vec::new();
    };
    let at = ObjRef {
        src: src.clone(),
        id: form,
    };
    vec![(FormRef::Base(at), onto)]
}

/// What an annotation's dictionary says that a flatten reads: its flags
/// (`/F`), the state it shows (`/AS`) and its normal appearance (`/AP
/// /N`), as written.
struct Shows {
    flags: u32,
    state: Option<Vec<u8>>,
    normal: Option<Object>,
}

impl Shows {
    /// What `dict`, an annotation of `pdf`, says.
    fn of(pdf: &LazyPdf, dict: &Dictionary) -> Shows {
        let value = |key: &[u8]| -> Option<Object> {
            match dict.get(key).ok()? {
                Object::Reference(r) => Some((*unkept(pdf, *r)?.1).clone()),
                direct => Some(direct.clone()),
            }
        };
        let flags = value(b"F").and_then(|f| f.as_i64().ok()).unwrap_or(0);
        // `/N` alone, the appearance dictionary read in place.
        let n = |ap: &Object| ap.as_dict().ok()?.get(b"N").ok().cloned();
        let normal = match dict.get(b"AP").ok() {
            Some(Object::Reference(r)) => unkept(pdf, *r).and_then(|(_, ap)| n(&ap)),
            Some(direct) => n(direct),
            None => None,
        };
        Shows {
            flags: u32::try_from(flags).unwrap_or(0),
            state: value(b"AS").and_then(|s| s.as_name().ok().map(<[u8]>::to_vec)),
            normal,
        }
    }
}

/// What annotation `id` of `pdf` says ([`Shows`]), read without keeping
/// it: None when it isn't a dictionary.
fn read(pdf: &LazyPdf, id: ObjectId) -> Option<Shows> {
    let (_, obj) = unkept(pdf, id)?;
    Some(Shows::of(pdf, obj.as_dict().ok()?))
}

/// The form an annotation's normal appearance `normal`, of `src`, shows:
/// the stream it names, or of a dictionary of states, the one `state`
/// names; with none named, the first when `first` says so (another tool's
/// comment, never a widget, M10 bug 10). None when there's none.
fn shown(src: &Src, normal: &Object, state: Option<&[u8]>, first: bool) -> Option<ObjectId> {
    let pdf = src.pdf();
    // A dictionary of states, read in place.
    let held;
    let states = match normal {
        Object::Reference(r) => {
            let (at, obj) = unkept(pdf, *r)?;
            if obj.as_stream().is_ok() {
                return Some(at);
            }
            held = obj;
            held.as_dict().ok()?
        }
        Object::Dictionary(states) => states,
        _ => return None,
    };
    let state = match state {
        Some(state) => state,
        None if first => states.iter().next()?.0.as_slice(),
        None => return None,
    };
    let at = states.get(state).ok()?.as_reference().ok()?;
    let (at, obj) = unkept(pdf, at)?;
    obj.as_stream().is_ok().then_some(at)
}

/// Form `form` of `pdf`'s `/BBox` and `/Matrix` (the identity when it has
/// none), read without keeping it: None when it isn't a form with a box.
fn frame(pdf: &LazyPdf, form: ObjectId) -> Option<(Rect, Affine)> {
    let (_, obj) = unkept(pdf, form)?;
    let dict = &obj.as_stream().ok()?.dict;
    let numbers = |key: &[u8], n: usize| -> Option<Vec<f32>> {
        let array = match dict.get(key).ok()? {
            Object::Reference(r) => (*unkept(pdf, *r)?.1).clone(),
            direct => direct.clone(),
        };
        let values: Vec<f32> = (array.as_array().ok()?.iter())
            .map(|v| v.as_float().unwrap_or(0.0) as f32)
            .collect();
        (values.len() == n).then_some(values)
    };
    let bbox = numbers(b"BBox", 4)?;
    let matrix = match numbers(b"Matrix", 6).as_deref() {
        Some(&[a, b, c, d, e, f]) => Affine { a, b, c, d, e, f },
        _ => Affine::IDENTITY,
    };
    Some((Rect::new(bbox[0], bbox[1], bbox[2], bbox[3]), matrix))
}

/// Object `id` of `pdf`, its references followed, read without keeping
/// it, and the object it is: what a flatten of thousands of widgets reads
/// once each.
fn unkept(pdf: &LazyPdf, id: ObjectId) -> Option<(ObjectId, Arc<Object>)> {
    let mut at = id;
    for _ in 0..crate::source::Limits::DEFAULT.reader.max_depth {
        let obj = pdf.object_unkept(at).ok()?;
        match *obj {
            Object::Reference(next) => at = next,
            _ => return Some((at, obj)),
        }
    }
    None
}
