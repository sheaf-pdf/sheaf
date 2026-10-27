//! What a save wrote, to hold to the model ([`written`]): the objects
//! sheaf made, each built whole or emitted over a source's dictionary as
//! its [`ObjKey`] says (the edit API's plan, §11.3), and the object
//! streams a Rewrite packs objects into, built whole; never a copy of a
//! file's object, which is the file's, faults and all.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use lopdf::{LazyPdf, Object, ObjectId, Stream, XrefKind};
use pdf_lint::{Hold, Made};
use sheaf_doc::materialize::key::{ObjKey, Part};
use sheaf_doc::materialize::record::ObjMap;
use sheaf_doc::model::{AnnotBody, DocState};
use sheaf_doc::serialize::objstm;
use sheaf_doc::source::Limits;
use sheaf_doc::store::{BlobId, BlobKind, MemoryKind, Origin, Store};

/// How deeply [`each_reference`] goes into the direct objects an indirect
/// one nests. lopdf's parser nests them up to 100 deep, keeping its cap to
/// itself; a real file nests a few.
const NESTING: usize = 64;

/// The objects a save of `st` wrote in `pdf`, the file it made, as
/// pdf-lint holds them: `map` is the map of its record, and `store` holds
/// the blobs its keys name.
///
/// - **An Append** (`appended`: the file's bytes, and where its section
///   starts, the saved file's length before it): every object its
///   cross-reference section lists, its cross-reference stream too, but a
///   copy of a file's object; listed, not read (`section_ids`), so one
///   that doesn't read is held, and checked as unresolved. An object's key
///   is found in `map`, which names every object a section writes; one it
///   doesn't name is held as merged. Nothing is, when the newest section
///   starts before the start, or doesn't read.
/// - **A Rewrite** (`appended` None): the object of each key in `map` that
///   isn't a copy of a file's; each object stream the writer packed
///   objects into, which no key names, as built; when the blob whose
///   objects are at their own numbers (`map`'s identity) is one sheaf made
///   in memory, every other object `map` doesn't name, as that blob's
///   objects are held; and the `/Encrypt` dictionary, which a Rewrite
///   writes as the source stores it, so as merged.
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
            let named: HashSet<ObjectId> = map.keys.values().copied().collect();
            for id in pdf.ids().filter(|id| !named.contains(id)) {
                match own {
                    // One the writer packed objects into, as a Rewrite
                    // never copies a source's.
                    _ if pdf.get(id).is_ok_and(objstm::is_container) => {
                        hold(&mut held, id, Made::Built)
                    }
                    Some(made) => hold(&mut held, id, made),
                    None => {}
                }
            }
            if let Some((_, at)) = pdf.encryption() {
                hold(&mut held, at, Made::Merged);
            }
        }
    }
    Hold::Only(held)
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
///   what the generators are, a pinned appearance, and an object stream
///   the writer packs objects into.
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
        | ObjKey::Built(_)
        | ObjKey::Container(_) => Made::Built,
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
/// [`page_forms`](sheaf_doc::stamp_lib::page_forms) made of them, the only
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

/// The object an indirect object's header at the start of `bytes`
/// (`12 0 obj`, after white-space) names.
fn object_header(bytes: &[u8]) -> Option<ObjectId> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
    let mut words = head.split_ascii_whitespace();
    let number = words.next()?.parse().ok()?;
    let generation = words.next()?.parse().ok()?;
    words
        .next()?
        .starts_with("obj")
        .then_some((number, generation))
}

/// Each reference in `o`, and in the direct objects it holds, no deeper
/// than [`NESTING`].
fn each_reference(o: &Object, mut f: impl FnMut(ObjectId)) {
    let mut stack = vec![(o, 0)];
    while let Some((o, depth)) = stack.pop() {
        match o {
            Object::Reference(id) => f(*id),
            _ if depth >= NESTING => {}
            Object::Array(a) => stack.extend(a.iter().map(|e| (e, depth + 1))),
            Object::Dictionary(d) | Object::Stream(Stream { dict: d, .. }) => {
                stack.extend(d.iter().map(|(_, v)| (v, depth + 1)))
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    //! What a save is held to the model for ([`written`]): an Append's
    //! section and a Rewrite's derived objects, each built or merged as
    //! its key says, and never a copy of a file's object; and a document
    //! made in memory's own objects, built.

    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::Arc;

    use lopdf::{LazyPdf, Object, ObjectId};
    use pdf_lint::{Hold, Made, Rule};
    use sheaf_doc::forms::fixture::{Form, every_mark_kind};
    use sheaf_doc::materialize::record::ObjMap;
    use sheaf_doc::materialize::{ObjKey, StrategyKind, save_appended, save_rewritten};
    use sheaf_doc::serialize::fixture::object_streams_of;
    use sheaf_doc::source::Limits;
    use sheaf_doc::store::{BlobId, Store};
    use sheaf_doc::tx::fixture as fx;
    use sheaf_doc::{AnnotKind, Document, PageSize};

    use super::written;
    use crate::common::{self, scratch};
    use crate::lint::{check, options, read};

    /// A two-page form, `a.pdf`, with a square marked on its first page,
    /// and the one-page form `b.pdf`'s page inserted at the end, both
    /// written in `dir`: the document, and `b.pdf`'s blob. lopdf writes
    /// their cross-references as a stream, so a Rewrite packs.
    fn marked_and_inserted(dir: &Path) -> (Document, BlobId) {
        let a = common::write(dir, "a.pdf", &mut Form::new(2));
        let b = common::write(dir, "b.pdf", &mut Form::new(1));
        let mut d = common::open(&a);
        let (b, _) = d.store().read_file(&b, None).unwrap();
        let p0 = d.page(0).unwrap();
        let square = (every_mark_kind().into_iter())
            .find(|a| matches!(a.kind, AnnotKind::Square { .. }))
            .unwrap();
        let ops = vec![fx::add(p0, &square), fx::insert(&b, [0], "b.pdf", None)];
        d.commit_ops(ops, "Mark and insert").unwrap();
        (d, b.id())
    }

    /// What `d`'s save to `path` holds, its section from byte `from` for an
    /// Append: the objects held, and the file.
    fn held(d: &Document, path: &Path, from: Option<usize>) -> (BTreeMap<ObjectId, Made>, LazyPdf) {
        let bytes = std::fs::read(path).unwrap();
        let pdf = open(&bytes);
        let map = &d.saved_record().unwrap().map;
        let appended = from.map(|from| (&bytes[..], from));
        let Hold::Only(held) = written(map, d.state(), d.store(), &pdf, appended) else {
            panic!("a save holds the objects it wrote");
        };
        (held, pdf)
    }

    fn open(bytes: &[u8]) -> LazyPdf {
        LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap()
    }

    /// Where the objects `d`'s first page's mark writes are: the page, the
    /// mark's dictionary and its appearance.
    fn mark_on_first_page(d: &Document, pdf: &LazyPdf, map: &ObjMap) -> [ObjectId; 3] {
        let page = map.get(&ObjKey::Page(d.page(0).unwrap())).unwrap();
        let annots = pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap();
        let annot = annots.as_array().unwrap()[0].as_reference().unwrap();
        let ap = (pdf.get_dictionary(annot).unwrap().get(b"AP"))
            .and_then(Object::as_dict)
            .and_then(|ap| ap.get(b"N"))
            .and_then(Object::as_reference)
            .unwrap();
        [page, annot, ap]
    }

    /// The objects of `map` that are copies of blob `blob`'s.
    fn copies_of(map: &ObjMap, blob: BlobId) -> Vec<ObjectId> {
        (map.keys.iter())
            .filter(|(key, _)| matches!(key, ObjKey::Base { blob: b, .. } if *b == blob))
            .map(|(_, &id)| id)
            .collect()
    }

    /// An Append holds what its section writes: the mark's dictionary and
    /// appearance as built, the pages it writes as merged; and not the
    /// copies of the inserted file's objects it writes too, nor anything
    /// before it.
    #[test]
    fn an_append_holds_what_its_section_writes_but_copies() {
        let dir = scratch("lint-append");
        let (mut d, b) = marked_and_inserted(&dir);
        let path = dir.join("a.pdf");
        let from = d.saved_record().unwrap().stamp.len as usize;
        save_appended(&mut d, &path).unwrap();
        let record = d.saved_record().unwrap().clone();
        assert_eq!(record.strategy, StrategyKind::Append);
        let (held, pdf) = held(&d, &path, Some(from));
        let [page, annot, ap] = mark_on_first_page(&d, &pdf, &record.map);
        assert_eq!(held.get(&page), Some(&Made::Merged));
        assert_eq!(held.get(&annot), Some(&Made::Built));
        assert_eq!(held.get(&ap), Some(&Made::Built));
        let inserted = record.map.get(&ObjKey::Page(d.page(2).unwrap())).unwrap();
        assert_eq!(held.get(&inserted), Some(&Made::Merged));
        let in_section = |id: &ObjectId| pdf.raw_span(*id).is_some_and(|s| s.start >= from);
        let copies = copies_of(&record.map, b);
        assert!(copies.iter().any(in_section), "the section copies b.pdf's");
        for id in copies {
            assert!(!held.contains_key(&id), "{id:?}, a copy, is held");
        }
        assert!(held.keys().all(in_section), "{held:?}");
    }

    /// An Append holds every object its section lists, read or not: one
    /// that doesn't read, which nothing references, is held, and checked
    /// as unresolved.
    #[test]
    fn an_object_a_section_lists_is_held_though_it_doesnt_read() {
        let dir = scratch("lint-unread");
        let (mut d, _) = marked_and_inserted(&dir);
        let path = dir.join("a.pdf");
        save_appended(&mut d, &path).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let saved = open(&bytes);
        let facts = saved.xref();
        let root = saved
            .trailer()
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        let from = bytes.len();
        bytes.extend_from_slice(b"\n9999 0 obj\n<< /Broken [ >>\nendobj\n");
        let section = bytes.len();
        let offset = |at: usize| at - facts.header_offset;
        bytes.extend_from_slice(
            format!(
                "xref\n9999 1\n{:010} 00000 n \ntrailer\n<< /Size 10000 /Root {} {} R /Prev {} >>\n\
                 startxref\n{}\n%%EOF\n",
                offset(from + 1),
                root.0,
                root.1,
                facts.start,
                offset(section)
            )
            .as_bytes(),
        );
        let pdf = open(&bytes);
        let map = &d.saved_record().unwrap().map;
        let hold = written(map, d.state(), d.store(), &pdf, Some((&bytes, from)));
        assert_eq!(
            hold,
            Hold::Only(BTreeMap::from([((9999, 0), Made::Merged)]))
        );
        let report = check(&read("an Append", &bytes, None), &hold, &options());
        let found: Vec<_> = (report.findings.iter())
            .map(|f| (f.object, f.rule))
            .collect();
        assert_eq!(found, [(Some((9999, 0)), Rule::Unresolved)], "{report}");
    }

    /// A Rewrite holds the objects it derived, the root objects and pages
    /// as merged and the mark's as built, and the object streams it packed
    /// objects into as built, which no key names; and none it copied: not
    /// the primary's own at their numbers, nor the inserted file's.
    #[test]
    fn a_rewrite_holds_what_it_derived_but_copies() {
        let dir = scratch("lint-rewrite");
        let (mut d, b) = marked_and_inserted(&dir);
        let path = dir.join("rewritten.pdf");
        save_rewritten(&mut d, &path).unwrap();
        let record = d.saved_record().unwrap().clone();
        assert_eq!(record.strategy, StrategyKind::Rewrite);
        let (held, pdf) = held(&d, &path, None);
        let [page, annot, ap] = mark_on_first_page(&d, &pdf, &record.map);
        assert_eq!(held.get(&page), Some(&Made::Merged));
        assert_eq!(held.get(&annot), Some(&Made::Built));
        assert_eq!(held.get(&ap), Some(&Made::Built));
        for key in [ObjKey::Catalog, ObjKey::PagesNode(0)] {
            let id = record.map.get(&key).unwrap();
            assert_eq!(held.get(&id), Some(&Made::Merged), "{key:?}");
        }
        let copies = copies_of(&record.map, b);
        assert!(!copies.is_empty(), "the Rewrite copies b.pdf's");
        let primary = record.map.identity.expect("stable numbering");
        let streams = object_streams_of(&pdf);
        assert!(!streams.is_empty(), "a stream file's Rewrite packs");
        for id in &streams {
            assert_eq!(held.get(id), Some(&Made::Built), "{id:?}");
        }
        let named: Vec<ObjectId> = record.map.keys.values().copied().collect();
        let own: Vec<ObjectId> = (pdf.ids())
            .filter(|id| !named.contains(id) && !streams.contains(id))
            .collect();
        assert!(
            !own.is_empty(),
            "the primary's own objects are at their numbers"
        );
        for id in copies
            .into_iter()
            .chain(own)
            .chain(copies_of(&record.map, primary))
        {
            assert!(!held.contains_key(&id), "{id:?}, a copy, is held");
        }
    }

    /// A document made in memory, as the comment summary makes one, blank
    /// pages inserted into it: a Rewrite holds its own objects, at their
    /// numbers and named by no key (its `/Info`), as built, and the blank
    /// pages' shared content too; and holds every object it writes.
    #[test]
    fn a_rewrite_of_a_document_made_in_memory_holds_its_own_objects_as_built() {
        let store = Store::new();
        let empty = sheaf_doc::memory::titled(&store, "Made here").unwrap();
        let mut d = common::ready(store, empty, "new").unwrap();
        let size = PageSize {
            width: 612.0,
            height: 792.0,
        };
        let blank = sheaf_doc::memory::blank(d.store(), size, 2).unwrap();
        d.commit_ops(vec![fx::insert(&blank, [0, 1], "blank", None)], "Blank")
            .unwrap();
        let dir = scratch("lint-in-memory");
        let path = dir.join("new.pdf");
        save_rewritten(&mut d, &path).unwrap();
        let (held, pdf) = held(&d, &path, None);
        let record = d.saved_record().unwrap();
        let identity = record.map.identity.expect("stable numbering");
        assert_ne!(identity, blank.id());
        let named: Vec<ObjectId> = record.map.keys.values().copied().collect();
        let own: Vec<ObjectId> = pdf.ids().filter(|id| !named.contains(id)).collect();
        assert!(
            !own.is_empty(),
            "the empty file's own objects are at their numbers"
        );
        for id in own.into_iter().chain(copies_of(&record.map, blank.id())) {
            assert_eq!(held.get(&id), Some(&Made::Built), "{id:?}");
        }
        let page = record.map.get(&ObjKey::Page(d.page(0).unwrap())).unwrap();
        assert_eq!(held.get(&page), Some(&Made::Merged));
        assert!(pdf.ids().all(|id| held.contains_key(&id)), "{held:?}");
    }
}
