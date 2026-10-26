//! What a save is held to the model for ([`Hold::written`]): an Append's
//! section and a Rewrite's derived objects, each built or merged as its
//! key says, and never a copy of a file's object; and a document made in
//! memory's own objects, built.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use lopdf::{LazyPdf, Object, ObjectId};

use super::*;
use crate::annot::{AnnotKind, Box2};
use crate::history::FixedCount;
use crate::materialize::record::ObjMap;
use crate::materialize::rewrite_tests::{info, make_pdf, opened, tempdir, thread_mark};
use crate::materialize::{ObjKey, StrategyKind, save_appended, save_rewritten};
use crate::model::Document;
use crate::serialize::fixture::object_streams_of;
use crate::source::Limits;
use crate::store::{BlobId, Store};
use crate::tx::fixture as fx;

/// A two-page file with a square marked on its first page, and the
/// one-page file `b.pdf`'s page inserted at the end: the document, and
/// `b.pdf`'s blob.
fn marked_and_inserted(dir: &Path) -> (Document, BlobId) {
    let a = make_pdf(dir, "a.pdf", "A", 2, &[]);
    let b = make_pdf(dir, "b.pdf", "B", 1, &[]);
    let mut d = opened(&a);
    let b = info(d.store(), &b);
    let p0 = d.page(0).unwrap();
    let rect = Box2 {
        x: 100.0,
        y: 100.0,
        w: 80.0,
        h: 40.0,
    };
    let square = thread_mark(AnnotKind::Square { rect }, "Check this");
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
    let Hold::Only(held) = Hold::written(map, d.state(), d.store(), &pdf, appended) else {
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
/// appearance as built, the pages it writes as merged; and not the copies
/// of the inserted file's objects it writes too, nor anything before it.
#[test]
fn an_append_holds_what_its_section_writes_but_copies() {
    let dir = tempdir();
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

/// An Append holds every object its section lists, read or not: one that
/// doesn't read, which nothing references, is held, and checked as
/// unresolved.
#[test]
fn an_object_a_section_lists_is_held_though_it_doesnt_read() {
    let dir = tempdir();
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
    let hold = Hold::written(map, d.state(), d.store(), &pdf, Some((&bytes, from)));
    assert_eq!(
        hold,
        Hold::Only(BTreeMap::from([((9999, 0), Made::Merged)]))
    );
    let report = check(&pdf, &bytes, &hold);
    let found: Vec<_> = (report.findings.iter())
        .map(|f| (f.object, f.rule))
        .collect();
    assert_eq!(found, [(Some((9999, 0)), Rule::Unresolved)], "{report}");
}

/// A Rewrite holds the objects it derived, the root objects and pages as
/// merged and the mark's as built, and the object streams it packed
/// objects into as built, which no key names; and none it copied: not the
/// primary's own at their numbers, nor the inserted file's.
#[test]
fn a_rewrite_holds_what_it_derived_but_copies() {
    let dir = tempdir();
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
    let empty = crate::memory::titled(&store, "Made here").unwrap();
    let facts = crate::open::finish(&empty, &in_progress::NoProgress);
    let mut d = crate::open::start(store, empty, "new".into(), None, Box::new(FixedCount(10)));
    d.ready(facts).unwrap();
    let size = sheaf_edit::PageSize {
        width: 612.0,
        height: 792.0,
    };
    let blank = crate::memory::blank(d.store(), size, 2).unwrap();
    d.commit_ops(vec![fx::insert(&blank, [0, 1], "blank", None)], "Blank")
        .unwrap();
    let dir = tempdir();
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
