//! Rewrites from records, read back: numbers kept, untouched objects
//! copied byte for byte, copy 0 owning its page's object, the page-level
//! `/StructParents` rule, object streams and encryption, raw passthrough
//! off when the encryption changes, Dense numbering, labels after a
//! reorder, determinism, and a second save keeping the first's numbers.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use in_progress::NoProgress;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, Stream, dictionary};

use super::*;
use crate::history::FixedCount;
use crate::labels::{LabelSpec, Style};
use crate::model::{Document, OutlineId, PageId, SourceId};
use crate::serialize::durable::FileStamp;
use crate::serialize::fixture::{
    FORM_SECRETS, USER, encrypted, form, holds, object_streams_encrypted, rc4_aes_128_and_aes_256,
};
use crate::source::Limits;
use crate::source::lazy_tests::same;
use crate::store::{BlobKind, Src, Store};
use crate::tx::fixture as fx;
use sheaf_edit::{BookmarkRef, Op, Turn};

/// A file of `n` pages, each drawing "`tag` i" in Helvetica from a stream
/// of its own, under one node that gives them their resources and media
/// box; with document information and XMP metadata. With `tagged`, it
/// has a structure tree, and each page its `/StructParents`.
pub(super) fn file(tag: &str, n: usize, tagged: bool) -> Pdf {
    let mut pdf = Pdf::with_version("1.5");
    let pages = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = pdf.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for i in 0..n {
        let content = text(&format!("{tag} {i}"));
        let content = pdf.add_object(Stream::new(dictionary! {}, content));
        let mut page = dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => content };
        if tagged {
            page.set("StructParents", i as i64);
        }
        kids.push(Object::Reference(pdf.add_object(page)));
    }
    pdf.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => n as i64, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let xmp = pdf.add_object(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        b"<x:xmpmeta/>".to_vec(),
    ));
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages, "Metadata" => xmp };
    if tagged {
        let root = pdf.add_object(dictionary! { "Type" => "StructTreeRoot" });
        catalog.set("StructTreeRoot", root);
        catalog.set("MarkInfo", dictionary! { "Marked" => true });
    }
    let catalog = pdf.add_object(catalog);
    let info = pdf.add_object(dictionary! { "Title" => Object::string_literal(tag) });
    pdf.trailer.set("Root", catalog);
    pdf.trailer.set("Info", info);
    pdf
}

/// A content stream showing `text` in `/F1`.
fn text(text: &str) -> Vec<u8> {
    let ops = vec![
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec!["F1".into(), 24.into()]),
        Operation::new("Td", vec![72.into(), 700.into()]),
        Operation::new("Tj", vec![Object::string_literal(text)]),
        Operation::new("ET", vec![]),
    ];
    Content { operations: ops }.encode().unwrap()
}

/// `pdf` as lopdf writes it, with a cross-reference table.
pub(super) fn bytes(pdf: &Pdf) -> Vec<u8> {
    let mut pdf = pdf.clone();
    pdf.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    let mut out = Vec::new();
    pdf.save_to(&mut out).unwrap();
    out
}

/// `bytes`, read into `store` as the file `name`, opened with `password`.
fn file_in(store: &Store, bytes: &[u8], name: &str, password: Option<&str>) -> Src {
    let kind = BlobKind::file(Path::new(name), password);
    store.open(Arc::from(bytes), kind).unwrap()
}

/// A document over `bytes`, opened with `password`.
pub(super) fn open(bytes: &[u8], password: Option<&str>) -> Document {
    let store = Store::new();
    let primary = file_in(&store, bytes, "doc.pdf", password);
    let src = primary.clone();
    let policy = Box::new(FixedCount(50));
    let mut d = crate::open::start(store, primary, "doc.pdf".into(), None, policy);
    d.ready(crate::open::finish(&src, &NoProgress)).unwrap();
    d
}

/// A file to insert pages of: its source, held while it's used, and the
/// name it goes by.
#[derive(Clone)]
pub(super) struct Other {
    pub(super) src: Src,
    pub(super) name: Arc<str>,
}

/// `bytes`, read into `d`'s store as the file `name`.
pub(super) fn add(d: &Document, bytes: &[u8], name: &str) -> Other {
    Other {
        src: file_in(d.store(), bytes, name, None),
        name: name.into(),
    }
}

/// Page `index` of `other` inserted at the end of `d`.
pub(super) fn insert(d: &mut Document, other: &Other, index: u32) {
    let insert = fx::insert(&other.src, [index], &other.name, None);
    d.commit_ops(vec![insert], "insert").unwrap();
}

fn rewrite_plan() -> SavePlan {
    SavePlan {
        strategy: Strategy::Rewrite { why: Vec::new() },
        asks: Vec::new(),
        transforms: Vec::new(),
        notes: Vec::new(),
    }
}

pub(super) fn target(profile: Profile) -> SaveTarget {
    SaveTarget {
        path: Path::new("out.pdf").into(),
        same_file: false,
        profile,
        stamp: None,
    }
}

/// `input` Rewritten as `profile` says.
fn write(input: &SaveInput, profile: Profile) -> Output {
    materialize(input, &rewrite_plan(), target(profile), &NoProgress).unwrap()
}

/// `d` as it stands, Rewritten as `profile` says, with nothing saved.
pub(super) fn rewrite(d: &Document, profile: Profile) -> Output {
    write(&SaveInput::detached(&d.snapshot()), profile)
}

/// `bytes` as a source, read with `password`.
pub(super) fn source(bytes: &[u8], password: Option<&str>) -> Src {
    file_in(&Store::new(), bytes, "out.pdf", password)
}

/// `bytes` read lazily with `password`.
pub(super) fn lazy(bytes: &[u8], password: Option<&str>) -> LazyPdf {
    LazyPdf::open(Arc::from(bytes), password, &Limits::DEFAULT.reader).unwrap()
}

/// The object numbers `pdf`'s table lists in use.
fn ids(pdf: &LazyPdf) -> BTreeSet<ObjectId> {
    pdf.ids().collect()
}

/// Object `id`'s bytes in `bytes`, as `pdf` (read from them) finds them.
pub(super) fn span<'a>(pdf: &LazyPdf, bytes: &'a [u8], id: ObjectId) -> Option<&'a [u8]> {
    Some(&bytes[pdf.raw_span(id)?])
}

/// The objects a Rewrite of an unedited file writes anew: the pages, the
/// tree's root and the catalog.
fn written_anew(src: &Src) -> BTreeSet<ObjectId> {
    let pdf = src.pdf();
    let catalog = pdf.trailer().get(b"Root").unwrap().as_reference().unwrap();
    let root = (pdf.catalog().unwrap().get(b"Pages").unwrap())
        .as_reference()
        .unwrap();
    src.page_ids().chain([catalog, root]).collect()
}

/// Whether `obj` describes where a file's objects are, which a write
/// writes for itself: an object stream or a cross-reference stream.
fn layout(obj: &Object) -> bool {
    matches!(obj, Object::Stream(s) if s.dict.has_type(b"ObjStm") || s.dict.has_type(b"XRef"))
}

/// Whether encrypting `obj` changes it: it holds a string, or is a stream.
fn encrypts(obj: &Object) -> bool {
    match obj {
        Object::String(..) | Object::Stream(_) => true,
        Object::Array(items) => items.iter().any(encrypts),
        Object::Dictionary(d) => d.iter().any(|(_, v)| encrypts(v)),
        _ => false,
    }
}

/// A Rewrite of a file nobody edited writes every object at its number,
/// and copies each it didn't write anew byte for byte.
#[test]
fn an_identity_rewrite_keeps_every_number_and_copies_what_it_didnt_touch() {
    let before = bytes(&file("A", 3, false));
    let d = open(&before, None);
    let out = rewrite(&d, Profile::default());
    let (was, now) = (lazy(&before, None), lazy(&out.bytes, None));
    assert!(now.base_sound());
    assert_eq!(ids(&now), ids(&was));
    let anew = written_anew(&source(&before, None));
    let mut copied = 0;
    for id in ids(&was) {
        if anew.contains(&id) || layout(was.get(id).unwrap()) {
            continue;
        }
        assert_eq!(
            span(&now, &out.bytes, id),
            span(&was, &before, id),
            "{id:?}"
        );
        copied += 1;
    }
    // The font, the resources, three contents, the XMP and the Info.
    assert_eq!(copied, 7);
    assert_eq!(out.record.map.identity, Some(d.primary().id()));
    let objects = ids(&was)
        .into_iter()
        .filter(|&id| !layout(was.get(id).unwrap()));
    assert_eq!(out.record.present.len(), objects.count());
}

/// The same document written twice, and on two threads, is the same
/// bytes: encrypted with AES too, whose IVs are keyed hashes.
#[test]
fn a_rewrite_is_the_same_bytes_every_time() {
    let other = bytes(&file("B", 2, false));
    let plain = bytes(&file("A", 3, false));
    let state = &rc4_aes_128_and_aes_256(&form())[1].1;
    let [aes, _] = encrypted(&form(), state, |_| {});
    for (base, password) in [(plain, None), (aes, Some(USER))] {
        let mut d = open(&base, password);
        let first = d.page(0).unwrap();
        d.commit_ops(vec![fx::duplicate(&[first])], "dup").unwrap();
        let turn = fx::rotate(&[first], Turn::Quarter);
        d.commit_ops(vec![turn], "turn").unwrap();
        let source = add(&d, &other, "other.pdf");
        insert(&mut d, &source, 1);
        let input = SaveInput::detached(&d.snapshot());
        let once = write(&input, Profile::default()).bytes;
        let again = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| write(&input, Profile::default()).bytes))
                .collect();
            (threads.into_iter())
                .map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(write(&input, Profile::default()).bytes, once);
        assert_eq!(again, [once.clone(), once]);
    }
}

/// A duplicate moved before the page it copies gets an object of its own;
/// the original keeps its object, which links, `/P` and structure name
/// (bug 3), and the two share their content.
#[test]
fn a_duplicate_moved_before_its_original_leaves_copy_0_with_the_original_object() {
    let before = bytes(&file("A", 2, false));
    let was = source(&before, None);
    let mut d = open(&before, None);
    let first = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[first])], "dup").unwrap();
    let copy = d.page(1).unwrap();
    let to_front = fx::move_pages(&[copy], Some(first));
    d.commit_ops(vec![to_front], "move").unwrap();
    let out = rewrite(&d, Profile::default());
    let now = source(&out.bytes, None);
    let pages: Vec<ObjectId> = now.page_ids().collect();
    let originals: Vec<ObjectId> = was.page_ids().collect();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[1..], originals[..]);
    assert!(!ids(was.pdf()).contains(&pages[0]), "{pages:?}");
    let contents = |id| {
        now.pdf()
            .get_dictionary(id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .clone()
    };
    assert_eq!(contents(pages[0]), contents(pages[1]));
    assert_eq!(out.record.map.get(&ObjKey::Page(first)), Some(originals[0]));
    assert_eq!(out.record.map.get(&ObjKey::Page(copy)), Some(pages[0]));
}

/// A page from another tagged file, and a duplicate of a page of this
/// one, lose `/StructParents`: the structure tree they'd index isn't
/// theirs. The file's own pages keep theirs, and the reader is told.
#[test]
fn a_tagged_page_inserted_into_a_tagged_file_has_no_struct_parents() {
    let mut d = open(&bytes(&file("A", 2, true)), None);
    let source = add(&d, &bytes(&file("B", 2, true)), "other.pdf");
    insert(&mut d, &source, 1);
    let first = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[first])], "dup").unwrap();
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let struct_parents: Vec<Option<i64>> = now.pages().iter().map(|p| p.struct_parents).collect();
    // A 0, its duplicate, A 1, B 1.
    assert_eq!(struct_parents, [Some(0), None, Some(1), None]);
    let told: Vec<String> = out.notes.iter().map(|n| n.0.clone()).collect();
    assert!(
        told.iter()
            .any(|n| n.contains("other.pdf") && n.contains("tags")),
        "{told:?}"
    );
    assert!(
        told.iter()
            .any(|n| n.contains("doc.pdf") && n.contains("tags")),
        "{told:?}"
    );
    // An untagged file's pages lose nothing worth a note.
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let source = add(&d, &bytes(&file("B", 1, false)), "plain.pdf");
    insert(&mut d, &source, 0);
    assert!(rewrite(&d, Profile::default()).notes.is_empty());
}

pub(super) fn source_of(out: &Output) -> Src {
    source(&out.bytes, None)
}

/// A file whose objects are in an object stream rewrites with each member
/// an object of its own, at its number, and no object stream.
#[test]
fn an_object_stream_base_rewrites_with_plain_objects() {
    let mut pdf = file("A", 2, false);
    let mut before = Vec::new();
    pdf.save_modern(&mut before).unwrap();
    assert!(holds(&before, b"/ObjStm"));
    let d = open(&before, None);
    let out = rewrite(&d, Profile::default());
    assert!(!holds(&out.bytes, b"/ObjStm"));
    let (was, now) = (lazy(&before, None), lazy(&out.bytes, None));
    assert!(now.base_sound());
    assert_eq!(now.xref().kind, lopdf::XrefKind::Stream);
    let anew = written_anew(&source(&before, None));
    for id in ids(&was) {
        let obj = was.get(id).unwrap();
        if layout(obj) {
            continue;
        }
        assert!(
            now.raw_span(id).is_some(),
            "{id:?} isn't an object of its own"
        );
        if !anew.contains(&id) {
            assert!(same(now.get(id).unwrap(), obj), "{id:?}");
        }
    }
}

/// An encrypted file rewrites encrypted as it was: its `/Encrypt` as
/// stored at its number, nothing in the clear, every object reading as it
/// did, and what the save didn't touch copied as it was, when the
/// encryption is kept. An object stream encrypted whole rewrites as
/// objects encrypted each on its own.
#[test]
fn an_encrypted_base_rewrites_encrypted() {
    let plain = form();
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        let [table, stream] = encrypted(&plain, &state, |_| {});
        let packed = object_streams_encrypted(&plain, &state);
        for (kind, before) in [("table", table), ("stream", stream), ("packed", packed)] {
            let name = format!("{name}, {kind}");
            let d = open(&before, Some(USER));
            let out = rewrite(&d, Profile::default());
            for secret in FORM_SECRETS {
                assert!(!holds(&out.bytes, secret), "{name}: {secret:?}");
            }
            assert!(!holds(&out.bytes, b"/ObjStm"), "{name}");
            assert!(
                matches!(
                    LazyPdf::open(Arc::from(&out.bytes[..]), None, &Limits::DEFAULT.reader),
                    Err(lopdf::Error::InvalidPassword)
                ),
                "{name}"
            );
            let (was, now) = (lazy(&before, Some(USER)), lazy(&out.bytes, Some(USER)));
            let (crypt, back) = (
                d.primary().crypt().unwrap().clone(),
                crate::serialize::crypt::Crypt::of(&now).unwrap().unwrap(),
            );
            assert_eq!(back.dict(), crypt.dict(), "{name}");
            assert_eq!(back.at(), crypt.at(), "{name}");
            assert_eq!(back.first_id(), crypt.first_id(), "{name}");
            let anew = written_anew(&source(&before, Some(USER)));
            for id in ids(&was) {
                let obj = was.get(id).unwrap();
                if layout(obj) || id == crypt.at() || anew.contains(&id) {
                    continue;
                }
                assert!(same(now.get(id).unwrap(), obj), "{name}: {id:?}");
                if kind != "packed" {
                    assert_eq!(
                        span(&now, &out.bytes, id),
                        span(&was, &before, id),
                        "{name}: {id:?}"
                    );
                }
            }
        }
    }
}

/// Written unencrypted, an encrypted file's objects can't be copied as
/// they were: each is written decrypted. A plain file's still are.
#[test]
fn profile_none_turns_raw_passthrough_off() {
    let plain = form();
    let unencrypted = Profile {
        encryption: Encryption::None,
        ..Profile::default()
    };
    let state = &rc4_aes_128_and_aes_256(&plain)[1].1;
    let [before, _] = encrypted(&plain, state, |_| {});
    let d = open(&before, Some(USER));
    let out = rewrite(&d, unencrypted);
    let (was, now) = (lazy(&before, Some(USER)), lazy(&out.bytes, None));
    assert!(!now.is_encrypted());
    let crypt = d.primary().crypt().unwrap().at();
    let anew = written_anew(&source(&before, Some(USER)));
    let mut decrypted = 0;
    for id in ids(&was) {
        let obj = was.get(id).unwrap();
        if layout(obj) || id == crypt || anew.contains(&id) {
            continue;
        }
        assert!(same(now.get(id).unwrap(), obj), "{id:?}");
        if encrypts(obj) {
            assert_ne!(
                span(&now, &out.bytes, id),
                span(&was, &before, id),
                "{id:?}"
            );
            decrypted += 1;
        }
    }
    assert!(decrypted > 0);
    assert!(!ids(&now).contains(&crypt));

    // A plain file's streams are copied as they were: written anew, their
    // dictionaries and `stream` keywords would be laid out otherwise.
    let before = bytes(&file("A", 1, false));
    let out = rewrite(&open(&before, None), unencrypted);
    let (was, now) = (lazy(&before, None), lazy(&out.bytes, None));
    let contents = source(&before, None).pages()[0].contents[0];
    let raw = span(&was, &before, contents).unwrap();
    assert_eq!(span(&now, &out.bytes, contents), Some(raw));
    let mut written = Vec::new();
    crate::serialize::object::indirect(&mut written, contents, was.get(contents).unwrap()).unwrap();
    assert_ne!(written.trim_ascii_end(), raw);
}

/// Dense numbers count from 1 with no gap, and a stream byte-identical to
/// another is written once, both pages naming it.
#[test]
fn dense_numbering_writes_alike_streams_once() {
    let mut pdf = file("A", 2, false);
    let page = |pdf: &Pdf, i: usize| pdf.get_pages()[&(i as u32 + 1)];
    let content = |pdf: &Pdf, i| {
        pdf.get_dictionary(page(pdf, i))
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .clone()
    };
    let first = content(&pdf, 0).as_reference().unwrap();
    let second = content(&pdf, 1).as_reference().unwrap();
    let data = pdf
        .get_object(first)
        .unwrap()
        .as_stream()
        .unwrap()
        .content
        .clone();
    pdf.get_object_mut(second)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .set_content(data);
    let d = open(&bytes(&pdf), None);
    let dense = Profile {
        numbering: Numbering::Dense,
        ..Profile::default()
    };
    let out = rewrite(&d, dense);
    let now = source_of(&out);
    let numbers: Vec<u32> = ids(now.pdf()).iter().map(|id| id.0).collect();
    assert_eq!(numbers, (1..=numbers.len() as u32).collect::<Vec<_>>());
    let contents: Vec<Object> = (now.page_ids())
        .map(|id| {
            now.pdf()
                .get_dictionary(id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .clone()
        })
        .collect();
    assert_eq!(contents[0], contents[1]);
    assert_eq!(out.record.map.identity, None);
    assert!(out.record.present.is_empty());
}

/// Labels anchored to pages follow them when the pages move.
#[test]
fn labels_follow_a_reorder() {
    let mut d = open(&bytes(&file("A", 4, false)), None);
    let ids: Vec<PageId> = d.state().order().iter().copied().collect();
    let spec = |start, style| LabelSpec {
        start,
        style,
        prefix: String::new(),
        first: 1,
    };
    let labels = fx::labels(&[
        spec(ids[0], Style::LowerRoman),
        spec(ids[2], Style::Decimal),
    ]);
    d.commit_ops(vec![labels], "labels").unwrap();
    let read = |out: &Output| {
        let pdf = Pdf::load_mem(&out.bytes).unwrap();
        crate::labels::label_ranges(&pdf).map(|r| crate::labels::labels_for(&r, 4))
    };
    assert_eq!(
        read(&rewrite(&d, Profile::default())).unwrap(),
        ["i", "ii", "1", "2"]
    );
    let to_front = fx::move_pages(&[ids[2]], Some(ids[0]));
    d.commit_ops(vec![to_front], "move").unwrap();
    assert_eq!(
        read(&rewrite(&d, Profile::default())).unwrap(),
        ["1", "i", "ii", "iii"]
    );
    d.commit_ops(vec![Op::SetPageLabels { ranges: None }], "none")
        .unwrap();
    assert_eq!(read(&rewrite(&d, Profile::default())), None);
}

/// `d`'s current version saved: sealed, Rewritten and recorded.
pub(super) fn save(d: &mut Document) -> Output {
    let v = d.current_version();
    let input = d.save_input(v).unwrap();
    let out = write(&input, Profile::default());
    let stamp = FileStamp {
        len: out.bytes.len() as u64,
        mtime: SystemTime::UNIX_EPOCH,
        tail: 0,
        window: 0,
    };
    d.record_save(out.record.clone().stamped(stamp, None))
        .unwrap();
    out
}

/// A save after a save keeps the numbers the first gave its objects, and
/// numbers what's new past them.
#[test]
fn a_second_save_keeps_the_first_save_s_numbers() {
    let mut d = open(&bytes(&file("A", 2, false)), None);
    let other = bytes(&file("B", 3, false));
    let source = add(&d, &other, "other.pdf");
    insert(&mut d, &source, 0);
    let inserted = d.page(2).unwrap();
    let first = save(&mut d);
    let numbered: Vec<(ObjKey, ObjectId)> = first
        .record
        .map
        .keys
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    assert!(numbered.iter().any(|(k, _)| *k == ObjKey::Page(inserted)));
    assert!(
        numbered
            .iter()
            .any(|(k, _)| matches!(k, ObjKey::Base { .. }))
    );
    insert(&mut d, &source, 2);
    let added = d.page(3).unwrap();
    let second = save(&mut d);
    for (key, at) in &numbered {
        assert_eq!(second.record.map.get(key), Some(*at), "{key:?}");
    }
    let past = numbered.iter().map(|(_, id)| id.0).max().unwrap();
    assert!(second.record.map.get(&ObjKey::Page(added)).unwrap().0 > past);
    // What the second save wrote reads as the document.
    let now = source_of(&second);
    assert_eq!(now.page_count(), 4);
}

/// The highest number a PDF may use, past which [`far`]'s objects are.
const LAST: u32 = crate::serialize::NUMBERS - 1;

/// A one-page file whose object stream (object 4, which its table lists)
/// holds two objects the table doesn't place, numbered 4,294,967,290 and
/// 4,294,967,291: the lazy reader never reads them, the eager load takes
/// them. The catalog names the first (`/Far`, "FARONE"), the page's font
/// the second (`/Far`, "FARTWO") and 4,294,967,000, which nothing is
/// (`/Gone`).
fn far() -> Vec<u8> {
    let far = |n: u32| Object::Reference((n, 0));
    let members = [
        (
            u32::MAX - 5,
            dictionary! { "Said" => Object::string_literal("FARONE") },
        ),
        (
            u32::MAX - 4,
            dictionary! { "Said" => Object::string_literal("FARTWO") },
        ),
    ];
    let mut pairs = String::new();
    let mut data = Vec::new();
    for (n, dict) in members {
        pairs.push_str(&format!("{n} {} ", data.len()));
        crate::serialize::object::write(&mut data, &Object::Dictionary(dict)).unwrap();
        data.push(b'\n');
    }
    let objstm = Stream::new(
        dictionary! { "Type" => "ObjStm", "N" => 2, "First" => pairs.len() as i64 },
        [pairs.into_bytes(), data].concat(),
    );
    let content = text("FAR 0");
    let media = vec![0.into(), 0.into(), 200.into(), 200.into()];
    let objects: [Object; 6] = [
        dictionary! { "Type" => "Catalog", "Pages" => (2, 0), "Far" => far(u32::MAX - 5) }.into(),
        dictionary! { "Type" => "Pages", "Kids" => vec![(3, 0).into()], "Count" => 1 }.into(),
        dictionary! {
            "Type" => "Page", "Parent" => (2, 0), "MediaBox" => media, "Contents" => (5, 0),
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => (6, 0) } },
        }
        .into(),
        objstm.into(),
        Stream::new(dictionary! {}, content).into(),
        dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Far" => far(u32::MAX - 4), "Gone" => far(4_294_967_000),
        }
        .into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut table = "xref\n0 7\n0000000000 65535 f \n".to_string();
    for (num, obj) in (1..).zip(&objects) {
        let pos = crate::serialize::object::indirect(&mut out, (num, 0), obj).unwrap();
        table.push_str(&format!("{pos:010} 00000 n \n"));
    }
    let start = out.len();
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(b"trailer\n<< /Size 7 /Root 1 0 R >>\n");
    out.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
    out
}

/// What `/Far` of object `id` in `pdf` names: its number, and what the
/// object it names says.
fn far_of(pdf: &LazyPdf, id: ObjectId) -> (ObjectId, Vec<u8>) {
    let at = (pdf.get_dictionary(id).unwrap().get(b"Far").unwrap())
        .as_reference()
        .unwrap();
    let said = pdf.get_dictionary(at).unwrap().get(b"Said").unwrap();
    (at, said.as_str().unwrap().to_vec())
}

/// Objects a base holds at numbers past what a PDF may use, which an
/// object stream its table doesn't place can, are given new numbers past
/// the primary's, so the file stays small and reads back, with a note;
/// what names them follows them there, a reference to such a number that
/// nothing is becomes null, and a second save keeps their numbers.
#[test]
fn objects_numbered_past_the_last_number_are_given_new_ones() {
    let base = far();
    let mut d = open(&base, None);
    assert!(lazy(&base, None).ids().all(|id| id.0 < 7));
    let first = save(&mut d);
    assert!(first.bytes.len() < 4096, "{} bytes", first.bytes.len());
    assert!(first.notes.iter().any(|n| n.0.contains("new numbers")));
    let pdf = lazy(&first.bytes, None);
    assert!(pdf.base_sound());
    assert!(pdf.ids().all(|id| id.0 < 16), "{:?}", ids(&pdf));
    assert!(pdf.xref().size < 16);
    let catalog = pdf.trailer().get(b"Root").unwrap().as_reference().unwrap();
    let (one, said) = far_of(&pdf, catalog);
    assert_eq!(said, b"FARONE");
    let (two, said) = far_of(&pdf, (6, 0));
    assert_eq!(said, b"FARTWO");
    assert!(one.0 >= 7 && two.0 >= 7 && one != two);
    let font = pdf.get_dictionary((6, 0)).unwrap();
    assert_eq!(font.get(b"Gone").unwrap(), &Object::Null);
    // The record keeps where they went, and the next save writes them
    // there.
    let key = |n: u32| ObjKey::Base {
        blob: d.primary().id(),
        obj: (n, 0),
        copy: 0,
    };
    assert_eq!(first.record.map.get(&key(u32::MAX - 5)), Some(one));
    assert_eq!(first.record.map.get(&key(u32::MAX - 4)), Some(two));
    let second = save(&mut d);
    let again = lazy(&second.bytes, None);
    let catalog = again
        .trailer()
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    assert_eq!(far_of(&again, catalog).0, one);
    assert_eq!(far_of(&again, (6, 0)).0, two);
}

/// A page that names a number past the file's objects, which nothing is,
/// still names nothing after a save that numbers new objects: they're
/// numbered past every primary number the emission names, so none takes
/// it, and the reference stays the input's damage.
#[test]
fn a_reference_to_nothing_stays_one() {
    let mut pdf = file("A", 1, false);
    let nothing = (pdf.max_id + 2, 0);
    let page = pdf.page_iter().next().unwrap();
    pdf.get_dictionary_mut(page)
        .unwrap()
        .set("Nothing", Object::Reference(nothing));
    let mut d = open(&bytes(&pdf), None);
    let other = bytes(&file("B", 3, false));
    let source = add(&d, &other, "other.pdf");
    for index in 0..3 {
        insert(&mut d, &source, index);
    }
    let out = rewrite(&d, Profile::default());
    let written = lazy(&out.bytes, None);
    assert!(
        written.ids().any(|id| id.0 > nothing.0),
        "{:?}",
        ids(&written)
    );
    assert!(written.get(nothing).is_err());
    let page = written.get_dictionary(page).unwrap();
    assert_eq!(page.get(b"Nothing").unwrap(), &Object::Reference(nothing));
}

/// A base whose cross-reference stream is the last number a PDF may use,
/// its `/Size` past it, leaves a save the numbers it needs: neither is
/// reserved, and new objects are numbered past those the file has. A
/// reference to a number past the last, which nothing is, is kept as the
/// input had it: the save gave no such number.
#[test]
fn a_size_at_the_last_number_isnt_reserved() {
    let gone = Object::Reference((u32::MAX, 0));
    let media = vec![0.into(), 0.into(), 200.into(), 200.into()];
    let objects = [
        dictionary! { "Type" => "Catalog", "Pages" => (2, 0) },
        dictionary! { "Type" => "Pages", "Kids" => vec![(3, 0).into()], "Count" => 1 },
        dictionary! {
            "Type" => "Page", "Parent" => (2, 0), "MediaBox" => media,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => (4, 0) } },
        },
        dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Gone" => gone.clone(),
        },
    ];
    let mut base = b"%PDF-1.7\n".to_vec();
    let mut rows = vec![0, 0, 0, 0, 0, 0xff, 0xff];
    for (num, dict) in (1..).zip(objects) {
        let pos = crate::serialize::object::indirect(&mut base, (num, 0), &dict.into()).unwrap();
        rows.push(1);
        rows.extend_from_slice(&(pos as u32).to_be_bytes());
        rows.extend_from_slice(&[0, 0]);
    }
    let start = base.len();
    let xref = dictionary! {
        "Type" => "XRef",
        "Size" => i64::from(LAST + 1),
        "W" => vec![1.into(), 4.into(), 2.into()],
        "Index" => vec![0.into(), 5.into()],
        "Root" => (1, 0),
    };
    let stream = Object::Stream(Stream::new(xref, rows));
    crate::serialize::object::indirect(&mut base, (LAST, 0), &stream).unwrap();
    base.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
    let opened = lazy(&base, None);
    assert_eq!(opened.xref().max_stream, Some(LAST));
    assert_eq!(opened.xref().size, LAST + 1);

    let mut d = open(&base, None);
    let page = d.page(0).unwrap();
    let turn = fx::rotate(&[page], Turn::Quarter);
    d.commit_ops(vec![turn], "turn").unwrap();
    let out = save(&mut d);
    assert!(out.bytes.len() < 4096, "{} bytes", out.bytes.len());
    let pdf = lazy(&out.bytes, None);
    assert!(pdf.base_sound());
    assert!(pdf.xref().size < 16, "/Size {}", pdf.xref().size);
    let font = pdf.get_dictionary((4, 0)).unwrap();
    assert_eq!(font.get(b"Gone").unwrap(), &gone);
}

/// A cancelled write stops as cancelled.
#[test]
fn a_cancel_stops_the_write() {
    let d = open(&bytes(&file("A", 1, false)), None);
    let input = SaveInput::detached(&d.snapshot());
    let tracker = in_progress::Tracker::new();
    tracker.cancel();
    let stop = materialize(
        &input,
        &rewrite_plan(),
        target(Profile::default()),
        &tracker,
    )
    .err()
    .unwrap();
    assert_eq!(stop, SaveStop::Cancelled(in_progress::Cancelled));
    let e = anyhow::Error::from(stop);
    assert!(e.chain().any(|c| c.is::<in_progress::Cancelled>()));
}

/// Every page sits in a balanced tree whose nodes give them nothing: each
/// page carries its four inheritable keys, `/Rotate` 0 too.
#[test]
fn every_page_carries_its_inheritable_keys() {
    let d = open(&bytes(&file("A", 40, false)), None);
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    assert_eq!(now.page_count(), 40);
    for &id in now.page_ids().collect::<Vec<_>>().iter() {
        let page: &Dictionary = now.pdf().get_dictionary(id).unwrap();
        for key in [b"Resources".as_slice(), b"MediaBox", b"Rotate"] {
            assert!(page.has(key), "{id:?}: {}", String::from_utf8_lossy(key));
        }
        assert_eq!(page.get(b"Rotate").unwrap().as_i64().unwrap(), 0);
    }
    let tree = &out.record.tree;
    assert_eq!(tree.nodes.len(), 3);
    assert_eq!(tree.nodes[0].kids.len(), 2);
}

/// The header gives the version the file needs: the primary's, its
/// catalog's `/Version` when that's later, which a Rewrite then drops, and
/// 1.6 for AES.
#[test]
fn the_header_gives_the_version_needed() {
    let header = |out: &Output| out.bytes[..8].to_vec();
    let mut pdf = file("A", 1, false);
    pdf.version = "1.4".into();
    let catalog = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let out = rewrite(&open(&bytes(&pdf), None), Profile::default());
    assert_eq!(header(&out), b"%PDF-1.4");
    (pdf.get_dictionary_mut(catalog).unwrap()).set("Version", Object::Name(b"1.7".to_vec()));
    let out = rewrite(&open(&bytes(&pdf), None), Profile::default());
    assert_eq!(header(&out), b"%PDF-1.7");
    assert!(!source_of(&out).pdf().catalog().unwrap().has(b"Version"));

    let mut plain = form();
    plain.version = "1.4".into();
    let state = &rc4_aes_128_and_aes_256(&plain)[1].1;
    let [aes, _] = encrypted(&plain, state, |_| {});
    let d = open(&aes, Some(USER));
    assert_eq!(header(&rewrite(&d, Profile::default())), b"%PDF-1.6");
    let unencrypted = Profile {
        encryption: Encryption::None,
        ..Profile::default()
    };
    assert_eq!(header(&rewrite(&d, unencrypted)), b"%PDF-1.4");
}

/// A file of two pages, the first with a link to the second and a
/// comment that names its page.
fn linked() -> Pdf {
    let mut pdf = file("B", 2, false);
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let link = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "P" => pages[0],
        "Rect" => vec![0.into(), 0.into(), 50.into(), 50.into()],
        "Dest" => vec![pages[1].into(), "Fit".into()],
    });
    let note = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "P" => pages[0],
        "Rect" => vec![100.into(), 100.into(), 150.into(), 150.into()],
        "Contents" => Object::string_literal("their note"),
    });
    (pdf.get_dictionary_mut(pages[0]).unwrap()).set("Annots", vec![link.into(), note.into()]);
    pdf
}

/// Page `id`'s annotations in `src`, each with its `/P` and its link's
/// destination page, when it has them.
fn annots(src: &Src, id: ObjectId) -> Vec<(ObjectId, Option<Object>, Option<Object>)> {
    let pdf = src.pdf();
    let page = pdf.get_dictionary(id).unwrap();
    let Ok(Object::Array(list)) = page.get(b"Annots") else {
        return Vec::new();
    };
    (list.iter())
        .map(|a| {
            let at = a.as_reference().unwrap();
            let d = pdf.get_dictionary(at).unwrap();
            let dest = (d.get(b"Dest").ok())
                .and_then(|d| d.as_array().ok())
                .map(|d| d[0].clone());
            (at, d.get(b"P").ok().cloned(), dest)
        })
        .collect()
}

/// Pages copied from another file bring what they reach: a link between
/// two of them follows, a link to one of its pages not copied goes, and a
/// copy's annotations are its own, naming it.
#[test]
fn copied_pages_bring_what_they_reach_but_other_pages() {
    let other = bytes(&linked());
    // Both pages: the link leads to the second.
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let source = add(&d, &other, "other.pdf");
    let both = fx::insert(&source.src, [0, 1], &source.name, None);
    d.commit_ops(vec![both], "insert").unwrap();
    let now = source_of(&rewrite(&d, Profile::default()));
    let pages: Vec<ObjectId> = now.page_ids().collect();
    let on_first = annots(&now, pages[1]);
    assert_eq!(on_first.len(), 2);
    assert_eq!(on_first[0].2, Some(Object::Reference(pages[2])));
    assert!(
        on_first
            .iter()
            .all(|a| a.1 == Some(Object::Reference(pages[1])))
    );
    assert!(holds(&now.bytes()[..], b"their note"));

    // Only the first: the link goes, the comment stays.
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let source = add(&d, &other, "other.pdf");
    insert(&mut d, &source, 0);
    let now = source_of(&rewrite(&d, Profile::default()));
    let pages: Vec<ObjectId> = now.page_ids().collect();
    assert_eq!(pages.len(), 2);
    let on_first = annots(&now, pages[1]);
    assert_eq!(on_first.len(), 1);
    assert_eq!(on_first[0].1, Some(Object::Reference(pages[1])));

    // A duplicate of a page of this file: its comment is its own.
    let mut d = open(&other, None);
    let first = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[first])], "dup").unwrap();
    let now = source_of(&rewrite(&d, Profile::default()));
    let pages: Vec<ObjectId> = now.page_ids().collect();
    let (original, copy) = (annots(&now, pages[0]), annots(&now, pages[1]));
    assert_eq!(original.len(), 2);
    assert_eq!(copy.len(), original.len());
    for (a, b) in original.iter().zip(&copy) {
        assert_ne!(a.0, b.0);
        assert_eq!(a.1, Some(Object::Reference(pages[0])));
        assert_eq!(b.1, Some(Object::Reference(pages[1])));
    }
    // Both links lead to the page after the duplicate.
    assert_eq!(original[0].2, Some(Object::Reference(pages[2])));
    assert_eq!(copy[0].2, Some(Object::Reference(pages[2])));
}

/// The catalog of `pdf`, to change.
fn catalog_of(pdf: &mut Pdf) -> &mut Dictionary {
    let id = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    pdf.get_dictionary_mut(id).unwrap()
}

/// `src`'s outline items in pre-order, each with its object.
fn items(src: &Src) -> Vec<(ObjectId, Dictionary)> {
    fn level(pdf: &LazyPdf, first: Option<ObjectId>, out: &mut Vec<(ObjectId, Dictionary)>) {
        let mut next = first;
        while let Some(id) = next {
            let item = pdf.get_dictionary(id).unwrap().clone();
            out.push((id, item.clone()));
            level(
                pdf,
                item.get(b"First").and_then(Object::as_reference).ok(),
                out,
            );
            next = item.get(b"Next").and_then(Object::as_reference).ok();
        }
    }
    let pdf = src.pdf();
    let root = pdf
        .catalog()
        .unwrap()
        .get(b"Outlines")
        .unwrap()
        .as_reference();
    let first = (root.ok())
        .and_then(|r| pdf.get_dictionary(r).ok())
        .and_then(|r| r.get(b"First").and_then(Object::as_reference).ok());
    let mut out = Vec::new();
    level(pdf, first, &mut out);
    out
}

/// The item of `items` titled `title`.
fn titled<'a>(items: &'a [(ObjectId, Dictionary)], title: &str) -> &'a (ObjectId, Dictionary) {
    (items.iter())
        .find(|(_, d)| d.get(b"Title").unwrap().as_str().unwrap() == title.as_bytes())
        .unwrap_or_else(|| panic!("no {title}"))
}

/// `item`'s action's `key`, its action direct or not.
fn action<'a>(pdf: &'a LazyPdf, item: &'a Dictionary, key: &[u8]) -> &'a Object {
    let (_, a) = pdf.dereference(item.get(b"A").unwrap()).unwrap();
    a.as_dict().unwrap().get(key).unwrap()
}

/// A tagged file of three pages whose outline has a link to a site, with
/// a key sheaf doesn't read; then a closed chapter on the second page,
/// with a structure element; and under it a section, a GoTo to the third.
/// The structure element's object too.
fn outlined() -> (Pdf, ObjectId) {
    let mut pdf = file("A", 3, true);
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let tree = (catalog_of(&mut pdf).get(b"StructTreeRoot").unwrap())
        .as_reference()
        .unwrap();
    let elem = pdf.add_object(dictionary! {
        "Type" => "StructElem", "S" => "Sect", "P" => tree, "Pg" => pages[1], "K" => 0,
    });
    (pdf.get_dictionary_mut(tree).unwrap()).set("K", elem);
    let [outlines, site, chapter, section] = [(); 4].map(|_| pdf.new_object_id());
    let link =
        dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com/") };
    let objects = [
        (
            site,
            dictionary! {
                "Title" => Object::string_literal("Site"), "Parent" => outlines, "Next" => chapter,
                "A" => link, "Zz" => 7,
            },
        ),
        (
            chapter,
            dictionary! {
                "Title" => Object::string_literal("Chapter"), "Parent" => outlines, "Prev" => site,
                "First" => section, "Last" => section, "Count" => -1, "SE" => elem,
                "Dest" => vec![pages[1].into(), "XYZ".into(), Object::Null, Object::Null, Object::Null],
            },
        ),
        (
            section,
            dictionary! {
                "Title" => Object::string_literal("Section"), "Parent" => chapter,
                "A" => dictionary! { "S" => "GoTo", "D" => vec![pages[2].into(), "Fit".into()] },
            },
        ),
        (
            outlines,
            dictionary! { "Type" => "Outlines", "First" => site, "Last" => chapter, "Count" => 2 },
        ),
    ];
    for (id, dict) in objects {
        pdf.objects.insert(id, Object::Dictionary(dict));
    }
    catalog_of(&mut pdf).set("Outlines", outlines);
    (pdf, elem)
}

/// After a reorder of pages and of bookmarks, then a save, each item keeps
/// its number and what sheaf doesn't model: the link's `/A` and the key
/// beside it, the chapter's `/SE`, its `/Dest` to its page, which moved,
/// and its closed state; the section's GoTo, to a page that moved, is the
/// item as it was, byte for byte (bug 15).
#[test]
fn outline_actions_se_and_closed_items_survive_a_reorder() {
    let (pdf, elem) = outlined();
    let before = bytes(&pdf);
    let was = source(&before, None);
    let old = items(&was);
    let mut d = open(&before, None);
    let roots: Vec<OutlineId> = d.state().outline_roots().iter().copied().collect();
    let (site, chapter) = (roots[0], roots[1]);
    let third = d.page(2).unwrap();
    let to_front = fx::move_pages(&[third], d.page(0));
    d.commit_ops(vec![to_front], "move").unwrap();
    let after_chapter = Op::MoveBookmarks {
        bookmarks: vec![BookmarkRef::Id(site)],
        parent: None,
        at: fx::sibling(Some(chapter)),
        with_pages: false,
    };
    d.commit_ops(vec![after_chapter], "move bookmark").unwrap();
    let out = save(&mut d);
    let now = source_of(&out);
    let new = items(&now);
    let titles: Vec<&[u8]> = (new.iter())
        .map(|(_, d)| d.get(b"Title").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(titles, [&b"Chapter"[..], b"Section", b"Site"]);
    for title in ["Site", "Chapter", "Section"] {
        assert_eq!(titled(&new, title).0, titled(&old, title).0, "{title}");
    }
    let pdf = now.pdf();
    let (_, site_item) = titled(&new, "Site");
    assert_eq!(action(pdf, site_item, b"S").as_name().unwrap(), b"URI");
    let uri = action(pdf, site_item, b"URI").as_str().unwrap();
    assert_eq!(uri, b"https://example.com/");
    assert_eq!(site_item.get(b"Zz").unwrap().as_i64().unwrap(), 7);
    let (_, chapter_item) = titled(&new, "Chapter");
    assert_eq!(chapter_item.get(b"SE").unwrap(), &Object::Reference(elem));
    assert!(pdf.get_dictionary(elem).unwrap().has_type(b"StructElem"));
    assert_eq!(chapter_item.get(b"Count").unwrap().as_i64().unwrap(), -1);
    let (section, _) = titled(&new, "Section");
    assert_eq!(
        span(now.pdf(), &out.bytes, *section),
        span(was.pdf(), &before, *section)
    );
    // Read back, each goes where it went, the chapter closed.
    let read = crate::open::outline(&now);
    let at: Vec<(&str, Option<u32>, bool, bool)> = (read.iter())
        .map(|i| (i.title.as_str(), i.page, i.open, i.kept))
        .collect();
    // The pages are A 2, A 0, A 1 now.
    assert_eq!(
        at,
        [
            ("Chapter", Some(2), false, false),
            ("Section", Some(0), true, false),
            ("Site", None, true, true),
        ]
    );
}

/// A file of two pages whose outline has a link to a site, its action an
/// object of its own, and a bookmark to its second page.
fn outlined_other() -> Pdf {
    let mut pdf = file("B", 2, false);
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let link = pdf.add_object(dictionary! {
        "S" => "URI", "URI" => Object::string_literal("https://b.example/"),
    });
    let [outlines, web, two] = [(); 3].map(|_| pdf.new_object_id());
    let objects = [
        (
            web,
            dictionary! {
                "Title" => Object::string_literal("Web"), "Parent" => outlines, "Next" => two,
                "A" => link,
            },
        ),
        (
            two,
            dictionary! {
                "Title" => Object::string_literal("Two"), "Parent" => outlines, "Prev" => web,
                "Dest" => vec![pages[1].into(), "Fit".into()],
            },
        ),
        (
            outlines,
            dictionary! { "Type" => "Outlines", "First" => web, "Last" => two, "Count" => 2 },
        ),
    ];
    for (id, dict) in objects {
        pdf.objects.insert(id, Object::Dictionary(dict));
    }
    catalog_of(&mut pdf).set("Outlines", outlines);
    pdf
}

/// Bookmarks inserted with another file's pages keep what they do: its
/// link keeps its action, copied with the object it is, and its bookmark
/// to a page goes to that page where it's written.
#[test]
fn a_kept_bookmark_from_an_inserted_file_keeps_its_action() {
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let source = add(&d, &bytes(&outlined_other()), "b.pdf");
    let both = fx::insert(&source.src, [0, 1], &source.name, None);
    d.commit_ops(vec![both], "insert").unwrap();
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let new = items(&now);
    assert_eq!(new.len(), 2);
    let pdf = now.pdf();
    let (_, web) = titled(&new, "Web");
    assert!(web.get(b"A").unwrap().as_reference().is_ok());
    assert_eq!(action(pdf, web, b"S").as_name().unwrap(), b"URI");
    let uri = action(pdf, web, b"URI").as_str().unwrap();
    assert_eq!(uri, b"https://b.example/");
    let (_, two) = titled(&new, "Two");
    let dest = two.get(b"Dest").unwrap().as_array().unwrap();
    let pages: Vec<ObjectId> = now.page_ids().collect();
    assert_eq!(dest[0], Object::Reference(pages[2]));
    let read = crate::open::outline(&now);
    let kept: Vec<(bool, Option<u32>)> = read.iter().map(|i| (i.kept, i.page)).collect();
    assert_eq!(kept, [(true, None), (false, Some(2))]);
}

/// How many header artifacts page `index` of `src` draws.
fn headers(src: &Src, index: u32) -> usize {
    let blocks = src.blocks(index).unwrap();
    (blocks.pagination.iter())
        .filter(|p| p.kind == crate::content::PaginationKind::Header)
        .count()
}

/// Page `index` of `src`'s content, its streams decoded and concatenated.
fn drawn(src: &Src, index: u32) -> Vec<u8> {
    let facts = src.page(index).unwrap();
    let lim = Limits::DEFAULT;
    let content = crate::content::PageContent::read(src.pdf(), &facts.contents, &lim).unwrap();
    let c = content.concat();
    (0..c.stream_count())
        .flat_map(|i| c.stream(i).to_vec())
        .collect()
}

/// A derived copy of page `index` of `d`'s primary, cropped to `crop`,
/// noted with `d`, and the page rebased onto it.
fn crop(d: &mut Document, page: PageId, index: u32, crop: [f32; 4]) -> SourceId {
    let bytes = crate::save::cropped_copy(d.primary(), &[(index, crop)]).unwrap();
    derive(d, &bytes, index, page)
}

/// `bytes`, a derived copy of page `index` of `d`'s primary, noted with
/// `d`, and `page` rebased onto its first page.
fn derive(d: &mut Document, bytes: &[u8], index: u32, page: PageId) -> SourceId {
    let kind = BlobKind::Pdf {
        password: None,
        origin: crate::store::Origin::Memory(crate::store::MemoryKind::Derived),
    };
    let copy = d.store().open(Arc::from(bytes), kind).unwrap();
    let derived = crate::Derived {
        from: d.primary().clone(),
        pages: vec![index],
    };
    d.add_derived(&copy, derived);
    let source = copy.id();
    let why = crate::tx::Derive::Crop;
    d.commit_page_base(vec![(page, copy, 0)], why, "rebase")
        .unwrap();
    source
}

/// A header and footer is drawn once on each page a Rewrite writes; opened
/// again, the pages that draw it already aren't drawn on twice, a cropped
/// one among them, and a page from another file is drawn on, numbered
/// where it is.
#[test]
fn the_header_is_drawn_once_and_not_again_where_the_base_draws_it() {
    let mut d = open(&bytes(&file("A", 2, false)), None);
    let head = fx::header("Head <<1>>");
    d.commit_ops(vec![fx::set_header(head.clone(), None)], "header")
        .unwrap();
    let first = rewrite(&d, Profile::default());
    let once = source_of(&first);
    assert_eq!([headers(&once, 0), headers(&once, 1)], [1, 1]);
    assert!(holds(&drawn(&once, 1), b"(Head 2)"));

    let mut d = open(&first.bytes, None);
    assert_eq!(d.state().header_footer().unwrap().texts, head.texts);
    let first_page = d.page(0).unwrap();
    crop(&mut d, first_page, 0, [0.0, 0.0, 400.0, 500.0]);
    let other = add(&d, &bytes(&file("B", 1, false)), "other.pdf");
    insert(&mut d, &other, 0);
    let again = source_of(&rewrite(&d, Profile::default()));
    let counts: Vec<usize> = (0..3).map(|i| headers(&again, i)).collect();
    assert_eq!(counts, [1, 1, 1]);
    assert!(holds(&drawn(&again, 2), b"(Head 3)"));
    assert!(holds(&drawn(&again, 0), b"(Head 1)"));
}

/// An OCR layer of `text`, one word, on a Letter page.
fn layer(text: &str) -> Arc<crate::text_layer::TextLayer> {
    let word = crate::text_layer::OcrWord {
        text: text.into(),
        rect: crate::annot::Box2 {
            x: 72.0,
            y: 100.0,
            w: 60.0,
            h: 12.0,
        },
        confidence: 0.9,
    };
    let geom = crate::geom::Geom::read(None, None, 0);
    Arc::new(crate::text_layer::from_engine(vec![word], &geom, "test"))
}

/// Page `index`'s `/Contents` entries in `pdf` marked as an OCR layer's.
fn ocr_streams(pdf: &Pdf, index: usize) -> Vec<ObjectId> {
    let page = pdf.get_pages().into_values().nth(index).unwrap();
    let page = pdf.get_dictionary(page).unwrap();
    let entries = match page.get(b"Contents").unwrap() {
        Object::Array(a) => a.clone(),
        Object::Reference(r) => match pdf.get_object(*r).unwrap() {
            Object::Array(a) => a.clone(),
            _ => vec![Object::Reference(*r)],
        },
        _ => Vec::new(),
    };
    (entries.iter())
        .filter_map(|e| e.as_reference().ok())
        .filter(|&r| {
            let s = pdf.get_object(r).unwrap().as_stream().unwrap();
            s.dict.has(b"SheafOcrLayer")
        })
        .collect()
}

/// Recognized again over a layer a save wrote, a page has one layer: the
/// new one, the old one gone from the file.
#[test]
fn a_re_ocr_over_a_base_layer_leaves_one_layer() {
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let page = d.page(0).unwrap();
    let (old, new) = (layer("first"), layer("second"));
    let set = |layer: &Arc<_>| fx::ocr([(page, Arc::clone(layer))]);
    d.commit_ops(set(&old), "ocr").unwrap();
    let first = rewrite(&d, Profile::default());
    let once = crate::file::load_pdf_mem(&first.bytes, None).unwrap();
    assert_eq!(ocr_streams(&once, 0).len(), 2, "the q and the layer");

    let mut d = open(&first.bytes, None);
    let page = d.page(0).unwrap();
    let set = fx::ocr([(page, Arc::clone(&new))]);
    d.commit_ops(set, "ocr again").unwrap();
    let again = rewrite(&d, Profile::default());
    let pdf = crate::file::load_pdf_mem(&again.bytes, None).unwrap();
    let marked = ocr_streams(&pdf, 0);
    assert_eq!(marked.len(), 2, "the q and one layer");
    let content = |id: ObjectId| {
        let s = pdf.get_object(id).unwrap().as_stream().unwrap();
        s.decompressed_content()
            .unwrap_or_else(|_| s.content.clone())
    };
    let (want, gone) = (
        crate::text_layer::to_content(&new).0,
        crate::text_layer::to_content(&old).0,
    );
    assert_eq!(content(marked[1]), want);
    let streams = pdf
        .objects
        .keys()
        .copied()
        .filter(|&id| pdf.get_object(id).is_ok_and(|o| o.as_stream().is_ok()));
    assert!(streams.into_iter().all(|id| content(id) != gone));
}

/// OCR layers on every page draw in one font and open with one `q`: a
/// resource is written once a file.
#[test]
fn ocr_pages_share_one_font() {
    let mut d = open(&bytes(&file("A", 3, false)), None);
    let layers: Vec<_> = (0..3)
        .map(|i| (d.page(i).unwrap(), layer(&format!("word{i}"))))
        .collect();
    d.commit_ops(fx::ocr(layers), "ocr").unwrap();
    let out = rewrite(&d, Profile::default());
    let pdf = crate::file::load_pdf_mem(&out.bytes, None).unwrap();
    let type0 = (pdf.objects.values())
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0"))
        .count();
    assert_eq!(type0, 1);
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let font = |page: ObjectId| {
        let page = pdf.get_dictionary(page).unwrap();
        let (_, res) = pdf.dereference(page.get(b"Resources").unwrap()).unwrap();
        let fonts = res
            .as_dict()
            .unwrap()
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap();
        fonts.get(b"SheafOcr").unwrap().clone()
    };
    assert!(pages.iter().all(|&p| font(p) == font(pages[0])));
    let q: Vec<ObjectId> = (0..3).map(|i| ocr_streams(&pdf, i)[0]).collect();
    assert!(q.iter().all(|&id| id == q[0]), "{q:?}");
    let layers: BTreeSet<ObjectId> = (0..3).map(|i| ocr_streams(&pdf, i)[1]).collect();
    assert_eq!(layers.len(), 3);
}

/// A page whose content a derived copy redrew is written as the page it
/// was made from: its object, its annotations, the copy's content and the
/// state's crop box. The copy's page isn't written.
#[test]
fn a_derived_page_is_written_as_its_original_object_with_derived_content() {
    let mut pdf = file("A", 2, false);
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let note = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "P" => pages[0],
        "Rect" => vec![100.into(), 100.into(), 150.into(), 150.into()],
    });
    (pdf.get_dictionary_mut(pages[0]).unwrap()).set("Annots", vec![note.into()]);
    let before = bytes(&pdf);
    let mut d = open(&before, None);
    let first = d.page(0).unwrap();
    let redrawn = crate::save::derived_copy(
        d.primary(),
        &[(0, Some([0.0, 0.0, 300.0, 400.0]))],
        crate::save::HeaderKept::Yes,
        &mut |pdf, page, _, _| {
            let stream = Stream::new(dictionary! {}, b"0 0 m 9 9 l S".to_vec());
            page.set("Contents", pdf.add_object(stream));
            Ok(())
        },
    )
    .unwrap();
    derive(&mut d, &redrawn, 0, first);
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let written: Vec<ObjectId> = now.page_ids().collect();
    assert_eq!(written, pages);
    let page = now.pdf().get_dictionary(pages[0]).unwrap();
    assert_eq!(
        page.get(b"Annots").unwrap(),
        &Object::Array(vec![note.into()])
    );
    assert_eq!(drawn(&now, 0).trim_ascii(), b"0 0 m 9 9 l S");
    let crop: Vec<f64> = (page.get(b"CropBox").unwrap().as_array().unwrap().iter())
        .map(|v| v.as_float().unwrap())
        .collect();
    assert_eq!(crop, [0.0, 0.0, 300.0, 400.0]);
    assert!(holds(&drawn(&now, 1), b"(A 1)"));
}

/// `objects`, each its number and body, as a file with a cross-reference
/// table, the first the catalog.
fn by_hand(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let objects: Vec<Body> = (objects.iter())
        .map(|(n, body)| Body {
            entry: *n,
            header: *n,
            body: body.clone(),
            endobj: true,
        })
        .collect();
    by_hand_as(&objects)
}

/// An object of a file made by hand: its entry in the table, the number
/// its header gives, its body and whether `endobj` ends it.
struct Body {
    entry: u32,
    header: u32,
    body: Vec<u8>,
    endobj: bool,
}

/// [`by_hand`], each object as it says.
fn by_hand_as(objects: &[Body]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = std::collections::BTreeMap::new();
    for o in objects {
        offsets.insert(o.entry, out.len());
        out.extend_from_slice(format!("{} 0 obj\n", o.header).as_bytes());
        out.extend_from_slice(&o.body);
        out.extend_from_slice(if o.endobj { b"\nendobj\n" } else { b"\n" });
    }
    let size = offsets.keys().max().unwrap() + 1;
    let start = out.len();
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for n in 1..size {
        match offsets.get(&n) {
            Some(at) => out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 00000 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n").as_bytes(),
    );
    out
}

/// A stream whose `/Length` is an object of its own is written whole,
/// with its length: its bytes as the file has them would name the object
/// holding the length, which nothing else names and the Rewrite prunes.
#[test]
fn a_stream_whose_length_is_an_object_is_written_anew() {
    let content = text("held");
    let before = by_hand(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_vec(),
        ),
        (
            4,
            [
                b"<< /Length 5 0 R >>\nstream\n".as_slice(),
                &content,
                b"\nendstream",
            ]
            .concat(),
        ),
        (5, content.len().to_string().into_bytes()),
    ]);
    let read = source(&before, None);
    assert!(read.raw_span((4, 0)).is_some() && read.raw_span_whole((4, 0)).is_none());
    assert!(read.raw_span_whole((3, 0)).is_some());
    let d = open(&before, None);
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let stream = now.pdf().get((4, 0)).unwrap().as_stream().unwrap().clone();
    assert_eq!(stream.content, content);
    assert!(!holds(&out.bytes, b"5 0 R"));
}

/// A file whose table lists object 4 at its current revision, and whose
/// entry 8 is at an older revision's `4 0 obj`: eagerly loaded, the older
/// one no longer shadows the current one, so the Rewrite's graph is the
/// object the bytes copied as they are say, and the image the current
/// revision names is written, by the raw path and by a Dense export
/// alike. The older revision went in over the current one, the image was
/// pruned, and the copied object 4 named it.
#[test]
fn an_object_at_another_entry_doesnt_shadow_the_one_the_table_lists() {
    let image = b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
        /BitsPerComponent 8 /Length 1 >>\nstream\n\x80\nendstream"
        .to_vec();
    let body = |entry: u32, header: u32, body: &[u8]| Body {
        entry,
        header,
        body: body.to_vec(),
        endobj: true,
    };
    let before = by_hand_as(&[
        body(1, 1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        body(2, 2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        body(
            3,
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources 4 0 R >>",
        ),
        body(4, 4, b"<< /XObject << /Im1 7 0 R >> >>"),
        body(7, 7, &image),
        body(8, 4, b"<< /ProcSet [/PDF] >>"),
    ]);
    let src = source(&before, None);
    let eager = graph::eager(&src).unwrap();
    let current = src.pdf().get((4, 0)).unwrap();
    assert!(same(&eager.objects[&(4, 0)], current));
    let d = open(&before, None);
    for numbering in [Numbering::Stable, Numbering::Dense] {
        let profile = Profile {
            numbering,
            ..Profile::default()
        };
        let now = source_of(&rewrite(&d, profile));
        let pdf = now.pdf();
        let page = pdf.get_dictionary(now.page(0).unwrap().id).unwrap();
        let (_, resources) = pdf.dereference(page.get(b"Resources").unwrap()).unwrap();
        let xobjects = resources.as_dict().unwrap().get(b"XObject").unwrap();
        let im = xobjects.as_dict().unwrap().get(b"Im1").unwrap();
        let (_, im) = pdf.dereference(im).unwrap();
        assert!(im.as_stream().unwrap().dict.has(b"Width"), "{numbering:?}");
    }
}

/// An object without its `endobj`, which the parser takes, isn't copied
/// as its bytes are: it's written anew, whole, as the written-anew path
/// writes it. Copied, the written file was as damaged (`qpdf --check`:
/// "expected endobj").
#[test]
fn an_object_without_its_endobj_is_written_with_it() {
    let content = text("bare");
    let body = |n: u32, body: Vec<u8>, endobj: bool| Body {
        entry: n,
        header: n,
        body,
        endobj,
    };
    let stream = [
        format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        &content,
        b"\nendstream",
    ]
    .concat();
    let before = by_hand_as(&[
        body(1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(), true),
        body(
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            true,
        ),
        body(
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources 4 0 R \
              /Contents 5 0 R >>"
                .to_vec(),
            true,
        ),
        body(4, b"<< /ProcSet [/PDF /Text] >>".to_vec(), false),
        body(5, stream, false),
    ]);
    let src = source(&before, None);
    for id in [(4, 0), (5, 0)] {
        assert!(src.raw_span(id).is_some() && src.raw_span_whole(id).is_none());
    }
    let d = open(&before, None);
    let out = rewrite(&d, Profile::default());
    let now = lazy(&out.bytes, None);
    for id in now.ids() {
        assert!(now.raw_span_whole(id).is_some(), "{id:?}");
    }
    assert!(holds(&drawn(&source_of(&out), 0), b"(bare)"));
}

/// An object a copied page's annotation names that doesn't parse is
/// copied as null, and the reader told: the copy loses only what the file
/// had lost. It stopped the save.
#[test]
fn an_object_a_copy_names_that_doesnt_read_is_copied_as_null() {
    let before = by_hand(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [4 0 R] >>".to_vec(),
        ),
        (
            4,
            b"<< /Type /Annot /Subtype /Square /Rect [10 10 50 50] /AP << /N 5 0 R >> >>".to_vec(),
        ),
        (
            5,
            b"<< /Type /XObject /Subtype /Form /BBox [0 0 40 ]] >>".to_vec(),
        ),
    ]);
    assert!(source(&before, None).pdf().object_unkept((5, 0)).is_err());
    let mut d = open(&before, None);
    let first = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[first])], "dup").unwrap();
    let out = rewrite(&d, Profile::default());
    assert!(
        out.notes.iter().any(|n| n.0.contains("couldn't be read")),
        "{:?}",
        out.notes
    );
    let now = source_of(&out);
    let pdf = now.pdf();
    let copy = pdf.get_dictionary(now.page(1).unwrap().id).unwrap();
    let annots = pdf.dereference(copy.get(b"Annots").unwrap()).unwrap().1;
    let square = pdf.dereference(&annots.as_array().unwrap()[0]).unwrap().1;
    let ap = square
        .as_dict()
        .unwrap()
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(matches!(
        pdf.dereference(ap.get(b"N").unwrap()),
        Ok((_, Object::Null))
    ));
}

/// An export's post-pass runs after the graph's checks, and its graph is
/// checked again: a post-pass that leaves a reference to nothing, or
/// reaches a page the file doesn't keep, stops the write.
#[test]
fn an_export_s_post_pass_is_checked_too() {
    let d = open(&bytes(&file("A", 2, false)), None);
    let input = SaveInput::detached(&d.snapshot());
    let dense = Profile {
        numbering: Numbering::Dense,
        ..Profile::default()
    };
    let first = |pdf: &mut Pdf| *pdf.get_pages().values().next().unwrap();
    let mut fine = |_: &mut Pdf| Ok(());
    let written = materialize_then(
        &input,
        &rewrite_plan(),
        target(dense),
        Some(&mut fine),
        &NoProgress,
    );
    assert!(written.is_ok());
    let mut dangling = |pdf: &mut Pdf| {
        let page = first(pdf);
        let nothing = (pdf.max_id + 7, 0);
        pdf.get_dictionary_mut(page)?
            .set("Annots", vec![Object::Reference(nothing)]);
        Ok(())
    };
    let stop = materialize_then(
        &input,
        &rewrite_plan(),
        target(dense),
        Some(&mut dangling),
        &NoProgress,
    );
    let Err(SaveStop::Failed(e)) = stop else {
        panic!("{:?}", stop.err());
    };
    assert!(e.contains("hasn't got"), "{e}");
    let mut reaching = |pdf: &mut Pdf| {
        let page = first(pdf);
        let stray = pdf.add_object(dictionary! { "Type" => "Page" });
        pdf.get_dictionary_mut(page)?
            .set("B", vec![Object::Reference(stray)]);
        Ok(())
    };
    let stop = materialize_then(
        &input,
        &rewrite_plan(),
        target(dense),
        Some(&mut reaching),
        &NoProgress,
    );
    let Err(SaveStop::Failed(e)) = stop else {
        panic!("{:?}", stop.err());
    };
    assert!(e.contains("doesn't keep"), "{e}");
}

/// The fast paths that copy an unchanged object as the file has it write
/// what the built path would: another tool's annotation with `/RT` and no
/// `/IRT` loses the `/RT` (it has no parent), an outline root with no
/// `/Type` gets `/Outlines`, and a field with an empty `/Kids` loses it;
/// each is written anew, where the fast path copied it as it was.
#[test]
fn the_fast_paths_write_what_the_built_path_would() {
    let before = by_hand(&[
        (
            1,
            b"<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R /AcroForm << /Fields [5 0 R] >> >>"
                .to_vec(),
        ),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [4 0 R 5 0 R] >>"
                .to_vec(),
        ),
        (
            4,
            b"<< /Type /Annot /Subtype /Square /Rect [10 10 50 50] /RT /R /NM (sq) /P 3 0 R >>"
                .to_vec(),
        ),
        (
            5,
            b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (f) /Kids [] /Rect [100 100 200 120] \
              /P 3 0 R >>"
                .to_vec(),
        ),
        (6, b"<< /First 7 0 R /Last 7 0 R /Count 1 >>".to_vec()),
        (
            7,
            b"<< /Title (one) /Parent 6 0 R /Dest [3 0 R /Fit] >>".to_vec(),
        ),
    ]);
    let d = open(&before, None);
    let out = rewrite(&d, Profile::default());
    let now = lazy(&out.bytes, None);
    let dict = |id: u32| now.get_dictionary((id, 0)).unwrap().clone();
    assert!(!dict(4).has(b"RT"));
    assert_eq!(
        dict(6).get(b"Type").unwrap(),
        &Object::Name(b"Outlines".to_vec())
    );
    assert!(!dict(5).has(b"Kids"));
    let was = lazy(&before, None);
    for id in [4, 5, 6] {
        assert_ne!(
            span(&now, &out.bytes, (id, 0)),
            span(&was, &before, (id, 0))
        );
    }
}
