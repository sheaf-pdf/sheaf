//! The header and footer as a save draws it (M10.5 slice 17): a block of
//! its own on each page it covers, marked with its generator, the digests
//! of its layout and recipe and, with Bates numbers, the page's number
//! (`/N`); each slot an artifact of the header's or the footer's, in the
//! spec's face, one font a file; its page tokens read as the document
//! being written has them, its Bates numbers as they were fixed, which an
//! open reads back from the blocks; the headers others drew on its pages
//! replaced, the author's own left; found again from `/SheafGenerators`.
//! And its live tokens drawn again after a save: a reorder appends the
//! pages whose numbers moved, their old blocks let go (§11.9), while a
//! change to its text rewrites.

use std::collections::BTreeSet;
use std::path::Path;

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::{BatesSpec, FontChoice, HeaderFooterSpec, LabelStyle, Op, PageSel};

use super::others_tests::{BODY, acrobat_file};
use super::plan::{Onto, Strategy};
use super::recognized_tests::{block_of, contents, drawn, everything, isolates, pages, stream};
use super::removal_tests::{opened, said, scratch, text_file};
use super::tests::{bytes, file};
use super::{AskAnswers, How, ObjKey, SaveReport, StrategyKind, Why, removal};
use crate::content::{GenId, Marker, PagOrigin, PaginationKind};
use crate::effects::Flags;
use crate::model::{Document, Generator, PageId, Scope};
use crate::realize::ViewInputs;
use crate::tx::Notice;
use crate::tx::fixture as fx;

/// Acrobat's header, after the page's content: its form drawn.
pub(crate) const ACROBAT_HEADER: &[u8] =
    b"/Artifact <</Type /Pagination /Subtype /Header>> BDC q 1 0 0 1 72 740 cm /Fm0 Do Q EMC";

/// A running head as a word processor marks it: the author's own, its
/// font set outside a `q`, which no splice takes out.
pub(crate) const AUTHORED: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Header>> BDC \
    BT /F1 9 Tf 72 760 Td (Chapter One) Tj ET EMC";

/// Bates numbers from `start`, `digits` long, after `prefix`.
fn bates(prefix: &str, digits: u8, start: u64) -> BatesSpec {
    BatesSpec {
        prefix: prefix.into(),
        suffix: String::new(),
        digits,
        start,
    }
}

/// The document's header and footer.
fn header_of(doc: &Document) -> &Generator {
    doc.state().header_footer().expect("a header and footer")
}

/// Every page of `doc`, in order.
fn ids(doc: &Document) -> Vec<PageId> {
    doc.state().order().iter().copied().collect()
}

/// The header's Bates numbers on `doc`'s pages, in order.
fn numbers(doc: &Document) -> Vec<Option<u64>> {
    let g = header_of(doc);
    ids(doc).iter().map(|&p| g.frozen.bates_number(p)).collect()
}

/// The `/SheafBlock` dictionary of stream `id` of `pdf`, if it has one.
fn marker(pdf: &Pdf, id: ObjectId) -> Option<Dictionary> {
    let (dict, _) = stream(pdf, id);
    dict.get(b"SheafBlock").ok()?.as_dict().ok().cloned()
}

/// The blocks of generators on each page of `pdf`, in order.
fn page_blocks(pdf: &Pdf) -> Vec<Vec<ObjectId>> {
    (pages(pdf).into_iter())
        .map(|p| {
            (contents(pdf, p).into_iter())
                .filter(|&c| block_of(pdf, c).is_some())
                .collect()
        })
        .collect()
}

/// How many fonts of `pdf` are `base`.
fn fonts(pdf: &Pdf, base: &[u8]) -> usize {
    (pdf.objects.values())
        .filter(|o| {
            o.as_dict()
                .is_ok_and(|d| d.get(b"BaseFont").and_then(Object::as_name).ok() == Some(base))
        })
        .count()
}

/// A header and footer with Bates numbers on two pages of three: a block
/// on each, marked `/K /Gen` with its generator, layout and recipe
/// digests, and its page's number; its header's and footer's artifacts
/// each named in the page's `/Properties`, `/Header` and `/Footer`, sheaf's
/// own; its text in the spec's face, one font in the file. Set again
/// without Bates numbers, a block says no number. Both ways.
fn block_shape(how: How) {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let [p0, p1, _] = ids(&doc)[..] else {
        panic!("three pages")
    };
    let mut spec = fx::header("<<bates>>");
    spec.texts[4] = "Page <<1>> of <<N>>".into();
    spec.font = FontChoice::TimesRoman;
    spec.bates = Some(bates("X", 4, 7));
    let set = fx::set_header(spec.clone(), Some(&[p0, p1]));
    doc.commit_ops(vec![set], "header").unwrap();
    let id = header_of(&doc).id;
    how.save(&mut doc, &path);
    let pdf = Pdf::load(&path).unwrap();
    let blocks = page_blocks(&pdf);
    assert_eq!(blocks.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1, 0]);
    for (i, page) in pages(&pdf).into_iter().take(2).enumerate() {
        let m = marker(&pdf, blocks[i][0]).unwrap();
        assert_eq!(m.get(b"K").unwrap().as_name().unwrap(), b"Gen");
        assert_eq!(
            m.get(b"G").unwrap().as_str().unwrap(),
            id.to_string().as_bytes()
        );
        for digest in [&b"L"[..], b"R"] {
            assert_eq!(m.get(digest).unwrap().as_str().unwrap().len(), 16);
        }
        let n = format!("{}", 7 + i);
        assert_eq!(m.get(b"N").unwrap().as_str().unwrap(), n.as_bytes());
        let text = drawn(&pdf, page);
        let want = [format!("(X000{n}) Tj"), format!("(Page {} of 3) Tj", i + 1)];
        assert!(want.iter().all(|w| text.contains(w)), "{how:?}: {text}");
    }
    assert_eq!(fonts(&pdf, b"Times-Roman"), 1, "{how:?}");
    let src = crate::forms::fixture::source(std::fs::read(&path).unwrap());
    let found: Vec<(PaginationKind, PagOrigin, Option<GenId>)> = (src.blocks(0).unwrap())
        .pagination
        .iter()
        .map(|p| (p.kind, p.origin, p.generator))
        .collect();
    let mine = |kind| (kind, PagOrigin::Sheaf, Some(id));
    assert_eq!(
        found,
        [mine(PaginationKind::Header), mine(PaginationKind::Footer)]
    );
    // Without Bates numbers, no block says one.
    spec.bates = None;
    let set = fx::set_header(spec, Some(&[p0, p1]));
    doc.commit_ops(vec![set], "header").unwrap();
    how.save(&mut doc, &path);
    let pdf = Pdf::load(&path).unwrap();
    for block in page_blocks(&pdf).into_iter().flatten() {
        assert!(!marker(&pdf, block).unwrap().has(b"N"), "{how:?}");
    }
}

crate::both_ways!(
    a_header_is_a_block_a_page_marked_with_its_generator,
    a_header_is_a_block_a_page_marked_with_its_generator_appended: block_shape
);

/// On every page, a header reaches a page inserted later, numbered where
/// it is, before a save and after an open; one of Bates numbers alone
/// draws nothing there, its block empty, saying no number, which keeps the
/// page in its scope. Both ways.
fn inserted(how: How) {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let mut spec = fx::header("Page <<1>>");
    spec.texts[5] = "<<bates>>".into();
    spec.bates = Some(bates("", 3, 1));
    doc.commit_ops(vec![fx::set_header(spec, None)], "header")
        .unwrap();
    let other = super::tests::add(&doc, &text_file(1), "other.pdf");
    super::tests::insert(&mut doc, &other, 0);
    assert_eq!(numbers(&doc), [Some(1), Some(2), None]);
    how.save(&mut doc, &path);
    let pdf = Pdf::load(&path).unwrap();
    let text = drawn(&pdf, pages(&pdf)[2]);
    assert!(
        text.contains("(Page 3)") && !text.contains("(003)"),
        "{text}"
    );
    let (_, mut again) = opened(&dir, "b.pdf", &std::fs::read(&path).unwrap());
    assert_eq!(header_of(&again).scope, Scope::All, "{how:?}");
    assert_eq!(numbers(&again), [Some(1), Some(2), None], "{how:?}");
    let other = super::tests::add(&again, &text_file(1), "other.pdf");
    super::tests::insert(&mut again, &other, 0);
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    let text = drawn(&pdf, pages(&pdf)[3]);
    assert!(text.contains("(Page 4)"), "{how:?}: {text}");
    // Bates numbers alone: an empty block on a page without one.
    let only = HeaderFooterSpec {
        bates: Some(bates("", 3, 1)),
        ..fx::header("<<bates>>")
    };
    let (_, mut doc) = opened(&dir, "c.pdf", &text_file(1));
    doc.commit_ops(vec![fx::set_header(only, None)], "header")
        .unwrap();
    let other = super::tests::add(&doc, &text_file(1), "other.pdf");
    super::tests::insert(&mut doc, &other, 0);
    let pdf = Pdf::load_mem(&How::Rewritten.write(&doc)).unwrap();
    let blocks = page_blocks(&pdf);
    let (dict, data) = stream(&pdf, blocks[1][0]);
    assert!(data.is_empty(), "{data:?}");
    assert!(
        !dict
            .get(b"SheafBlock")
            .unwrap()
            .as_dict()
            .unwrap()
            .has(b"N")
    );
    assert!(marker(&pdf, blocks[0][0]).unwrap().has(b"N"));
}

crate::both_ways!(
    a_header_on_every_page_reaches_a_page_inserted_later,
    a_header_on_every_page_reaches_a_page_inserted_later_appended: inserted
);

/// Bates numbers survive a save and an open, read back from the blocks
/// and keyed by the pages of the document opened, and a reorder after
/// keeps each page's: what's drawn moves with its page. Both ways.
fn numbered(how: How) {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(4));
    let spec = HeaderFooterSpec {
        bates: Some(bates("B-", 6, 100)),
        ..fx::header("<<bates>>")
    };
    doc.commit_ops(vec![fx::set_header(spec, None)], "header")
        .unwrap();
    how.save(&mut doc, &path);
    let (path, mut again) = opened(&dir, "b.pdf", &std::fs::read(&path).unwrap());
    let all: Vec<Option<u64>> = (100..104).map(Some).collect();
    assert_eq!(numbers(&again), all, "{how:?}");
    let g = header_of(&again);
    let keys: Vec<PageId> = g.frozen.bates.as_ref().unwrap().keys().copied().collect();
    assert_eq!(keys, ids(&again));
    let [first, .., last] = ids(&again)[..] else {
        panic!("four pages")
    };
    again
        .commit_ops(vec![fx::move_pages(&[last], Some(first))], "reorder")
        .unwrap();
    let moved = [Some(103), Some(100), Some(101), Some(102)];
    assert_eq!(numbers(&again), moved);
    how.save(&mut again, &path);
    let pdf = Pdf::load(&path).unwrap();
    let said: Vec<bool> = (pages(&pdf).into_iter().zip(moved))
        .map(|(p, n)| drawn(&pdf, p).contains(&format!("(B-000{}) Tj", n.unwrap())))
        .collect();
    assert_eq!(said, [true; 4], "{how:?}");
    let (_, third) = opened(&dir, "c.pdf", &std::fs::read(&path).unwrap());
    assert_eq!(numbers(&third), moved, "{how:?}");
}

crate::both_ways!(
    bates_numbers_survive_a_save_and_an_open,
    bates_numbers_survive_a_save_and_an_open_appended: numbered
);

/// `bytes` with page `index`'s streams merged into one, as another tool
/// may write it: its blocks' dictionaries gone, their lists left.
fn merged(bytes: &[u8], index: usize) -> Vec<u8> {
    let mut pdf = Pdf::load_mem(bytes).unwrap();
    let page = pages(&pdf)[index];
    let data: Vec<u8> = (contents(&pdf, page).into_iter())
        .flat_map(|c| [stream(&pdf, c).1, b"\n".to_vec()].concat())
        .collect();
    let one = pdf.add_object(Stream::new(dictionary! {}, data));
    pdf.get_dictionary_mut(page).unwrap().set("Contents", one);
    let mut out = Vec::new();
    pdf.save_to(&mut out).unwrap();
    out
}

/// A page whose header another tool merged into its content has no Bates
/// number when it opens again: the open logs it, and a save that writes
/// the file says so.
#[test]
fn a_merged_header_s_bates_number_is_noted() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let spec = HeaderFooterSpec {
        bates: Some(bates("", 3, 1)),
        ..fx::header("<<bates>>")
    };
    doc.commit_ops(vec![fx::set_header(spec, None)], "header")
        .unwrap();
    let file = merged(&How::Rewritten.write(&doc), 1);
    let (path, mut again) = opened(&dir, "b.pdf", &file);
    assert_eq!(numbers(&again), [Some(1), None, Some(3)]);
    let saved = again.primary().generators();
    assert_eq!(saved[0].unnumbered(), [1]);
    let report = How::Rewritten.save(&mut again, &path);
    let note = "The Bates numbers on page 2 can't be read: another program merged the header \
                into the page. They're saved without them; set the header and footer again to \
                number them.";
    let notes: Vec<String> = report.notes.iter().map(|n| n.to_string()).collect();
    assert!(notes.iter().any(|n| n == note), "{notes:?}");
}

/// An extract of a document not saved keeps the Bates numbers the binder
/// fixed, and numbers its pages as the extract has them.
#[test]
fn an_extract_keeps_the_binder_s_bates_numbers_and_renumbers_its_pages() {
    let mut doc = crate::tx::tests::doc_of(6, &[]);
    let mut spec = HeaderFooterSpec {
        bates: Some(bates("", 6, 1)),
        ..fx::header("<<bates>>")
    };
    spec.texts[4] = "Page <<1>> of <<N>>".into();
    doc.commit_ops(vec![fx::set_header(spec, None)], "header")
        .unwrap();
    let all = ids(&doc);
    let out = crate::save::extract_bytes(&doc.snapshot(), &[all[2], all[4]]).unwrap();
    let pdf = Pdf::load_mem(&out).unwrap();
    let said: Vec<String> = pages(&pdf).into_iter().map(|p| drawn(&pdf, p)).collect();
    let want = [("(000003)", "(Page 1 of 2)"), ("(000005)", "(Page 2 of 2)")];
    for (text, (bates, page)) in said.iter().zip(want) {
        assert!(text.contains(bates) && text.contains(page), "{text}");
    }
}

/// A label change that makes a `<<label>>` header draw what its face
/// can't says so, and the header draws `?` for it: saving still works.
/// Both ways.
fn unencodable(how: How) {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let set = fx::set_header(fx::header("<<label>>"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    how.save(&mut doc, &path);
    let first = doc.page(0).unwrap();
    let labels = Op::SetPageLabels {
        ranges: Some(vec![fx::label(first, LabelStyle::Decimal, "Ω-", 1)]),
    };
    let notices = doc.commit_ops(vec![labels], "labels").unwrap().notices;
    assert_eq!(notices, [Notice::Unencodable]);
    how.save(&mut doc, &path);
    let pdf = Pdf::load(&path).unwrap();
    for (i, page) in pages(&pdf).into_iter().enumerate() {
        let text = drawn(&pdf, page);
        assert!(
            text.contains(&format!("(?-{}) Tj", i + 1)),
            "{how:?}: {text}"
        );
    }
}

crate::both_ways!(
    an_unencodable_label_draws_a_question_mark,
    an_unencodable_label_draws_a_question_mark_appended: unencodable
);

/// A header set over pages Acrobat drew a header on replaces Acrobat's,
/// taken out of the page's content with the form it drew; a running head
/// the author drew stays. Both ways.
fn replaces(how: How) {
    let dir = scratch();
    let file = acrobat_file(&[&[BODY, ACROBAT_HEADER], &[BODY, AUTHORED]]);
    let (path, mut doc) = opened(&dir, "a.pdf", &file);
    let set = fx::set_header(fx::header("Mine"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    how.save(&mut doc, &path);
    let pdf = Pdf::load(&path).unwrap();
    let [acrobat, authored] = pages(&pdf)[..] else {
        panic!("two pages")
    };
    let text = drawn(&pdf, acrobat);
    assert!(
        text.contains("(Mine)") && !text.contains("/Fm0 Do"),
        "{how:?}: {text}"
    );
    let text = drawn(&pdf, authored);
    assert!(
        text.contains("(Mine)") && text.contains("(Chapter One)"),
        "{how:?}: {text}"
    );
}

crate::both_ways!(
    a_header_replaces_acrobat_s_and_leaves_the_author_s,
    a_header_replaces_acrobat_s_and_leaves_the_author_s_appended: replaces
);

/// A file of PDF 1.3, which has no marked content, is written as 1.4 once
/// a header is drawn on it: its header by a Rewrite, its catalog's
/// `/Version` by an Append.
fn version(how: How) {
    let dir = scratch();
    let mut pdf = file("A", 2, false);
    pdf.version = "1.3".into();
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes(&pdf));
    let set = fx::set_header(fx::header("Draft"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    how.save(&mut doc, &path);
    let out = std::fs::read(&path).unwrap();
    let saved = Pdf::load_mem(&out).unwrap();
    let catalog = saved.catalog().unwrap();
    match how {
        How::Rewritten => assert!(out.starts_with(b"%PDF-1.4")),
        How::Appended => assert_eq!(catalog.get(b"Version").unwrap().as_name().unwrap(), b"1.4"),
    }
}

crate::both_ways!(
    a_header_raises_1_3_to_1_4,
    a_header_raises_1_3_to_1_4_appended: version
);

/// A header saved and opened again is the generator it was, its id, spec,
/// date and file, on the pages it was on, and the file's pages are written
/// as they are, untouched under the screen's inputs; changed, each page
/// holds one block of it, the new one, its old one set aside. Both ways.
fn found(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let [_, second, third] = ids(&doc)[..] else {
        panic!("three pages")
    };
    let set = fx::set_header(fx::header("Page <<1>> of <<N>>"), Some(&[second, third]));
    doc.commit_ops(vec![set], "header").unwrap();
    let (_, mut again) = opened(&dir, "b.pdf", &how.write(&doc));
    let (was, now) = (header_of(&doc), header_of(&again));
    let said = |g: &Generator| {
        (
            g.id,
            g.spec.clone(),
            g.frozen.date.clone(),
            g.frozen.file.clone(),
        )
    };
    assert_eq!(said(was), said(now), "{how:?}");
    let on: im::OrdSet<PageId> = ids(&again)[1..].iter().copied().collect();
    assert_eq!(now.scope, Scope::Pages(on), "{how:?}");
    let st = again.state();
    for (id, _, _) in st.pages() {
        assert!(
            crate::realize::untouched(st, id, &ViewInputs::screen()),
            "{how:?}"
        );
    }
    let file = Pdf::load_mem(again.primary().bytes()).unwrap();
    let written = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    for (a, b) in pages(&file).into_iter().zip(pages(&written)) {
        assert_eq!(contents(&file, a), contents(&written, b), "{how:?}");
    }
    let id = header_of(&again).id;
    let set = fx::set_header(fx::header("Final <<1>>"), Some(&[ids(&again)[1]]));
    again.commit_ops(vec![set], "header").unwrap();
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    let blocks: Vec<Vec<GenId>> = (page_blocks(&pdf).into_iter())
        .map(|b| b.into_iter().filter_map(|c| block_of(&pdf, c)).collect())
        .collect();
    assert_eq!(blocks, [vec![], vec![id], vec![]], "{how:?}");
    let text = drawn(&pdf, pages(&pdf)[1]);
    assert!(
        text.contains("(Final 2)") && !text.contains("of 3"),
        "{how:?}: {text}"
    );
}

crate::both_ways!(
    a_saved_header_is_found_again,
    a_saved_header_is_found_again_appended: found
);

/// A header found again and removed leaves nothing of it: the save
/// rewrites, and the file has no block, no stream isolating content, no
/// list naming a generator, and nowhere the header's text.
#[test]
fn a_found_header_removed_leaves_no_trace() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let set = fx::set_header(fx::header("CONFIDENTIAL"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    let (path, mut again) = opened(&dir, "b.pdf", &How::Rewritten.write(&doc));
    let remove = Op::RemoveHeaderFooter {
        pages: PageSel::All,
        include_authored: false,
    };
    again.commit_ops(vec![remove], "remove").unwrap();
    let report = super::save(
        &mut again,
        &path,
        &AskAnswers::default(),
        &in_progress::NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let pdf = Pdf::load(&path).unwrap();
    for page in pages(&pdf) {
        let streams = contents(&pdf, page);
        assert_eq!(streams.len(), 1);
        assert!(!streams.iter().any(|&s| isolates(&pdf, s)));
    }
    let all = everything(&pdf);
    for gone in ["CONFIDENTIAL", "SheafBlock", "SheafGen", "Isolate"] {
        assert!(!all.contains(gone), "{gone}");
    }
    let (_, third) = opened(&dir, "c.pdf", &std::fs::read(&path).unwrap());
    assert!(third.state().generators().is_empty());
    let kinds: BTreeSet<PaginationKind> = (0..3)
        .flat_map(|i| third.primary().blocks(i).unwrap().pagination.to_vec())
        .map(|p| p.kind)
        .collect();
    assert!(kinds.is_empty(), "{kinds:?}");
}

/// What sheaf wouldn't write as a header isn't found, and the rest still
/// is: one under the content, one that replaces another kind, one that
/// says nothing it fixed, one that can't be drawn, a header whose spec is
/// a watermark's; and of two headers, the second is passed over.
#[test]
fn what_sheaf_wouldn_t_write_as_a_header_isn_t_found() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let watermark = fx::set_watermark(fx::watermark("DRAFT"), sheaf_edit::Z::Over, None);
    let header = fx::set_header(fx::header("Page <<1>>"), None);
    doc.commit_ops(vec![header, watermark], "header").unwrap();
    let bytes = How::Rewritten.write(&doc);
    // The headers and the watermarks found once `edit` changes the JSON.
    let found = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut pdf = Pdf::load_mem(&bytes).unwrap();
        let catalog = pdf.catalog().unwrap();
        let at = (catalog.get(b"SheafGenerators"))
            .and_then(Object::as_reference)
            .unwrap();
        let mut json: serde_json::Value =
            serde_json::from_slice(pdf.get_object(at).unwrap().as_str().unwrap()).unwrap();
        edit(&mut json);
        let json = serde_json::to_vec(&json).unwrap();
        pdf.objects.insert(at, Object::string_literal(json));
        let mut out = Vec::new();
        pdf.save_to(&mut out).unwrap();
        let (_, again) = opened(&dir, "b.pdf", &out);
        let gens = again.state().generators();
        let headers = gens.values().filter(|g| g.spec.header_footer().is_some());
        let watermarks = gens.values().filter(|g| g.spec.watermark().is_some());
        (headers.count(), watermarks.count())
    };
    let header_of = |json: &mut serde_json::Value, edit: &dyn Fn(&mut serde_json::Value)| {
        for g in json["generators"].as_array_mut().unwrap() {
            if g["kind"] == "header-footer" {
                edit(g);
            }
        }
    };
    assert_eq!(found(&|_| {}), (1, 1));
    assert_eq!(
        found(&|j| header_of(j, &|g| g["z"] = "under".into())),
        (0, 1)
    );
    let one_kind = |g: &mut serde_json::Value| g["replaces"] = serde_json::json!(["header"]);
    assert_eq!(found(&|j| header_of(j, &one_kind)), (0, 1));
    let unfixed = |g: &mut serde_json::Value| {
        g.as_object_mut().unwrap().remove("frozen");
    };
    assert_eq!(found(&|j| header_of(j, &unfixed)), (0, 1));
    assert_eq!(
        found(&|j| header_of(j, &|g| g["spec"]["size"] = 0.into())),
        (0, 1)
    );
    assert_eq!(
        found(&|j| header_of(j, &|g| g["kind"] = "watermark".into())),
        (0, 1)
    );
    // The watermark said to be a second header: one of them is found.
    let second = |json: &mut serde_json::Value| {
        let gens = json["generators"].as_array_mut().unwrap();
        let header = (gens.iter())
            .find(|g| g["kind"] == "header-footer")
            .unwrap()
            .clone();
        for g in gens.iter_mut().filter(|g| g["kind"] == "watermark") {
            for key in ["kind", "replaces", "spec", "frozen"] {
                g[key] = header[key].clone();
            }
        }
    };
    assert_eq!(found(&second), (1, 0));
}

/// A file of three pages, each saying its number, with a header an older
/// sheaf drew on the third, "Draft", its settings in the catalog.
pub(crate) fn older_header_file() -> Vec<u8> {
    use crate::generate::fixture::{HEADER_SETTINGS, legacy_header};
    let mut pdf = Pdf::load_mem(&text_file(3)).unwrap();
    let third = pages(&pdf)[2];
    let root = (pdf.catalog().unwrap().get(b"Pages"))
        .and_then(Object::as_reference)
        .unwrap();
    let inherited = pdf
        .get_dictionary(root)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .clone();
    let mut page = pdf.get_dictionary(third).unwrap().clone();
    page.set("Resources", inherited);
    legacy_header(&mut pdf, &mut page);
    pdf.objects.insert(third, Object::Dictionary(page));
    let catalog = pdf
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .unwrap();
    (pdf.get_dictionary_mut(catalog).unwrap())
        .set("SheafHeaderFooter", Object::string_literal(HEADER_SETTINGS));
    bytes(&pdf)
}

/// A header an older sheaf drew on the third page, its settings in the
/// catalog: the settings aren't read, so no header opens; a header set
/// over its page replaces it, and the save, a Rewrite for what that takes
/// off, writes neither the settings nor the old text.
#[test]
fn an_older_sheaf_s_header_is_replaced_and_its_settings_dropped() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "older.pdf", &older_header_file());
    assert!(doc.state().generators().is_empty());
    let found = doc.primary().blocks(2).unwrap();
    let older: Vec<(PaginationKind, PagOrigin)> = found
        .pagination
        .iter()
        .map(|p| (p.kind, p.origin))
        .collect();
    assert_eq!(older, [(PaginationKind::Header, PagOrigin::SheafLegacy)]);
    let set = fx::set_header(fx::header("Final"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    let report = super::save(
        &mut doc,
        &path,
        &AskAnswers::default(),
        &in_progress::NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let pdf = Pdf::load(&path).unwrap();
    assert!(!pdf.catalog().unwrap().has(b"SheafHeaderFooter"));
    let all = everything(&pdf);
    assert!(!all.contains("Draft") && !all.contains("SheafHF"), "{all}");
    assert!(drawn(&pdf, pages(&pdf)[2]).contains("(Final) Tj"));
}

/// The objects `section`, an Append's, writes, each as its first bytes
/// say it.
fn objects_of(section: &[u8]) -> Vec<String> {
    let mut objects = Vec::new();
    let mut rest = section;
    while let Some(at) = rest.windows(6).position(|w| w == b" 0 obj") {
        rest = rest[at + 6..].trim_ascii_start();
        let head = &rest[..rest.len().min(80)];
        objects.push(String::from_utf8_lossy(head).into_owned());
    }
    objects
}

/// `doc` saved to `path`, its file, as planned.
fn planned(doc: &mut Document, path: &Path) -> SaveReport {
    super::save(doc, path, &AskAnswers::default(), &NoProgress).unwrap()
}

/// `doc`'s second and third pages traded, and saved to `path`, its file,
/// as planned: an Append of a live redraw (`LIVE_REDRAW`, which removes
/// nothing), its scope those two pages and the page tree, and its section
/// their dictionaries, their blocks the file hasn't got (another page's,
/// drawn alike before, is the file's) and the tree's node alone, no
/// catalog or generators; the removal check, letting their old blocks go,
/// finds nothing. The pages then say `said`, in order, each numbered
/// where it is.
fn traded(doc: &mut Document, path: &Path, said_now: [&str; 5]) {
    let ids = ids(doc);
    let trade = fx::move_pages(&[ids[2]], Some(ids[1]));
    doc.commit_ops(vec![trade], "trade").unwrap();
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let plan = super::plan(&input, &doc.save_target(path));
    assert_eq!(plan.strategy, Strategy::Append { onto: Onto::Saved });
    let saved = input.saved.as_ref().unwrap();
    let flags = crate::effects::judge(&saved.state, &input.target, &input.path_keys).flags;
    assert!(flags.contains(Flags::LIVE_REDRAW), "{flags:?}");
    let scope = super::deps::closure(&input, saved, &[]).unwrap();
    let written = [
        ObjKey::Page(ids[1]),
        ObjKey::Page(ids[2]),
        ObjKey::PagesNode(0),
    ];
    assert_eq!(scope.keys, BTreeSet::from(written));
    drop(input);
    doc.abandon_save(v);
    let before = std::fs::read(path).unwrap().len();
    let vetoes = removal::vetoes_here();
    let report = planned(doc, path);
    assert_eq!(
        report.strategy,
        Some(StrategyKind::Append),
        "{:?}",
        report.why
    );
    assert_eq!(removal::vetoes_here(), vetoes);
    let file = std::fs::read(path).unwrap();
    let objects = objects_of(&file[before..]);
    let count = |what: &str| objects.iter().filter(|o| o.contains(what)).count();
    let (dicts, nodes) = (count("/Type/Page/"), count("/Type/Pages"));
    let (blocks, xref) = (count("/SheafBlock"), count("/Type/XRef"));
    assert_eq!((dicts, nodes), (2, 1), "{objects:#?}");
    assert!(blocks <= 2 && dicts + nodes + blocks + xref == objects.len());
    assert_eq!(said(path), said_now);
    let pdf = Pdf::load(path).unwrap();
    for (n, page) in (1..).zip(pages(&pdf)) {
        let text = drawn(&pdf, page);
        assert!(text.contains(&format!("(Page {n} of 5) Tj")), "{text}");
    }
}

/// "Page <<1>> of <<N>>" on every page, saved, then two pages traded: an
/// Append of those pages alone, the numbers on them drawn again
/// ([`traded`]); then again on the file opened, the blocks its record says
/// were drawn found by their markers. Both ways for the first save.
fn live(how: How) {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(5));
    let set = fx::set_header(fx::header("Page <<1>> of <<N>>"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    how.save(&mut doc, &path);
    // What the save drew on each page, as its record says it: a block the
    // file's map numbers.
    let drew = |doc: &Document| {
        let record = doc.saved_record().unwrap();
        let g = header_of(doc).id;
        assert_eq!(record.drawn.len(), 5, "{how:?}");
        for page in ids(doc) {
            let key = record.drawn[&(page, g)].key;
            assert!(matches!(key, ObjKey::Block(_)), "{how:?}: {key:?}");
            assert!(record.map.keys.contains_key(&key), "{how:?}: {key:?}");
        }
    };
    drew(&doc);
    let traded_once = ["page 0", "page 2", "page 1", "page 3", "page 4"];
    traded(&mut doc, &path, traded_once);
    drew(&doc);
    let (path, mut again) = opened(&dir, "b.pdf", &std::fs::read(&path).unwrap());
    let g = header_of(&again).id;
    let record = again.saved_record().unwrap();
    assert_eq!(record.drawn.len(), 5);
    for (i, page) in ids(&again).into_iter().enumerate() {
        let block = record.drawn[&(page, g)];
        let markers = again.primary().markers(i as u32);
        let header = markers
            .iter()
            .find(|(_, m)| matches!(m, Marker::Gen { .. }));
        let stream = header.expect("its block").0;
        let key = ObjKey::Base {
            blob: again.primary().id(),
            obj: stream,
            copy: 0,
        };
        assert_eq!(block.key, key, "{how:?}");
    }
    let traded_back = ["page 0", "page 1", "page 2", "page 3", "page 4"];
    traded(&mut again, &path, traded_back);
}

crate::both_ways!(
    a_reorder_after_a_save_draws_the_numbers_again_and_appends,
    a_reorder_after_a_save_draws_the_numbers_again_and_appends_appended: live
);

/// The header's text changed after a save is a Rewrite, for the text it
/// replaces, which leaves the old words nowhere in the file; its margins
/// alone changed, it appends, the words it drew drawn anew. A header of
/// Bates numbers alone, its pages reordered, appends the page tree alone:
/// each page keeps its number, and draws what it drew.
#[test]
fn a_header_s_text_changed_rewrites_and_its_margins_and_numbers_append() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let spec = fx::header("Draft <<1>>");
    doc.commit_ops(vec![fx::set_header(spec.clone(), None)], "header")
        .unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let mut text = spec;
    text.texts[0] = "Final <<1>>".into();
    doc.commit_ops(vec![fx::set_header(text.clone(), None)], "text")
        .unwrap();
    let report = planned(&mut doc, &path);
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(
        matches!(&report.why[..], [Why::ReplacesText(_)]),
        "{:?}",
        report.why
    );
    let all = everything(&Pdf::load(&path).unwrap());
    assert!(!all.contains("Draft") && all.contains("(Final 3)"), "{all}");
    let mut margins = text;
    margins.margins.top += 18.0;
    doc.commit_ops(vec![fx::set_header(margins, None)], "margins")
        .unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );

    let (path, mut doc) = opened(&dir, "b.pdf", &text_file(3));
    let spec = HeaderFooterSpec {
        bates: Some(bates("N-", 2, 1)),
        ..fx::header("<<bates>>")
    };
    doc.commit_ops(vec![fx::set_header(spec, None)], "bates")
        .unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let [first, .., last] = ids(&doc)[..] else {
        panic!("three pages")
    };
    doc.commit_ops(vec![fx::move_pages(&[last], Some(first))], "reorder")
        .unwrap();
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let saved = input.saved.as_ref().unwrap();
    let flags = crate::effects::judge(&saved.state, &input.target, &input.path_keys).flags;
    assert!(!flags.contains(Flags::LIVE_REDRAW), "{flags:?}");
    let scope = super::deps::closure(&input, saved, &[]).unwrap();
    assert_eq!(scope.keys, BTreeSet::from([ObjKey::PagesNode(0)]));
    drop(input);
    doc.abandon_save(v);
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let (path, again) = opened(&dir, "c.pdf", &std::fs::read(&path).unwrap());
    assert_eq!(numbers(&again), [Some(3), Some(1), Some(2)]);
    assert_eq!(said(&path), ["page 2", "page 0", "page 1"]);
    let pdf = Pdf::load(&path).unwrap();
    for (n, page) in [3, 1, 2].into_iter().zip(pages(&pdf)) {
        assert!(drawn(&pdf, page).contains(&format!("(N-0{n}) Tj")));
    }
}

/// A header's text changed after a save, which the planner misjudges as
/// removing nothing: its old blocks were drawn from another recipe, so
/// they aren't live redraws, and the removal check stops the Append for
/// the words they drew, a misclassification. The save rewrites.
#[test]
fn a_block_drawn_from_another_recipe_still_stops_an_append() {
    use crate::effects::{PathEffects, misjudged};

    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let set = fx::set_header(fx::header("Draft <<1>>"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let set = fx::set_header(fx::header("Final <<1>>"), None);
    doc.commit_ops(vec![set], "text").unwrap();
    fn removes_nothing(path: &mut PathEffects) {
        path.flags.remove(Flags::REMOVES | Flags::REPLACES_TEXT);
        path.removed.clear();
        path.removing.clear();
    }
    let vetoes = removal::vetoes_here();
    let report = misjudged(removes_nothing, || planned(&mut doc, &path));
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(
        matches!(&report.why[..], [Why::RemovalCheck { .. }]),
        "{:?}",
        report.why
    );
    assert_eq!(removal::vetoes_here().derived, vetoes.derived + 1);
    assert!(!everything(&Pdf::load(&path).unwrap()).contains("Draft"));
}

/// A header of the pages' labels, saved, then labels given a prefix: an
/// Append that draws the labels again where they changed (`LIVE_REDRAW`),
/// their old blocks let go; the prefix replaced by another, a Rewrite for
/// the prefix gone, which leaves it nowhere in the file.
#[test]
fn a_label_change_draws_the_labels_again() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let set = fx::set_header(fx::header("<<label>>"), None);
    doc.commit_ops(vec![set], "header").unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let (path, mut doc) = opened(&dir, "b.pdf", &std::fs::read(&path).unwrap());
    let labelled = |doc: &mut Document, prefix: &str| {
        let first = doc.page(0).unwrap();
        let labels = Op::SetPageLabels {
            ranges: Some(vec![fx::label(first, LabelStyle::Decimal, prefix, 1)]),
        };
        doc.commit_ops(vec![labels], "labels").unwrap();
    };
    labelled(&mut doc, "A-");
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let saved = input.saved.as_ref().unwrap();
    let flags = crate::effects::judge(&saved.state, &input.target, &input.path_keys).flags;
    assert!(
        flags.contains(Flags::LIVE_REDRAW | Flags::LABELS),
        "{flags:?}"
    );
    drop(input);
    doc.abandon_save(v);
    let vetoes = removal::vetoes_here();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    assert_eq!(removal::vetoes_here(), vetoes);
    let pdf = Pdf::load(&path).unwrap();
    for (n, page) in (1..).zip(pages(&pdf)) {
        assert!(drawn(&pdf, page).contains(&format!("(A-{n}) Tj")));
    }
    labelled(&mut doc, "B-");
    let report = planned(&mut doc, &path);
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(!everything(&Pdf::load(&path).unwrap()).contains("A-"));
}

/// A header pages laid out alike share one block of, saved, then a page
/// added where it isn't, so the count is drawn again on every page it's on,
/// and the header taken off one of them, which the planner misjudges as
/// removing nothing: the others' redraw lets the shared block's words go on
/// them alone, so the removal check judges it on the page it went from too,
/// and stops the Append for what it drew there, a misclassification.
#[test]
fn a_shared_block_drawn_again_elsewhere_is_judged_where_it_went() {
    use crate::effects::{PathEffects, misjudged};

    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(6));
    let all = ids(&doc);
    let spec = fx::header("CONFIDENTIAL <<N>> pages");
    let set = fx::set_header(spec.clone(), Some(&all[..5]));
    doc.commit_ops(vec![set], "header").unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let shared: BTreeSet<ObjectId> = (page_blocks(&Pdf::load(&path).unwrap()).into_iter())
        .flatten()
        .collect();
    assert_eq!(shared.len(), 1, "one block on the five pages");
    doc.commit_ops(vec![fx::duplicate(&[all[5]])], "count")
        .unwrap();
    let narrowed = [all[0], all[1], all[3], all[4]];
    doc.commit_ops(vec![fx::set_header(spec, Some(&narrowed))], "narrow")
        .unwrap();
    fn removes_nothing(path: &mut PathEffects) {
        path.flags.remove(Flags::REMOVES | Flags::REPLACES_TEXT);
        path.removed.clear();
        path.removing.clear();
    }
    let vetoes = removal::vetoes_here();
    let report = misjudged(removes_nothing, || planned(&mut doc, &path));
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(
        matches!(&report.why[..], [Why::RemovalCheck { .. }]),
        "{:?}",
        report.why
    );
    assert_eq!(removal::vetoes_here().derived, vetoes.derived + 1);
    let pdf = Pdf::load(&path).unwrap();
    let texts: Vec<String> = pages(&pdf).into_iter().map(|p| drawn(&pdf, p)).collect();
    assert!(!texts[2].contains("CONFIDENTIAL"), "{}", texts[2]);
    assert!(
        texts[0].contains("(CONFIDENTIAL 7 pages) Tj"),
        "{}",
        texts[0]
    );
}

/// A centered footer's left margin changed after a reopen, which lays it
/// out as before but from another recipe: its blocks are drawn again, so
/// the record says the recipe they're drawn from, and a reorder after is a
/// live redraw, an Append with nothing vetoed.
#[test]
fn a_change_laid_out_alike_draws_its_blocks_again() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let mut spec = fx::header("");
    spec.texts[4] = "Page <<1>> of <<N>>".into();
    doc.commit_ops(vec![fx::set_header(spec.clone(), None)], "footer")
        .unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let (path, mut doc) = opened(&dir, "b.pdf", &std::fs::read(&path).unwrap());
    spec.margins.left += 18.0;
    doc.commit_ops(vec![fx::set_header(spec, None)], "margins")
        .unwrap();
    assert_eq!(
        planned(&mut doc, &path).strategy,
        Some(StrategyKind::Append)
    );
    let [first, .., last] = ids(&doc)[..] else {
        panic!("three pages")
    };
    doc.commit_ops(vec![fx::move_pages(&[last], Some(first))], "reorder")
        .unwrap();
    let vetoes = removal::vetoes_here();
    let report = planned(&mut doc, &path);
    assert_eq!(
        report.strategy,
        Some(StrategyKind::Append),
        "{:?}",
        report.why
    );
    assert_eq!(removal::vetoes_here(), vetoes);
    assert_eq!(said(&path), ["page 2", "page 0", "page 1"]);
    let pdf = Pdf::load(&path).unwrap();
    for (n, page) in (1..).zip(pages(&pdf)) {
        let text = drawn(&pdf, page);
        assert!(text.contains(&format!("(Page {n} of 3) Tj")), "{text}");
    }
}

/// Bates numbers its text doesn't draw, set again from another start after
/// a reopen: its blocks lay out as before but say other numbers, so they're
/// drawn again, and the file opened again reads the new ones.
#[test]
fn bates_numbers_not_drawn_are_saved_as_renumbered() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let from = |start| HeaderFooterSpec {
        bates: Some(bates("N-", 2, start)),
        ..fx::header("Draft")
    };
    doc.commit_ops(vec![fx::set_header(from(1), None)], "bates")
        .unwrap();
    planned(&mut doc, &path);
    let (path, mut doc) = opened(&dir, "b.pdf", &std::fs::read(&path).unwrap());
    assert_eq!(numbers(&doc), [Some(1), Some(2)]);
    doc.commit_ops(vec![fx::set_header(from(10), None)], "renumber")
        .unwrap();
    planned(&mut doc, &path);
    let (_, again) = opened(&dir, "c.pdf", &std::fs::read(&path).unwrap());
    assert_eq!(numbers(&again), [Some(10), Some(11)]);
}
