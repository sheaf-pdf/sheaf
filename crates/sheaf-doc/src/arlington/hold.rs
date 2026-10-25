//! What a save wrote, to hold to the model ([`Hold::written`]): the
//! objects sheaf made, each built whole or emitted over a source's
//! dictionary as its [`ObjKey`] says (the edit API's plan, §11.3), and
//! never a copy of a file's object, which is the file's, faults and all.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use lopdf::{LazyPdf, Object, ObjectId, XrefKind};

use super::check::{Hold, Made, each_reference, object_header};
use crate::materialize::key::{ObjKey, Part};
use crate::materialize::record::ObjMap;
use crate::model::{AnnotBody, DocState};
use crate::source::Limits;
use crate::store::{BlobId, BlobKind, MemoryKind, Origin, Store};

impl Hold {
    /// The objects a save of `st` wrote in `pdf`, the file it made: `map`
    /// is the map of its record, and `store` holds the blobs its keys
    /// name.
    ///
    /// - **An Append** (`appended`: the file's bytes, and where its section
    ///   starts, the saved file's length before it): every object its
    ///   cross-reference section lists, its cross-reference stream too,
    ///   but a copy of a file's object; listed, not read (`section_ids`),
    ///   so one that doesn't read is held, and checked as unresolved. An object's key is found in `map`, which names
    ///   every object a section writes; one it doesn't name is held as
    ///   merged. Nothing is, when the newest section starts before the
    ///   start, or doesn't read.
    /// - **A Rewrite** (`appended` None): the object of each key in
    ///   `map` that isn't a copy of a file's; when the blob whose objects
    ///   are at their own numbers (`map`'s identity) is one sheaf made in
    ///   memory, every object `map` doesn't name, as that blob's objects
    ///   are held; and the `/Encrypt` dictionary, which a Rewrite writes
    ///   as the source stores it, so as merged.
    ///
    /// Each key is held as `made` says; an object two keys name (a widget
    /// merged with its field) is merged when either says so.
    pub fn written(
        map: &ObjMap,
        st: &DocState,
        store: &Store,
        pdf: &LazyPdf,
        appended: Option<(&[u8], usize)>,
    ) -> Hold {
        let kind = |blob: &BlobId| memory_kind(store, blob);
        let pictures = RefCell::new(HashMap::new());
        let base = |blob: &BlobId, obj: ObjectId| match kind(blob)? {
            MemoryKind::Summary => {
                let mut pictures = pictures.borrow_mut();
                let copied = (pictures.entry(*blob)).or_insert_with(|| page_pictures(store, *blob));
                Some(if copied.contains(&obj) {
                    Made::Merged
                } else {
                    Made::Built
                })
            }
            kind => of_memory(kind),
        };
        let mut held = BTreeMap::new();
        match appended {
            Some((bytes, from)) => {
                let mut keyed: HashMap<ObjectId, Option<Made>> = HashMap::new();
                for (key, &id) in &map.keys {
                    let made = made(key, st, &base);
                    keyed
                        .entry(id)
                        .and_modify(|m| *m = (*m).max(made))
                        .or_insert(made);
                }
                for id in section_ids(pdf, bytes, from) {
                    match keyed.get(&id) {
                        Some(Some(made)) => hold(&mut held, id, *made),
                        Some(None) => {}
                        None => hold(&mut held, id, Made::Merged),
                    }
                }
            }
            None => {
                for (key, &id) in &map.keys {
                    if let Some(made) = made(key, st, &base) {
                        hold(&mut held, id, made);
                    }
                }
                let own = (map.identity.as_ref()).and_then(kind).and_then(of_memory);
                if let Some(made) = own {
                    let named: HashSet<ObjectId> = map.keys.values().copied().collect();
                    for id in pdf.ids().filter(|id| !named.contains(id)) {
                        hold(&mut held, id, made);
                    }
                }
                if let Some((_, at)) = pdf.encryption() {
                    hold(&mut held, at, Made::Merged);
                }
            }
        }
        Hold::Only(held)
    }
}

/// The objects the newest cross-reference section of `pdf`, whose bytes
/// are `bytes`, lists in use, read from the section as written (a table's
/// rows, or a cross-reference stream's entries: ISO 32000-1 7.5.4 and
/// 7.5.8.3), never from the objects, so one that doesn't read is listed
/// too. Empty when the section starts before byte `from`, so isn't the
/// save's, or doesn't read.
fn section_ids(pdf: &LazyPdf, bytes: &[u8], from: usize) -> Vec<ObjectId> {
    let facts = pdf.xref();
    let at = facts.header_offset + facts.start;
    let Some(section) = bytes.get(at..).filter(|_| at >= from && pdf.base_sound()) else {
        return Vec::new();
    };
    match facts.kind {
        XrefKind::Table => table_rows(section),
        XrefKind::Stream => stream_entries(pdf, section),
    }
    .unwrap_or_default()
}

/// The objects the cross-reference table at the start of `section` lists
/// in use (`n`), each subsection's rows to its `trailer`.
fn table_rows(section: &[u8]) -> Option<Vec<ObjectId>> {
    let mut words = (section.split(u8::is_ascii_whitespace)).filter(|w| !w.is_empty());
    let number = |word: &[u8]| std::str::from_utf8(word).ok()?.parse::<u32>().ok();
    if words.next()? != b"xref" {
        return None;
    }
    let mut ids = Vec::new();
    loop {
        let first = match words.next()? {
            b"trailer" => return Some(ids),
            word => number(word)?,
        };
        let count = number(words.next()?)?;
        for n in first..first.checked_add(count)? {
            let (_offset, generation, kind) = (words.next()?, words.next()?, words.next()?);
            match kind {
                b"n" => ids.push((n, u16::try_from(number(generation)?).ok()?)),
                b"f" => {}
                _ => return None,
            }
        }
    }
}

/// The objects the cross-reference stream at the start of `section`
/// lists in use: its entries of type 1 (at an offset) and 2 (in an object
/// stream), as its `/W` and `/Index` lay them out.
fn stream_entries(pdf: &LazyPdf, section: &[u8]) -> Option<Vec<ObjectId>> {
    let Object::Stream(stream) = pdf.get(object_header(section)?).ok()? else {
        return None;
    };
    let number = |o: &Object| usize::try_from(o.as_i64().ok()?).ok();
    let numbers = |key: &[u8]| -> Option<Vec<usize>> {
        let array = stream.dict.get(key).ok()?.as_array().ok()?;
        array.iter().map(number).collect()
    };
    let [w0, w1, w2] = numbers(b"W")?[..] else {
        return None;
    };
    if [w0, w1, w2].iter().any(|&w| w > 8) || w0 + w1 + w2 == 0 {
        return None;
    }
    let index = match stream.dict.has(b"Index") {
        true => numbers(b"Index")?,
        false => vec![0, number(stream.dict.get(b"Size").ok()?)?],
    };
    let limit = Limits::DEFAULT.reader.max_decompressed;
    let data = stream.get_plain_content_with_limit(limit).ok()?;
    let field = |bytes: &[u8]| bytes.iter().fold(0u64, |n, &b| n << 8 | u64::from(b));
    let mut rows = data.chunks_exact(w0 + w1 + w2);
    let mut ids = Vec::new();
    for subsection in index.chunks(2) {
        let [first, count] = *subsection else {
            return None;
        };
        for n in first..first.checked_add(count)? {
            let row = rows.next()?;
            // A type field 0 bytes wide is type 1 (Table 17).
            let kind = if w0 == 0 { 1 } else { field(&row[..w0]) };
            let n = u32::try_from(n).ok()?;
            match kind {
                1 => ids.push((n, u16::try_from(field(&row[w0 + w1..])).ok()?)),
                2 => ids.push((n, 0)),
                _ => {}
            }
        }
    }
    Some(ids)
}

/// `id` held as `made`, or as merged when it's held so already.
fn hold(held: &mut BTreeMap<ObjectId, Made>, id: ObjectId, made: Made) {
    let m = held.entry(id).or_insert(made);
    *m = (*m).max(made);
}

/// How sheaf made the object of `key`, written for `st`, `base` saying how
/// an object of a blob was (None for a file's): None for a copy of an
/// object it didn't make.
///
/// - **Built**: what a mark writes (its dictionary, its appearance, its
///   companion and its file's specification), what content draws with (a
///   resource and what it reaches), a block, the isolating `q`s and `Q`s,
///   what the generators are, and a pinned appearance.
/// - **Merged**, emitted over a source's dictionary whose keys it keeps:
///   the root objects, pages, another tool's comment and its pop-up, a
///   widget, a field, a bookmark, the page labels, a name tree of
///   `/Names`, a page's resources with sheaf's names added, and a page's
///   own content with ranges spliced out.
/// - **A source's object** as its blob was made: built for blank pages and
///   images as pages, which sheaf writes whole, and for a comment
///   summary's own objects; merged for the summary's pictures of the
///   document's pages ([`page_pictures`]) and a stamp's page, which draw a
///   file's pages; and a copy, not held, for a file's, a restored file's
///   and comments read as data.
fn made(
    key: &ObjKey,
    st: &DocState,
    base: &impl Fn(&BlobId, ObjectId) -> Option<Made>,
) -> Option<Made> {
    Some(match key {
        ObjKey::Annot(id) => match st.annot(*id).map(|a| &a.body) {
            Some(AnnotBody::Mark(_)) => Made::Built,
            Some(AnnotBody::Foreign(_) | AnnotBody::Widget(_)) | None => Made::Merged,
        },
        ObjKey::AnnotPart(_, Part::Popup) => Made::Merged,
        ObjKey::AnnotPart(..)
        | ObjKey::Res(_)
        | ObjKey::ResPart(..)
        | ObjKey::Block(_)
        | ObjKey::Shared(_)
        | ObjKey::Generators
        | ObjKey::Pinned(_)
        | ObjKey::Built(_) => Made::Built,
        ObjKey::Catalog
        | ObjKey::PagesNode(_)
        | ObjKey::Page(_)
        | ObjKey::OutlinesRoot
        | ObjKey::AcroForm
        | ObjKey::Info
        | ObjKey::Field(_)
        | ObjKey::Bookmark(_)
        | ObjKey::Labels
        | ObjKey::Names(_)
        | ObjKey::PageRes { .. }
        | ObjKey::Filtered { .. } => Made::Merged,
        ObjKey::Base { blob, obj, .. } => return base(blob, *obj),
    })
}

/// How the objects of a blob sheaf made in memory as `kind` are held, if
/// they are; a comment summary's, but its page pictures
/// ([`page_pictures`]), as its own.
fn of_memory(kind: MemoryKind) -> Option<Made> {
    match kind {
        MemoryKind::Blank | MemoryKind::Images | MemoryKind::Summary => Some(Made::Built),
        MemoryKind::Library => Some(Made::Merged),
        MemoryKind::Recovered | MemoryKind::Comments => None,
    }
}

/// The objects of the comment summary in blob `blob` of `store` that
/// picture the document's pages: the form XObjects
/// [`page_forms`](crate::stamp_lib::page_forms) made of them, the only
/// forms a summary writes, and what they reach, which is copied from the
/// printed pages, a file's content and resources with sheaf's marks drawn
/// in. Its other objects (its pages, its text and its fonts) are sheaf's
/// own. Each object is gone through once.
fn page_pictures(store: &Store, blob: BlobId) -> HashSet<ObjectId> {
    let Some(src) = store.get(blob).and_then(|b| store.source(&b).ok()) else {
        return HashSet::new();
    };
    let pdf = src.pdf();
    let form = |id: &ObjectId| {
        matches!(pdf.get(*id), Ok(Object::Stream(s))
            if s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form"))
    };
    let mut queue: Vec<ObjectId> = pdf.ids().filter(form).collect();
    let mut pictures: HashSet<ObjectId> = queue.iter().copied().collect();
    while let Some(id) = queue.pop() {
        if let Ok(object) = pdf.get(id) {
            each_reference(object, |to| {
                if pictures.insert(to) {
                    queue.push(to);
                }
            });
        }
    }
    pictures
}

/// What made blob `blob` of `store` in memory: None for a file's, and for
/// one the store no longer holds.
fn memory_kind(store: &Store, blob: &BlobId) -> Option<MemoryKind> {
    match store.get(*blob)?.kind() {
        BlobKind::Pdf {
            origin: Origin::Memory(kind),
            ..
        } => Some(*kind),
        _ => None,
    }
}
