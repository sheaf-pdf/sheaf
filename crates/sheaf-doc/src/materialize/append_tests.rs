//! Appends emitted in memory (M10.5 slice 10): what a section writes and
//! where, after an opened file and after a Rewrite; the page tree spliced
//! in place; numbers past everything the file uses; the same section
//! twice; the catalog's `/Version`; and files of both cross-reference
//! kinds reopened.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::xref::XrefType;
use lopdf::{
    ByteSource, Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, Stream, dictionary,
};
use sheaf_edit::{Rotation, Turn};

use super::append::append;
use super::emit::PdfVersion;
use super::key::{ObjKey, Part};
use super::plan::{Onto, SavePlan, Strategy};
use super::removal::Mode;
use super::removal_tests::{layer, opened, rewrite_answers, said, scratch};
use super::{
    AskAnswers, SaveHow, SaveStop, StrategyKind, save_appended, save_rewritten, save_with,
};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::model::{Document, PageId};
use crate::source::Limits;
use crate::tx::fixture as fx;

/// A box as written.
fn rect(r: [i64; 4]) -> Object {
    r.map(Object::from).to_vec().into()
}

/// A file whose root holds a node per entry of `nodes`, each holding that
/// many pages (the root holds them itself for one entry), each page
/// saying its number, as lopdf writes it in `kind`: the root gives every
/// page its box and its font, and the first node, of more than one,
/// turns its pages a quarter on a smaller box.
pub(super) fn file(nodes: &[usize], kind: XrefType, version: &str) -> Vec<u8> {
    let mut pdf = Pdf::with_version(version);
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let mut n = 0;
    let mut page = |pdf: &mut Pdf, parent: ObjectId| {
        let text = format!("BT /F1 24 Tf 72 72 Td (page {n}) Tj ET");
        n += 1;
        let content = pdf.add_object(Stream::new(dictionary! {}, text.into_bytes()));
        let page = dictionary! { "Type" => "Page", "Parent" => parent, "Contents" => content };
        Object::Reference(pdf.add_object(page))
    };
    let kids: Vec<Object> = match nodes {
        [one] => (0..*one).map(|_| page(&mut pdf, root)).collect(),
        _ => (nodes.iter().enumerate())
            .map(|(i, &count)| {
                let node = pdf.new_object_id();
                let kids: Vec<Object> = (0..count).map(|_| page(&mut pdf, node)).collect();
                let mut dict = dictionary! {
                    "Type" => "Pages", "Parent" => root, "Kids" => kids, "Count" => count as i64,
                };
                if i == 0 {
                    dict.set("Rotate", 90);
                    dict.set("MediaBox", rect([0, 0, 300, 400]));
                }
                pdf.objects.insert(node, Object::Dictionary(dict));
                Object::Reference(node)
            })
            .collect(),
    };
    let count: usize = nodes.iter().sum();
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count as i64,
            "MediaBox" => rect([0, 0, 612, 792]),
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    pdf.reference_table.cross_reference_type = kind;
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// `doc` saved over its file as the plan says, which must be an Append:
/// the file's bytes before, and the section after them.
pub(super) fn planned_in_place(doc: &mut Document) -> (Vec<u8>, Vec<u8>) {
    let path = doc.file().unwrap().0.to_path_buf();
    let was = std::fs::read(&path).unwrap();
    let report = save_with(
        doc,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&was), "an Append keeps the file's bytes");
    let section = now[was.len()..].to_vec();
    (was, section)
}

/// `doc`'s current version appended to its file in place, whatever a plan
/// would decide ([`save_appended`]), the save recorded: the file's bytes
/// before, and the section after them.
pub(super) fn appended_in_place(doc: &mut Document) -> Result<(Vec<u8>, Vec<u8>), SaveStop> {
    let path = doc.file().unwrap().0.to_path_buf();
    let was = std::fs::read(&path).unwrap();
    save_appended(doc, &path)?;
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&was), "an Append keeps the file's bytes");
    let section = now[was.len()..].to_vec();
    Ok((was, section))
}

fn lazy(bytes: &[u8]) -> LazyPdf {
    LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap()
}

/// The objects `section`, after a file of `base` bytes, writes: those
/// that read from past its end, but its cross-reference stream.
pub(super) fn written(base: usize, section: &[u8], whole: &Path) -> BTreeSet<ObjectId> {
    let bytes = std::fs::read(whole).unwrap();
    assert_eq!(bytes.len(), base + section.len());
    let pdf = lazy(&bytes);
    let xref = |id| (pdf.get(id).ok()).is_some_and(|o| o.type_name().ok() == Some(b"XRef"));
    pdf.ids()
        .filter(|&id| pdf.raw_span(id).is_some_and(|span| span.start >= base) && !xref(id))
        .collect()
}

/// The keys `doc`'s saved record has at `numbers`.
pub(super) fn keys_at(doc: &Document, numbers: &BTreeSet<ObjectId>) -> BTreeSet<ObjKey> {
    let map = &doc.saved_record().unwrap().map;
    (map.keys.iter())
        .filter(|(_, at)| numbers.contains(at))
        .map(|(key, _)| *key)
        .collect()
}

pub(super) fn square(id: u64) -> Annot {
    Annot {
        id: AnnotId(id),
        kind: AnnotKind::Square {
            rect: Box2 {
                x: 100.0,
                y: 100.0,
                w: 50.0,
                h: 40.0,
            },
        },
        color: [200, 30, 30],
        opacity: 1.0,
        stroke: 1.0,
        author: "me".into(),
        contents: "a square".into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// `doc`'s one mark on page 0 recolored `color`: its id.
fn recolor(doc: &mut Document, color: [u8; 3]) -> AnnotId {
    let page = doc.page(0).unwrap();
    let mut annot = doc.state().marks_on(page).next().unwrap().clone();
    annot.color = color;
    let id = annot.id;
    doc.commit_ops(vec![fx::update(&annot)], "color").unwrap();
    id
}

/// The color of the one mark on page 0 of the file at `path`, reopened.
fn color_of(path: &Path) -> [u8; 3] {
    let doc = crate::open::file(path, None).unwrap().doc;
    let page = doc.page(0).unwrap();
    doc.state().marks_on(page).next().unwrap().color
}

/// A mark's color changed after a Rewrite is planned as an Append, which
/// writes the mark and its appearance over themselves, at the numbers the
/// Rewrite gave them, and nothing else; after the file is opened again,
/// the mark over its own object and its appearance anew. Each reopens
/// recolored.
#[test]
fn a_mark_s_color_change_writes_the_mark_alone() {
    let dir = scratch();
    let (path, mut doc) = opened(
        &dir,
        "a.pdf",
        &file(&[3], XrefType::CrossReferenceTable, "1.7"),
    );
    let page = doc.page(0).unwrap();
    let add = fx::add(page, &square(1));
    doc.commit_ops(vec![add], "add").unwrap();
    save_rewritten(&mut doc, &path).unwrap();
    let rewritten = doc.saved_record().unwrap().clone();
    assert_eq!(rewritten.strategy, StrategyKind::Rewrite);
    let id = recolor(&mut doc, [10, 200, 10]);
    let (base, section) = planned_in_place(&mut doc);
    let at = |key| rewritten.map.keys[&key];
    let want = BTreeSet::from([at(ObjKey::Annot(id)), at(ObjKey::AnnotPart(id, Part::Ap))]);
    assert_eq!(written(base.len(), &section, &path), want);
    let record = doc.saved_record().unwrap();
    assert_eq!(record.strategy, StrategyKind::Append);
    assert_eq!(record.map.keys[&ObjKey::Annot(id)], at(ObjKey::Annot(id)));
    assert_eq!(record.xref.sections, 2);
    assert_eq!(color_of(&path), [10, 200, 10]);

    // Opened again: the mark is the file's own object.
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let opened = doc.saved_record().unwrap().clone();
    let id = recolor(&mut doc, [10, 10, 200]);
    let (base, section) = planned_in_place(&mut doc);
    let was = lazy(&base);
    let page = doc.page(0).unwrap();
    let object = doc.state().annot(id).unwrap().mark_record().unwrap();
    let own = object.src.as_ref().unwrap().id;
    let wrote = written(base.len(), &section, &path);
    assert!(wrote.contains(&own), "{wrote:?}");
    // The mark and its appearance, with what that draws with: no page.
    let keys = keys_at(&doc, &wrote);
    assert_eq!(keys.len(), wrote.len(), "{keys:?}");
    assert!(keys.contains(&ObjKey::AnnotPart(id, Part::Ap)), "{keys:?}");
    for key in &keys {
        let ours = matches!(key, ObjKey::Annot(a) | ObjKey::AnnotPart(a, _) if *a == id);
        assert!(
            ours || matches!(key, ObjKey::Res(_) | ObjKey::ResPart(..)),
            "{key:?}"
        );
    }
    for &at in wrote.iter().filter(|&&at| at != own) {
        assert!(at.0 >= opened.xref.size, "{at:?} past {}", opened.xref.size);
        assert!(was.get(at).is_err());
    }
    assert!(doc.state().annot_ids_on(page).is_some());
    assert_eq!(color_of(&path), [10, 10, 200]);
    assert_eq!(lazy(&std::fs::read(&path).unwrap()).xref().sections, 3);
}

/// Edits of every kind a page has, appended to a file written with a
/// cross-reference table and to one written with a stream, reopen as
/// edited, the section in the file's kind after a sound base.
#[test]
fn both_kinds_of_cross_reference_reopen_after_an_append() {
    let dir = scratch();
    for (kind, name) in [
        (XrefType::CrossReferenceTable, "table.pdf"),
        (XrefType::CrossReferenceStream, "stream.pdf"),
    ] {
        let (path, mut doc) = opened(&dir, name, &file(&[2, 3], kind, "1.7"));
        let pages: Vec<PageId> = (0..5).map(|i| doc.page(i).unwrap()).collect();
        let add = fx::add(pages[0], &square(1));
        doc.commit_ops(vec![add], "add").unwrap();
        let turn = fx::rotate(&[pages[3]], Turn::Quarter);
        doc.commit_ops(vec![turn], "turn").unwrap();
        let first = fx::move_pages(&[pages[4]], Some(pages[0]));
        doc.commit_ops(vec![first], "move").unwrap();
        let (base, section) = appended_in_place(&mut doc).unwrap();
        assert!(!section.is_empty());
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(&base));
        let pdf = lazy(&bytes);
        assert!(pdf.base_sound(), "{name}");
        assert_eq!(pdf.xref().sections, 2, "{name}");
        assert_eq!(pdf.xref().kind, lazy(&base).xref().kind, "{name}");
        assert_eq!(
            said(&path),
            ["page 4", "page 0", "page 1", "page 2", "page 3"]
        );
        let back = crate::open::file(&path, None).unwrap().doc;
        assert_eq!(back.page_count(), 5);
        let turned = back.geom(back.page(4).unwrap()).rotate;
        assert_eq!(turned, Rotation::R90, "{name}");
        // The page from the turned node, moved under the other, turns as
        // it did.
        let moved = back.geom(back.page(0).unwrap());
        assert_eq!(moved.rotate, Rotation::R0, "{name}");
        assert_eq!(back.state().marks_on(back.page(1).unwrap()).count(), 1);
    }
}

/// A file whose linearization holds, appended to, is linearized no more
/// (B3): its `/L` is not the file's length now. The save's record says so,
/// and so do the file's cross-reference facts reopened. A Rewrite writes
/// no linearization either.
#[test]
fn a_linearized_file_appended_to_is_linearized_no_more() {
    use crate::serialize::fixture::linearized_file;
    // Whether the file at `path`, reopened, is linearized.
    let linearized = |path: &Path| {
        let doc = crate::open::file(path, None).unwrap().doc;
        doc.primary().xref().linearized
    };
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &linearized_file());
    assert!(doc.primary().xref().linearized);
    assert!(doc.saved_record().unwrap().xref.linearized);
    let add = fx::add(doc.page(0).unwrap(), &square(1));
    doc.commit_ops(vec![add], "add").unwrap();
    let report = save_appended(&mut doc, &path).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let record = doc.saved_record().unwrap();
    assert_eq!(record.xref.sections, 2);
    assert!(!record.xref.linearized);
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&linearized_file()));
    assert!(!linearized(&path));

    let (path, mut doc) = opened(&dir, "b.pdf", &linearized_file());
    let add = fx::add(doc.page(0).unwrap(), &square(1));
    doc.commit_ops(vec![add], "add").unwrap();
    let report = save_rewritten(&mut doc, &path).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(!doc.saved_record().unwrap().xref.linearized);
    assert!(!linearized(&path));
}

/// A page moved from a node that turns its pages and boxes them to one
/// that doesn't is written under its new node, its turn and box on itself;
/// and a reorder within a node writes that node alone.
#[test]
fn a_page_moved_across_nodes_carries_its_parent_and_inherited_keys() {
    let dir = scratch();
    let (path, mut doc) = opened(
        &dir,
        "a.pdf",
        &file(&[2, 2], XrefType::CrossReferenceTable, "1.7"),
    );
    let pages: Vec<PageId> = (0..4).map(|i| doc.page(i).unwrap()).collect();
    let was = doc.geom(pages[0]);
    assert_eq!(was.rotate, Rotation::R90);
    let last = fx::move_pages(&[pages[0]], None);
    doc.commit_ops(vec![last], "move").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    let wrote = written(base.len(), &section, &path);
    let pdf = Pdf::load(&path).unwrap();
    let ids: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let moved = pdf.get_dictionary(ids[3]).unwrap();
    // Its own object, written under the second node with all it inherited.
    assert!(wrote.contains(&ids[3]));
    let parent = moved.get(b"Parent").unwrap().as_reference().unwrap();
    let base_pdf = Pdf::load_mem(&base).unwrap();
    let second = base_pdf.get_dictionary(base_pdf.get_pages()[&3]).unwrap();
    assert_eq!(
        parent,
        second.get(b"Parent").unwrap().as_reference().unwrap()
    );
    assert_eq!(moved.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
    assert_eq!(moved.get(b"MediaBox").unwrap(), &rect([0, 0, 300, 400]));
    assert!(moved.has(b"Resources"));
    // The two leaves, and that page: the root's kids and count stay.
    assert_eq!(wrote.len(), 3, "{wrote:?}");
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(back.geom(back.page(3).unwrap()), was);
    assert_eq!(said(&path), ["page 1", "page 2", "page 3", "page 0"]);

    // A reorder within the second node: that node alone.
    let mut doc = back;
    let pages: Vec<PageId> = (0..4).map(|i| doc.page(i).unwrap()).collect();
    let last = fx::move_pages(&[pages[2]], None);
    doc.commit_ops(vec![last], "move").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    assert_eq!(
        written(base.len(), &section, &path),
        BTreeSet::from([parent])
    );
    assert_eq!(said(&path), ["page 1", "page 2", "page 0", "page 3"]);
}

/// [`file`]'s pages under two nodes of two, written with a stream, the
/// first page holding a mark of sheaf's and another tool's comment.
fn commented() -> Vec<u8> {
    let bytes = file(&[2, 2], XrefType::CrossReferenceStream, "1.7");
    let mut pdf = Pdf::load_mem(&bytes).unwrap();
    let first = pdf.get_pages()[&1];
    let mark = crate::serialize::built::to_pdf(&square(3), &mut pdf).main;
    let mark = pdf.add_object(mark);
    let theirs = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "NM" => Object::string_literal("theirs"),
        "Rect" => rect([100, 700, 120, 720]), "Contents" => Object::string_literal("their note"),
    });
    let annots = vec![Object::Reference(mark), Object::Reference(theirs)];
    pdf.get_dictionary_mut(first).unwrap().set("Annots", annots);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// A crop is the page's boxes alone (proposal 27): saved, the plan is an
/// Append whose section writes the cropped pages' dictionaries and nothing
/// else, though the first page under a node that turns and boxes its pages
/// holds a mark and another tool's comment. Reopened, each cropped page
/// has its crop, the content and annotations it had, and the comments.
#[test]
fn a_crop_appends_only_page_dicts() {
    let dir = scratch();
    let bytes = commented();
    let margins = sheaf_edit::Margins {
        top: 50.0,
        bottom: 20.0,
        left: 72.0,
        right: 30.0,
    };
    for (name, cropped) in [("first.pdf", &[0][..]), ("every.pdf", &[0, 1, 2, 3])] {
        let (path, mut doc) = opened(&dir, name, &bytes);
        let pages: Vec<PageId> = cropped.iter().map(|&i| doc.page(i).unwrap()).collect();
        doc.commit_ops(vec![fx::crop(&pages, margins)], "crop")
            .unwrap();
        let (base, section) = planned_in_place(&mut doc);
        let wrote = written(base.len(), &section, &path);
        let was = Pdf::load_mem(&base).unwrap();
        let own: BTreeSet<ObjectId> = (cropped.iter())
            .map(|&i| was.get_pages()[&(i as u32 + 1)])
            .collect();
        assert_eq!(wrote, own, "{name}: the cropped pages alone");
        let keys = keys_at(&doc, &wrote);
        let want: BTreeSet<ObjKey> = pages.iter().map(|&p| ObjKey::Page(p)).collect();
        assert_eq!(keys, want, "{name}");
        let now = Pdf::load(&path).unwrap();
        for &at in &wrote {
            let page = now.get_dictionary(at).unwrap();
            assert_eq!(page.get(b"Type").unwrap().as_name().unwrap(), b"Page");
            let before = was.get_dictionary(at).unwrap();
            for key in [&b"Contents"[..], b"Annots", b"Parent"] {
                assert_eq!(page.get(key).ok(), before.get(key).ok(), "{name}: {key:?}");
            }
        }
        let back = crate::open::file(&path, None).unwrap().doc;
        for (&i, &p) in cropped.iter().zip(&pages) {
            let crop = doc.geom(p).crop;
            let at = was.get_pages()[&(i as u32 + 1)];
            let written = now.get_dictionary(at).unwrap().get(b"CropBox").unwrap();
            let [x0, y0, x1, y1] = [crop.x0, crop.y0, crop.x1, crop.y1].map(Object::from);
            assert_eq!(written, &Object::from(vec![x0, y0, x1, y1]), "{name}");
            let geom = back.geom(back.page(i).unwrap());
            assert_eq!(geom, doc.geom(p), "{name}: page {i}");
            assert_ne!(geom.crop, geom.media, "{name}: page {i}");
        }
        let first = back.page(0).unwrap();
        assert_eq!(back.state().marks_on(first).count(), 1, "{name}");
        assert_eq!(back.state().foreign_on(first).count(), 1, "{name}");
        assert_eq!(said(&path), ["page 0", "page 1", "page 2", "page 3"]);
    }
}

/// A node grown past max(128, its size at open) splits; a second Append
/// writes the node split off under the same key and number, and the pages
/// reopen in order.
#[test]
fn a_split_node_keeps_its_key_through_another_append() {
    let dir = scratch();
    let (path, mut doc) = opened(
        &dir,
        "a.pdf",
        &file(&[128, 2], XrefType::CrossReferenceStream, "1.7"),
    );
    let first = doc.page(5).unwrap();
    doc.commit_ops(vec![fx::duplicate(&[first])], "duplicate")
        .unwrap();
    let opened = doc.saved_record().unwrap().clone();
    appended_in_place(&mut doc).unwrap();
    let record = doc.saved_record().unwrap().clone();
    let nodes = |map: &super::record::ObjMap| -> Vec<(ObjKey, ObjectId)> {
        let range = ObjKey::PagesNode(0)..=ObjKey::PagesNode(u32::MAX);
        map.keys.range(range).map(|(k, v)| (*k, *v)).collect()
    };
    let (was, now) = (nodes(&opened.map), nodes(&record.map));
    assert_eq!(now.len(), was.len() + 1);
    assert_eq!(now[..was.len()], was[..]);
    let (split, at) = now[was.len()];
    assert_eq!(split, ObjKey::PagesNode(3));
    assert_eq!(record.tree.nodes.len(), 4);
    let mut want: Vec<String> = (0..130).map(|n| format!("page {n}")).collect();
    want.insert(6, "page 5".into());
    assert_eq!(said(&path), want);

    // Turn the last page of the node split off.
    let last = doc.page(128).unwrap();
    let turn = fx::rotate(&[last], Turn::Half);
    doc.commit_ops(vec![turn], "turn").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    let again = doc.saved_record().unwrap().clone();
    assert_eq!(nodes(&again.map), now);
    assert_eq!(again.tree, record.tree);
    let wrote = written(base.len(), &section, &path);
    assert_eq!(wrote.len(), 1, "the page alone: {wrote:?}");
    assert!(!wrote.contains(&at));
    assert_eq!(said(&path), want);
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(back.page_count(), 131);
    assert_eq!(back.geom(back.page(128).unwrap()).rotate, Rotation::R270);
}

/// New objects are numbered past the file's `/Size` and its
/// cross-reference stream (slice 2's 5801985), the section's own stream
/// after them.
#[test]
fn new_numbers_start_past_the_size_and_the_cross_reference_stream() {
    let dir = scratch();
    let bytes = file(&[2], XrefType::CrossReferenceStream, "1.7");
    let was = lazy(&bytes);
    let (size, stream) = (was.xref().size, was.xref().max_stream.unwrap());
    assert!(stream + 1 >= size, "lopdf numbers its stream last");
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
    let add = fx::add(doc.page(0).unwrap(), &square(1));
    doc.commit_ops(vec![add], "add").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    let wrote = written(base.len(), &section, &path);
    let fresh: Vec<u32> = wrote.iter().map(|id| id.0).filter(|&n| n >= size).collect();
    assert!(!fresh.is_empty());
    assert!(fresh.iter().all(|&n| n > stream), "{fresh:?} past {stream}");
    let record = doc.saved_record().unwrap();
    let own = record.xref.max_stream.unwrap();
    assert!(wrote.iter().all(|id| id.0 < own), "{wrote:?} before {own}");
    assert_eq!(record.xref.size, own + 1);
}

/// The same Append twice writes the same section, encrypted as the file
/// is, its AES IVs keyed by what each object is: nothing the mark says is
/// in it in the clear, and it reads back.
#[test]
fn the_same_section_twice_is_the_same_bytes() {
    use crate::serialize::fixture::{USER, encrypted, holds, rc4_aes_128_and_aes_256};
    let dir = scratch();
    let plain = crate::source::lazy_tests::plaintext();
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        let [bytes, _] = encrypted(&plain, &state, |_| {});
        let path = dir.join(format!("{name}.pdf"));
        std::fs::write(&path, &bytes).unwrap();
        let mut doc = crate::open::file(&path, Some(USER)).unwrap().doc;
        let mut note = square(1);
        note.contents = "a secret square".into();
        let add = fx::add(doc.page(0).unwrap(), &note);
        doc.commit_ops(vec![add], "add").unwrap();
        let v = doc.current_version();
        let input = doc.save_input(v).unwrap();
        let plan = SavePlan {
            strategy: Strategy::Append { onto: Onto::Saved },
            asks: Vec::new(),
            transforms: Vec::new(),
            notes: Vec::new(),
        };
        let base: Arc<dyn ByteSource> = Arc::new(Arc::<[u8]>::from(&bytes[..]));
        let once = append(&input, &plan, base.clone(), Mode::Veto, &NoProgress).unwrap();
        let twice = append(&input, &plan, base, Mode::Veto, &NoProgress).unwrap();
        doc.abandon_save(v);
        assert_eq!(once.bytes, twice.bytes, "{name}");
        assert_eq!(once.after, Some(bytes.len()));
        assert!(!holds(&once.bytes, b"a secret square"), "{name}");
        let whole = [&bytes[..], &once.bytes].concat();
        let pdf = LazyPdf::open(
            Arc::<[u8]>::from(&whole[..]),
            Some(USER),
            &Limits::DEFAULT.reader,
        )
        .unwrap();
        assert!(pdf.base_sound(), "{name}");
        assert_eq!(
            pdf.trailer().get(b"Encrypt").ok(),
            lazy_encrypt(&bytes).as_ref(),
            "{name}"
        );
    }
}

/// The trailer's `/Encrypt` of `bytes`, read with the user password.
fn lazy_encrypt(bytes: &[u8]) -> Option<Object> {
    use crate::serialize::fixture::USER;
    let pdf = LazyPdf::open(
        Arc::<[u8]>::from(bytes),
        Some(USER),
        &Limits::DEFAULT.reader,
    )
    .ok()?;
    pdf.trailer().get(b"Encrypt").ok().cloned()
}

/// A section that needs a later version than its file is says so in the
/// catalog, written for it; one that doesn't writes no catalog.
#[test]
fn a_version_bump_writes_the_catalog_with_its_version() {
    let dir = scratch();
    let (path, mut doc) = opened(
        &dir,
        "a.pdf",
        &file(&[2], XrefType::CrossReferenceTable, "1.1"),
    );
    let catalog = doc.saved_record().unwrap().map.keys[&ObjKey::Catalog];
    let page = doc.page(0).unwrap();
    let add = fx::add(page, &square(1));
    doc.commit_ops(vec![add], "add").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    assert!(!written(base.len(), &section, &path).contains(&catalog));
    assert_eq!(
        doc.saved_record().unwrap().pdf_version,
        PdfVersion::new(1, 1)
    );

    let ocr = fx::ocr([(page, layer("words"))]);
    doc.commit_ops(ocr, "ocr").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    assert!(written(base.len(), &section, &path).contains(&catalog));
    let pdf = Pdf::load(&path).unwrap();
    let version = pdf.catalog().unwrap().get(b"Version").unwrap();
    assert_eq!(version.as_name().unwrap(), b"1.2");
    assert_eq!(
        doc.saved_record().unwrap().pdf_version,
        PdfVersion::CID_FONTS
    );
    let page = pdf.get_dictionary(pdf.get_pages()[&1]).unwrap();
    let contents = page.get(b"Contents").unwrap().as_array().unwrap();
    let layered = (contents.iter()).any(|c| {
        let stream = pdf.get_object(c.as_reference().unwrap()).unwrap();
        super::ocr_tests::ocr_block(&stream.as_stream().unwrap().dict).is_some()
    });
    assert!(layered);
}

/// An Append that would refer to what a Rewrite pruned from the file (a
/// page deleted, saved, then undone) stops before writing: only a Rewrite
/// writes those objects again.
#[test]
fn what_a_rewrite_pruned_stops_an_append() {
    let dir = scratch();
    let (path, mut doc) = opened(
        &dir,
        "a.pdf",
        &file(&[3], XrefType::CrossReferenceTable, "1.7"),
    );
    let gone = doc.page(1).unwrap();
    doc.commit_ops(vec![fx::delete(&[gone])], "delete").unwrap();
    save_with(
        &mut doc,
        &path,
        SaveHow::Planned,
        &rewrite_answers(),
        &NoProgress,
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    doc.undo().unwrap();
    let e = appended_in_place(&mut doc).unwrap_err();
    assert!(e.to_string().contains("the saved file hasn't got"), "{e}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(doc.sealed(doc.current_version()), 0);
}

/// A file that keeps its pages' keys in nodes that aren't the root's, and
/// a catalog with keys the state doesn't own, appended to with a page
/// inserted: the catalog isn't written, and the root's other keys stay.
#[test]
fn an_insert_leaves_the_catalog_and_keeps_the_nodes_keys() {
    let dir = scratch();
    let (path, mut doc) = opened(
        &dir,
        "a.pdf",
        &file(&[2, 2], XrefType::CrossReferenceTable, "1.7"),
    );
    let catalog = doc.saved_record().unwrap().map.keys[&ObjKey::Catalog];
    let second = doc.page(1).unwrap();
    doc.commit_ops(vec![fx::duplicate(&[second])], "duplicate")
        .unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    let wrote = written(base.len(), &section, &path);
    assert!(!wrote.contains(&catalog));
    // The new page, the first node, and the root for its count.
    assert_eq!(wrote.len(), 3, "{wrote:?}");
    let pdf = Pdf::load(&path).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let node = |id: ObjectId| -> Dictionary {
        let page = pdf.get_dictionary(id).unwrap();
        let parent = page.get(b"Parent").unwrap().as_reference().unwrap();
        pdf.get_dictionary(parent).unwrap().clone()
    };
    let first = node(pages[2]);
    assert_eq!(first.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
    assert_eq!(first.get(b"Count").unwrap().as_i64().unwrap(), 3);
    assert_eq!(
        said(&path),
        ["page 0", "page 1", "page 1", "page 2", "page 3"]
    );
}

/// A one-page file holding a mark and another tool's reply to it, where
/// the page's `/PieceInfo` names objects the file hasn't got at `page`
/// past its `/Size`, and the reply's `/ExData` at `reply` past it:
/// another tool's damage, which a save keeps. With its `/Size`.
fn dangling(page: &[u32], reply: &[u32]) -> (Vec<u8>, u32) {
    let mut f = crate::forms::fixture::Form::new(1);
    let main = crate::serialize::built::to_pdf(&square(3), &mut f.pdf).main;
    let main = f.pdf.add_object(main);
    let theirs = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "IRT" => main,
        "NM" => Object::string_literal("r1"), "Rect" => rect([100, 700, 120, 720]),
        "Contents" => Object::string_literal("their reply"),
    });
    f.set_annots(0, &[main, theirs]);
    f.pdf.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
    let size = f.pdf.max_id + 1;
    let gone = |past: &[u32]| -> Dictionary {
        (past.iter().enumerate())
            .map(|(i, &n)| (format!("D{i}"), Object::Reference((size + n, 0))))
            .collect()
    };
    f.dict(f.pages[0]).set("PieceInfo", gone(page));
    f.dict(theirs).set("ExData", gone(reply));
    let bytes = f.bytes();
    assert_eq!(lazy(&bytes).xref().size, size);
    (bytes, size)
}

/// What `key` of the object at `at` of the file at `path` names, each
/// with whether the file has it.
fn named_in(path: &Path, at: ObjectId, key: &[u8]) -> Vec<(ObjectId, bool)> {
    let pdf = Pdf::load(path).unwrap();
    let dict = pdf.get_dictionary(at).unwrap();
    let names = dict.get(key).unwrap().as_dict().unwrap();
    (names.iter())
        .map(|(_, v)| v.as_reference().unwrap())
        .map(|r| (r, pdf.get_object(r).is_ok()))
        .collect()
}

/// A page that names objects its file hasn't got, at its `/Size` and past
/// it, edited so the section writes it, beside a mark and pages from
/// another file that take new numbers: the new numbers start past every
/// number the page names, so each still names nothing, as a Rewrite of
/// the same edits leaves it (review finding 5, after the commit; before,
/// the first new objects took those numbers, and the page named them).
#[test]
fn a_dangling_reference_stays_dangling_past_the_new_numbers() {
    let dir = scratch();
    let (bytes, size) = dangling(&[0, 4], &[]);
    let gone = [((size, 0), false), ((size + 4, 0), false)];
    let other = file(&[3], XrefType::CrossReferenceTable, "1.7");
    let edit = |doc: &mut Document| {
        let add = fx::add(doc.page(0).unwrap(), &square(1));
        doc.commit_ops(vec![add], "add").unwrap();
        let other = super::tests::add(doc, &other, "other.pdf");
        let insert = fx::insert(&other.src, 0..3, &other.name, None);
        doc.commit_ops(vec![insert], "insert").unwrap();
    };
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
    let page = Pdf::load_mem(&bytes).unwrap().get_pages()[&1];
    edit(&mut doc);
    let (base, section) = planned_in_place(&mut doc);
    let wrote = written(base.len(), &section, &path);
    assert!(wrote.contains(&page), "{wrote:?}");
    assert_eq!(named_in(&path, page, b"PieceInfo"), gone);
    let fresh: Vec<u32> = wrote.iter().map(|id| id.0).filter(|&n| n >= size).collect();
    assert!(fresh.len() > 4, "{fresh:?}");
    assert!(fresh.iter().all(|&n| n > size + 4), "{fresh:?}");
    assert_eq!(crate::open::file(&path, None).unwrap().doc.page_count(), 4);

    let (path, mut doc) = opened(&dir, "b.pdf", &bytes);
    edit(&mut doc);
    save_rewritten(&mut doc, &path).unwrap();
    assert_eq!(named_in(&path, page, b"PieceInfo"), gone);
}

/// Another tool's reply that names an object its file hasn't got, at its
/// `/Size`, written by the section as the file has it (its mark recolored),
/// beside the mark's new appearance: the new numbers start past what the
/// reply names, so it still names nothing (review finding 5, after the
/// commit).
#[test]
fn a_reply_written_as_it_was_keeps_its_dangling_reference() {
    let dir = scratch();
    let (bytes, size) = dangling(&[], &[0]);
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
    let page = doc.page(0).unwrap();
    let (_, reply) = doc.state().foreign_on(page).next().unwrap();
    let reply = reply.src.as_ref().unwrap().id;
    recolor(&mut doc, [10, 200, 10]);
    let (base, section) = planned_in_place(&mut doc);
    let wrote = written(base.len(), &section, &path);
    assert!(wrote.contains(&reply), "{wrote:?}");
    assert_eq!(named_in(&path, reply, b"ExData"), [((size, 0), false)]);
    let fresh: Vec<u32> = wrote.iter().map(|id| id.0).filter(|&n| n >= size).collect();
    assert!(!fresh.is_empty());
    assert!(fresh.iter().all(|&n| n > size), "{fresh:?}");
    assert_eq!(color_of(&path), [10, 200, 10]);
}

/// A page that names the last number a file may use, which nothing is,
/// leaves no numbers past it for the section's new objects: the Append is
/// refused, so the save is a Rewrite, which numbers it for itself, and
/// stops here too, its own numbering past that number run out as well:
/// the file can't be saved, and is left as it was (review finding 5,
/// after the commit; where new numbers start is left for Greg).
#[test]
fn numbers_named_near_the_last_refuse_the_append() {
    use super::append::Refused;
    use super::plan::Why;
    let dir = scratch();
    let mut pdf = Pdf::load_mem(&file(&[1], XrefType::CrossReferenceTable, "1.7")).unwrap();
    let page = pdf.get_pages()[&1];
    let last = crate::serialize::NUMBERS - 1;
    let gone = dictionary! { "D0" => Object::Reference((last, 0)) };
    (pdf.get_dictionary_mut(page).unwrap()).set("PieceInfo", gone);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let add = fx::add(doc.page(0).unwrap(), &square(1));
    doc.commit_ops(vec![add], "add").unwrap();
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let plan = SavePlan {
        strategy: Strategy::Append { onto: Onto::Saved },
        asks: Vec::new(),
        transforms: Vec::new(),
        notes: Vec::new(),
    };
    let base: Arc<dyn ByteSource> = Arc::new(Arc::<[u8]>::from(&bytes[..]));
    let out = append(&input, &plan, base, Mode::Veto, &NoProgress);
    doc.abandon_save(v);
    let Err(e) = out else {
        panic!("the Append wasn't refused");
    };
    let refused = e.chain().find_map(|c| c.downcast_ref::<Refused>());
    assert!(matches!(refused, Some(Refused::Unsound(_))), "{e:#}");
    assert_eq!(refused.unwrap().why(), Some(Why::BaseUnsound));
    // Planned, the save is that Rewrite, whose numbers run out the same
    // way: it stops, and nothing is written.
    let path = doc.file().unwrap().0.to_path_buf();
    let stop = save_with(
        &mut doc,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap_err();
    assert!(
        stop.to_string().contains("objects a PDF may have"),
        "{stop}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
