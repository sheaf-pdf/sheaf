//! What a save writes, by the new writer: save.rs's tests of the old
//! writers, ported (M10.5 slice 9). Pages and their order, bookmarks,
//! marks and other tools' comments, stamps and attachments, encryption,
//! the header and footer and labels; OCR layers are `ocr_tests`'. A test
//! that saves both ways has an Append half ([`crate::both_ways!`]).
//! The fixtures here are the export tests' too (`export::tests`).

use std::collections::HashMap;
use std::path::Path;

use in_progress::NoProgress;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, StringFormat, dictionary};
use sheaf_edit::{AnnotRef, BookmarkPatch, DestView, Op, PageRef, Patch, Rotation, Sibling, Turn};

use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::history::FixedCount;
use crate::materialize::{self, AskAnswers, Came, How, SaveHow, StrategyKind, Why};
use crate::model::{Document, Mark, PageId};
use crate::open::FlatOutlineItem;
use crate::save::{rewrite, rewrite_to_path, text_string};
use crate::serialize::crypt::Crypt;
use crate::store::{BlobKind, Src, Store};
use crate::tx::fixture as fx;

/// A PDF whose page i has content "MARK <tag> <i>" and /Rotate from `rot`.
pub(crate) fn make_pdf(
    dir: &Path,
    name: &str,
    tag: &str,
    n: usize,
    outline: &[(&str, usize, u16)],
) -> std::path::PathBuf {
    let mut doc = Pdf::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    // Resources on the Pages node, so inheritance is exercised.
    let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
    let mut kids = Vec::new();
    for i in 0..n {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::string_literal(format!("MARK {tag} {i}"))],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(lopdf::Stream::new(
            dictionary! {},
            content.encode().unwrap(),
        ));
        let page_id = doc.add_object(
            dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id },
        );
        kids.push(Object::Reference(page_id));
    }
    let mut items: Vec<ObjectId> = Vec::new();
    let outlines_id = doc.new_object_id();
    if !outline.is_empty() {
        // Flat list only (depth ignored beyond 0) is enough for the source side.
        let ids: Vec<ObjectId> = outline.iter().map(|_| doc.new_object_id()).collect();
        for (i, (title, page, _)) in outline.iter().enumerate() {
            let mut d = dictionary! {
                "Title" => Object::string_literal(*title), "Parent" => outlines_id,
                "Dest" => vec![kids[*page].clone(), "XYZ".into(), Object::Null, Object::Null, Object::Null],
            };
            if i > 0 {
                d.set("Prev", ids[i - 1]);
            }
            if i + 1 < ids.len() {
                d.set("Next", ids[i + 1]);
            }
            doc.objects.insert(ids[i], Object::Dictionary(d));
        }
        items = ids;
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => n as i64, "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    if !items.is_empty() {
        doc.objects.insert(
            outlines_id,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines", "First" => items[0], "Last" => *items.last().unwrap(), "Count" => items.len() as i64,
            }),
        );
        catalog.set("Outlines", outlines_id);
    }
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);
    let path = dir.join(name);
    doc.save(&path).unwrap();
    path
}

/// The file at `path`, read into `store`.
pub(crate) fn info(store: &Store, path: &Path) -> Src {
    store.read_file(path, None).unwrap().0
}

/// The annotations of the file `bytes`, read through its source as the
/// open reads them ([`Source::annots`](crate::Source::annots)), before it
/// gives copied marks new ids.
pub(crate) fn annots_read(bytes: &[u8]) -> crate::file::AnnotsInFile {
    let src = crate::forms::fixture::source(bytes.to_vec());
    let pages = (src.pages().iter()).map(|page| (page.id, page.geom.rotate));
    crate::file::annots_in(src.pdf(), pages)
}

/// A document over the file at `path`, in a store of its own.
pub(crate) fn open(
    path: &Path,
    outline: &[FlatOutlineItem],
    policy: Box<dyn crate::history::HistoryPolicy>,
) -> Document {
    let store = Store::new();
    let (primary, stamp) = store.read_file(path, None).unwrap();
    let name = crate::store::file_name(path);
    let came = Came::Read {
        path: path.into(),
        stamp,
    };
    Document::new(store, primary, name, Some(came), outline, policy)
}

/// (marker text, /Rotate) per page of a written PDF, in order.
pub(crate) fn read_pages(bytes: &[u8]) -> Vec<(String, i64)> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    pdf.get_pages()
        .into_values()
        .map(|id| {
            let content = pdf.get_page_content(id);
            let text = String::from_utf8_lossy(&content);
            let mark = text
                .split("MARK ")
                .nth(1)
                .unwrap()
                .split(')')
                .next()
                .unwrap()
                .to_string();
            // Attributes may live on the page or be inherited; both are valid.
            let inherited = |key: &[u8]| {
                let mut at = id;
                for _ in 0..64 {
                    let dict = pdf.get_dictionary(at).ok()?;
                    if let Ok(v) = dict.get(key) {
                        return Some(v.clone());
                    }
                    at = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
                }
                None
            };
            let rot = (inherited(b"Rotate").and_then(|o| o.as_i64().ok())).unwrap_or(0);
            assert!(inherited(b"Resources").is_some(), "page lost its resources");
            assert!(inherited(b"MediaBox").is_some(), "page lost its media box");
            (mark, rot)
        })
        .collect()
}

/// Outline as (depth, title, page number) by walking the written tree.
pub(crate) fn read_outline(bytes: &[u8]) -> Vec<(u16, String, Option<u32>)> {
    outline_in(&Pdf::load_mem(bytes).unwrap())
}

/// [`read_outline`] of a file already read.
pub(crate) fn outline_in(pdf: &Pdf) -> Vec<(u16, String, Option<u32>)> {
    let page_no: HashMap<ObjectId, u32> =
        pdf.get_pages().into_iter().map(|(n, id)| (id, n)).collect();
    fn walk(
        pdf: &Pdf,
        first: Option<ObjectId>,
        depth: u16,
        page_no: &HashMap<ObjectId, u32>,
        out: &mut Vec<(u16, String, Option<u32>)>,
    ) {
        let mut cur = first;
        while let Some(id) = cur {
            let d = pdf.get_dictionary(id).unwrap();
            let raw = d.get(b"Title").unwrap().as_str().unwrap();
            let title = if raw.starts_with(&[0xFE, 0xFF]) {
                let units: Vec<u16> = raw[2..]
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                String::from_utf8_lossy(raw).to_string()
            };
            let page = d
                .get(b"Dest")
                .ok()
                .and_then(|o| o.as_array().ok())
                .and_then(|a| a.first())
                .and_then(|o| o.as_reference().ok())
                .and_then(|r| page_no.get(&r).copied());
            out.push((depth, title, page));
            walk(
                pdf,
                d.get(b"First").ok().and_then(|o| o.as_reference().ok()),
                depth + 1,
                page_no,
                out,
            );
            cur = d.get(b"Next").ok().and_then(|o| o.as_reference().ok());
        }
    }
    let catalog = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let catalog = pdf.get_dictionary(catalog).unwrap();
    let mut out = Vec::new();
    if let Ok(o) = catalog.get(b"Outlines").and_then(|o| o.as_reference()) {
        let od = pdf.get_dictionary(o).unwrap();
        walk(
            pdf,
            od.get(b"First").ok().and_then(|o| o.as_reference().ok()),
            0,
            &page_no,
            &mut out,
        );
    }
    out
}

/// Each bookmark's `/Dest` after its page reference, as text
/// ("/XYZ 36 692 null"), in written order.
pub(crate) fn read_dests(bytes: &[u8]) -> Vec<String> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let outlines = pdf
        .get_dictionary(root)
        .unwrap()
        .get(b"Outlines")
        .unwrap()
        .as_reference()
        .unwrap();
    let mut cur = pdf
        .get_dictionary(outlines)
        .unwrap()
        .get(b"First")
        .ok()
        .and_then(|o| o.as_reference().ok());
    let mut out = Vec::new();
    while let Some(id) = cur {
        let d = pdf.get_dictionary(id).unwrap();
        let dest = d.get(b"Dest").unwrap().as_array().unwrap();
        let parts: Vec<String> = dest[1..]
            .iter()
            .map(|o| match o {
                Object::Name(n) => format!("/{}", String::from_utf8_lossy(n)),
                Object::Null => "null".into(),
                o => format!("{}", o.as_float().unwrap()),
            })
            .collect();
        out.push(parts.join(" "));
        cur = d.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }
    out
}

crate::both_ways!(
    bookmark_positions_survive_saves_and_our_rotation,
    bookmark_positions_survive_saves_and_our_rotation_appended: bookmark_positions
);

fn bookmark_positions(how: How) {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let item = |title: &str, page, view| FlatOutlineItem {
        title: title.into(),
        page: Some(page),
        depth: 0,
        view,
        color: None,
        style: Default::default(),
        open: true,
        object: None,
        kept: false,
    };
    let mut model = open(
        &a,
        &[
            item(
                "At a spot",
                0,
                Some(DestView::Xyz {
                    left: Some(36.0),
                    top: Some(692.0),
                    zoom: None,
                }),
            ),
            item(
                "Fit width",
                1,
                Some(DestView::Other {
                    kind: "FitH".into(),
                    params: vec![Some(500.0)],
                }),
            ),
            item("Page top", 1, None),
        ],
        Box::new(FixedCount(10)),
    );
    // The bookmarks in the file, as an Append writes after them.
    How::Rewritten.save(&mut model, &a);
    let mut model = crate::open::file(&a, None).unwrap().doc;
    // Turning the page in sheaf mustn't move the stored position: it's
    // in PageSpace, which a turn leaves alone.
    let p0 = model.page(0).unwrap();
    model
        .commit_ops(vec![fx::rotate(&[p0], Turn::Quarter)], "rotate")
        .unwrap();
    let want = ["/XYZ 36 692 null", "/FitH 500", "/XYZ null null null"];
    assert_eq!(read_dests(&how.write(&model)), want);
    // A save writes the outline when it changed.
    let p1 = model.page(1).unwrap();
    model
        .commit_ops(
            vec![Op::AddBookmark {
                parent: None,
                at: Sibling::First,
                title: "New".into(),
                target: Some(PageRef::Id(p1)),
                view: Some(DestView::Xyz {
                    left: None,
                    top: Some(792.0),
                    zoom: Some(1.5),
                }),
                color: None,
                style: Default::default(),
                local: None,
            }],
            "add",
        )
        .unwrap();
    let mut want_added = vec!["/XYZ null 792 1.5"];
    want_added.extend(want);
    let bytes = how.write(&model);
    assert_eq!(read_dests(&bytes), want_added);
}

#[test]
fn identity_round_trip() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 3, &[("One", 0, 0), ("Three", 2, 0)]);
    let model = open(
        &a,
        &[
            FlatOutlineItem {
                title: "One".into(),
                page: Some(0),
                depth: 0,
                view: None,
                color: None,
                style: Default::default(),
                open: true,
                object: None,
                kept: false,
            },
            FlatOutlineItem {
                title: "Three".into(),
                page: Some(2),
                depth: 0,
                view: None,
                color: None,
                style: Default::default(),
                open: true,
                object: None,
                kept: false,
            },
        ],
        Box::new(FixedCount(10)),
    );
    let bytes = rewrite(&model).unwrap();
    assert_eq!(
        read_pages(&bytes),
        vec![("A 0".into(), 0), ("A 1".into(), 0), ("A 2".into(), 0)]
    );
    assert_eq!(
        read_outline(&bytes),
        vec![(0, "One".into(), Some(1)), (0, "Three".into(), Some(3))]
    );
}

#[test]
fn edits_materialize() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 4, &[]);
    let b = make_pdf(&dir, "b.pdf", "B", 2, &[]);
    let mut model = open(
        &a,
        &[
            FlatOutlineItem {
                title: "Top".into(),
                page: Some(0),
                depth: 0,
                view: None,
                color: None,
                style: Default::default(),
                open: true,
                object: None,
                kept: false,
            },
            FlatOutlineItem {
                title: "Child é".into(),
                page: Some(2),
                depth: 1,
                view: None,
                color: None,
                style: Default::default(),
                open: true,
                object: None,
                kept: false,
            },
        ],
        Box::new(FixedCount(10)),
    );
    let sb = info(model.store(), &b);
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    model
        .commit_ops(vec![fx::delete(&[ids[1]])], "del")
        .unwrap(); // A0 A2 A3
    model
        .commit_ops(vec![fx::rotate(&[ids[2]], Turn::Quarter)], "rot")
        .unwrap();
    model
        .commit_ops(vec![fx::move_pages(&[ids[3]], Some(ids[0]))], "mv")
        .unwrap(); // A3 A0 A2
    model
        .commit_ops(vec![fx::insert(&sb, 0..2, "a.pdf", Some(ids[2]))], "ins")
        .unwrap(); // A3 A0 B0 B1 A2
    model
        .commit_ops(vec![fx::duplicate(&[ids[0]])], "dup")
        .unwrap(); // A3 A0 A0 B0 B1 A2

    let bytes = rewrite(&model).unwrap();
    assert_eq!(
        read_pages(&bytes),
        vec![
            ("A 3".into(), 0),
            ("A 0".into(), 0),
            ("A 0".into(), 0),
            ("B 0".into(), 0),
            ("B 1".into(), 0),
            ("A 2".into(), 90),
        ]
    );
    // "Top" -> A0 is now page 2; "Child é" -> A2 is page 6; written as UTF-16.
    let outline = read_outline(&bytes);
    assert_eq!(outline.len(), 2);
    assert_eq!((outline[0].0, outline[0].2), (0, Some(2)));
    assert_eq!((outline[1].0, outline[1].2), (1, Some(6)));
    assert_eq!(outline[0].1, "Top");
    assert_eq!(outline[1].1, "Child é");

    // The written file re-opens cleanly and renders the same page count.
    let reopened = Pdf::load_mem(&bytes).unwrap();
    assert_eq!(reopened.get_pages().len(), 6);
}

fn save_modern(path: &Path) {
    let mut pdf = Pdf::load(path).unwrap();
    let mut bytes = Vec::new();
    pdf.save_modern(&mut bytes).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// An Append keeps the file's bytes and adds what changed: a page turned,
/// then pages deleted and inserted and a bookmark renamed. A save in place
/// that leaves a page out is a Rewrite, so the page isn't left in an
/// earlier revision (bug 14), and so is one elsewhere.
fn appends_onto(modern: bool) {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 4, &[("One", 2, 0)]);
    let b = make_pdf(&dir, "b.pdf", "B", 1, &[]);
    if modern {
        save_modern(&a);
    }
    let original = std::fs::read(&a).unwrap();
    // Its bookmark as the open reads it: the file's object.
    let items = crate::open::outline(&info(&Store::new(), &a));
    let mut model = open(&a, &items, Box::new(FixedCount(10)));
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();

    // Rotation only: tree untouched, one page object rewritten.
    model
        .commit_ops(vec![fx::rotate(&[ids[1]], Turn::Half)], "rot")
        .unwrap();
    let bytes = How::Appended.write(&model);
    assert!(bytes.starts_with(&original), "original bytes preserved");
    assert_eq!(
        read_pages(&bytes),
        vec![
            ("A 0".into(), 0),
            ("A 1".into(), 180),
            ("A 2".into(), 0),
            ("A 3".into(), 0)
        ]
    );
    assert_eq!(read_outline(&bytes), vec![(0, "One".into(), Some(3))]);

    // Structural: delete, insert from another file, bookmark rename.
    let sb = info(model.store(), &b);
    model
        .commit_ops(vec![fx::delete(&[ids[0]])], "del")
        .unwrap();
    model
        .commit_ops(vec![fx::insert(&sb, [0], "a.pdf", None)], "ins")
        .unwrap();
    let one = model.state().bookmarks_preorder()[0].1;
    model
        .commit_ops(
            vec![fx::patch_bookmark(
                one,
                BookmarkPatch {
                    title: Some("Uno".into()),
                    target: Patch::Set(PageRef::Id(ids[2])),
                    ..Default::default()
                },
            )],
            "bm",
        )
        .unwrap();
    let bytes = How::Appended.write(&model);
    assert!(bytes.starts_with(&original));
    assert_eq!(
        read_pages(&bytes),
        vec![
            ("A 1".into(), 180),
            ("A 2".into(), 0),
            ("A 3".into(), 0),
            ("B 0".into(), 0)
        ]
    );
    assert_eq!(read_outline(&bytes), vec![(0, "Uno".into(), Some(2))]);

    // A page was deleted and a bookmark renamed: saving the primary path
    // is a full rewrite, so neither is left in an earlier revision (bug
    // 14, strict removal).
    let report = materialize::save_with(
        &mut model,
        &a,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let removed = |kind, count| crate::effects::Removed { kind, count };
    let why = [
        Why::Removes(vec![removed(crate::effects::RemovedKind::Pages, 1)]),
        Why::ReplacesText(vec![removed(
            crate::effects::RemovedKind::BookmarkTitles,
            1,
        )]),
    ];
    assert_eq!(report.why, why);
    let saved = std::fs::read(&a).unwrap();
    assert!(!saved.starts_with(&original));
    assert!(!streams_say(&saved, "MARK A 0") && streams_say(&saved, "MARK A 1"));
    let elsewhere = How::Rewritten.save(&mut model, &dir.join("elsewhere.pdf"));
    assert_eq!(elsewhere.strategy, Some(StrategyKind::Rewrite));
}

/// Whether the file says `text` anywhere: in its bytes, or in a
/// stream's decompressed (an object stream's members among them).
pub(crate) fn says(bytes: &[u8], text: &str) -> bool {
    bytes.windows(text.len()).any(|w| w == text.as_bytes()) || streams_say(bytes, text)
}

/// Whether any stream in the file says `text`, decompressed.
pub(crate) fn streams_say(bytes: &[u8], text: &str) -> bool {
    let pdf = Pdf::load_mem(bytes).unwrap();
    pdf.objects.values().any(|o| match o {
        Object::Stream(s) => {
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            String::from_utf8_lossy(&data).contains(text)
        }
        _ => false,
    })
}

#[test]
fn a_classic_xref_is_appended_to() {
    appends_onto(false);
}

#[test]
fn an_xref_stream_is_appended_to() {
    appends_onto(true);
}

/// `path`'s bytes, written with a cross-reference table and a trailer.
fn classic(path: &Path) -> Vec<u8> {
    let mut pdf = Pdf::load(path).unwrap();
    pdf.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// `bytes` with `keys` written into its trailer, as lopdf writes one.
fn with_trailer_keys(bytes: &[u8], keys: &str) -> Vec<u8> {
    let open = b"trailer\n<<";
    let at = bytes.windows(open.len()).rposition(|w| w == open).unwrap();
    let at = at + open.len();
    [&bytes[..at], keys.as_bytes(), &bytes[at..]].concat()
}

/// A one-page model of `path`, its page turned a quarter.
fn rotated(path: &Path) -> Document {
    let mut model = opened(path);
    let page = model.state().order()[0];
    let rotate = fx::rotate(&[page], Turn::Quarter);
    model.commit_ops(vec![rotate], "rot").unwrap();
    model
}

crate::both_ways!(
    a_stale_xrefstm_is_ignored,
    a_stale_xrefstm_is_ignored_appended: stale_xrefstm
);

/// lopdf 0.45 kept a one-section hybrid-reference file's /XRefStm when it
/// rewrote it, pointing past the end of the new file: an update chains to
/// the table, and a Rewrite leaves the key out.
fn stale_xrefstm(how: How) {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 1, &[]);
    let bytes = classic(&a);
    let table = Pdf::load_mem(&bytes).unwrap().xref_start;
    let stale = format!("/XRefStm {}", bytes.len() + 5000);
    std::fs::write(&a, with_trailer_keys(&bytes, &stale)).unwrap();
    let original = std::fs::read(&a).unwrap();
    let model = rotated(&a);
    let saved = how.write(&model);
    match how {
        How::Appended => {
            let update = String::from_utf8_lossy(&saved[original.len()..]);
            let prev = update.split("/Prev ").nth(1).unwrap();
            let prev = prev.split(|c: char| !c.is_ascii_digit()).next().unwrap();
            assert_eq!(prev.parse::<usize>().unwrap(), table);
        }
        How::Rewritten => assert!(!saved.windows(8).any(|w| w == b"/XRefStm")),
    }
    assert_eq!(read_pages(&saved), [("A 0".into(), 90)]);
}

#[test]
fn a_file_lopdf_rebuilds_is_rewritten() {
    // Its startxref points past the end, so lopdf rebuilds its
    // cross-references by scanning, and keeps its trailer's keys.
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 1, &[]);
    let mut bytes = with_trailer_keys(&classic(&a), "/Prev 9 /XRefStm 9");
    let end = bytes.windows(9).rposition(|w| w == b"startxref").unwrap();
    bytes.truncate(end);
    let past = bytes.len() + 5000;
    bytes.extend_from_slice(format!("startxref\n{past}\n%%EOF\n").as_bytes());
    std::fs::write(&a, &bytes).unwrap();
    assert_eq!(Pdf::load_mem(&bytes).unwrap().xref_start, 0);
    let mut model = rotated(&a);

    // An update would have no section for /Prev to point at.
    let report = materialize::save_with(
        &mut model,
        &a,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert_eq!(report.why, [Why::BaseUnsound]);
    let saved = std::fs::read(&a).unwrap();
    assert_ne!(Pdf::load_mem(&saved).unwrap().xref_start, 0);
    assert_eq!(read_pages(&saved), [("A 0".into(), 90)]);
    let has = |key: &[u8]| saved.windows(key.len()).any(|w| w == key);
    assert!(!has(b"/Prev") && !has(b"/XRefStm"));
}

/// Put a mark of a kind no build knows on page 0 of `path`, as a newer
/// sheaf would have written it.
fn plant_unknown_mark(path: &Path) {
    let mut pdf = Pdf::load(path).unwrap();
    let page0 = pdf.get_pages()[&1];
    let json = r#"{"id":77,"kind":{"Hexagon":{"rect":{"x":1,"y":1,"w":5,"h":5}}},"color":[1,2,3],"opacity":1.0,"stroke":1.0,"author":"future","contents":"from a newer sheaf","modified":""}"#;
    let ap = pdf.add_object(lopdf::Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 60.into(), 60.into()] },
        b"0 0 60 60 re S".to_vec(),
    ));
    let annot = pdf.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Polygon",
        "Rect" => vec![100.into(), 600.into(), 160.into(), 660.into()],
        "NM" => Object::string_literal("sheaf:77"),
        "Contents" => Object::string_literal("from a newer sheaf"),
        "SheafAnnot" => Object::string_literal(json),
        "AP" => dictionary! { "N" => ap },
    });
    pdf.get_dictionary_mut(page0)
        .unwrap()
        .set("Annots", vec![annot.into()]);
    pdf.save(path).unwrap();
}

crate::both_ways!(
    a_mark_this_build_cant_read_survives_a_save,
    a_mark_this_build_cant_read_survives_a_save_appended: unknown_mark_survives
);

fn unknown_mark_survives(how: How) {
    let dir = tempdir();
    let path = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    plant_unknown_mark(&path);
    let found = annots_read(&std::fs::read(&path).unwrap());
    assert!(found.sheaf().is_empty());
    assert_eq!(found.foreign().len(), 1);
    assert_eq!(found.foreign()[0].1.subtype, "Polygon");
    let mut model = open(&path, &[], Box::new(FixedCount(10)));
    let p0 = model.page(0).unwrap();
    let ours = Annot {
        id: AnnotId(0),
        kind: AnnotKind::Square {
            rect: crate::annot::Box2 {
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 20.0,
            },
        },
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "t".into(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    model.commit_ops(vec![fx::add(p0, &ours)], "add").unwrap();
    let bytes = how.write(&model);
    let out = dir.join(format!("out-{how:?}.pdf"));
    std::fs::write(&out, &bytes).unwrap();
    let back = annots_read(&std::fs::read(&out).unwrap());
    assert_eq!(back.sheaf()[&0].len(), 1, "ours, once ({how:?})");
    assert_eq!(back.foreign().len(), 1, "the unknown mark, kept ({how:?})");
    assert_eq!(back.foreign()[0].1.contents, "from a newer sheaf");
}

crate::both_ways!(
    bookmark_colors_and_styles_round_trip,
    bookmark_colors_and_styles_round_trip_appended: bookmark_colors_and_styles
);

fn bookmark_colors_and_styles(how: How) {
    let dir = tempdir();
    let path = make_pdf(
        &dir,
        "a.pdf",
        "A",
        3,
        &[("Red", 0, 0), ("Plain", 1, 0), ("Both", 2, 0)],
    );
    // Colored and styled as another tool (PDFlyer, Acrobat) would.
    {
        let mut pdf = Pdf::load(&path).unwrap();
        let first = pdf
            .catalog()
            .unwrap()
            .get(b"Outlines")
            .unwrap()
            .as_reference()
            .unwrap();
        let first = pdf
            .get_dictionary(first)
            .unwrap()
            .get(b"First")
            .unwrap()
            .as_reference()
            .unwrap();
        let second = pdf
            .get_dictionary(first)
            .unwrap()
            .get(b"Next")
            .unwrap()
            .as_reference()
            .unwrap();
        let third = pdf
            .get_dictionary(second)
            .unwrap()
            .get(b"Next")
            .unwrap()
            .as_reference()
            .unwrap();
        pdf.get_dictionary_mut(first)
            .unwrap()
            .set("C", vec![1.0.into(), 0.0.into(), 0.0.into()]);
        pdf.get_dictionary_mut(third).unwrap().set("F", 3);
        pdf.save(&path).unwrap();
    }
    let items = crate::open::outline(&info(&Store::new(), &path));
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].color, Some([255, 0, 0]));
    assert!(items[1].color.is_none() && items[1].style.is_plain());
    assert!(items[2].style.bold && items[2].style.italic);
    let mut model = open(&path, &items, Box::new(FixedCount(10)));
    // A bookmark edit, so an Append writes the outline.
    let plain = model.state().outline_roots()[1];
    model
        .commit_ops(
            vec![fx::patch_bookmark(
                plain,
                BookmarkPatch {
                    title: Some("Renamed".into()),
                    ..Default::default()
                },
            )],
            "rename",
        )
        .unwrap();
    let bytes = how.write(&model);
    let back = crate::open::outline(&crate::forms::fixture::source(bytes));
    assert_eq!(back.len(), 3, "{how:?}");
    assert_eq!(back[0].color, Some([255, 0, 0]), "{how:?}");
    assert_eq!(back[1].title, "Renamed");
    assert!(back[2].style.bold && back[2].style.italic, "{how:?}");
}

crate::both_ways!(
    a_closed_bookmark_stays_closed_through_a_rewrite,
    a_closed_bookmark_stays_closed_through_a_rewrite_appended: closed_bookmark_stays_closed
);

/// A closed bookmark comes in closed and goes out closed: its
/// `/Count` negative, how many would show were it opened, and an open
/// one's positive, whether the save rewrites the file or appends.
fn closed_bookmark_stays_closed(how: How) {
    let dir = tempdir();
    let mut f = crate::forms::fixture::Form::new(3);
    let p = f.pages.clone();
    let item = |title: &str, page: usize| {
        dictionary! { "Title" => text_string(title), "Dest" => vec![p[page].into(), "Fit".into()] }
    };
    let mut closed = item("Closed", 0);
    closed.set("Count", -2);
    crate::forms::fixture::outline_tree(
        &mut f,
        vec![
            (0, closed),
            (1, item("Under closed", 1)),
            (2, item("Deeper", 1)),
            (0, item("Open", 2)),
            (1, item("Under open", 2)),
        ],
    );
    let path = dir.join("closed.pdf");
    std::fs::write(&path, f.bytes()).unwrap();
    let items = crate::open::outline(&info(&Store::new(), &path));
    let open_flags = |items: &[FlatOutlineItem]| items.iter().map(|i| i.open).collect::<Vec<_>>();
    assert_eq!(open_flags(&items), [false, true, true, true, true]);
    /// (title, `/Count`) of each item below `first`, in pre-order.
    fn counts(pdf: &Pdf, first: Option<ObjectId>, out: &mut Vec<(String, Option<i64>)>) {
        let mut next = first;
        while let Some(id) = next {
            let d = pdf.get_dictionary(id).unwrap();
            let count = d.get(b"Count").and_then(Object::as_i64).ok();
            out.push((crate::file::pdf_text(d.get(b"Title").unwrap()), count));
            counts(
                pdf,
                d.get(b"First").and_then(Object::as_reference).ok(),
                out,
            );
            next = d.get(b"Next").and_then(Object::as_reference).ok();
        }
    }
    let mut model = open(&path, &items, Box::new(FixedCount(10)));
    // A bookmark edit, so an Append writes the outline.
    let st = model.state();
    let leaf = st.bookmark(st.outline_roots()[1]).unwrap().kids[0];
    model
        .commit_ops(
            vec![fx::patch_bookmark(
                leaf,
                BookmarkPatch {
                    title: Some("Renamed".into()),
                    ..Default::default()
                },
            )],
            "rename",
        )
        .unwrap();
    let bytes = how.write(&model);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let root = pdf
        .catalog()
        .unwrap()
        .get(b"Outlines")
        .and_then(Object::as_reference)
        .unwrap();
    let root = pdf.get_dictionary(root).unwrap();
    // Shown: Closed, Open and what's under Open.
    assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 3);
    let mut got = Vec::new();
    counts(
        &pdf,
        root.get(b"First").and_then(Object::as_reference).ok(),
        &mut got,
    );
    let want: Vec<(String, Option<i64>)> = [
        ("Closed", Some(-2)),
        ("Under closed", Some(1)),
        ("Deeper", None),
        ("Open", Some(1)),
        ("Renamed", None),
    ]
    .into_iter()
    .map(|(t, c)| (t.to_string(), c))
    .collect();
    assert_eq!(got, want, "{how:?}");
    let back = crate::open::outline(&crate::forms::fixture::source(bytes));
    assert_eq!(open_flags(&back), open_flags(&items), "{how:?}");
}

/// Bug 13 (todos/m9.md): after a save in place, a version from before
/// it still means the file it was made from, whatever the path holds
/// now: undone past the save and saved again, its pages are written,
/// whether the first save was a Rewrite or an Append, and whether the
/// save after the undo is planned (an Append onto the file the first
/// wrote, as a move removes nothing) or a Rewrite.
#[test]
fn undo_past_a_save_then_save_writes_the_right_pages() {
    undo_past_a_save(How::Rewritten);
}

#[test]
fn undo_past_a_save_then_save_writes_the_right_pages_appended() {
    undo_past_a_save(How::Appended);
}

fn undo_past_a_save(first: How) {
    for again in [None, Some(How::Rewritten)] {
        let what = format!("{first:?}, then {again:?}");
        let dir = tempdir();
        let a = make_pdf(&dir, "a.pdf", "A", 3, &[]);
        let mut model = open(&a, &[], Box::new(FixedCount(10)));
        let ids: Vec<PageId> = model.state().order().iter().copied().collect();
        model
            .commit_ops(vec![fx::move_pages(&[ids[2]], Some(ids[0]))], "move")
            .unwrap();
        let saved = first.save(&mut model, &a);
        assert_eq!(saved.strategy, Some(first.strategy()), "{what}");
        let saved = read_pages(&std::fs::read(&a).unwrap());
        assert_eq!(saved[0].0, "A 2", "{what}");
        model.undo().unwrap();
        let report = match again {
            None => materialize::save_with(
                &mut model,
                &a,
                SaveHow::Planned,
                &AskAnswers::default(),
                &NoProgress,
            )
            .unwrap(),
            Some(how) => how.save(&mut model, &a),
        };
        let want = again.map_or(StrategyKind::Append, How::strategy);
        assert_eq!(report.strategy, Some(want), "{what}");
        let pages: Vec<String> = read_pages(&std::fs::read(&a).unwrap())
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        assert_eq!(pages, vec!["A 0", "A 1", "A 2"], "{what}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A note of ours as the thread tests use it.
pub(crate) fn thread_mark(kind: crate::annot::AnnotKind, contents: &str) -> Annot {
    Annot {
        id: crate::annot::AnnotId(0),
        kind,
        color: [255, 200, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "Greg".into(),
        contents: contents.into(),
        modified: "D:20261014000000".into(),
        created: "D:20261014000000".into(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// Page 1's annotations: (object, dict), in /Annots order.
pub(crate) fn page_annots(bytes: &[u8]) -> Vec<(ObjectId, Dictionary)> {
    page_annots_on(bytes, 0)
}

/// Page `index`'s annotations (from 0): (object, dict), in /Annots
/// order.
pub(crate) fn page_annots_on(bytes: &[u8], index: usize) -> Vec<(ObjectId, Dictionary)> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let page = pdf.page_iter().nth(index).unwrap();
    pdf.get_dictionary(page)
        .unwrap()
        .get(b"Annots")
        .map(|a| a.as_array().unwrap().clone())
        .unwrap_or_default()
        .iter()
        .map(|o| {
            let id = o.as_reference().unwrap();
            (id, pdf.get_dictionary(id).unwrap().clone())
        })
        .collect()
}

pub(crate) fn opened(path: &Path) -> Document {
    open(path, &[], Box::new(FixedCount(10)))
}

crate::both_ways!(
    replies_and_statuses_thread_under_their_comment,
    replies_and_statuses_thread_under_their_comment_appended: replies_and_statuses_thread
);

fn replies_and_statuses_thread(how: How) {
    use crate::annot::{AnnotKind, Pt};
    let dir = tempdir();
    let a_path = make_pdf(&dir, "a.pdf", "A", 1, &[]);
    let mut model = opened(&a_path);
    let p0 = model.page(0).unwrap();
    let note = thread_mark(
        AnnotKind::Note {
            pos: Pt { x: 100.0, y: 100.0 },
        },
        "Tie to the K-1",
    );
    model.commit_ops(vec![fx::add(p0, &note)], "note").unwrap();
    let parent = model.state().marks_on(p0).next().unwrap().name();
    for (kind, text) in [
        (
            AnnotKind::Reply {
                parent: parent.clone(),
            },
            "Tied.",
        ),
        (
            AnnotKind::Status {
                parent: parent.clone(),
                model: crate::annot::REVIEW.into(),
                state: "Accepted".into(),
            },
            "Accepted set by Greg",
        ),
    ] {
        model
            .commit_ops(vec![fx::add(p0, &thread_mark(kind, text))], "thread")
            .unwrap();
    }
    let want: Vec<Annot> = model.state().marks_on(p0).cloned().collect();
    let bytes = how.write(&model);
    let list = page_annots(&bytes);
    assert_eq!(list.len(), 3);
    let (note_id, note_dict) = &list[0];
    for (_, d) in &list[1..] {
        assert_eq!(d.get(b"Subtype").unwrap().as_name().unwrap(), b"Text");
        assert_eq!(d.get(b"IRT").unwrap().as_reference().unwrap(), *note_id);
        assert_eq!(d.get(b"Rect").unwrap(), note_dict.get(b"Rect").unwrap());
    }
    let status = &list[2].1;
    assert_eq!(status.get(b"State").unwrap().as_str().unwrap(), b"Accepted");
    assert_eq!(
        status.get(b"StateModel").unwrap().as_str().unwrap(),
        b"Review"
    );
    // Hidden, as Acrobat writes a state.
    assert_eq!(status.get(b"F").unwrap().as_i64().unwrap() & 2, 2);
    assert_eq!(annots_read(&bytes).sheaf()[&0], want);

    // Deleting the note takes its thread.
    let id = want[0].id;
    model
        .commit_ops(vec![fx::delete_annots(&[id])], "rm")
        .unwrap();
    assert!(model.state().marks_on(p0).next().is_none());
}

/// A one-page file with a note of ours and, as Acrobat leaves them, a
/// reply to it and a comment of its own with no /NM.
fn file_with_their_comments(dir: &Path) -> (std::path::PathBuf, Vec<Annot>) {
    use crate::annot::{AnnotKind, Pt};
    let a_path = make_pdf(dir, "a.pdf", "A", 1, &[]);
    let mut model = opened(&a_path);
    let p0 = model.page(0).unwrap();
    let note = thread_mark(
        AnnotKind::Note {
            pos: Pt { x: 100.0, y: 100.0 },
        },
        "ours",
    );
    model.commit_ops(vec![fx::add(p0, &note)], "note").unwrap();
    let ours: Vec<Annot> = model.state().marks_on(p0).cloned().collect();
    let bytes = rewrite(&model).unwrap();
    let mut pdf = Pdf::load_mem(&bytes).unwrap();
    let page0 = pdf.get_pages()[&1];
    let note_obj = page_annots(&bytes)[0].0;
    let rect = || Object::Array(vec![300.into(), 300.into(), 320.into(), 320.into()]);
    let reply = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "IRT" => note_obj, "NM" => Object::string_literal("acro-reply"),
        "T" => Object::string_literal("Pat"), "Contents" => Object::string_literal("Agreed"),
    });
    let theirs = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "T" => Object::string_literal("Pat"), "Contents" => Object::string_literal("No name"),
    });
    let mut annots = pdf
        .get_dictionary(page0)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    annots.extend([Object::Reference(reply), Object::Reference(theirs)]);
    pdf.get_dictionary_mut(page0)
        .unwrap()
        .set("Annots", Object::Array(annots));
    let path = dir.join("theirs.pdf");
    pdf.save(&path).unwrap();
    (path, ours)
}

crate::both_ways!(
    their_replies_follow_our_marks_across_saves,
    their_replies_follow_our_marks_across_saves_appended: replies_follow_our_marks
);

fn replies_follow_our_marks(how: How) {
    // Bug 6: every save rewrites our marks as new objects.
    let dir = tempdir();
    let (path, ours) = file_with_their_comments(&dir);
    let found = annots_read(&std::fs::read(&path).unwrap());
    let reply = found
        .foreign()
        .into_iter()
        .map(|(_, f)| f)
        .find(|f| f.name == "acro-reply")
        .unwrap();
    assert_eq!(reply.parent.as_deref(), Some(ours[0].name().as_str()));

    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    let mut note = model.state().marks_on(p0).next().unwrap().clone();
    note.contents = "ours, edited".into();
    model.commit_ops(vec![fx::update(&note)], "edit").unwrap();
    let out = how.write(&model);
    let list = page_annots(&out);
    let note_obj = list
        .iter()
        .find(|(_, d)| d.has(b"SheafAnnot"))
        .map(|(id, _)| *id)
        .unwrap();
    let (_, reply) = list
        .iter()
        .find(|(_, d)| {
            d.get(b"NM").ok().map(crate::file::pdf_text).as_deref() == Some("acro-reply")
        })
        .unwrap();
    assert_eq!(
        reply.get(b"IRT").unwrap().as_reference().unwrap(),
        note_obj,
        "{how:?}"
    );
    // Read back from what was saved, it's still in the note's thread.
    let again = annots_read(&out);
    let reply = again
        .foreign()
        .into_iter()
        .map(|(_, f)| f)
        .find(|f| f.name == "acro-reply")
        .unwrap();
    assert_eq!(
        reply.parent.as_deref(),
        Some(ours[0].name().as_str()),
        "{how:?}"
    );
}

/// A page with another tool's square ("sq", with Pat's reply), and an
/// Acrobat replace-text edit (Caret "c1", its StrikeOut grouped under it).
fn file_with_their_shapes(dir: &Path) -> std::path::PathBuf {
    let a_path = make_pdf(dir, "a.pdf", "A", 1, &[]);
    let mut pdf = Pdf::load(&a_path).unwrap();
    let page0 = pdf.get_pages()[&1];
    let square = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square",
        "Rect" => vec![100.into(), 100.into(), 200.into(), 150.into()],
        "NM" => Object::string_literal("sq"), "T" => Object::string_literal("Pat"),
        "C" => vec![0.into(), 0.into(), 1.into()], "BS" => dictionary! { "W" => 2 },
    });
    let reply = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text",
        "Rect" => vec![100.into(), 100.into(), 120.into(), 120.into()],
        "IRT" => square, "NM" => Object::string_literal("sq-reply"),
        "T" => Object::string_literal("Pat"), "Contents" => Object::string_literal("Why?"),
    });
    let caret = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Caret",
        "Rect" => vec![300.into(), 300.into(), 306.into(), 312.into()],
        "NM" => Object::string_literal("c1"), "Contents" => Object::string_literal("the"),
    });
    let strike = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "StrikeOut",
        "Rect" => vec![306.into(), 300.into(), 330.into(), 312.into()],
        "QuadPoints" => vec![306.into(), 312.into(), 330.into(), 312.into(), 306.into(), 300.into(), 330.into(), 300.into()],
        "IRT" => caret, "RT" => "Group", "NM" => Object::string_literal("c1-strike"),
    });
    pdf.get_dictionary_mut(page0).unwrap().set(
        "Annots",
        vec![square.into(), reply.into(), caret.into(), strike.into()],
    );
    let path = dir.join("shapes.pdf");
    pdf.save(&path).unwrap();
    path
}

crate::both_ways!(
    a_comment_made_ours_takes_its_place_and_keeps_its_replies,
    a_comment_made_ours_takes_its_place_and_keeps_its_replies_appended: made_ours_takes_its_place
);

fn made_ours_takes_its_place(how: How) {
    let dir = tempdir();
    let path = file_with_their_shapes(&dir);
    let found = annots_read(&std::fs::read(&path).unwrap());
    let mark = |name: &str| {
        *found
            .foreign()
            .into_iter()
            .map(|(_, f)| f)
            .find(|f| f.name == name)
            .unwrap()
            .mark
            .clone()
            .unwrap()
    };
    let mut square = mark("sq");
    assert_eq!(square.color, [0, 0, 255]);
    square.kind = square.kind.translated(10.0, 0.0);
    let replace = mark("c1");
    assert!(matches!(
        replace.kind,
        crate::annot::AnnotKind::Replace { .. }
    ));
    // The strikeout is part of the replacement, not a mark of its own.
    assert!(
        found
            .foreign()
            .into_iter()
            .map(|(_, f)| f)
            .find(|f| f.name == "c1-strike")
            .unwrap()
            .mark
            .is_none()
    );
    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    let object = |model: &Document, name: &str| {
        (model.state().annots_on(p0))
            .find(|(_, a)| a.name() == name)
            .and_then(|(_, a)| match &a.body {
                crate::model::AnnotBody::Mark(m) => m.src.clone(),
                crate::model::AnnotBody::Foreign(f) => f.src.clone(),
                crate::model::AnnotBody::Widget(w) => w.src.clone(),
            })
    };
    let was = object(&model, "sq");
    // Each made ours, and the square moved.
    let named = |nm: &str| AnnotRef::Nm {
        nm: nm.into(),
        page: Some(PageRef::Id(p0)),
    };
    let ops = vec![
        Op::AdoptAnnotations {
            annots: vec![named("sq"), named("c1")],
        },
        Op::PatchMarks {
            marks: vec![named("sq")],
            patch: sheaf_edit::MarkPatch {
                kind: Some(crate::tx::mark_kind(&square.kind)),
                ..Default::default()
            },
            imported: None,
        },
    ];
    model.commit_ops(ops, "adopt").unwrap();
    // Theirs are ours now, the objects they were, and their parts
    // are gone with them; Pat's reply stays.
    let theirs: Vec<String> = (model.state().foreign_on(p0))
        .map(|(_, f)| f.summary.name.clone())
        .collect();
    assert_eq!(theirs, ["sq-reply"]);
    assert!(was.is_some() && object(&model, "sq") == was);
    let out = how.write(&model);
    let list = page_annots(&out);
    let subtypes: Vec<String> = list
        .iter()
        .map(|(_, d)| crate::import::subtype(d))
        .collect();
    // Theirs are gone; ours (a Caret with its own StrikeOut) are
    // there instead, and Pat's reply.
    assert_eq!(
        subtypes.iter().filter(|s| *s == "Square").count(),
        1,
        "{subtypes:?}"
    );
    assert_eq!(subtypes.iter().filter(|s| *s == "StrikeOut").count(), 1);
    let (sq_obj, sq) = list
        .iter()
        .find(|(_, d)| crate::import::subtype(d) == "Square")
        .unwrap();
    assert!(sq.has(b"SheafAnnot"));
    assert_eq!(crate::file::thread_name(sq), "sq");
    let (_, reply) = list
        .iter()
        .find(|(_, d)| crate::file::thread_name(d) == "sq-reply")
        .unwrap();
    assert_eq!(reply.get(b"IRT").unwrap().as_reference().unwrap(), *sq_obj);
    // Read back, the square is ours, named as it was, and the reply
    // is still in its thread.
    let again = annots_read(&out);
    let ours = &again.sheaf()[&0];
    assert!(ours.iter().any(|a| a.name == "sq" && a.kind == square.kind));
    assert!(ours.iter().any(|a| a.name == "c1"));
    let names: Vec<(&str, Option<&str>)> = again
        .foreign()
        .into_iter()
        .map(|(_, f)| f)
        .map(|f| (f.name.as_str(), f.parent.as_deref()))
        .collect();
    assert_eq!(names, [("sq-reply", Some("sq"))], "{how:?}");
}

/// Another tool's comment made ours is the comment's object, and the
/// part grouped under the replacement is what the mark draws, but a save
/// in place rewrites the file. Appended, the removal check vetoes it: the
/// replacement writes a strikeout of its own, and the old one's `/NM`
/// would stay readable in the earlier revision. So a Rewrite is right,
/// whatever the planner says. What it says now is a stopgap (slice 10's
/// notes, "After the commit", a comment made ours, left for Greg): the
/// adoption keeps each comment's id (slice 11's `AdoptAnnotations`), so
/// the square and the caret are kept, but the strikeout grouped under the
/// caret goes as a comment, the replacement writing one of its own: it
/// plans a Rewrite for one comment removed, where the reader made two
/// theirs and removed none. Slice 8 counted an adopted comment as kept;
/// counted so again, the plan appends and the removal check's veto makes
/// the save a Rewrite (`Why::RemovalCheck`): either reason passes here.
/// Appended anyway, as the both-ways tests append, the reader is told the
/// text stays readable. Either way, the mark deleted then is something
/// gone.
#[test]
fn a_comment_made_ours_is_saved_in_place() {
    use std::sync::Arc;

    use super::append::{self, Refused};
    use super::key::ObjKey;
    use super::plan::{Onto, forced_append};
    use super::removal::Mode;
    use crate::effects::{Removed, RemovedKind};

    let removes = |count| {
        vec![Why::Removes(vec![Removed {
            kind: RemovedKind::Comments,
            count,
        }])]
    };
    for appended in [false, true] {
        let dir = tempdir();
        let path = file_with_their_shapes(&dir);
        let original = std::fs::read(&path).unwrap();
        let mut model = opened(&path);
        let p0 = model.page(0).unwrap();
        for name in ["sq", "c1"] {
            let (id, _) = (model.state().foreign_on(p0))
                .find(|(_, f)| f.summary.name == name)
                .unwrap();
            model.commit_ops(vec![fx::adopt(&[id])], "adopt").unwrap();
        }
        let theirs: Vec<String> = (model.state().foreign_on(p0))
            .map(|(_, f)| f.summary.name.clone())
            .collect();
        assert_eq!(theirs, ["sq-reply"]);
        match appended {
            false => {
                // The removal check, were it appended.
                let input = model.recovery_input();
                let plan = forced_append(&input, Onto::Saved);
                let base: Arc<[u8]> = original.clone().into();
                let refused =
                    append::append(&input, &plan, Arc::new(base), Mode::Veto, &NoProgress)
                        .err()
                        .unwrap();
                let strike = (page_annots(&original).into_iter())
                    .find(|(_, d)| crate::file::thread_name(d) == "c1-strike")
                    .unwrap()
                    .0;
                assert!(
                    matches!(
                        refused.downcast_ref::<Refused>(),
                        Some(Refused::Removes(found)) if !found.derived
                            && matches!(found.key, ObjKey::Base { obj, copy: 0, .. } if obj == strike)
                    ),
                    "{refused:#}"
                );
                let saved = materialize::save_with(
                    &mut model,
                    &path,
                    SaveHow::Planned,
                    &AskAnswers::default(),
                    &NoProgress,
                )
                .unwrap();
                assert_eq!(saved.strategy, Some(StrategyKind::Rewrite));
                // KNOWN MISCOUNT, not the spec: `Removes([Comments: 1])`
                // today, the caret's strikeout (see the doc comment).
                assert!(
                    matches!(
                        saved.why.as_slice(),
                        [Why::Removes(_) | Why::RemovalCheck { .. }]
                    ),
                    "{:?}",
                    saved.why
                );
                // The old strikeout is gone with its name.
                assert!(!says(&std::fs::read(&path).unwrap(), "c1-strike"));
            }
            true => {
                let saved = materialize::save_appended(&mut model, &path).unwrap();
                assert_eq!(saved.strategy, Some(StrategyKind::Append));
                let told = materialize::Note(append::STAYS_READABLE.into());
                assert!(saved.notes.contains(&told), "{:?}", saved.notes);
                assert!(std::fs::read(&path).unwrap().starts_with(&original));
            }
        }
        assert!(!model.is_dirty(), "{appended}");
        let sq = (model.state().marks_on(p0))
            .find(|a| a.name == "sq")
            .unwrap()
            .id;
        model
            .commit_ops(vec![fx::delete_annots(&[sq])], "delete")
            .unwrap();
        // The mark and its thread's reply.
        let saved = materialize::save_with(
            &mut model,
            &path,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        );
        let saved = saved.unwrap();
        assert_eq!(saved.strategy, Some(StrategyKind::Rewrite), "{appended}");
        assert_eq!(saved.why, removes(2), "{appended}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Of two comments of another tool's named alike on a page, the one
/// made ours is the one that goes, whichever it is and whatever was
/// made ours before, and the mark is its object.
#[test]
fn of_two_comments_named_alike_the_one_made_ours_goes() {
    let dir = tempdir();
    let path = make_pdf(&dir, "a.pdf", "A", 1, &[]);
    let mut pdf = Pdf::load(&path).unwrap();
    let page0 = pdf.get_pages()[&1];
    let squares = ["left box", "right box"].map(|text| {
        let x = if text == "left box" { 100 } else { 300 };
        let square = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square",
            "Rect" => vec![x.into(), 100.into(), (x + 100).into(), 200.into()],
            "NM" => Object::string_literal("dup"), "T" => Object::string_literal("Pat"),
            "C" => vec![0.into(), 0.into(), 1.into()],
            "Contents" => Object::string_literal(text),
        });
        Object::Reference(square)
    });
    (pdf.get_dictionary_mut(page0).unwrap()).set("Annots", squares.to_vec());
    pdf.save(&path).unwrap();
    for picks in [["left box", "right box"], ["right box", "left box"]] {
        let mut model = opened(&path);
        let p0 = model.page(0).unwrap();
        for (n, pick) in picks.into_iter().enumerate() {
            let (id, f) = (model.state().foreign_on(p0))
                .find(|(_, f)| f.summary.contents == pick)
                .unwrap();
            let was = f.src.clone();
            model.commit_ops(vec![fx::adopt(&[id])], "adopt").unwrap();
            let st = model.state();
            let left: Vec<&str> = (st.foreign_on(p0))
                .map(|(_, f)| f.summary.contents.as_str())
                .collect();
            assert_eq!(left, picks[n + 1..], "{picks:?}");
            let mark = (st.mark_records_on(p0))
                .find(|m| m.annot.contents == pick)
                .unwrap();
            assert!(was.is_some() && mark.src == was, "{pick}");
        }
    }
}

/// A link to a page a save didn't keep isn't in the file it wrote. The
/// document keeps it, as it keeps every record a save wrote from, and
/// once the page is back, the next save writes it again.
#[test]
fn a_link_a_save_left_out_is_gone_from_its_file() {
    let dir = tempdir();
    let path = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let mut pdf = Pdf::load(&path).unwrap();
    let pages = pdf.get_pages();
    let link = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![72.into(), 72.into(), 200.into(), 100.into()],
        "Dest" => vec![pages[&2].into(), "Fit".into()],
    });
    let note = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text",
        "Rect" => vec![300.into(), 300.into(), 320.into(), 320.into()],
        "NM" => Object::string_literal("c1"), "Contents" => Object::string_literal("hello"),
    });
    (pdf.get_dictionary_mut(pages[&1]).unwrap()).set("Annots", vec![link.into(), note.into()]);
    pdf.save(&path).unwrap();
    let mut model = opened(&path);
    let [p0, p1] = [0, 1].map(|i| model.page(i).unwrap());
    assert_eq!(model.state().foreign_on(p0).count(), 2);
    model.commit_ops(vec![fx::delete(&[p1])], "delete").unwrap();
    let subtypes = |path: &Path| -> Vec<String> {
        let saved = Pdf::load(path).unwrap();
        let page = saved.page_iter().next().unwrap();
        (saved.get_page_annotations(page).unwrap().into_iter())
            .map(|a| String::from_utf8_lossy(a.get(b"Subtype").unwrap().as_name().unwrap()).into())
            .collect()
    };
    let report = materialize::save_with(
        &mut model,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    );
    assert_eq!(report.unwrap().strategy, Some(StrategyKind::Rewrite));
    assert_eq!(subtypes(&path), ["Text"]);
    assert_eq!(model.state().foreign_on(p0).count(), 2);
    model.undo().unwrap();
    materialize::save_with(
        &mut model,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    assert_eq!(subtypes(&path), ["Link", "Text"]);
}

crate::both_ways!(
    deleted_comments_and_their_threads_stay_out_of_the_file,
    deleted_comments_and_their_threads_stay_out_of_the_file_appended: deleted_comments_stay_out
);

/// Another tool's comment deleted, and our note with Acrobat's reply to
/// it, are out of both writers' page, and out of the file a save
/// writes, whose bytes say neither comment: a save in place of a
/// document that deleted something is a Rewrite, so they aren't left in
/// an earlier revision. Undone, they're all written again.
fn deleted_comments_stay_out(how: How) {
    let dir = tempdir();
    let (path, ours) = file_with_their_comments(&dir);
    let found = annots_read(&std::fs::read(&path).unwrap());
    let unnamed = found
        .foreign()
        .into_iter()
        .map(|(_, f)| f)
        .find(|f| f.contents == "No name")
        .unwrap();
    let contents = |bytes: &[u8]| -> Vec<String> {
        page_annots(bytes)
            .iter()
            .map(|(_, d)| {
                d.get(b"Contents")
                    .map(crate::file::pdf_text)
                    .unwrap_or_default()
            })
            .collect()
    };
    let copy = dir.join(format!("theirs-{how:?}.pdf"));
    std::fs::copy(&path, &copy).unwrap();
    let mut model = opened(&copy);
    let p0 = model.page(0).unwrap();
    // Theirs, by name.
    model
        .commit_ops(
            vec![fx::delete_named(p0, &[&unnamed.name])],
            "delete theirs",
        )
        .unwrap();
    // Ours: Acrobat's reply to it goes too.
    model
        .commit_ops(vec![fx::delete_annots(&[ours[0].id])], "delete ours")
        .unwrap();
    let save = |m: &Document| how.write(m);
    let written = save(&model);
    assert!(contents(&written).is_empty(), "{how:?}");
    // Saved in place, it's a Rewrite: something's gone.
    let file = match how {
        How::Appended => {
            let saved = materialize::save_with(
                &mut model,
                &copy,
                SaveHow::Planned,
                &AskAnswers::default(),
                &NoProgress,
            );
            assert_eq!(saved.unwrap().strategy, Some(StrategyKind::Rewrite));
            std::fs::read(&copy).unwrap()
        }
        How::Rewritten => written,
    };
    for gone in ["No name", "Agreed"] {
        assert!(!says(&file, gone), "{gone}, {how:?}");
    }
    // Undone, they're all written again: after the save in place pruned
    // them, which no Append can refer to, by a Rewrite.
    model.undo().unwrap().unwrap();
    model.undo().unwrap().unwrap();
    let again = match how {
        How::Appended => {
            let refused = materialize::appended(&model).unwrap_err().to_string();
            assert!(refused.contains("the saved file hasn't got"), "{refused}");
            materialize::save_with(
                &mut model,
                &copy,
                SaveHow::Planned,
                &AskAnswers::default(),
                &NoProgress,
            )
            .unwrap();
            std::fs::read(&copy).unwrap()
        }
        How::Rewritten => save(&model),
    };
    let mut back = contents(&again);
    back.sort();
    assert_eq!(back, ["Agreed", "No name", "ours"], "{how:?}");
}

/// A save writes each of the page's annotations, ours and other tools',
/// under its name, and leaves the records as they were (M10.5 §9.1).
/// A comment of theirs deleted then is out of the next save's file.
#[test]
fn a_save_writes_marks_and_their_comments() {
    saves_marks_and_comments(false);
}

#[test]
fn a_save_writes_marks_and_their_comments_appended() {
    saves_marks_and_comments(true);
}

fn saves_marks_and_comments(in_place: bool) {
    let dir = tempdir();
    let (path, _) = file_with_their_comments(&dir);
    let copy = dir.join(format!("marks-{in_place}.pdf"));
    std::fs::copy(&path, &copy).unwrap();
    let mut model = opened(&copy);
    let p0 = model.page(0).unwrap();
    let note = thread_mark(
        crate::annot::AnnotKind::Note {
            pos: crate::annot::Pt { x: 50.0, y: 50.0 },
        },
        "another",
    );
    model.commit_ops(vec![fx::add(p0, &note)], "note").unwrap();
    let before = model.state().clone();
    let (report, out) = match in_place {
        true => (materialize::save_appended(&mut model, &copy), copy.clone()),
        false => {
            let out = dir.join("marks-out.pdf");
            let report = materialize::save_with(
                &mut model,
                &out,
                SaveHow::Planned,
                &AskAnswers::default(),
                &NoProgress,
            );
            (report, out)
        }
    };
    let want = [StrategyKind::Rewrite, StrategyKind::Append][usize::from(in_place)];
    assert_eq!(report.unwrap().strategy, Some(want));
    assert!(*model.state() == before && !model.is_dirty());
    let saved = Pdf::load(&out).unwrap();
    let page = saved.page_iter().next().unwrap();
    let names: Vec<String> = (saved.get_page_annotations(page).unwrap().into_iter())
        .map(crate::file::thread_name)
        .collect();
    let st = model.state();
    for (_, a) in st.annots_on(p0) {
        assert!(names.contains(&a.name()), "{} in {names:?}", a.name());
    }
    // A comment of theirs deleted now goes from the next save.
    let name = (st.foreign_on(p0))
        .find(|(_, f)| f.summary.contents == "No name")
        .map(|(_, f)| f.summary.name.clone())
        .unwrap();
    model
        .commit_ops(vec![fx::delete_named(p0, &[&name])], "delete theirs")
        .unwrap();
    let report = materialize::save_with(
        &mut model,
        &out,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    );
    assert_eq!(report.unwrap().strategy, Some(StrategyKind::Rewrite));
    let bytes = std::fs::read(&out).unwrap();
    assert!(!says(&bytes, "No name") && says(&bytes, "Agreed"));
}

crate::both_ways!(
    a_duplicate_of_a_page_without_their_comment_stays_without_it,
    a_duplicate_of_a_page_without_their_comment_stays_without_it_appended: dup_without_theirs
);

/// Another tool's comment deleted from a page doesn't come back on the
/// page's duplicate (M10.5 bug 12): the copy's comments are copies of
/// the page's records, each copy 1 of its object under an id of its
/// own, and neither page of either writer, nor the file a save writes,
/// has the deleted one.
fn dup_without_theirs(how: How) {
    let dir = tempdir();
    let (path, _) = file_with_their_comments(&dir);
    let copy = dir.join(format!("dup-{how:?}.pdf"));
    std::fs::copy(&path, &copy).unwrap();
    let mut model = opened(&copy);
    let p0 = model.page(0).unwrap();
    let unnamed = (model.state().foreign_on(p0))
        .find(|(_, f)| f.summary.contents == "No name")
        .map(|(_, f)| f.summary.name.clone())
        .unwrap();
    model
        .commit_ops(vec![fx::delete_named(p0, &[&unnamed])], "delete theirs")
        .unwrap();
    model
        .commit_ops(vec![fx::duplicate(&[p0])], "duplicate")
        .unwrap();
    let st = model.state();
    let p1 = st.page_at(1).unwrap();
    let theirs = |page| -> Vec<(AnnotId, String, u32)> {
        (st.foreign_on(page))
            .map(|(id, f)| (id, f.summary.contents.clone(), f.copy))
            .collect()
    };
    let (on0, on1) = (theirs(p0), theirs(p1));
    let texts = |list: &[(AnnotId, String, u32)]| -> Vec<String> {
        list.iter().map(|(_, t, _)| t.clone()).collect()
    };
    assert_eq!(texts(&on0), ["Agreed"]);
    assert_eq!(texts(&on1), ["Agreed"]);
    assert_eq!((on0[0].2, on1[0].2), (0, 1));
    assert_ne!(on0[0].0, on1[0].0, "a copy has an id of its own");
    let written = how.write(&model);
    let pdf = Pdf::load_mem(&written).unwrap();
    for page in pdf.page_iter() {
        let list = pdf.get_dictionary(page).unwrap().get(b"Annots");
        let list = list
            .map(|l| l.as_array().unwrap().clone())
            .unwrap_or_default();
        for e in list {
            let d = pdf.get_dictionary(e.as_reference().unwrap()).unwrap();
            let text = d.get(b"Contents").map(crate::file::pdf_text);
            assert_ne!(text.ok().as_deref(), Some("No name"), "{how:?}");
        }
    }
    let file = match how {
        How::Appended => {
            let saved = materialize::save_with(
                &mut model,
                &copy,
                SaveHow::Planned,
                &AskAnswers::default(),
                &NoProgress,
            );
            assert_eq!(saved.unwrap().strategy, Some(StrategyKind::Rewrite));
            std::fs::read(&copy).unwrap()
        }
        How::Rewritten => written,
    };
    assert!(!says(&file, "No name"), "{how:?}");
}

crate::both_ways!(
    a_reply_names_a_comment_that_had_no_name,
    a_reply_names_a_comment_that_had_no_name_appended: a_reply_names_a_comment
);

fn a_reply_names_a_comment(how: How) {
    use crate::annot::AnnotKind;
    let dir = tempdir();
    let (path, _) = file_with_their_comments(&dir);
    let found = annots_read(&std::fs::read(&path).unwrap());
    let unnamed = found
        .foreign()
        .into_iter()
        .map(|(_, f)| f)
        .find(|f| f.contents == "No name")
        .unwrap();
    assert!(unnamed.name.starts_with("sheaf-anon:"));
    // The same name every time it's read.
    assert_eq!(
        annots_read(&std::fs::read(&path).unwrap()).pages,
        found.pages
    );
    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    model
        .commit_ops(
            vec![fx::add(
                p0,
                &thread_mark(
                    AnnotKind::Reply {
                        parent: unnamed.name.clone(),
                    },
                    "Answered",
                ),
            )],
            "reply",
        )
        .unwrap();
    let out = how.write(&model);
    let list = page_annots(&out);
    let (theirs_id, theirs) = list
        .iter()
        .find(|(_, d)| {
            d.get(b"Contents")
                .ok()
                .map(crate::file::pdf_text)
                .as_deref()
                == Some("No name")
        })
        .unwrap();
    assert_eq!(
        crate::file::pdf_text(theirs.get(b"NM").unwrap()),
        unnamed.name
    );
    let (_, reply) = list
        .iter()
        .find(|(_, d)| {
            d.get(b"Contents")
                .ok()
                .map(crate::file::pdf_text)
                .as_deref()
                == Some("Answered")
        })
        .unwrap();
    assert_eq!(
        reply.get(b"IRT").unwrap().as_reference().unwrap(),
        *theirs_id
    );
    // Read back, the reply is in its thread and theirs keeps the name.
    let back = annots_read(&out);
    assert!(
        back.foreign()
            .into_iter()
            .map(|(_, f)| f)
            .any(|f| f.name == unnamed.name)
    );
}

/// A one-page stamp PDF of its own, 100 by 50 points, unlike any
/// other test's: its content says `tag`.
fn stamp_page(tag: &str) -> Vec<u8> {
    let mut out = Pdf::with_version("1.5");
    let pages_id = out.new_object_id();
    let content = out.add_object(lopdf::Stream::new(
        dictionary! {},
        format!("% {tag}\n1 0 0 rg 0 0 100 50 re f").into_bytes(),
    ));
    let page = out.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 50.into()],
    });
    out.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let cat = out.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    out.trailer.set("Root", cat);
    let mut b = Vec::new();
    out.save_to(&mut b).unwrap();
    b
}

fn page_stamp(stem: &str) -> crate::annot::AnnotKind {
    crate::annot::AnnotKind::PageStamp {
        rect: crate::annot::Box2 {
            x: 100.0,
            y: 100.0,
            w: 100.0,
            h: 50.0,
        },
        stamp: stem.into(),
        name: "Paid".into(),
        text: Some("GE 10/14/2026".into()),
    }
}

fn attachment(stem: &str, size: usize) -> crate::annot::AnnotKind {
    crate::annot::AnnotKind::FileAttachment {
        pos: crate::annot::Pt { x: 50.0, y: 50.0 },
        icon: "Paperclip".into(),
        file: stem.into(),
        name: "notes.txt".into(),
        size: size as u64,
    }
}

/// An appearance's one form: its name in the resources, its number,
/// and the form.
fn drawn_form<'a>(pdf: &'a Pdf, annot: &Dictionary) -> (Vec<u8>, ObjectId, &'a lopdf::Stream) {
    let n = annot.get(b"AP").unwrap().as_dict().unwrap();
    let n = n.get(b"N").unwrap().as_reference().unwrap();
    let ap = pdf.get_object(n).unwrap().as_stream().unwrap();
    let resources = ap.dict.get(b"Resources").unwrap().as_dict().unwrap();
    let forms = resources.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(forms.len(), 1);
    let (name, form) = forms.iter().next().unwrap();
    let content = ap
        .decompressed_content()
        .unwrap_or_else(|_| ap.content.clone());
    let content = String::from_utf8_lossy(&content).into_owned();
    let shown = format!("/{} Do", String::from_utf8_lossy(name));
    assert!(
        content.contains(&shown) && content.contains("Tj"),
        "{content}"
    );
    let id = form.as_reference().unwrap();
    (
        name.clone(),
        id,
        pdf.get_object(id).unwrap().as_stream().unwrap(),
    )
}

/// An attachment's embedded file: its stream, and its bytes.
fn embedded(pdf: &Pdf, annot: &Dictionary) -> (ObjectId, Vec<u8>) {
    let fs = annot.get(b"FS").unwrap().as_dict().unwrap();
    let ef = fs.get(b"EF").unwrap().as_dict().unwrap();
    let id = ef.get(b"F").unwrap().as_reference().unwrap();
    let s = pdf.get_object(id).unwrap().as_stream().unwrap();
    (id, s.decompressed_content().unwrap())
}

crate::both_ways!(
    a_page_stamp_is_written_once_under_a_name_of_its_own,
    a_page_stamp_is_written_once_under_a_name_of_its_own_appended: a_page_stamp_is_written_once
);

/// Stamps on two pages draw one page of it, a form named as a
/// resource of the appearance's own, never `/SheafStamp`.
fn a_page_stamp_is_written_once(how: How) {
    let tag = format!("{:x}", fastrand::u64(..));
    let dir = tempdir();
    let a_path = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let mut model = open(&a_path, &[], Box::new(FixedCount(10)));
    // Placed, as the stamps panel places one: held until it's added.
    let held = (model.store())
        .intern(stamp_page(&tag).into(), BlobKind::stamp())
        .unwrap();
    let stem = held.id().stem();
    let (p0, p1) = (model.page(0).unwrap(), model.page(1).unwrap());
    for page in [p0, p1] {
        let annot = thread_mark(page_stamp(&stem), "");
        model
            .commit_ops(vec![fx::add(page, &annot)], "stamp")
            .unwrap();
    }
    let bytes = how.write(&model);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let mut forms = std::collections::HashSet::new();
    for page in pdf.get_pages().into_values() {
        let annots = pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap();
        let id = annots.as_array().unwrap()[0].as_reference().unwrap();
        let (name, form, stream) = drawn_form(&pdf, pdf.get_dictionary(id).unwrap());
        assert!(name.starts_with(b"Sh") && name != b"SheafStamp");
        forms.insert(form);
        let content = String::from_utf8_lossy(&stream.decompressed_content().unwrap()).into_owned();
        assert!(content.contains(&tag), "{content}");
    }
    assert_eq!(forms.len(), 1);
}

/// Two attachments of one file embed it once.
#[test]
fn two_attachments_of_one_file_embed_it_once() {
    let bytes = format!("workpaper notes {:x}", fastrand::u64(..)).into_bytes();
    let dir = tempdir();
    let a_path = make_pdf(&dir, "a.pdf", "A", 1, &[]);
    let mut model = opened(&a_path);
    let held = (model.store())
        .intern(bytes.as_slice().into(), BlobKind::Attachment)
        .unwrap();
    let stem = held.id().stem();
    let p0 = model.page(0).unwrap();
    for _ in 0..2 {
        let annot = thread_mark(attachment(&stem, bytes.len()), "");
        model
            .commit_ops(vec![fx::add(p0, &annot)], "attach")
            .unwrap();
    }
    let out = rewrite(&model).unwrap();
    let pdf = Pdf::load_mem(&out).unwrap();
    let files: Vec<(ObjectId, Vec<u8>)> = page_annots(&out)
        .iter()
        .map(|(_, d)| embedded(&pdf, d))
        .collect();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0], files[1]);
    assert_eq!(files[0].1, bytes);
    // Ours aren't listed with other tools' attachments.
    assert!(crate::attachments::attachments_in(&pdf, pdf.page_iter()).is_empty());
    let streams = pdf
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"EmbeddedFile"))
        .count();
    assert_eq!(streams, 1);
}

crate::both_ways!(
    stamps_and_attachments_are_read_from_the_file,
    stamps_and_attachments_are_read_from_the_file_appended: read_from_the_file
);

/// A stamp placed from the library and a file attached, saved, opened
/// again in a store that holds neither: the marks are still sheaf's, and
/// saved again either way, the stamp's page draws and the file's bytes
/// are the same, from the file each time. Appended to, the file keeps
/// the page and the file where they are.
fn read_from_the_file(how: How) {
    use crate::annot::AnnotKind;
    let tag = format!("{:x}", fastrand::u64(..));
    let pdf = stamp_page(&tag);
    let stamp = crate::store::blob_id(&pdf).stem();
    let bytes = format!("workpaper notes {tag}").into_bytes();
    let dir = tempdir();
    let a_path = make_pdf(&dir, "a.pdf", "A", 1, &[]);
    let mut model = opened(&a_path);
    // Placed, as the stamps panel places one; attached, as the attach
    // tool does: interned, and held until they're added.
    let held = [
        model.store().intern(pdf.into(), BlobKind::stamp()).unwrap(),
        (model.store())
            .intern(bytes.as_slice().into(), BlobKind::Attachment)
            .unwrap(),
    ];
    assert_eq!(held[0].id().stem(), stamp);
    let file = held[1].id().stem();
    let p0 = model.page(0).unwrap();
    for kind in [page_stamp(&stamp), attachment(&file, bytes.len())] {
        let annot = thread_mark(kind, "");
        model.commit_ops(vec![fx::add(p0, &annot)], "add").unwrap();
    }
    drop(held);
    let saved = dir.join("saved.pdf");
    materialize::save_with(
        &mut model,
        &saved,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();

    let check = |path: &Path| -> (Document, [ObjectId; 2]) {
        let mut doc = crate::open::file(path, None).unwrap().doc;
        let p0 = doc.page(0).unwrap();
        let marks: Vec<Mark> = doc.state().mark_records_on(p0).cloned().collect();
        let [placed, attached] = &marks[..] else {
            panic!("two marks")
        };
        assert!(matches!(&placed.annot.kind, AnnotKind::PageStamp { stamp: s, .. } if *s == stamp));
        assert!(
            matches!(&attached.annot.kind, AnnotKind::FileAttachment { file: f, .. } if *f == file)
        );
        assert_eq!(**attached.file.as_ref().unwrap().bytes(), *bytes);
        let page = placed.stamp.as_ref().unwrap().to_pdf().unwrap();
        assert_eq!(Pdf::load_mem(&page).unwrap().get_pages().len(), 1);
        let written = std::fs::read(path).unwrap();
        let pdf = Pdf::load_mem(&written).unwrap();
        let annots = page_annots(&written);
        let (_, form, stream) = drawn_form(&pdf, &annots[0].1);
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        assert!(String::from_utf8_lossy(&content).contains(&tag));
        let (file_id, embedded) = embedded(&pdf, &annots[1].1);
        assert_eq!(embedded, bytes);
        // Their next save writes them again.
        for m in marks {
            let mut a = m.annot.clone();
            a.contents = format!("{} again", a.contents);
            doc.commit_ops(vec![fx::update(&a)], "edit").unwrap();
        }
        (doc, [form, file_id])
    };
    let (doc, held) = check(&saved);
    let again = dir.join("again.pdf");
    std::fs::write(&again, how.write(&doc)).unwrap();
    let (_, now) = check(&again);
    if how == How::Appended {
        // An Append draws and embeds them where the file has them.
        assert_eq!(now, held);
    }
}

/// Pages that come in from a file whose mark names one of the
/// document's stamps by its stem, with another page behind it, bring
/// that page, held by their mark; the document's own mark draws its
/// own.
#[test]
fn a_file_whose_pages_come_in_cant_replace_a_stamp() {
    use crate::model::FormRef;
    let (ours, theirs) = (
        format!("ours-{:x}", fastrand::u64(..)),
        format!("theirs-{:x}", fastrand::u64(..)),
    );
    let ours_pdf = stamp_page(&ours);
    let stem = crate::store::blob_id(&ours_pdf).stem();
    let dir = tempdir();
    // A file whose mark names the document's stamp, drawing another.
    let other = Store::new();
    let theirs_pdf = other.intern(stamp_page(&theirs).into(), BlobKind::stamp());
    let mark = Mark {
        stamp: Some(FormRef::Blob(theirs_pdf.unwrap().into())),
        ..Mark::from(thread_mark(page_stamp(&stem), ""))
    };
    let mut f = crate::forms::fixture::Form::new(1);
    let mut page = f.dict(f.pages[0]).clone();
    crate::forms::fixture::write_marks(&mut f.pdf, &mut page, &[mark]);
    *f.dict(f.pages[0]) = page;
    let b_saved = dir.join("b-saved.pdf");
    std::fs::write(&b_saved, f.bytes()).unwrap();

    let mut model = opened(&make_pdf(&dir, "a.pdf", "A", 1, &[]));
    let held = model.store().intern(ours_pdf.into(), BlobKind::stamp());
    let page = model.page(0).unwrap();
    let annot = thread_mark(page_stamp(&stem), "");
    model
        .commit_ops(vec![fx::add(page, &annot)], "stamp")
        .unwrap();
    drop(held);
    let incoming = info(model.store(), &b_saved);
    assert!(incoming.annots().stamp(&stem).is_some());
    let insert = fx::insert(&incoming, [0], "a.pdf", None);
    model.commit_ops(vec![insert], "ins").unwrap();

    let out = rewrite(&model).unwrap();
    let pdf = Pdf::load_mem(&out).unwrap();
    let drawn = |page: usize| {
        let (_, _, stream) = drawn_form(&pdf, &page_annots_on(&out, page)[0].1);
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        String::from_utf8_lossy(&content).into_owned()
    };
    let (mine, came) = (drawn(0), drawn(1));
    assert!(mine.contains(&ours) && !mine.contains(&theirs), "{mine}");
    assert!(came.contains(&theirs) && !came.contains(&ours), "{came}");
}

crate::both_ways!(
    a_text_replacement_is_a_caret_with_a_grouped_strikeout,
    a_text_replacement_is_a_caret_with_a_grouped_strikeout_appended: caret_and_strikeout
);

fn caret_and_strikeout(how: How) {
    use crate::annot::{AnnotKind, Box2};
    let dir = tempdir();
    let a_path = make_pdf(&dir, "a.pdf", "A", 1, &[]);
    let mut model = open(&a_path, &[], Box::new(FixedCount(10)));
    let p0 = model.page(0).unwrap();
    let words = Box2 {
        x: 72.0,
        y: 80.0,
        w: 120.0,
        h: 14.0,
    };
    let mk = |kind: AnnotKind, contents: &str| Annot {
        id: crate::annot::AnnotId(0),
        kind,
        color: [0, 0, 255],
        opacity: 1.0,
        stroke: 1.0,
        author: "greg".into(),
        contents: contents.into(),
        modified: "D:20261014000000".into(),
        created: "D:20261014000000".into(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    let replace = mk(
        AnnotKind::Replace {
            rects: vec![words],
            struck: "old words".into(),
            at: crate::annot::caret_box(words.right(), words.bottom(), words.h),
        },
        "new words",
    );
    let underline = mk(
        AnnotKind::Underline {
            rects: vec![Box2 { y: 120.0, ..words }],
        },
        "",
    );
    for (a, label) in [(replace, "replace"), (underline, "underline")] {
        model.commit_ops(vec![fx::add(p0, &a)], label).unwrap();
    }
    let want: Vec<Annot> = model.state().marks_on(p0).cloned().collect();

    let annots_of = |bytes: &[u8]| {
        let pdf = Pdf::load_mem(bytes).unwrap();
        let page0 = pdf.get_pages()[&1];
        let list: Vec<lopdf::Dictionary> = pdf
            .get_dictionary(page0)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| pdf.dereference(o).unwrap().1.as_dict().unwrap().clone())
            .collect();
        let ids: Vec<lopdf::ObjectId> = pdf
            .get_dictionary(page0)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_reference().unwrap())
            .collect();
        (list, ids)
    };
    let bytes = how.write(&model);
    let (list, ids) = annots_of(&bytes);
    // Caret, its StrikeOut, the underline.
    let subtypes: Vec<&[u8]> = list
        .iter()
        .map(|d| d.get(b"Subtype").unwrap().as_name().unwrap())
        .collect();
    assert_eq!(
        subtypes,
        [
            b"Caret".as_slice(),
            b"StrikeOut".as_slice(),
            b"Underline".as_slice()
        ]
    );
    let (caret, strike) = (&list[0], &list[1]);
    assert_eq!(
        caret.get(b"Contents").unwrap().as_str().unwrap(),
        b"new words"
    );
    assert_eq!(strike.get(b"IRT").unwrap().as_reference().unwrap(), ids[0]);
    assert_eq!(strike.get(b"RT").unwrap().as_name().unwrap(), b"Group");
    // A companion is known by its group and its Caret: it's marked no
    // other way.
    assert!(!strike.has(crate::annot::PART.as_bytes()));
    assert!(!strike.has(b"SheafAnnot"));
    assert!(strike.has(b"QuadPoints"));
    // The names the renderer hides are exactly what was written.
    let names: Vec<String> = list
        .iter()
        .map(|d| crate::file::pdf_text(d.get(b"NM").unwrap()))
        .collect();
    let mut expected: Vec<String> = want.iter().flat_map(|a| a.pdf_names()).collect();
    expected.sort();
    let mut got = names.clone();
    got.sort();
    assert_eq!(got, expected);

    // Read back: two marks, nothing foreign.
    let out = dir.join("out.pdf");
    std::fs::write(&out, &bytes).unwrap();
    let back = annots_read(&std::fs::read(&out).unwrap());
    assert_eq!(back.sheaf()[&0], want);
    assert!(back.foreign().is_empty(), "{:?}", back.foreign());

    // Saved again from the reopened file: the companion isn't doubled.
    let mut model2 = opened(&out);
    let p = model2.page(0).unwrap();
    let mut changed = model2.state().marks_on(p).next().unwrap().clone();
    changed.contents = "newer words".into();
    model2
        .commit_ops(vec![fx::update(&changed)], "edit")
        .unwrap();
    let bytes2 = how.write(&model2);
    let (list2, _) = annots_of(&bytes2);
    assert_eq!(list2.len(), 3);
    assert_eq!(
        list2[0].get(b"Contents").unwrap().as_str().unwrap(),
        b"newer words"
    );
}

crate::both_ways!(
    annots_written_and_read_back,
    annots_written_and_read_back_appended: annots_read_back
);

fn annots_read_back(how: How) {
    use crate::annot::{AnnotKind, Box2, Pt};
    let dir = tempdir();
    let a_path = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let original = std::fs::read(&a_path).unwrap();
    let mut model = open(&a_path, &[], Box::new(FixedCount(10)));
    let p0 = model.page(0).unwrap();
    let mk = |kind: AnnotKind| Annot {
        id: crate::annot::AnnotId(0),
        kind,
        color: [255, 200, 0],
        opacity: 0.5,
        stroke: 1.5,
        author: "greg".into(),
        contents: "note".into(),
        modified: "D:20261011000000".into(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    model
        .commit_ops(
            vec![fx::add(
                p0,
                &mk(AnnotKind::Highlight {
                    rects: vec![Box2 {
                        x: 72.0,
                        y: 80.0,
                        w: 200.0,
                        h: 14.0,
                    }],
                }),
            )],
            "hl",
        )
        .unwrap();
    model
        .commit_ops(
            vec![fx::add(
                p0,
                &mk(AnnotKind::Note {
                    pos: Pt { x: 300.0, y: 80.0 },
                }),
            )],
            "note",
        )
        .unwrap();
    model
        .commit_ops(
            vec![fx::add(
                p0,
                &mk(AnnotKind::Stamp {
                    rect: Box2 {
                        x: 400.0,
                        y: 80.0,
                        w: 24.0,
                        h: 24.0,
                    },
                    glyph: "✓".into(),
                }),
            )],
            "tick",
        )
        .unwrap();

    let bytes = how.write(&model);
    assert_eq!(bytes.starts_with(&original), how == How::Appended);
    let out = dir.join("inc.pdf");
    std::fs::write(&out, &bytes).unwrap();
    let back = annots_read(&std::fs::read(&out).unwrap()).sheaf();
    assert_eq!(back.len(), 1);
    let want: Vec<Annot> = model.state().marks_on(p0).cloned().collect();
    assert_eq!(back[&0], want);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let page0 = pdf.get_pages()[&1];
    let annots = pdf
        .get_dictionary(page0)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(annots.len(), 3);

    // Reopen, delete one, save again: no duplicates.
    let out2 = dir.join("inc2.pdf");
    std::fs::copy(&out, &out2).unwrap();
    let mut model2 = opened(&out2);
    let p0b = model2.page(0).unwrap();
    let first = model2.state().marks_on(p0b).next().unwrap().clone();
    model2
        .commit_ops(vec![fx::delete_annots(&[first.id])], "rm")
        .unwrap();
    let bytes2 = how.write(&model2);
    let pdf = Pdf::load_mem(&bytes2).unwrap();
    let page0 = pdf.get_pages()[&1];
    let annots = pdf
        .get_dictionary(page0)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(annots.len(), 2);

    let rw = rewrite(&model).unwrap();
    let out3 = dir.join("rw.pdf");
    std::fs::write(&out3, &rw).unwrap();
    assert_eq!(
        annots_read(&std::fs::read(&out3).unwrap()).sheaf()[&0],
        want
    );
}

crate::both_ways!(
    connector_groups_link_in_a_cycle,
    connector_groups_link_in_a_cycle_appended: connectors_link_in_a_cycle
);

/// A connector group's /Dest walk it in page order and wrap; a new
/// member, saved later, is fitted into the cycle on every page.
fn connectors_link_in_a_cycle(how: How) {
    use crate::annot::{AnnotKind, Box2};
    let dir = tempdir();
    let a_path = make_pdf(&dir, "g.pdf", "G", 3, &[]);
    let mut model = opened(&a_path);
    let page = |i: usize| model.page(i).unwrap();
    let (p0, p1, p2) = (page(0), page(1), page(2));
    let connector = |y: f32| Annot {
        id: crate::annot::AnnotId(0),
        kind: AnnotKind::Connector {
            rect: Box2 {
                x: 100.0,
                y,
                w: 22.0,
                h: 22.0,
            },
            label: "C".into(),
            partner: None,
        },
        color: [220, 40, 40],
        opacity: 1.0,
        stroke: 1.0,
        author: "greg".into(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    for pg in [p0, p1, p2] {
        model
            .commit_ops(vec![fx::add(pg, &connector(100.0))], "c")
            .unwrap();
    }
    // Each page's links, top to bottom, as the page number each goes to.
    let dests = |bytes: &[u8]| -> Vec<Vec<u32>> {
        let pdf = Pdf::load_mem(bytes).unwrap();
        let pages = pdf.get_pages();
        let number: HashMap<ObjectId, u32> = pages.iter().map(|(n, id)| (*id, *n)).collect();
        pages
            .values()
            .map(|id| {
                let mut links: Vec<(f32, u32)> = pdf
                    .get_dictionary(*id)
                    .unwrap()
                    .get(b"Annots")
                    .map(|a| a.as_array().unwrap().clone())
                    .unwrap_or_default()
                    .iter()
                    .map(|o| {
                        let d = pdf.get_dictionary(o.as_reference().unwrap()).unwrap();
                        let top = d.get(b"Rect").unwrap().as_array().unwrap()[3]
                            .as_float()
                            .unwrap() as f32;
                        let to = d.get(b"Dest").unwrap().as_array().unwrap()[0]
                            .as_reference()
                            .unwrap();
                        (-top, number[&to])
                    })
                    .collect();
                links.sort_by(|a, b| a.0.total_cmp(&b.0));
                links.into_iter().map(|(_, n)| n).collect()
            })
            .collect()
    };
    let bytes = how.write(&model);
    assert_eq!(dests(&bytes), [vec![2], vec![3], vec![1]]);

    // Saved, reopened, and a fourth added below the first: page 1's
    // first now goes to its neighbour, and page 3 comes back to it.
    let out = dir.join("g2.pdf");
    std::fs::write(&out, &bytes).unwrap();
    let mut model = opened(&out);
    let p0 = model.page(0).unwrap();
    model
        .commit_ops(vec![fx::add(p0, &connector(400.0))], "c")
        .unwrap();
    let bytes = how.write(&model);
    assert_eq!(dests(&bytes), [vec![1, 2], vec![3], vec![1]]);
}

crate::both_ways!(
    connectors_link_to_partner_page,
    connectors_link_to_partner_page_appended: connectors_link_to_partner
);

/// Connectors come out as Link annotations whose /Dest is the partner's
/// page, in both save strategies.
fn connectors_link_to_partner(how: How) {
    use crate::annot::{AnnotKind, Box2};
    let dir = tempdir();
    let a_path = make_pdf(&dir, "c.pdf", "C", 3, &[]);
    let mut model = open(&a_path, &[], Box::new(FixedCount(10)));
    let (p0, p2) = (model.page(0).unwrap(), model.page(2).unwrap());
    let mk = || Annot {
        id: crate::annot::AnnotId(0),
        kind: AnnotKind::Connector {
            rect: Box2 {
                x: 100.0,
                y: 100.0,
                w: 22.0,
                h: 22.0,
            },
            label: "1".into(),
            partner: None,
        },
        color: [220, 40, 40],
        opacity: 1.0,
        stroke: 1.0,
        author: "greg".into(),
        contents: String::new(),
        modified: "D:20261011000000".into(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    model
        .commit_ops(vec![fx::connect((p0, &mk()), (p2, &mk()))], "c")
        .unwrap();

    let check = |bytes: &[u8]| {
        let pdf = Pdf::load_mem(bytes).unwrap();
        let pages = pdf.get_pages();
        let link_on = |n: u32| -> Dictionary {
            let page = pdf.get_dictionary(pages[&n]).unwrap();
            let annots = pdf
                .dereference(page.get(b"Annots").unwrap())
                .unwrap()
                .1
                .as_array()
                .unwrap()
                .clone();
            assert_eq!(annots.len(), 1);
            pdf.get_dictionary(annots[0].as_reference().unwrap())
                .unwrap()
                .clone()
        };
        let a = link_on(1);
        let b = link_on(3);
        assert_eq!(a.get(b"Subtype").unwrap().as_name().unwrap(), b"Link");
        let dest_page = |d: &Dictionary| {
            d.get(b"Dest").unwrap().as_array().unwrap()[0]
                .as_reference()
                .unwrap()
        };
        assert_eq!(dest_page(&a), pages[&3]);
        assert_eq!(dest_page(&b), pages[&1]);
        // Round trip keeps the pairing.
        let back = annots_read(bytes).sheaf();
        assert_eq!(back[&0][0].partner(), Some(back[&2][0].id));
    };
    check(&how.write(&model));
}

crate::both_ways!(
    encrypted_source_saves_encrypted,
    encrypted_source_saves_encrypted_appended: encrypted_source_saves
);

/// An encrypted original opens with its password and saves encrypted
/// as it was, with the edits applied: the same password opens it, and
/// its `/P` and `/ID[0]` are the original's. Saved as another file, it's
/// a Rewrite; in place, an Append keeps its bytes.
fn encrypted_source_saves(how: How) {
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};
    let dir = tempdir();
    let plain = make_pdf(&dir, "plain.pdf", "E", 2, &[("Top", 0, 0)]);
    let enc = dir.join("enc.pdf");
    {
        let mut pdf = Pdf::load(&plain).unwrap();
        // The standard security handler keys off the file /ID.
        let id = Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal);
        pdf.trailer.set("ID", Object::Array(vec![id.clone(), id]));
        let state = EncryptionState::try_from(EncryptionVersion::V2 {
            document: &pdf,
            owner_password: "owner",
            user_password: "secret",
            key_length: 128,
            permissions: Permissions::default(),
        })
        .unwrap();
        pdf.encrypt(&state).unwrap();
        pdf.save(&enc).unwrap();
    }
    assert!(Pdf::load(&enc).unwrap().is_encrypted());
    assert!(crate::file::load_pdf(&enc, Some("wrong")).is_err());
    assert!(
        !crate::file::load_pdf(&enc, Some("secret"))
            .unwrap()
            .is_encrypted()
    );

    let store = Store::new();
    let (primary, stamp) = store.read_file(&enc, Some("secret")).unwrap();
    let came = Came::Read {
        path: enc.as_path().into(),
        stamp,
    };
    let mut model = Document::new(
        store,
        primary,
        crate::store::file_name(&enc),
        Some(came),
        &[FlatOutlineItem {
            title: "Top".into(),
            page: Some(0),
            depth: 0,
            view: None,
            color: None,
            style: Default::default(),
            open: true,
            object: None,
            kept: false,
        }],
        Box::new(FixedCount(10)),
    );
    let p1 = model.page(1).unwrap();
    model
        .commit_ops(vec![fx::rotate(&[p1], Turn::Quarter)], "rot")
        .unwrap();
    let lazy = |path: &Path, password| {
        let bytes = sheaf_util::read_file(path).unwrap();
        lopdf::LazyPdf::open(bytes, password, &crate::source::Limits::DEFAULT.reader)
    };
    let crypt = |path: &Path| {
        let pdf = lazy(path, Some("secret")).unwrap();
        Crypt::of(&pdf).unwrap().unwrap()
    };
    let was = crypt(&enc);
    if how == How::Appended {
        // Saved in place, it's appended to, and encrypted the same way.
        let original = std::fs::read(&enc).unwrap();
        let report = How::Appended.save(&mut model, &enc);
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let bytes = std::fs::read(&enc).unwrap();
        assert!(bytes.starts_with(&original));
        assert!(matches!(
            lazy(&enc, None),
            Err(lopdf::Error::InvalidPassword)
        ));
        assert_eq!(crypt(&enc).dict(), was.dict());
        let saved = crate::file::load_pdf(&enc, Some("secret")).unwrap();
        let pages = saved.get_pages();
        let turned = saved.get_dictionary(pages[&2]).unwrap().get(b"Rotate");
        assert_eq!(turned.unwrap().as_i64().unwrap(), 90);
        return;
    }
    let out = dir.join("saved.pdf");
    let report = How::Rewritten.save(&mut model, &out);
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !bytes.windows(6).any(|w| w == b"MARK E"),
        "content in the clear"
    );
    assert!(matches!(
        lazy(&out, None),
        Err(lopdf::Error::InvalidPassword)
    ));
    let now = crypt(&out);
    assert_eq!(now.dict().get(b"P").ok(), was.dict().get(b"P").ok());
    assert_eq!(now.first_id().unwrap(), was.first_id().unwrap());
    let saved = crate::file::load_pdf(&out, Some("secret")).unwrap();
    let pages = saved.get_pages();
    assert_eq!(pages.len(), 2);
    assert_eq!(
        saved
            .get_dictionary(pages[&2])
            .unwrap()
            .get(b"Rotate")
            .unwrap()
            .as_i64()
            .unwrap(),
        90
    );
    // The content reads back with the password.
    let content = saved.get_page_content(pages[&1]);
    assert!(
        String::from_utf8_lossy(&content).contains("MARK E 0"),
        "content stream not decrypted"
    );
    // Outline came through too.
    let items = outline_in(&saved);
    assert_eq!(items[0].1, "Top");
}

/// A header/footer is drawn into page content as its generator's blocks
/// on save, pagination artifacts of sheaf's own, what it is said in the
/// catalog's `/SheafGenerators` and nothing in `/SheafHeaderFooter`; an
/// open finds it again (taking it off is `header_tests`' and
/// `others_tests`').
#[test]
fn header_footer_is_drawn_and_found_again() {
    use crate::content::{PagOrigin, PaginationKind};
    let dir = tempdir();
    let a = make_pdf(&dir, "hf.pdf", "H", 2, &[]);
    let mut model = open(&a, &[], crate::history::default_policy());
    let mut footer = fx::header("");
    footer.texts[4] = "Page <<1>> of <<N>>".into();
    model
        .commit_ops(vec![fx::set_header(footer.clone(), None)], "hf")
        .unwrap();
    let out = dir.join("hf-out.pdf");
    rewrite_to_path(&model, &out).unwrap();
    let pdf = Pdf::load(&out).unwrap();
    let src = crate::forms::fixture::source(std::fs::read(&out).unwrap());
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    for (i, &p) in pages.iter().enumerate() {
        let dict = pdf.get_dictionary(p).unwrap();
        // A footer of sheaf's.
        let blocks = src.blocks(i as u32).unwrap();
        let found: Vec<_> = blocks
            .pagination
            .iter()
            .map(|p| (p.kind, p.origin))
            .collect();
        assert_eq!(
            found,
            [(PaginationKind::Footer, PagOrigin::Sheaf)],
            "page {i}"
        );
        let all: Vec<u8> = dict
            .get(b"Contents")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|o| {
                let s = pdf
                    .get_object(o.as_reference().unwrap())
                    .unwrap()
                    .as_stream()
                    .unwrap();
                s.decompressed_content()
                    .unwrap_or_else(|_| s.content.clone())
            })
            .collect();
        let want = format!("(Page {} of 2)", i + 1);
        assert!(String::from_utf8_lossy(&all).contains(&want), "page {i}");
    }
    assert!(!pdf.catalog().unwrap().has(b"SheafHeaderFooter"));
    // Read from a source, as it opens: the same.
    let [saved] = &src.generators()[..] else {
        panic!("one generator")
    };
    let page = |i: u32| Some(crate::model::PageId(u64::from(i)));
    let g = saved.generator(&src, page).unwrap();
    assert_eq!(g.spec.header_footer().unwrap().texts, footer.texts);
    assert_eq!(g.scope, crate::model::Scope::All);
}

crate::both_ways!(
    page_labels_are_written_and_follow_pages,
    page_labels_are_written_and_follow_pages_appended: page_labels_follow_pages
);

/// Labels set in the model follow their pages through a reorder, saved
/// in place and written as another file, and removing them removes them.
fn page_labels_follow_pages(how: How) {
    use crate::labels::{LabelSpec, Style, label_ranges, labels_for};
    let dir = tempdir();
    let a = make_pdf(&dir, "labels.pdf", "L", 4, &[]);
    let mut model = open(&a, &[], crate::history::default_policy());
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    let spec = |id, style, first| LabelSpec {
        start: id,
        style,
        prefix: String::new(),
        first,
    };
    model
        .commit_ops(
            vec![fx::labels(&[
                spec(ids[0], Style::LowerRoman, 1),
                spec(ids[2], Style::Decimal, 1),
            ])],
            "labels",
        )
        .unwrap();
    let read = |p: &Path| {
        let pdf = Pdf::load(p).unwrap();
        label_ranges(&pdf).map(|r| labels_for(&r, 4))
    };
    // In place.
    assert_eq!(how.save(&mut model, &a).strategy, Some(how.strategy()));
    assert_eq!(read(&a).unwrap(), vec!["i", "ii", "1", "2"]);
    // Move the decimal range's first page to the front; rewrite.
    model
        .commit_ops(vec![fx::move_pages(&[ids[2]], Some(ids[0]))], "move")
        .unwrap();
    let out = dir.join("moved.pdf");
    rewrite_to_path(&model, &out).unwrap();
    assert_eq!(read(&out).unwrap(), vec!["1", "i", "ii", "iii"]);
    // Removed.
    model
        .commit_ops(vec![Op::SetPageLabels { ranges: None }], "none")
        .unwrap();
    let out = dir.join("none.pdf");
    rewrite_to_path(&model, &out).unwrap();
    assert_eq!(read(&out), None);
}

pub(crate) fn tempdir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "sheaf-save-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(crate) fn rand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    t ^ N.fetch_add(1, Ordering::Relaxed)
}
