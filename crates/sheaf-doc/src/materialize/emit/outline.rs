//! The outline as a save writes it (M10.5 §11.3, bug 15): the state's
//! bookmarks, each in place by provenance.
//!
//! - **A bookmark read from the primary** is written at its item's number
//!   (the first in order, of bookmarks read from one item), with the item's
//!   keys sheaf doesn't model and its `/SE`. Its `/Title`, `/C` and `/F`
//!   stay as written while they read as the bookmark's, and its `/Dest` or
//!   `/A` while the bookmark goes where they go: to the page written as
//!   the object they name, so a named destination or a GoTo action stays
//!   one. An item nothing changed, its place in the tree included, is the
//!   primary's object as it is, which a Rewrite copies byte for byte.
//! - **A [`Target::Kept`] bookmark** keeps its item's `/A` and `/Dest`, an
//!   action or a destination that isn't a page here, whichever file it was
//!   read from. Another file's are copied with what they reach, a page of
//!   that file that isn't written as null.
//! - **Anything else** is written from the record: the title, the color
//!   and style, and a `/Dest` to its page, where on it in PageSpace.
//! - **The structure** (`/Parent`, `/Prev`, `/Next`, `/First`, `/Last`,
//!   `/Count`) is the state's: a `/Count` counts what shows below an open
//!   bookmark, negated for a closed one.
//!
//! The root is the primary's, at its number, with its keys but those the
//! state owns ([`OUTLINES_OWNED`]). With no bookmarks there's no
//! `/Outlines`.

use std::collections::HashMap;

use im::Vector;
use lopdf::{Dictionary, Object};
use sheaf_edit::DestView;

use super::Cx;
use crate::materialize::copy::Scope;
use crate::materialize::key::ObjKey;
use crate::materialize::record::OUTLINES_OWNED;
use crate::model::{Bookmark, OutlineId, Target};
use crate::open::Dests;
use crate::source::ObjRef;
use crate::store::{BlobId, Src};

/// The keys an item's place in the tree is.
const STRUCTURE: [&[u8]; 6] = [b"Parent", b"Prev", b"Next", b"First", b"Last", b"Count"];

/// The outline's root, its bookmarks emitted: a reference to it, or None
/// when there are no bookmarks.
pub(crate) fn outline(cx: &mut Cx) -> Option<Object> {
    let input = cx.input;
    let st = &input.target;
    if st.outline_roots().is_empty() {
        return None;
    }
    let primary = &input.primary;
    let pdf = primary.pdf();
    let held = (pdf.catalog().ok()).and_then(|c| c.get(b"Outlines").ok());
    let root_at = held.and_then(|o| o.as_reference().ok());
    let read = (held.and_then(|o| pdf.dereference(o).ok())).and_then(|(_, o)| o.as_dict().ok());
    // What keeps its item's number, before anything refers to it.
    let root_owned = root_at.is_some_and(|at| cx.own(ObjKey::OutlinesRoot, at));
    for (_, id, b) in st.bookmarks_preorder() {
        if let Some(r) = b.src.as_ref().filter(|r| r.src.id() == primary.id()) {
            cx.own(ObjKey::Bookmark(id), r.id);
        }
    }
    let mut items = Items {
        primary,
        dests: HashMap::new(),
    };
    let (first, last, count) = items.level(cx, ObjKey::OutlinesRoot, st.outline_roots());
    let mut root = Dictionary::new();
    let scope = Scope {
        src: primary,
        copy: 0,
    };
    // An Append writes the keys the state doesn't own as the saved file
    // holds them.
    let saved = cx.saved().and_then(|record| record.roots.outlines.as_ref());
    for (key, value) in saved.or(read).into_iter().flat_map(|d| d.iter()) {
        if !OUTLINES_OWNED.contains(&key.as_slice()) {
            let value = cx.sym(value, &scope);
            root.set(key.clone(), value);
        }
    }
    root.set("Type", Object::Name(b"Outlines".to_vec()));
    let place = Place {
        first,
        last,
        count: first.map(|_| count),
        ..Place::default()
    };
    place.write(cx, &mut root);
    match (root_at, read) {
        (Some(at), Some(read))
            if root_owned
                && super::annots::is(read, b"Type", b"Outlines")
                && place.is(cx, read, &[b"First", b"Last", b"Count"]) =>
        {
            let raw = ObjRef {
                src: primary.clone(),
                id: at,
            };
            cx.push_raw(ObjKey::OutlinesRoot, raw);
        }
        _ => cx.push(ObjKey::OutlinesRoot, Object::Dictionary(root)),
    }
    Some(cx.placeholder(ObjKey::OutlinesRoot))
}

/// An item's place in the tree, by key.
#[derive(Default)]
struct Place {
    parent: Option<ObjKey>,
    prev: Option<ObjKey>,
    next: Option<ObjKey>,
    first: Option<ObjKey>,
    last: Option<ObjKey>,
    count: Option<i64>,
}

impl Place {
    fn links(&self) -> [(&'static [u8], Option<ObjKey>); 5] {
        [
            (b"Parent", self.parent),
            (b"Prev", self.prev),
            (b"Next", self.next),
            (b"First", self.first),
            (b"Last", self.last),
        ]
    }

    /// The place written into `out`.
    fn write(&self, cx: &mut Cx, out: &mut Dictionary) {
        for (name, key) in self.links() {
            if let Some(key) = key {
                out.set(name, cx.placeholder(key));
            }
        }
        if let Some(count) = self.count {
            out.set("Count", count);
        }
    }

    /// Whether `item` holds this place already, as far as `keys` go: each
    /// link to the number the key it names keeps, and the count, as
    /// written.
    fn is(&self, cx: &Cx, item: &Dictionary, keys: &[&[u8]]) -> bool {
        let links = self
            .links()
            .into_iter()
            .filter(|(name, _)| keys.contains(name));
        let links_hold = links.into_iter().all(|(name, key)| {
            let at = key.map(|key| cx.owned_at(key));
            match (item.get(name).ok(), at) {
                (None, None) => true,
                (Some(&Object::Reference(r)), Some(Some(at))) => r == at,
                _ => false,
            }
        });
        let count = match item.get(b"Count").ok() {
            None => None,
            Some(&Object::Integer(n)) => Some(n),
            Some(_) => return false,
        };
        links_hold && (!keys.contains(&b"Count".as_slice()) || count == self.count)
    }
}

/// The primary, and how each file's items read, made the first time one
/// of its items is written.
struct Items<'a> {
    primary: &'a Src,
    dests: HashMap<BlobId, Dests<'a>>,
}

impl<'a> Items<'a> {
    /// The bookmarks `kids`, under `parent`, emitted: the first's and the
    /// last's keys, and how many items show below `parent` while it's
    /// open.
    fn level(
        &mut self,
        cx: &mut Cx<'a>,
        parent: ObjKey,
        kids: &'a Vector<OutlineId>,
    ) -> (Option<ObjKey>, Option<ObjKey>, i64) {
        let input = cx.input;
        let st = &input.target;
        let nodes: Vec<(OutlineId, &'a Bookmark)> = (kids.iter())
            .filter_map(|&id| Some((id, &**st.bookmark(id)?)))
            .collect();
        let keys: Vec<ObjKey> = nodes.iter().map(|&(id, _)| ObjKey::Bookmark(id)).collect();
        let mut shown = 0;
        for (i, &(id, b)) in nodes.iter().enumerate() {
            let (first, last, below) = self.level(cx, keys[i], &b.kids);
            let place = Place {
                parent: Some(parent),
                prev: i.checked_sub(1).map(|i| keys[i]),
                next: keys.get(i + 1).copied(),
                first,
                last,
                count: first.map(|_| if b.open { below } else { -below }),
            };
            shown += 1 + if b.open { below } else { 0 };
            let (mut out, untouched) = self.item(cx, id, b);
            match untouched {
                Some((r, item)) if place.is(cx, item, &STRUCTURE) => {
                    cx.push_raw(keys[i], r.clone());
                }
                _ => {
                    place.write(cx, &mut out);
                    cx.push(keys[i], Object::Dictionary(out));
                }
            }
        }
        (keys.first().copied(), keys.last().copied(), shown)
    }

    /// Bookmark `id`, `b`, but for its place in the tree: what its item
    /// keeps, then what the record says. And its item, when it's the
    /// primary's at its own number and the bookmark changed nothing of it
    /// but perhaps its place.
    fn item(
        &mut self,
        cx: &mut Cx<'a>,
        id: OutlineId,
        b: &'a Bookmark,
    ) -> (Dictionary, Option<(&'a ObjRef, &'a Dictionary)>) {
        let mut out = Dictionary::new();
        let read = (b.src.as_ref()).and_then(|r| Some((r, r.src.pdf().get_dictionary(r.id).ok()?)));
        let mut same = false;
        if let Some((r, item)) = read {
            let src = &r.src;
            let primary = src.id() == self.primary.id();
            same = primary && cx.owned_at(ObjKey::Bookmark(id)) == Some(r.id);
            let dests = (self.dests.entry(src.id())).or_insert_with(|| Dests::new(src));
            let (color, style) = dests.style(item);
            let goes = match &b.target {
                Some(Target::Kept) => true,
                Some(Target::Page { page, view }) => {
                    primary
                        && (dests.page_of(item))
                            .is_some_and(|(at, read)| cx.owns(*page, at) && read == *view)
                }
                None => false,
            };
            let scope = Scope { src, copy: 0 };
            for (key, value) in item.iter() {
                let keep = match key.as_slice() {
                    k if STRUCTURE.contains(&k) => continue,
                    b"Title" => dests.title(item) == *b.title,
                    b"C" => color == b.color,
                    b"F" => style == b.style,
                    b"SE" => primary,
                    b"A" | b"Dest" => goes,
                    _ => true,
                };
                if keep {
                    let value = cx.sym(value, &scope);
                    out.set(key.clone(), value);
                } else {
                    same = false;
                }
            }
        }
        if !out.has(b"Title") {
            out.set("Title", crate::save::text_string(&b.title));
            same = false;
        }
        if let Some(Target::Page { page, view }) = &b.target
            && !out.has(b"Dest")
            && !out.has(b"A")
        {
            let page = cx.placeholder(ObjKey::Page(*page));
            out.set("Dest", dest(page, view.as_ref()));
            same = false;
        }
        if !out.has(b"C")
            && let Some([r, g, b]) = b.color
        {
            let c = [r, g, b].map(|c| Object::from(f32::from(c) / 255.0));
            out.set("C", c.to_vec());
            same = false;
        }
        let flags = i64::from(b.style.italic) | i64::from(b.style.bold) << 1;
        if !out.has(b"F") && flags != 0 {
            out.set("F", flags);
            same = false;
        }
        (out, read.filter(|_| same))
    }
}

/// A bookmark's `/Dest`: `page` and, when it has one, its position (see
/// [`DestView`], in PageSpace as the file has it); otherwise the top of
/// the page.
fn dest(page: Object, view: Option<&DestView>) -> Vec<Object> {
    let num = |v: Option<f32>| v.map(Object::from).unwrap_or(Object::Null);
    let mut dest = vec![page];
    match view {
        Some(DestView::Xyz { left, top, zoom }) => {
            dest.extend(["XYZ".into(), num(*left), num(*top), num(*zoom)]);
        }
        Some(DestView::Other { kind, params }) => {
            dest.push(Object::Name(kind.as_bytes().to_vec()));
            dest.extend(params.iter().map(|p| num(*p)));
        }
        None => dest.extend(["XYZ".into(), Object::Null, Object::Null, Object::Null]),
    }
    dest
}
