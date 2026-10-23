//! Copying objects from one file into another (M10.5 §11.2): a page's
//! dictionary or any object with the closure of what it references, under
//! new numbers, never following a reference to a page that isn't being
//! copied too (M9 bug 14). And the inheritable keys a page takes from the
//! nodes above it, which a page placed elsewhere writes on itself.
//!
//! The emitter copies by key ([`Closure`]): an object reached from a copy
//! is [`ObjKey::Base`] of its source at the copy's number, so each object
//! is copied once a copy, whichever path reached it. Nor is a copy
//! followed into its file's document-level objects (its catalog, form,
//! page tree, structure tree, outline, name trees and information), which
//! the written file has its own of: a signature's `/Reference /Data` names
//! its file's catalog, and copying that copied the file's whole form, its
//! widgets on no page. A PDF built in memory
//! ([`serialize::built`](crate::serialize::built)) copies into its graph
//! by number ([`copy_object`], [`remap`]): a stamp's page made a form, a
//! form back to a page, another tool's comment gathered, comments copied
//! out to an FDF file.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use anyhow::Result;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId};

use super::emit::Syms;
use super::key::ObjKey;
use super::record::ObjMap;
use crate::attachments::Objects;
use crate::model::{DocState, PageId};
use crate::source::PageObj;
use crate::store::{BlobId, Src};

/// Where references are followed from: `src`'s objects, as `copy` of
/// them. The primary's copy 0 is its objects where they are.
#[derive(Clone)]
pub(crate) struct Scope<'a> {
    pub(crate) src: &'a Src,
    pub(crate) copy: u32,
}

/// The objects copies reach, by key, copied once each.
pub(crate) struct Closure<'a> {
    primary: BlobId,
    /// The pages written: what a copied reference to a page names.
    pages: Pages<'a>,
    /// Objects reached and not copied yet, in the order they were reached.
    queue: VecDeque<(Src, ObjectId, u32)>,
    queued: HashSet<ObjKey>,
    /// Objects the emitter writes itself, under their keys: never copied.
    claimed: HashSet<ObjKey>,
    /// Each source's document-level objects, read when a copy of it first
    /// names an object: what a reference to each is ([`document_level`]).
    levels: HashMap<BlobId, HashMap<ObjectId, Option<ObjKey>>>,
    /// How many objects reached couldn't be read, and were copied as null.
    pub(crate) unreadable: usize,
    /// An Append's saved file's map: an object it has is referred to there,
    /// never copied again, unless it's of one of `relinked`.
    saved: Option<Arc<ObjMap>>,
    /// The sources whose pages a copy's reference names otherwise than the
    /// saved file's copies of their objects do (a copy of the page came or
    /// went): the objects an Append copies of them are copied again, so
    /// what they name is the page written as now (§11.2).
    relinked: BTreeSet<BlobId>,
}

/// The pages an emission writes, as a copied reference finds them.
pub(crate) enum Pages<'a> {
    /// A save's: each page, by its source page and its copy as the page is
    /// written ([`written_pages`]).
    ///
    /// [`written_pages`]: super::emit::written_pages
    Written(BTreeMap<(PageObj, u32), PageId>),
    /// An overlay's: the state's pages ([`DocState::page_named`]), of which
    /// those `within` it are written there. A reference to any other names
    /// nothing in it, though a link to one still leads somewhere.
    Overlay {
        st: &'a DocState,
        within: HashSet<PageId>,
    },
}

impl Pages<'_> {
    /// The page written as copy `copy` of page `page` of `blob`, else as
    /// its lowest copy written ([`page_named`]); in an overlay, one that's
    /// written there.
    fn named(&self, blob: BlobId, page: ObjectId, copy: u32) -> Option<PageId> {
        let at = PageObj { blob, id: page };
        match self {
            Pages::Written(pages) => page_named(pages, at, copy),
            Pages::Overlay { st, within } => {
                (st.page_named(at, copy)).filter(|p| within.contains(p))
            }
        }
    }

    /// Whether copy 0 of page `page` of `blob` is written, or with `any`,
    /// any copy of it: in an overlay, whether the document has it.
    fn has(&self, blob: BlobId, page: ObjectId, any: bool) -> bool {
        let at = PageObj { blob, id: page };
        match (self, any) {
            (Pages::Written(pages), false) => pages.contains_key(&(at, 0)),
            (Pages::Written(pages), true) => pages.range((at, 0)..=(at, u32::MAX)).next().is_some(),
            (Pages::Overlay { st, .. }, any) => page_of(st, blob, page, any).is_some(),
        }
    }
}

/// The page of `st` showing copy 0 of page `page` of `blob`, or with
/// `any`, the one a reference to any copy of it names: where an overlay
/// finds a reference to that page leads, written there or not.
pub(crate) fn page_of(st: &DocState, blob: BlobId, page: ObjectId, any: bool) -> Option<PageId> {
    let at = PageObj { blob, id: page };
    match any {
        false => st.page_showing(at, 0),
        true => st.page_named(at, 0),
    }
}

impl<'a> Closure<'a> {
    /// A closure over `primary`'s blob, whose copy 0 isn't copied, with
    /// `pages` the pages written.
    pub(crate) fn new(primary: BlobId, pages: Pages<'a>) -> Closure<'a> {
        Closure {
            primary,
            pages,
            queue: VecDeque::new(),
            queued: HashSet::new(),
            claimed: HashSet::new(),
            levels: HashMap::new(),
            unreadable: 0,
            saved: None,
            relinked: BTreeSet::new(),
        }
    }

    /// The closure of an Append onto the file whose map is `map`, copying
    /// the objects of `relinked`'s sources again.
    pub(crate) fn saved(&mut self, map: Arc<ObjMap>, relinked: &BTreeSet<BlobId>) {
        self.saved = Some(map);
        self.relinked = relinked.clone();
    }

    /// Whether `key` is an object of a source whose objects an Append
    /// copies again, though the saved file has them.
    pub(crate) fn relinks(&self, key: &ObjKey) -> bool {
        matches!(key, ObjKey::Base { blob, .. } if self.relinked.contains(blob))
    }

    /// `key`, an object of a source the emitter writes itself (another
    /// tool's annotation on a copied page, its `/StructParent` taken off):
    /// the closure never copies it, whatever reaches it. Whether it wasn't
    /// claimed already.
    pub(crate) fn claim(&mut self, key: ObjKey) -> bool {
        self.claimed.insert(key)
    }

    /// Whether page `page` of `scope`'s source is written, as a reference
    /// to it from `scope` resolves: as the primary's copy 0 itself, or
    /// else as any copy. In an overlay, whether a save would write it.
    pub(crate) fn writes_page(&self, page: ObjectId, scope: &Scope) -> bool {
        let blob = scope.src.id();
        let own = blob == self.primary && scope.copy == 0;
        self.pages.has(blob, page, !own)
    }

    /// `obj`, an object of `scope`'s source, with each reference a
    /// placeholder: to the primary's own object where `scope` is the
    /// primary's copy 0; otherwise to the page written as that page's copy
    /// in `scope`, else as its lowest copy written (a duplicate's link to
    /// another page of its file leads there), or null when none is (never
    /// followed: bug 14); to a document-level object of its file as the
    /// written file's ([`document_level`]); and to any other object as
    /// `scope`'s copy of it, which the closure then copies. A stream's
    /// `/Length` is the writer's to give.
    pub(crate) fn sym(&mut self, syms: &mut Syms, obj: &Object, scope: &Scope) -> Object {
        match obj {
            Object::Reference(r) => self.reference(syms, *r, scope),
            Object::Array(items) => {
                Object::Array(items.iter().map(|o| self.sym(syms, o, scope)).collect())
            }
            Object::Dictionary(d) => Object::Dictionary(self.sym_dict(syms, d, scope, false)),
            Object::Stream(s) => {
                let dict = self.sym_dict(syms, &s.dict, scope, true);
                let mut stream = lopdf::Stream::new(dict, s.content.clone());
                stream.allows_compression = s.allows_compression;
                Object::Stream(stream)
            }
            other => other.clone(),
        }
    }

    /// [`sym`](Self::sym) of dictionary `d`, a stream's when `stream`.
    pub(crate) fn sym_dict(
        &mut self,
        syms: &mut Syms,
        d: &Dictionary,
        scope: &Scope,
        stream: bool,
    ) -> Dictionary {
        let mut out = Dictionary::new();
        for (k, v) in d.iter() {
            if !(stream && k == b"Length") {
                out.set(k.clone(), self.sym(syms, v, scope));
            }
        }
        out
    }

    fn reference(&mut self, syms: &mut Syms, r: ObjectId, scope: &Scope) -> Object {
        let blob = scope.src.id();
        if blob == self.primary && scope.copy == 0 {
            return syms.placeholder(ObjKey::Base {
                blob,
                obj: r,
                copy: 0,
            });
        }
        let pdf = scope.src.pdf();
        let levels = (self.levels.entry(blob)).or_insert_with(|| document_level(pdf));
        let level = match levels.get(&r) {
            Some(&to) => Some(to),
            None => is_type(pdf, r, b"Catalog").then_some(Some(ObjKey::Catalog)),
        };
        if let Some(to) = level {
            return to.map_or(Object::Null, |key| syms.placeholder(key));
        }
        if is_page_node(pdf, r) {
            return match self.pages.named(blob, r, scope.copy) {
                Some(page) => syms.placeholder(ObjKey::Page(page)),
                None => Object::Null,
            };
        }
        let key = ObjKey::Base {
            blob,
            obj: r,
            copy: scope.copy,
        };
        let had = !self.relinked.contains(&blob)
            && (self.saved.as_ref()).is_some_and(|map| map.get(&key).is_some());
        if !had && self.queued.insert(key) {
            self.queue.push_back((scope.src.clone(), r, scope.copy));
        }
        syms.placeholder(key)
    }

    /// The next object reached and not copied yet: its key, and the
    /// object with placeholders, which may reach more. An object the
    /// source hasn't got is null, as a reference to it reads, and so is
    /// one it can't read (it doesn't parse, or decrypt), counted in
    /// [`unreadable`](Self::unreadable): the copy loses only what was lost
    /// already. And so is a widget annotation: the widgets a save writes
    /// are its pages' annotations, which the emitter writes itself, so one
    /// the closure reaches (another widget of a field a copied widget
    /// names, which no form reaches) would be a copy on no page.
    pub(crate) fn next(&mut self, syms: &mut Syms) -> Option<Result<(ObjKey, Object)>> {
        let (src, obj, copy) = loop {
            let (src, obj, copy) = self.queue.pop_front()?;
            let key = ObjKey::Base {
                blob: src.id(),
                obj,
                copy,
            };
            if !self.claimed.contains(&key) {
                break (src, obj, copy);
            }
        };
        let key = ObjKey::Base {
            blob: src.id(),
            obj,
            copy,
        };
        let read = match src.pdf().object_unkept(obj) {
            Ok(o) => o,
            Err(lopdf::Error::MissingXrefEntry | lopdf::Error::ObjectNotFound(_)) => {
                return Some(Ok((key, Object::Null)));
            }
            Err(e) => {
                log::warn!("copying object {} {} as null: {e}", obj.0, obj.1);
                self.unreadable += 1;
                return Some(Ok((key, Object::Null)));
            }
        };
        if is_widget(&read) {
            return Some(Ok((key, Object::Null)));
        }
        let scope = Scope { src: &src, copy };
        Some(Ok((key, self.sym(syms, &read, &scope))))
    }
}

/// The page of `pages` (the pages written, by their source page and
/// their copy) a reference to source page `page`, read as `copy` of it,
/// names: the page written as that copy, else as its lowest copy written;
/// None when no copy is. What [`Closure::sym`] writes for one, but from
/// the primary's copy 0, whose references are its own objects' numbers.
pub(crate) fn page_named(
    pages: &BTreeMap<(PageObj, u32), PageId>,
    page: PageObj,
    copy: u32,
) -> Option<PageId> {
    (pages.get(&(page, copy)))
        .or_else(|| {
            (pages.range((page, 0)..=(page, u32::MAX)))
                .next()
                .map(|(_, p)| p)
        })
        .copied()
}

/// What a copy's reference to each of `pdf`'s document-level objects is:
/// its catalog the written catalog, its form the written form (null when
/// the file has none); its page tree's root, outline, structure tree, name
/// dictionary, destinations, threads, optional content and page labels,
/// and its information dictionary, null. The written file has its own of
/// each, and a copy of one would bring its file's whole structure along.
/// Any other dictionary typed `/Catalog` is the written catalog too.
pub(crate) fn document_level(pdf: &lopdf::LazyPdf) -> HashMap<ObjectId, Option<ObjKey>> {
    let mut out = HashMap::new();
    let trailer = pdf.trailer();
    if let Ok(info) = trailer.get(b"Info").and_then(Object::as_reference) {
        out.insert(info, None);
    }
    let Ok(root) = trailer.get(b"Root").and_then(Object::as_reference) else {
        return out;
    };
    out.insert(root, Some(ObjKey::Catalog));
    for (k, v) in pdf.get_dictionary(root).into_iter().flat_map(|c| c.iter()) {
        let Ok(r) = v.as_reference() else {
            continue;
        };
        let to = match k.as_slice() {
            b"AcroForm" => Some(ObjKey::AcroForm),
            b"Pages" | b"Outlines" | b"StructTreeRoot" | b"Names" | b"Dests" | b"Threads"
            | b"OCProperties" | b"PageLabels" => None,
            _ => continue,
        };
        out.entry(r).or_insert(to);
    }
    out
}

/// Whether `obj` is a widget annotation.
fn is_widget(obj: &Object) -> bool {
    let subtype = obj.as_dict().ok().and_then(|d| d.get(b"Subtype").ok());
    subtype.and_then(|s| s.as_name().ok()) == Some(b"Widget".as_slice())
}

/// Whether object `id` of `pdf` is a dictionary of `/Type` `kind`.
fn is_type<P: Objects + ?Sized>(pdf: &P, id: ObjectId, kind: &[u8]) -> bool {
    matches!(
        pdf.get_object(id),
        Ok(Object::Dictionary(d)) if d.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(kind)
    )
}

/// A page or a node of the page tree.
pub(crate) fn is_page_node<P: Objects + ?Sized>(pdf: &P, id: ObjectId) -> bool {
    matches!(
        pdf.get_object(id),
        Ok(Object::Dictionary(d)) if d
            .get(b"Type")
            .ok()
            .and_then(|t| t.as_name().ok())
            .is_some_and(|t| t == b"Page" || t == b"Pages")
    )
}

/// `id` copied into `dst`, or None for a page that isn't being copied
/// (bug 14: following a link, or a page's `/Parent`, used to copy the
/// whole file it came from).
pub(crate) fn copy_object<P: Objects + ?Sized>(
    src: &P,
    id: ObjectId,
    dst: &mut Pdf,
    map: &mut HashMap<ObjectId, ObjectId>,
) -> Result<Option<ObjectId>> {
    if let Some(&m) = map.get(&id) {
        return Ok(Some(m));
    }
    if is_page_node(src, id) {
        return Ok(None);
    }
    let new_id = dst.new_object_id();
    map.insert(id, new_id);
    // Placeholder first so cycles terminate.
    dst.objects.insert(new_id, Object::Null);
    let obj = src.get_object(id)?.clone();
    let obj = remap(src, &obj, dst, map)?;
    dst.objects.insert(new_id, obj);
    Ok(Some(new_id))
}

/// `obj` from `src` in `dst`, copying what it references. A page is never
/// copied this way: one in `map` (being copied too) is referred to there,
/// and any other becomes null, for [`super::kept::keep_only`] to tidy.
pub(crate) fn remap<P: Objects + ?Sized>(
    src: &P,
    obj: &Object,
    dst: &mut Pdf,
    map: &mut HashMap<ObjectId, ObjectId>,
) -> Result<Object> {
    Ok(match obj {
        Object::Reference(r) => match copy_object(src, *r, dst, map)? {
            Some(id) => Object::Reference(id),
            None => Object::Null,
        },
        Object::Array(items) => Object::Array(
            items
                .iter()
                .map(|o| remap(src, o, dst, map))
                .collect::<Result<_>>()?,
        ),
        Object::Dictionary(d) => {
            let mut out = Dictionary::new();
            for (k, v) in d.iter() {
                out.set(k.clone(), remap(src, v, dst, map)?);
            }
            Object::Dictionary(out)
        }
        Object::Stream(s) => {
            let mut dict = Dictionary::new();
            for (k, v) in s.dict.iter() {
                dict.set(k.clone(), remap(src, v, dst, map)?);
            }
            let mut stream = lopdf::Stream::new(dict, s.content.clone());
            stream.allows_compression = s.allows_compression;
            Object::Stream(stream)
        }
        other => other.clone(),
    })
}
