//! Page ops lowered (M10.5 §3.3, §6.5): the order, what each page shows
//! and which copy of its source page it is, its boxes, and what comes in
//! with pages from a file: the other tools' annotations and links on
//! them, sheaf's marks, the form and the outline, read from the source's
//! facts.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use im::Vector;
use sheaf_edit::{BoxSpec, Bring, OutlineBring, Patch, Rotation};

use super::error::EditError;
use super::marks::drop_annots;
use super::resolve::file_named;
use super::{Lower, Made, Notice, forms, put_page_annots, splice};
use crate::annot::{AnnotId, AnnotKind, Box2};
use crate::file::ForeignAnnot;
use crate::geom::{Geom, MIN_SIDE, Rect};
use crate::labels::LabelSpec;
use crate::model::{
    AnnotBody, AnnotRec, Bookmark, ContentStack, DocState, Foreign, FormRef, Generator, Mark,
    PageBase, PageDef, PageId, Scope,
};
use crate::open::FileBody;
use crate::prim::{Key, Val};
use crate::source::ObjRef;
use crate::store::Src;

/// Moves `pages`, kept in document order, to just before `before`, none
/// of them, or to the end.
pub(crate) fn move_pages(lw: &mut Lower, pages: &[PageId], before: Option<PageId>) {
    let set: HashSet<PageId> = pages.iter().copied().collect();
    let (moving, rest): (Vector<PageId>, Vector<PageId>) = lw
        .rec
        .order()
        .iter()
        .copied()
        .partition(|id| set.contains(id));
    let at = match before {
        Some(b) => rest
            .iter()
            .position(|&id| id == b)
            .expect("a page not moving"),
        None => rest.len(),
    };
    lw.rec
        .put(Key::Order, Some(Val::Order(splice(&rest, at, moving))));
}

/// Turns `pages` by `by` from where each is; the prim holds the turn it
/// comes to.
pub(crate) fn rotate(lw: &mut Lower, pages: &[PageId], by: Rotation) {
    for &id in pages {
        let geom = lw.rec.geom(id).expect("resolved");
        let geom = Geom {
            rotate: geom.rotate.plus(by),
            ..**geom
        };
        lw.rec.put(Key::Geom(id), Some(Val::Geom(Arc::new(geom))));
    }
}

/// The boxes of `pages`, as `crop`, `trim`, `bleed` and `art` say: each
/// page's `Geom` read and written, nothing else, so an OCR flush, which
/// writes what the page shows, never conflicts with a crop (§6.3). Each
/// box's margins come off the page as it showed before the op. A cleared
/// crop is the media box; the others cleared are gone. A box that isn't
/// on the page, or leaves less than [`MIN_SIDE`] of it, fails the op: it's
/// never repaired, and only float noise is clipped (§6.4).
pub(crate) fn set_boxes(
    lw: &mut Lower,
    pages: &[PageId],
    crop: &Patch<BoxSpec>,
    trim: &Patch<BoxSpec>,
    bleed: &Patch<BoxSpec>,
    art: &Patch<BoxSpec>,
) -> Result<(), EditError> {
    for (i, &id) in pages.iter().enumerate() {
        lw.pages_done(i, pages.len())?;
        lw.read(Key::Geom(id));
        let was = **lw.rec.geom(id).expect("resolved");
        let mut geom = was;
        match crop {
            Patch::Keep => {}
            Patch::Clear => geom.crop = was.media,
            Patch::Set(spec) => geom.crop = page_box(lw, id, &was, spec)?,
        }
        let others = [
            (&mut geom.trim, trim),
            (&mut geom.bleed, bleed),
            (&mut geom.art, art),
        ];
        for (slot, patch) in others {
            match patch {
                Patch::Keep => {}
                Patch::Clear => *slot = None,
                Patch::Set(spec) => *slot = Some(page_box(lw, id, &was, spec)?),
            }
        }
        if geom != was {
            lw.rec.put(Key::Geom(id), Some(Val::Geom(Arc::new(geom))));
        }
    }
    Ok(())
}

/// How far a box may stray past the media box and be float noise, in
/// points.
const NOISE: f32 = 0.01;

/// The box `spec` says on page `id`, whose boxes are `geom`, in PageSpace.
/// Margins come off the box the page shows, as it shows after its
/// `/Rotate`; a rectangle must lie within the media box. Either must leave
/// at least [`MIN_SIDE`] a side, as the caller states it.
fn page_box(lw: &Lower, id: PageId, geom: &Geom, spec: &BoxSpec) -> Result<Rect, EditError> {
    let page = || lw.rec.pos_of(id).expect("a page there is") + 1;
    match *spec {
        BoxSpec::Margins(m) => {
            let sides = [m.top, m.bottom, m.left, m.right];
            if !sides.iter().all(|s| s.is_finite()) {
                return Err(lw.invalid("a margin isn't a number"));
            }
            if sides.iter().any(|&s| s < 0.0) {
                return Err(lw.invalid("a margin can't be less than 0"));
            }
            let shown = geom.shown();
            let kept = Box2 {
                x: m.left,
                y: m.top,
                w: shown.width - m.left - m.right,
                h: shown.height - m.top - m.bottom,
            };
            if kept.w < MIN_SIDE || kept.h < MIN_SIDE {
                let why = format!("those margins leave nothing of page {}", page());
                return Err(lw.invalid(why));
            }
            let b = geom.from_view().apply_box(kept);
            let r = Rect::new(b.x, b.y, b.right(), b.bottom());
            Ok(r.intersect(geom.visible()))
        }
        BoxSpec::Rect(b) => {
            if ![b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite()) {
                return Err(lw.invalid("the box isn't a number"));
            }
            if b.w < MIN_SIDE || b.h < MIN_SIDE {
                let why = format!("the box leaves nothing of page {}", page());
                return Err(lw.invalid(why));
            }
            let r = Rect::new(b.x, b.y, b.right(), b.bottom());
            let media = geom.media;
            let within = r.x0 >= media.x0 - NOISE
                && r.y0 >= media.y0 - NOISE
                && r.x1 <= media.x1 + NOISE
                && r.y1 <= media.y1 + NOISE;
            if !within {
                return Err(lw.invalid(format!("the box isn't on page {}", page())));
            }
            Ok(r.intersect(media))
        }
    }
}

/// Deletes `pages`; the cascades take what's only on them.
pub(crate) fn delete(lw: &mut Lower, pages: &[PageId]) -> Result<(), EditError> {
    let set: HashSet<PageId> = pages.iter().copied().collect();
    let order: Vector<PageId> = (lw.rec.order().iter())
        .filter(|id| !set.contains(id))
        .copied()
        .collect();
    lw.rec.put(Key::Order, Some(Val::Order(order)));
    for (i, &id) in pages.iter().enumerate() {
        lw.pages_done(i, pages.len())?;
        lw.rec.put(Key::Page(id), None);
        lw.rec.put(Key::Geom(id), None);
    }
    Ok(())
}

/// A copy of each of `pages` right after it: the next copy of its source
/// page, with the same text over it, shared (§6.5), the same boxes, fields
/// of its own, its annotations with fresh ids, and in the header/footer's
/// pages when it is.
pub(crate) fn duplicate(lw: &mut Lower, pages: &[PageId]) -> Result<(), EditError> {
    let set: HashSet<PageId> = pages.iter().copied().collect();
    let mut copies = NextCopy::of(&lw.rec);
    let all: Vec<(PageId, Arc<PageDef>, Arc<Geom>)> = (lw.rec.pages())
        .map(|(id, def, geom)| (id, def.clone(), geom.clone()))
        .collect();
    let mut order = Vector::new();
    // Each page, its copy and the copy's number.
    let mut pairs = Vec::new();
    for (id, def, geom) in all {
        order.push_back(id);
        if !set.contains(&id) {
            continue;
        }
        lw.pages_done(pairs.len(), set.len())?;
        lw.read(Key::Page(id));
        lw.read(Key::Geom(id));
        lw.read(Key::PageAnnots(id));
        let copy = lw.page_id();
        let n = copies.take(&def.base);
        let def = PageDef::new(def.base.clone(), n, def.content.clone());
        lw.rec.put(Key::Page(copy), Some(Val::Page(Arc::new(def))));
        lw.rec.put(Key::Geom(copy), Some(Val::Geom(geom)));
        pairs.push((id, copy, n));
        order.push_back(copy);
        lw.made.push(Made::Page(copy));
    }
    lw.rec.put(Key::Order, Some(Val::Order(order)));
    let widgets = forms::copy_forms(lw, &pairs);
    // Each mark's copy, across the pages: a connector copied with its
    // partner is the partner's copy's.
    let marks: Vec<AnnotId> = (pairs.iter())
        .flat_map(|&(page, ..)| lw.rec.annots_on(page))
        .filter(|(_, a)| a.mark().is_some())
        .map(|(id, _)| id)
        .collect();
    let marks: HashMap<AnnotId, AnnotId> =
        (marks.into_iter()).map(|id| (id, lw.annot_id())).collect();
    for &(page, copy, n) in &pairs {
        copy_annots(lw, page, copy, n, &marks, &widgets);
    }
    join_scope(lw, &pairs);
    forms::turn_scripts_off(lw);
    Ok(())
}

/// Each copy in `pairs` (a page, its copy, and the copy's number) on the
/// pages of each generator whose scope names its page. None gets a Bates
/// number, which only setting the header again gives (§6.5).
fn join_scope(lw: &mut Lower, pairs: &[(PageId, PageId, u32)]) {
    let generators: Vec<Arc<Generator>> = lw.rec.generators().values().cloned().collect();
    for g in generators {
        let Scope::Pages(on) = &g.scope else {
            continue;
        };
        let copies: Vec<PageId> = (pairs.iter())
            .filter(|(p, ..)| on.contains(p))
            .map(|&(_, copy, _)| copy)
            .collect();
        if copies.is_empty() {
            continue;
        }
        let mut on = on.clone();
        on.extend(copies);
        let g = Generator {
            scope: Scope::Pages(on),
            ..Generator::clone(&g)
        };
        lw.rec
            .put(Key::Generator(g.id), Some(Val::Generator(Arc::new(g))));
    }
}

/// Pages `pages` of `from` (0-based, ascending), named `name`, before
/// `before` or at the end, with what `bring` says comes with them.
pub(crate) fn insert(
    lw: &mut Lower,
    from: &Src,
    name: &Arc<str>,
    pages: &[u32],
    before: Option<PageId>,
    bring: &Bring,
) -> Result<(), EditError> {
    let at = match before {
        Some(b) => lw.rec.pos_of(b).expect("resolved"),
        None => lw.rec.page_count(),
    };
    let mut copies = NextCopy::of(&lw.rec);
    // Source index -> new PageId, for bookmark targets.
    let mut by_index: HashMap<u32, PageId> = HashMap::new();
    let mut shown = Vec::with_capacity(pages.len());
    for (i, &index) in pages.iter().enumerate() {
        lw.pages_done(i, pages.len())?;
        let id = lw.page_id();
        let (def, geom) = copies.page(from, index);
        lw.rec.put(Key::Page(id), Some(Val::Page(def)));
        lw.rec.put(Key::Geom(id), Some(Val::Geom(geom)));
        by_index.insert(index, id);
        shown.push((index, id));
        lw.made.push(Made::Page(id));
    }
    let order = splice(lw.rec.order(), at, shown.iter().map(|&(_, id)| id));
    lw.rec.put(Key::Order, Some(Val::Order(order)));
    match bring.outline {
        OutlineBring::None => {}
        OutlineBring::MergeByTitle => {
            // Bookmarks to pages that didn't come go nowhere.
            let page = |index: u32| by_index.get(&index).copied();
            let incoming =
                crate::model::outline_from_flat(|| lw.outline_id(), from.outline(), page, from);
            merge_by_title(lw, incoming);
        }
    }
    bring_in(lw, from, name, &shown, bring)
}

/// Shows `pages` from pages `with` of `from` (0-based, ascending), in
/// place: as many as both have keep their ids, and so their bookmarks and
/// marks, in PageSpace; extra pages are deleted, extra replacements
/// inserted after the last page replaced. What showed on the pages from
/// their old source page goes: other tools' annotations, links and
/// widgets.
pub(crate) fn replace(
    lw: &mut Lower,
    pages: &[PageId],
    from: &Src,
    name: &Arc<str>,
    with: &[u32],
    bring: &Bring,
) -> Result<(), EditError> {
    let mut copies = NextCopy::of(&lw.rec);
    let mut replacements = with.iter().copied();
    let mut last_pos = None;
    let mut extra_old = HashSet::new();
    // Which page now shows each replacement.
    let mut shown_by: HashMap<u32, PageId> = HashMap::new();
    let total = pages.len().max(with.len());
    for (i, &id) in pages.iter().enumerate() {
        lw.pages_done(i, total)?;
        let pos = lw.rec.pos_of(id).expect("resolved");
        drop_annots(&mut lw.rec, id, |_, a| a.mark().is_none());
        match replacements.next() {
            Some(index) => {
                // Its OCR was of the page it showed.
                let (def, geom) = copies.page(from, index);
                lw.rec.put(Key::Page(id), Some(Val::Page(def)));
                lw.rec.put(Key::Geom(id), Some(Val::Geom(geom)));
                shown_by.insert(index, id);
                last_pos = Some(pos);
            }
            None => {
                extra_old.insert(id);
            }
        }
    }
    // Extra replacements go after the last replaced page.
    let at = last_pos.map_or(lw.rec.page_count(), |i| i + 1);
    let mut extra = Vec::new();
    for index in replacements {
        lw.pages_done(pages.len() + extra.len(), total)?;
        let id = lw.page_id();
        let (def, geom) = copies.page(from, index);
        lw.rec.put(Key::Page(id), Some(Val::Page(def)));
        lw.rec.put(Key::Geom(id), Some(Val::Geom(geom)));
        shown_by.insert(index, id);
        extra.push(id);
    }
    if !extra.is_empty() || !extra_old.is_empty() {
        let order: Vector<PageId> = splice(lw.rec.order(), at, extra)
            .into_iter()
            .filter(|id| !extra_old.contains(id))
            .collect();
        lw.rec.put(Key::Order, Some(Val::Order(order)));
    }
    for &id in &extra_old {
        lw.rec.put(Key::Page(id), None);
        lw.rec.put(Key::Geom(id), None);
    }
    let mut shown: Vec<(u32, PageId)> = shown_by.into_iter().collect();
    shown.sort();
    bring_in(lw, from, name, &shown, bring)
}

/// The page labels: `ranges`, or none. A header whose `<<label>>` draws a
/// label its face can't draw all of is said ([`Notice::Unencodable`]).
pub(crate) fn set_labels(lw: &mut Lower, ranges: Option<Vec<LabelSpec>>) {
    let labels = (ranges.filter(|r| !r.is_empty())).map(|r| Val::Labels(r.into()));
    lw.rec.put(Key::Labels, labels);
    let header = lw.rec.header_footer();
    if header.is_some_and(|g| super::content::label_unencodable(&lw.rec, g)) {
        lw.notices.push(Notice::Unencodable);
    }
}

/// What comes in with pages of `from` named `name` (`shown`: each page's
/// index in `from`, and the page showing it): the form, then each page's
/// annotations from the source's facts, the reader told what to.
fn bring_in(
    lw: &mut Lower,
    from: &Src,
    name: &Arc<str>,
    shown: &[(u32, PageId)],
    bring: &Bring,
) -> Result<(), EditError> {
    let (_, placed) = forms::take_in(lw, from, name, shown);
    let unencodable = annots_in(lw, from, shown, bring.marks, &placed)?;
    forms::turn_scripts_off(lw);
    if unencodable {
        lw.notices.push(Notice::Unencodable);
    }
    Ok(())
}

/// What pages that came in from `src` hold beside their content (`shown`:
/// each page's index in `src`, and the page showing it): the other tools'
/// comments and links its fact has on each, with the form's widgets
/// [`forms::take_in`] `placed` among them as the file has them, and then,
/// with `marks`, sheaf's. Each gets a fresh id (the file's could clash
/// with ours), and goes after what its page has. A mark is the object the
/// fact has it as on a page showing copy 0, holding what that holds, else
/// what its stems name in the store ([`super::resolve::incoming`] put
/// them there); a connector keeps its partner only if the partner came
/// too; and a reply to a mark, ours or another tool's, follows it to its
/// new name. Whether any mark draws text the built-in fonts can't.
fn annots_in(
    lw: &mut Lower,
    src: &Src,
    shown: &[(u32, PageId)],
    marks: bool,
    placed: &forms::Placed,
) -> Result<bool, EditError> {
    let fact = src.annots().clone();
    let incoming = |index: u32| {
        (fact.on(index).iter()).filter_map(move |f| match &f.body {
            FileBody::Mark { annot, held } if marks => Some((f.object, &**annot, *held)),
            _ => None,
        })
    };
    let mut ids = HashMap::new();
    let mut unencodable = false;
    for &(index, _) in shown {
        for (_, a, _) in incoming(index) {
            unencodable |= a.unencodable();
            ids.insert(a.id, lw.annot_id());
        }
    }
    let at = |id| ObjRef {
        src: src.clone(),
        id,
    };
    for &(index, page) in shown {
        let copy = lw.rec.def(page).map_or(0, |def| def.copy);
        // The names of the marks sheaf made, which change with their ids.
        let names: HashMap<String, String> = (incoming(index))
            .filter(|(_, a, _)| a.name.is_empty())
            .map(|(_, a, _)| {
                let ([was, _], [now, _]) = (a.id.sheaf_names(), ids[&a.id].sheaf_names());
                (was, now)
            })
            .collect();
        let mut on_page = lw.rec.annot_ids_on(page).cloned().unwrap_or_default();
        let mut theirs = placed.get(&index).cloned().unwrap_or_default();
        for (_, object, summary) in fact.foreign_on(index) {
            let id = lw.annot_id();
            let parent = (summary.parent.as_ref()).and_then(|p| names.get(p));
            let summary = match parent {
                Some(now) => Arc::new(ForeignAnnot {
                    parent: Some(now.clone()),
                    ..ForeignAnnot::clone(summary)
                }),
                None => summary.clone(),
            };
            theirs.push((summary.slot, id));
            let src = object.map(at);
            let body = AnnotBody::Foreign(Arc::new(Foreign { src, copy, summary }));
            (lw.rec).put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
        }
        theirs.sort_by_key(|&(slot, _)| slot);
        on_page.extend(theirs.into_iter().map(|(_, id)| id));
        for (object, a, held) in incoming(index) {
            let mut a = a.clone();
            a.id = ids[&a.id];
            match &mut a.kind {
                AnnotKind::Connector { partner, .. } => {
                    *partner = partner.and_then(|p| ids.get(&p).copied());
                }
                AnnotKind::Reply { parent } | AnnotKind::Status { parent, .. } => {
                    if let Some(now) = names.get(parent) {
                        *parent = now.clone();
                    }
                }
                _ => {}
            }
            let id = a.id;
            let mark = Mark {
                src: object.filter(|_| copy == 0).map(at),
                ..Mark::from(a)
            };
            let mark = holding(lw, mark, src, held)?;
            let body = AnnotBody::Mark(Arc::new(mark));
            (lw.rec).put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
            on_page.push_back(id);
        }
        put_page_annots(&mut lw.rec, page, on_page);
    }
    Ok(unencodable)
}

/// `mark`, which came in from `src`, holding what it holds there (`held`,
/// its page stamp's page form or its attachment's file object), else
/// what its stems name: an attachment's file the store has under its stem
/// (decoded there before lowering), a stamp's page the store or a source
/// has. A stem that names nothing is an error; a file that couldn't be
/// decoded, the mark holds none.
fn holding(
    lw: &Lower,
    mut mark: Mark,
    src: &Src,
    held: Option<lopdf::ObjectId>,
) -> Result<Mark, EditError> {
    let store = &lw.cx.store;
    match (&mark.annot.kind, held) {
        (AnnotKind::PageStamp { .. }, Some(id)) => {
            mark.stamp = Some(FormRef::Base(ObjRef {
                src: src.clone(),
                id,
            }));
        }
        (AnnotKind::FileAttachment { file, .. }, Some(_)) => {
            mark.file = file_named(store, file);
        }
        (AnnotKind::PageStamp { stamp, name, .. }, None) => {
            let form = crate::model::held_stamp(store, &lw.cx.primary, stamp);
            mark.stamp = Some(form.ok_or_else(|| lw.not_found(format!("the stamp {name}")))?);
        }
        (AnnotKind::FileAttachment { file, name, .. }, None) => {
            let file = file_named(store, file);
            mark.file =
                Some(file.ok_or_else(|| lw.not_found(format!("the attached file {name}")))?);
        }
        _ => {}
    }
    Ok(mark)
}

/// Bookmarks that came with pages, into the outline PDFlyer's way: one at
/// the top level titled as one there already gives that one its kids, and
/// the others go at the end.
fn merge_by_title(lw: &mut Lower, (roots, made): crate::model::Nested) {
    let rec = &mut lw.rec;
    let mut made: HashMap<crate::model::OutlineId, Bookmark> = made.into_iter().collect();
    let mut top = rec.outline_roots().clone();
    for id in roots {
        let title = made[&id].title.clone();
        let same = top.iter().copied().find(|t| match made.get(t) {
            Some(b) => b.title == title,
            None => rec.bookmark(*t).is_some_and(|b| b.title == title),
        });
        let Some(same) = same else {
            top.push_back(id);
            continue;
        };
        let kids = made.remove(&id).expect("one that came in").kids;
        match made.get_mut(&same) {
            Some(b) => b.kids.append(kids),
            None => {
                let mut b = Bookmark::clone(rec.bookmark(same).expect("a top-level bookmark"));
                b.kids.append(kids);
                rec.put(Key::Bookmark(same), Some(Val::Bookmark(Arc::new(b))));
            }
        }
    }
    for (id, b) in made {
        rec.put(Key::Bookmark(id), Some(Val::Bookmark(Arc::new(b))));
    }
    rec.put(Key::OutlineRoots, Some(Val::OutlineRoots(top)));
}

/// Page `page`'s annotations onto page `copy`, copy `n` of the same page:
/// each with a fresh id (M10.5 bug 12). A mark is a mark of its own, its
/// id in `marks` by the original's, with no name taken from another tool
/// (the copy isn't the comment replies elsewhere are to) and no object,
/// holding what the original holds; a connector's partner is its
/// partner's copy, or none when that wasn't copied. Another tool's
/// annotation is copy `n` of its object, so one deleted from the page
/// isn't on the copy either. A reply, ours or another tool's, to a mark
/// copied with it is to the mark's copy, by its name. A widget's copy is
/// in `widgets`, by the original's id: [`forms::copy_forms`] made it.
fn copy_annots(
    lw: &mut Lower,
    page: PageId,
    copy: PageId,
    n: u32,
    marks: &HashMap<AnnotId, AnnotId>,
    widgets: &HashMap<AnnotId, AnnotId>,
) {
    let annots: Vec<(AnnotId, AnnotBody)> = (lw.rec.annots_on(page))
        .map(|(id, a)| (id, a.body.clone()))
        .collect();
    // The names of the page's marks, and their copies'.
    let names: HashMap<String, String> = (annots.iter())
        .filter_map(|(was, body)| {
            let m = match body {
                AnnotBody::Mark(m) => m,
                AnnotBody::Foreign(_) | AnnotBody::Widget(_) => return None,
            };
            let [now, _] = marks[was].sheaf_names();
            Some((m.annot.name(), now))
        })
        .collect();
    let renamed = |parent: &String| names.get(parent).cloned();
    let mut ids = Vector::new();
    for (was, body) in annots {
        let (id, body) = match body {
            AnnotBody::Mark(m) => {
                let id = marks[&was];
                let mut annot = m.annot.clone();
                annot.id = id;
                annot.name.clear();
                match &mut annot.kind {
                    AnnotKind::Connector { partner, .. } => {
                        *partner = partner.and_then(|p| marks.get(&p).copied());
                    }
                    AnnotKind::Reply { parent } | AnnotKind::Status { parent, .. } => {
                        if let Some(now) = renamed(parent) {
                            *parent = now;
                        }
                    }
                    _ => {}
                }
                let mark = Mark {
                    annot,
                    src: None,
                    ..Mark::clone(&m)
                };
                (id, AnnotBody::Mark(Arc::new(mark)))
            }
            AnnotBody::Foreign(f) => {
                let parent = f.summary.parent.as_ref().and_then(renamed);
                let summary = match parent {
                    Some(now) => Arc::new(ForeignAnnot {
                        parent: Some(now),
                        ..ForeignAnnot::clone(&f.summary)
                    }),
                    None => f.summary.clone(),
                };
                let f = Foreign {
                    copy: n,
                    summary,
                    ..Foreign::clone(&f)
                };
                (lw.annot_id(), AnnotBody::Foreign(Arc::new(f)))
            }
            AnnotBody::Widget(_) => {
                ids.extend(widgets.get(&was).copied());
                continue;
            }
        };
        (lw.rec).put(
            Key::Annot(id),
            Some(Val::Annot(AnnotRec { page: copy, body })),
        );
        ids.push_back(id);
    }
    put_page_annots(&mut lw.rec, copy, ids);
}

/// The copy of each source page a page made next takes (M10.5 §3.3): one
/// past the highest any page holds, by the source and the page, whichever
/// op brought it. Counted once an op.
struct NextCopy(HashMap<(crate::store::BlobId, u32), u32>);

impl NextCopy {
    fn of(st: &DocState) -> NextCopy {
        let mut next = HashMap::new();
        for (_, def, _) in st.pages() {
            let n = next.entry((def.base.src.id(), def.base.index)).or_insert(0);
            *n = (*n).max(def.copy + 1);
        }
        NextCopy(next)
    }

    /// The copy a page showing `base` takes.
    fn take(&mut self, base: &PageBase) -> u32 {
        let n = self.0.entry((base.src.id(), base.index)).or_insert(0);
        *n += 1;
        *n - 1
    }

    /// A new page showing page `index` of `src`, as its next copy:
    /// nothing drawn over it, and the page's own boxes.
    fn page(&mut self, src: &Src, index: u32) -> (Arc<PageDef>, Arc<Geom>) {
        let base = PageBase {
            src: src.clone(),
            index,
        };
        let copy = self.take(&base);
        let def = PageDef::new(base, copy, ContentStack::empty());
        (Arc::new(def), Arc::new(src.geom(index)))
    }
}
