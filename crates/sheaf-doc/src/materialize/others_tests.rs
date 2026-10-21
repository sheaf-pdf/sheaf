//! Others' watermarks and backgrounds (M10.5 slice 16, proposal 31): a
//! watermark of the document's replaces those of its kind Acrobat, an
//! older sheaf or another file drew on the pages it covers, never the
//! author's own; RemoveWatermark takes them off, and the author's only when
//! asked to, saying what it found. What's taken out is spliced from the
//! streams it's in, each a copy of its own; the names only it used leave
//! the page's resources, and what they named the file. One a splice
//! refuses stays, counted. What an older sheaf kept in the watermarks it
//! drew, its settings with an image's path, Clean Copy takes out of them,
//! what they draw kept; a list it can't be taken out of keeps it, counted.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::{Op, Z};

use super::recognized_tests::{contents, depth, drawn, everything, pages, stream};
use super::removal_tests::{opened, scratch};
use super::{How, Onto, Profile, SaveInput, SaveTarget, StrategyKind, Why};
use crate::content::{Balance, PagOrigin, PaginationKind};
use crate::effects::{Flags, Removed, RemovedKind};
use crate::generate::fixture as older;
use crate::model::{Document, Filter, LegacySel, MarkedSel, PageDef, PageId};
use crate::prim::{Key, Val};
use crate::tx::fixture as fx;

/// The page's own text.
pub(crate) const BODY: &[u8] = b"BT /F1 12 Tf 72 700 Td (body) Tj ET";
/// Acrobat's watermark, after the page's content: its form drawn in its
/// graphics state.
pub(crate) const ACROBAT: &[u8] =
    b"/Artifact <</Type /Pagination /Subtype /Watermark>> BDC q /GS0 gs 1 0 0 1 150 350 cm \
      /Fm0 Do Q EMC";
/// Acrobat's watermark as Acrobat lays it out: a `q` before the page's
/// content, which the watermark's `Q` ends.
const ACROBAT_Q: [&[u8]; 3] = [
    b"q\n",
    b"BT /F1 12 Tf 72 700 Td (body) Tj ET 0.5 0 0 0.5 0 0 cm",
    b"/Artifact <</Type /Pagination /Subtype /Watermark>> BDC Q q /GS0 gs 1 0 0 1 150 350 cm \
      /Fm0 Do Q EMC",
];
/// Acrobat's watermark leaving a color set after it: no splice takes it
/// out without changing what's drawn after it.
const ACROBAT_UNSAFE: &[u8] =
    b"/Artifact <</Type /Pagination /Subtype /Watermark>> BDC q 1 0 0 1 150 350 cm /Fm0 Do Q \
      0 0 1 rg EMC";

/// A file of a page for each of `streams`, each its `/Contents`' streams,
/// under a node whose resources are Helvetica as `/F1`, a form Acrobat
/// drew a watermark in as `/Fm0` (its `/PieceInfo` says so) and the
/// graphics state Acrobat draws it in, `/GS0`. Pages with the same streams
/// share them.
pub(crate) fn acrobat_file(streams: &[&[&[u8]]]) -> Vec<u8> {
    file_of(streams, |pdf, font| {
        let piece = dictionary! { "ADBE_CompoundType" => dictionary! {
            "DocSettings" => dictionary! {}, "Private" => "Watermark",
        } };
        let form = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "PieceInfo" => piece,
                "BBox" => vec![0.into(), 0.into(), 300.into(), 60.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            },
            b"BT /F1 40 Tf 2 10 Td (ACROBAT) Tj ET".to_vec(),
        ));
        let gs = pdf.add_object(dictionary! { "Type" => "ExtGState", "ca" => 0.5, "CA" => 0.5 });
        dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Fm0" => form },
            "ExtGState" => dictionary! { "GS0" => gs },
        }
    })
}

/// A file of a page for each of `streams`, each its `/Contents`' streams,
/// under a node whose resources `resources` makes, given Helvetica. Pages
/// with the same streams share them.
fn file_of(
    streams: &[&[&[u8]]],
    resources: impl FnOnce(&mut Pdf, ObjectId) -> Dictionary,
) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources = resources(&mut pdf, font);
    let mut made: Vec<(&[&[u8]], Object)> = Vec::new();
    let mut kids = Vec::new();
    for &page in streams {
        let entries = match made.iter().find(|(s, _)| *s == page) {
            Some((_, entries)) => entries.clone(),
            None => {
                let entries: Vec<Object> = (page.iter())
                    .map(|s| {
                        pdf.add_object(Stream::new(dictionary! {}, s.to_vec()))
                            .into()
                    })
                    .collect();
                let entries = Object::Array(entries);
                made.push((page, entries.clone()));
                entries
            }
        };
        let page = dictionary! { "Type" => "Page", "Parent" => root, "Contents" => entries };
        kids.push(Object::Reference(pdf.add_object(page)));
    }
    let resources = pdf.add_object(resources);
    let count = kids.len() as i64;
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// A watermark of `text` over every page.
fn watermark(text: &str) -> Op {
    fx::set_watermark(fx::watermark(text), Z::Over, None)
}

/// The resources page `page` of `pdf` draws with, its own or a node's,
/// and the names each of its categories binds.
fn names(pdf: &Pdf, page: ObjectId) -> Vec<(String, Vec<String>)> {
    let mut at = pdf.get_dictionary(page).unwrap();
    let resources = loop {
        match at.get(b"Resources") {
            Ok(r) => break pdf.dereference(r).unwrap().1.as_dict().unwrap().clone(),
            Err(_) => {
                at = pdf
                    .get_dictionary(at.get(b"Parent").unwrap().as_reference().unwrap())
                    .unwrap()
            }
        }
    };
    let name = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (resources.iter())
        .filter_map(|(kind, category)| {
            let category: &Dictionary = pdf.dereference(category).ok()?.1.as_dict().ok()?;
            Some((name(kind), category.iter().map(|(n, _)| name(n)).collect()))
        })
        .collect()
}

/// The names page `page` of `pdf` binds in category `kind`.
fn bound(pdf: &Pdf, page: ObjectId, kind: &str) -> Vec<String> {
    (names(pdf, page).into_iter())
        .find(|(k, _)| k == kind)
        .map_or(Vec::new(), |(_, names)| names)
}

/// Whether `pdf` holds a form Acrobat drew a pagination artifact in.
fn holds_acrobat_s(pdf: &Pdf) -> bool {
    (pdf.objects.values()).any(|o| matches!(o, Object::Stream(s) if s.dict.has(b"PieceInfo")))
}

/// Acrobat's watermark on a page, under one of the document's: replaced,
/// as Acrobat lays it out too, the `Q` it opens with kept. The form it drew
/// and its graphics state leave the page's resources, which keep the font
/// the page's own text draws in, and a Rewrite holds neither the form nor
/// its text. The save is planned a Rewrite for what's taken off (§7.1).
/// Both ways.
fn acrobat_s_replaced(how: How) {
    let dir = scratch();
    for (layout, streams) in [("after", &[BODY, ACROBAT][..]), ("acrobat", &ACROBAT_Q[..])] {
        let (path, mut doc) = opened(&dir, "a.pdf", &acrobat_file(&[streams]));
        let c = doc
            .commit_ops(vec![watermark("MINE")], "watermark")
            .unwrap();
        assert!(
            !c.effects.effects.flags.contains(Flags::REMOVES),
            "a step reads no blocks"
        );
        let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
        let page = pages(&pdf)[0];
        let text = drawn(&pdf, page);
        assert!(
            !text.contains("/Fm0") && text.matches("(MINE)").count() == 1,
            "{how:?}, {layout}: {text}"
        );
        assert!(text.contains("(body)"), "{how:?}, {layout}: {text}");
        assert_eq!(depth(&pdf, page).1, Balance::default(), "{how:?}, {layout}");
        assert_eq!(bound(&pdf, page, "XObject"), Vec::<String>::new());
        assert!(!bound(&pdf, page, "ExtGState").contains(&"GS0".to_string()));
        assert_eq!(
            bound(&pdf, page, "Font")
                .iter()
                .filter(|n| *n == "F1")
                .count(),
            1
        );
        if how == How::Rewritten {
            assert!(!holds_acrobat_s(&pdf), "{layout}: the form is gone");
            assert!(!everything(&pdf).contains("ACROBAT"), "{layout}");
        }
        let report = super::save(
            &mut doc,
            &path,
            &Default::default(),
            &in_progress::NoProgress,
        )
        .unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Rewrite), "{layout}");
        let filtered = Removed {
            kind: RemovedKind::Filtered,
            count: 1,
        };
        assert!(
            report.why.contains(&Why::Removes(vec![filtered])),
            "{:?}",
            report.why
        );
    }
}

crate::both_ways!(
    acrobat_s_watermark_is_replaced,
    acrobat_s_watermark_is_replaced_appended: acrobat_s_replaced
);

/// A watermark on a page with none of others' to replace is drawn, and
/// the save appends; under a background, Acrobat's watermark stays, of a
/// kind it doesn't replace.
#[test]
fn only_its_own_kind_is_replaced() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &acrobat_file(&[&[BODY]]));
    doc.commit_ops(vec![watermark("MINE")], "watermark")
        .unwrap();
    let report = super::save(
        &mut doc,
        &path,
        &Default::default(),
        &in_progress::NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let (_, mut doc) = opened(&dir, "b.pdf", &acrobat_file(&[&[BODY, ACROBAT]]));
    let background = fx::set_watermark(fx::watermark("MINE"), Z::Under, None);
    doc.commit_ops(vec![background], "background").unwrap();
    let pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let page = pages(&pdf)[0];
    assert!(drawn(&pdf, page).contains("/Fm0 Do"));
    assert_eq!(bound(&pdf, page, "XObject"), ["Fm0"]);
}

/// Acrobat's watermark taken off with no watermark of the document's, a
/// page at a time: a filter on each page, the line of the two saying what
/// they found together; written, the pages draw no form and their
/// resources name none, so a Rewrite holds neither the form nor its text.
#[test]
fn acrobat_s_watermark_is_removed() {
    let dir = scratch();
    let other: &[u8] = b"BT /F1 12 Tf 72 700 Td (other) Tj ET";
    let (_, mut doc) = opened(
        &dir,
        "a.pdf",
        &acrobat_file(&[&[BODY, ACROBAT], &[other, ACROBAT]]),
    );
    let all: Vec<PageId> = doc.state().order().iter().copied().collect();
    let each = all
        .iter()
        .map(|&p| fx::remove_watermark(None, &[p]))
        .collect();
    let c = doc.commit_ops(each, "remove").unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark and background from 2 pages (2 made in Acrobat)"
    );
    let filter = Filter::DropMarked(MarkedSel::Pagination {
        kinds: BTreeSet::from([PaginationKind::Watermark, PaginationKind::Background]),
        origins: BTreeSet::from([PagOrigin::Acrobat]),
    });
    for &page in &all {
        let def = doc.state().def(page).unwrap();
        assert_eq!(def.content.filters, im::Vector::unit(filter.clone()));
    }
    let pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    for page in pages(&pdf) {
        assert!(!drawn(&pdf, page).contains("/Fm0"));
        assert_eq!(bound(&pdf, page, "XObject"), Vec::<String>::new());
    }
    assert!(!holds_acrobat_s(&pdf));
    assert!(!everything(&pdf).contains("ACROBAT"));
}

/// The author's own running heads, footers and watermark, as Word marks
/// them: a watermark of the document's leaves them all. RemoveWatermark
/// leaves the author's watermark and says so; asked to take the author's,
/// it takes the one a splice can, and says the other stays.
#[test]
fn the_author_s_are_left_unless_asked() {
    let dir = scratch();
    let first: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Header>> BDC BT /F1 10 Tf \
        72 760 Td (RUNNING HEAD) Tj ET EMC BT /F1 12 Tf 72 700 Td (body) Tj ET \
        /Artifact <</Type /Pagination /Subtype /Footer>> BDC BT /F1 10 Tf 72 30 Td \
        (RUNNING FOOT) Tj ET EMC /Artifact <</Type /Pagination /Subtype /Watermark>> BDC q \
        BT /F1 40 Tf 100 300 Td (AUTHOR MARK) Tj ET Q EMC";
    // Its font set outside a `q`, which would last past it.
    let second: &[u8] = b"BT /F1 12 Tf 72 700 Td (body) Tj ET \
        /Artifact <</Type /Pagination /Subtype /Watermark>> BDC BT /F1 40 Tf 100 300 Td \
        (UNSAFE MARK) Tj ET EMC";
    let file = acrobat_file(&[&[first], &[second]]);
    let (_, mut doc) = opened(&dir, "a.pdf", &file);
    doc.commit_ops(vec![watermark("MINE")], "watermark")
        .unwrap();
    let pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let text = drawn(&pdf, pages(&pdf)[0]) + &drawn(&pdf, pages(&pdf)[1]);
    for kept in ["RUNNING HEAD", "RUNNING FOOT", "AUTHOR MARK", "UNSAFE MARK"] {
        assert!(text.contains(kept), "{kept}: {text}");
    }
    let all: Vec<PageId> = doc.state().order().iter().copied().collect();
    let c = (doc.commit_ops(vec![fx::remove_watermark(Some(Z::Over), &all)], "remove")).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark from pages 1, 2 (2 the author's left in place)"
    );
    assert!(doc.state().watermark(Z::Over).is_none());
    assert!(
        all.iter()
            .all(|&p| doc.state().def(p).unwrap().content.filters.is_empty())
    );
    let asked = Op::RemoveWatermark {
        z: Some(Z::Over),
        pages: fx::ids(&all),
        include_authored: true,
    };
    let c = doc.commit_ops(vec![asked], "remove theirs too").unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark from pages 1, 2 (1 the author's; 1 the author's left in place)"
    );
    assert!(doc.state().def(all[1]).unwrap().content.filters.is_empty());
    for how in [How::Rewritten, How::Appended] {
        let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
        let (first, second) = (drawn(&pdf, pages(&pdf)[0]), drawn(&pdf, pages(&pdf)[1]));
        assert!(!first.contains("AUTHOR MARK"), "{how:?}: {first}");
        assert!(first.contains("RUNNING HEAD") && first.contains("RUNNING FOOT"));
        assert!(second.contains("UNSAFE MARK"), "{how:?}: {second}");
    }
}

/// A watermark an older sheaf drew, its list marked `/SheafStamp`, is
/// replaced by the document's, drawn once.
#[test]
fn an_older_sheaf_s_watermark_is_replaced() {
    let dir = scratch();
    let legacy: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Watermark /SheafStamp true \
        /SheafSettings (settings) >> BDC q /GS0 gs 0.7 0.7 -0.7 0.7 200 300 cm BT 0.5 0.5 0.5 rg \
        /F1 60 Tf 0 0 Td (OLD SHEAF) Tj ET\nQ\nEMC\n";
    let (_, mut doc) = opened(&dir, "a.pdf", &acrobat_file(&[&[BODY, legacy]]));
    doc.commit_ops(vec![watermark("MINE")], "watermark")
        .unwrap();
    for how in [How::Rewritten, How::Appended] {
        let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
        let text = drawn(&pdf, pages(&pdf)[0]);
        assert!(
            !text.contains("OLD SHEAF") && text.matches("(MINE)").count() == 1,
            "{how:?}: {text}"
        );
        assert!(!text.contains("SheafSettings"), "{how:?}");
    }
    let (_, mut doc) = opened(&dir, "b.pdf", &acrobat_file(&[&[BODY, legacy]]));
    let page = doc.page(0).unwrap();
    let c = (doc.commit_ops(vec![fx::remove_watermark(None, &[page])], "remove")).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark and background from page 1 (1 made by an older sheaf)"
    );
}

/// Acrobat's watermark that no splice takes out stays where the
/// document's is drawn, and the save is made, a note counting it;
/// RemoveWatermark says it stays, and a page whose content can't be read.
#[test]
fn what_a_splice_refuses_stays_and_is_counted() {
    let dir = scratch();
    let unread: &[u8] = b"0 0 1 rg 10 10 50 50 re f EMC";
    let file = acrobat_file(&[&[BODY, ACROBAT_UNSAFE], &[unread]]);
    for how in [How::Rewritten, How::Appended] {
        let (path, mut doc) = opened(&dir, "a.pdf", &file);
        doc.commit_ops(vec![watermark("MINE")], "watermark")
            .unwrap();
        let report = how.save(&mut doc, &path);
        let notes: Vec<String> = report.notes.iter().map(|n| n.to_string()).collect();
        let note = "1 watermark or background other programs or files drew stays on page 1: \
                    taking it out would change what else the page draws.";
        assert!(notes.iter().any(|n| n == note), "{how:?}: {notes:?}");
        let pdf = Pdf::load(&path).unwrap();
        let text = drawn(&pdf, pages(&pdf)[0]);
        assert!(
            text.contains("/Fm0 Do") && text.contains("(MINE)"),
            "{how:?}: {text}"
        );
        assert_eq!(depth(&pdf, pages(&pdf)[0]).1, Balance::default(), "{how:?}");
    }
    let (_, mut doc) = opened(&dir, "b.pdf", &file);
    let all: Vec<PageId> = doc.state().order().iter().copied().collect();
    let c = (doc.commit_ops(vec![fx::remove_watermark(None, &all)], "remove")).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark and background from pages 1, 2 (1 left in place, as taking it \
         out would change the page; 1 page that can't be read left as it was)"
    );
    assert!(
        all.iter()
            .all(|&p| doc.state().def(p).unwrap().content.filters.is_empty())
    );
}

/// Three blank pages share one stream, which holds Acrobat's watermark: a
/// watermark of the document's on the first draws a copy of the stream
/// without it there, and the others draw the stream as it was. Both ways.
fn shared(how: How) {
    let dir = scratch();
    let blank: &[&[u8]] = &[ACROBAT];
    let (_, mut doc) = opened(&dir, "blank.pdf", &acrobat_file(&[blank, blank, blank]));
    let first = doc.page(0).unwrap();
    let mine = fx::set_watermark(fx::watermark("MINE"), Z::Over, Some(&[first]));
    doc.commit_ops(vec![mine], "watermark").unwrap();
    let original = Pdf::load_mem(&std::fs::read(doc.file().unwrap().0).unwrap()).unwrap();
    let shared = contents(&original, pages(&original)[0])[0];
    let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
    let firsts = contents(&pdf, pages(&pdf)[0]);
    assert!(!firsts.contains(&shared), "{how:?}: a copy");
    let text = drawn(&pdf, pages(&pdf)[0]);
    assert!(!text.contains("/Fm0") && text.contains("(MINE)"), "{text}");
    for page in &pages(&pdf)[1..] {
        assert_eq!(contents(&pdf, *page), [shared], "{how:?}");
        assert_eq!(bound(&pdf, *page, "XObject"), ["Fm0"], "{how:?}");
    }
    assert_eq!(stream(&pdf, shared).1, ACROBAT.to_vec(), "{how:?}");
}

crate::both_ways!(
    a_stream_blank_pages_share_stays_where_uncovered,
    a_stream_blank_pages_share_stays_where_uncovered_appended: shared
);

/// A file as [`acrobat_file`] makes one, whose resources are Helvetica as
/// `/F1` and what an older sheaf drew its watermarks with: its font,
/// graphics state and image.
fn older_file(streams: &[&[&[u8]]]) -> Vec<u8> {
    file_of(streams, older::resources)
}

/// `doc`'s current version written `how`, its state with what an older
/// sheaf kept in its watermarks and backgrounds taken out of `pages`, as
/// Clean Copy takes it out of the pages that hold one: the file, and its
/// notes.
fn stripped(doc: &Document, pages: &[PageId], how: How) -> (Vec<u8>, Vec<String>) {
    let mut target = doc.state().clone();
    for &p in pages {
        let def = target.def(p).unwrap();
        let older = MarkedSel::Pagination {
            kinds: BTreeSet::from([PaginationKind::Watermark, PaginationKind::Background]),
            origins: BTreeSet::from([PagOrigin::SheafLegacy]),
        };
        let content = (def.content).with_filter(Filter::StripLegacy(LegacySel::Marked(older)));
        let def = PageDef::new(def.base.clone(), def.copy, content);
        target
            .set(&Key::Page(p), Some(Val::Page(Arc::new(def))))
            .unwrap();
    }
    let out = match how {
        How::Rewritten => {
            let input = SaveInput {
                target,
                ..SaveInput::detached(&doc.snapshot())
            };
            let to = SaveTarget {
                path: Path::new("").into(),
                same_file: false,
                profile: Profile::default(),
                stamp: None,
            };
            let plan = super::plan(&input, &to);
            super::materialize(&input, &plan, to, &NoProgress).unwrap()
        }
        How::Appended => {
            let mut input = doc.append_input(doc.current_version());
            input.target = target;
            input.path_keys = Arc::new(pages.iter().map(|&p| Key::Page(p)).collect());
            let plan = super::forced_append(&input, Onto::Saved);
            let base = input.primary.bytes().clone();
            let mode = super::removal::Mode::ReportAll;
            let mut out =
                super::append::append(&input, &plan, Arc::new(base.clone()), mode, &NoProgress)
                    .unwrap();
            out.bytes = [&base[..], &out.bytes].concat();
            out
        }
    };
    (out.bytes, out.notes.iter().map(|n| n.to_string()).collect())
}

/// Pages an older sheaf drew a watermark and a background on, as it wrote
/// them, the image's path in the background's settings: a clean copy
/// keeps what they draw and takes out what it kept in them, so no object
/// of the copy holds the settings or the path. The two pages that shared
/// their streams share the copies, and a page with none draws its own as
/// it was.
#[test]
fn a_clean_copy_takes_out_what_an_older_sheaf_kept() {
    let dir = scratch();
    let marked: &[&[u8]] = &[older::Q, older::BACKGROUND, BODY, older::WATERMARK];
    let other: &[u8] = b"BT /F1 12 Tf 72 700 Td (other) Tj ET";
    let (_, doc) = opened(&dir, "a.pdf", &older_file(&[marked, marked, &[other]]));
    let out = dir.join("clean.pdf");
    crate::save::export_clean_copy(&doc.snapshot(), &out).unwrap();
    let pdf = Pdf::load(&out).unwrap();
    let all = everything(&pdf);
    for gone in ["SheafSettings", "SheafStamp", "/home/someone"] {
        assert!(!all.contains(gone), "{gone}");
    }
    let [first, second, third] = pages(&pdf)[..] else {
        panic!("three pages")
    };
    let text = drawn(&pdf, first);
    for kept in [
        "/Subtype/Background>> BDC",
        "/SheafWMim Do",
        "(body) Tj",
        "/Subtype/Watermark>> BDC",
        "(DRAFT) Tj",
    ] {
        assert!(text.contains(kept), "{kept}: {text}");
    }
    assert_eq!(depth(&pdf, first).1, Balance::default());
    assert_eq!(contents(&pdf, first), contents(&pdf, second));
    assert_eq!(drawn(&pdf, third), String::from_utf8_lossy(other));
}

/// Three blank pages share one stream, which holds a background an older
/// sheaf drew: what it kept taken out of the first's alone, that page
/// draws a copy of the stream without it, and the others the stream as it
/// was. Both ways.
fn older_shared(how: How) {
    let dir = scratch();
    let blank: &[&[u8]] = &[older::BACKGROUND];
    let (_, doc) = opened(&dir, "blank.pdf", &older_file(&[blank, blank, blank]));
    let original = Pdf::load_mem(doc.snapshot().primary.bytes()).unwrap();
    let shared = contents(&original, pages(&original)[0])[0];
    let (bytes, notes) = stripped(&doc, &[doc.page(0).unwrap()], how);
    assert_eq!(notes, Vec::<String>::new(), "{how:?}");
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let firsts = contents(&pdf, pages(&pdf)[0]);
    assert_eq!(firsts.len(), 1);
    assert_ne!(firsts[0], shared, "{how:?}: a copy");
    let text = drawn(&pdf, pages(&pdf)[0]);
    assert!(
        !text.contains("SheafSettings") && text.contains("/SheafWMim Do"),
        "{how:?}: {text}"
    );
    for page in &pages(&pdf)[1..] {
        assert_eq!(contents(&pdf, *page), [shared], "{how:?}");
    }
    assert_eq!(
        stream(&pdf, shared).1,
        older::BACKGROUND.to_vec(),
        "{how:?}"
    );
}

crate::both_ways!(
    a_stream_blank_pages_share_keeps_what_an_older_sheaf_kept_elsewhere,
    a_stream_blank_pages_share_keeps_what_an_older_sheaf_kept_elsewhere_appended: older_shared
);

/// An older sheaf's watermark whose settings are in a list the resources
/// name, `/P0`, which no rewrite of the content reaches, keeps them, and
/// the save is made, a note counting it.
#[test]
fn settings_a_list_cant_lose_stay_and_are_counted() {
    let dir = scratch();
    let named: &[u8] = b"/Artifact /P0 BDC q BT /F1 60 Tf 100 300 Td (NAMED) Tj ET Q EMC";
    let file = file_of(&[&[BODY, named]], |pdf, font| {
        let mut resources = older::resources(pdf, font);
        let list = dictionary! {
            "Type" => "Pagination", "Subtype" => "Watermark",
            "SheafSettings" => Object::string_literal("{}"),
        };
        resources.set("Properties", dictionary! { "P0" => list });
        resources
    });
    let (_, doc) = opened(&dir, "a.pdf", &file);
    for how in [How::Rewritten, How::Appended] {
        let (bytes, notes) = stripped(&doc, &[doc.page(0).unwrap()], how);
        let note = "1 watermark or background an older sheaf drew on page 1 keeps the settings \
                    it kept in it, which can't be taken out of it as it's written.";
        assert_eq!(notes, [note], "{how:?}");
        let pdf = Pdf::load_mem(&bytes).unwrap();
        assert!(
            drawn(&pdf, pages(&pdf)[0]).contains("(NAMED) Tj"),
            "{how:?}"
        );
    }
}
