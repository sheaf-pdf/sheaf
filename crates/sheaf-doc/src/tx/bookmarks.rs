//! Bookmark ops lowered (M10.5 §3.8): each bookmark is its own record,
//! its kids by id, and the top level is [`Key::OutlineRoots`].

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use im::Vector;

use super::error::EditError;
use super::resolve::{At, BookmarkChange};
use super::{Lower, Made, splice};
use crate::model::{Bookmark, DocState, OutlineId, PageId, Target};
use crate::prim::{Key, Rec, Val};

/// Adds bookmark `b` under `parent` (or at the top level), `at` there.
pub(crate) fn add(
    lw: &mut Lower,
    parent: Option<OutlineId>,
    at: At,
    b: Bookmark,
) -> Result<(), EditError> {
    let id = lw.outline_id();
    lw.rec
        .put(Key::Bookmark(id), Some(Val::Bookmark(Arc::new(b))));
    insert(lw, parent, at, id)?;
    lw.made.push(Made::Bookmark(id));
    Ok(())
}

/// `change` made to each of `ids`: what it names, the rest as it was. A
/// new page is at its top unless the view says otherwise.
pub(crate) fn patch(lw: &mut Lower, ids: &[OutlineId], change: &BookmarkChange) {
    for &id in ids {
        let mut b = Bookmark::clone(lw.rec.bookmark(id).expect("resolved"));
        if let Some(t) = &change.title {
            b.title = t.as_str().into();
        }
        match change.target {
            sheaf_edit::Patch::Keep => {}
            sheaf_edit::Patch::Clear => b.target = None,
            // A new page: the old position meant the old page.
            sheaf_edit::Patch::Set(page) => b.target = Some(Target::Page { page, view: None }),
        }
        if let Some(Target::Page { view, .. }) = &mut b.target {
            change.view.clone().apply(view);
        }
        change.color.clone().apply(&mut b.color);
        if let Some(s) = change.style {
            b.style = s;
        }
        lw.rec
            .put(Key::Bookmark(id), Some(Val::Bookmark(Arc::new(b))));
    }
}

/// Moves `ids` and what's under them under `parent` (or to the top
/// level), none of them, the first `at` there and each after the one
/// before. With `with_pages`, the pages each spans move with it: to just
/// after the span of the bookmark before it in the new order (or the
/// start).
pub(crate) fn move_bookmarks(
    lw: &mut Lower,
    ids: &[OutlineId],
    parent: Option<OutlineId>,
    at: At,
    with_pages: bool,
) -> Result<(), EditError> {
    let mut at = at;
    for &id in ids {
        let span = with_pages.then(|| bookmark_span(&lw.rec, id)).flatten();
        unlink(&mut lw.rec, id).expect("resolved");
        insert(lw, parent, at, id)?;
        at = At::After(id);
        if let Some(span) = span {
            move_span(&mut lw.rec, id, span);
        }
    }
    Ok(())
}

/// Puts `span`, the pages bookmark `id` spanned, just after the span of
/// the bookmark before it in the outline's order now, or first.
fn move_span(rec: &mut Rec, id: OutlineId, span: Vec<PageId>) {
    let flat = rec.bookmarks_preorder();
    let mine = flat.iter().position(|&(_, at, _)| at == id).expect("moved");
    let moving: std::collections::HashSet<PageId> = span.iter().copied().collect();
    let rest: Vector<PageId> = (rec.order().iter())
        .filter(|id| !moving.contains(id))
        .copied()
        .collect();
    let prev = flat[..mine]
        .iter()
        .rev()
        .find(|(_, _, b)| b.page().is_some());
    let dest = match prev {
        Some(&(_, prev, b)) => {
            // The end of the previous bookmark's span, in `rest`.
            let at: HashMap<PageId, usize> =
                rest.iter().enumerate().map(|(i, &id)| (id, i)).collect();
            let pos = |id| at.get(&id).copied();
            match span_at(&flat, prev, rest.len(), pos) {
                Some(owned) => owned.end,
                None => b.page().and_then(pos).map_or(0, |i| i + 1),
            }
        }
        None => 0,
    };
    rec.put(Key::Order, Some(Val::Order(splice(&rest, dest, span))));
}

/// Deletes `ids` and what's under them.
pub(crate) fn delete(lw: &mut Lower, ids: &[OutlineId]) {
    for &id in ids {
        // Under one deleted before it.
        if lw.rec.bookmark(id).is_none() {
            continue;
        }
        unlink(&mut lw.rec, id).expect("in the outline");
        for gone in subtree(&lw.rec, id) {
            lw.rec.put(Key::Bookmark(gone), None);
        }
    }
}

/// The bookmarks under `parent`, or at the top level: None when there's
/// no such bookmark.
pub(crate) fn kids_of(st: &DocState, parent: Option<OutlineId>) -> Option<Vector<OutlineId>> {
    match parent {
        None => Some(st.outline_roots().clone()),
        Some(p) => Some(st.bookmark(p)?.kids.clone()),
    }
}

/// Puts `kids` under `parent`, a bookmark there is, or at the top level.
pub(crate) fn put_kids(rec: &mut Rec, parent: Option<OutlineId>, kids: Vector<OutlineId>) {
    match parent {
        None => rec.put(Key::OutlineRoots, Some(Val::OutlineRoots(kids))),
        Some(p) => {
            let parent = rec.bookmark(p).expect("a parent in the outline");
            let b = Bookmark {
                kids,
                ..Bookmark::clone(parent)
            };
            rec.put(Key::Bookmark(p), Some(Val::Bookmark(Arc::new(b))));
        }
    }
}

/// Puts bookmark `id` under `parent` (or at the top level), `at` there.
fn insert(
    lw: &mut Lower,
    parent: Option<OutlineId>,
    at: At,
    id: OutlineId,
) -> Result<(), EditError> {
    let mut kids = kids_of(&lw.rec, parent).ok_or_else(|| lw.not_found("the parent bookmark"))?;
    let i = match at {
        At::First => 0,
        At::Last => kids.len(),
        At::After(a) => {
            let i = kids.index_of(&a);
            i.ok_or_else(|| lw.invalid("the bookmark it goes after isn't among its siblings"))? + 1
        }
    };
    kids.insert(i, id);
    put_kids(&mut lw.rec, parent, kids);
    Ok(())
}

/// Takes bookmark `id` from under its parent, or from the top level; its
/// record and those under it stay. None when it isn't in the outline.
fn unlink(rec: &mut Rec, id: OutlineId) -> Option<()> {
    let (parent, _) = rec.bookmark_place(id)?;
    let mut kids = kids_of(rec, parent)?;
    kids.retain(|&k| k != id);
    put_kids(rec, parent, kids);
    Some(())
}

/// Bookmark `id` and every one under it, in pre-order.
pub(crate) fn subtree(st: &DocState, id: OutlineId) -> Vec<OutlineId> {
    let mut out = Vec::new();
    let mut next = vec![id];
    while let Some(id) = next.pop() {
        out.push(id);
        if let Some(b) = st.bookmark(id) {
            next.extend(b.kids.iter().rev());
        }
    }
    out
}

/// The pages a bookmark "owns": from its first target (in document order)
/// up to, but not including, the first page targeted by a bookmark outside
/// its subtree that comes after it in reading order.
pub(crate) fn bookmark_span(st: &DocState, id: OutlineId) -> Option<Vec<PageId>> {
    let flat = st.bookmarks_preorder();
    let owned = span_at(&flat, id, st.page_count(), |p| st.pos_of(p))?;
    Some(
        st.order()
            .iter()
            .skip(owned.start)
            .take(owned.len())
            .copied()
            .collect(),
    )
}

/// The positions [`bookmark_span`] gives bookmark `id`, of `count` pages
/// whose positions `pos` gives, under the outline `flat` (in pre-order,
/// where a bookmark's subtree follows it).
fn span_at(
    flat: &[(u16, OutlineId, &Bookmark)],
    id: OutlineId,
    count: usize,
    pos: impl Fn(PageId) -> Option<usize>,
) -> Option<Range<usize>> {
    let at = flat.iter().position(|&(_, other, _)| other == id)?;
    let depth = flat[at].0;
    let len = 1 + flat[at + 1..]
        .iter()
        .take_while(|(d, _, _)| *d > depth)
        .count();
    let page = |&(_, _, b): &(u16, OutlineId, &Bookmark)| b.page().and_then(&pos);
    let start = flat[at..at + len].iter().filter_map(page).min()?;
    let end = (flat[..at].iter().chain(&flat[at + len..]))
        .filter_map(page)
        .filter(|&i| i > start)
        .min()
        .unwrap_or(count);
    Some(start..end)
}
