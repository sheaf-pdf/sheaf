//! Recognized text as a save writes it (M10.5 slice 18): each page's layer
//! a block of its own, `/SheafBlock << /K /Ocr /B (b…) >>`, drawn after
//! the page's isolated content and before the generators over it, in one
//! glyphless font a file; and the text a file holds taken out by a re-OCR
//! or a Clear, whatever shape it was written in: sheaf's blocks, one
//! another tool merged into a stream, and the layer an older sheaf wrote
//! (`text_layer::fixture`). A file's blocks are found again where they're
//! drawn, after the page's isolated content, and kept there as it's
//! edited and saved again, so nesting never grows. Slice 9's ports of the
//! old writers' layer tests are here too.

use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId};
use sheaf_edit::Turn;

use super::append_tests::{planned_in_place, written};
use super::emit::content;
use super::recognized_tests::{
    block_of, contents, depth, everything, isolates, pages, reopened, stream,
};
use super::removal;
use super::removal_tests::{opened, scratch, text_file};
use super::rewrite_tests::{make_pdf, open, read_pages, tempdir};
use super::{AskAnswers, How, SaveHow, StrategyKind, Why, save_with};
use crate::content::{Balance, BlockId, Concat, FontDecoders, Marker, ResCx, text_bag};
use crate::effects::{Removed, RemovedKind};
use crate::history::FixedCount;
use crate::model::{Document, PageId};
use crate::realize::{ViewInputs, emission_is_base, untouched};
use crate::save::rewrite;
use crate::source::Limits;
use crate::text_layer::fixture as saved;
use crate::text_layer::{OcrWord, TextLayer};
use crate::tx::fixture as fx;

/// Recognized text saying `text`, one word, on an upright Letter page.
fn ocr_layer(text: &str) -> Arc<TextLayer> {
    use crate::annot::Box2;
    Arc::new(crate::text_layer::from_engine(
        vec![OcrWord {
            text: text.into(),
            rect: Box2 {
                x: 72.0,
                y: 100.0,
                w: 100.0,
                h: 12.0,
            },
            confidence: 0.95,
        }],
        &crate::geom::Geom::read(None, None, 0),
        "test",
    ))
}

/// Page `page` of `model` given [`ocr_layer`] of `text`.
pub(crate) fn set_layer(model: &mut Document, page: PageId, text: &str) {
    model
        .commit_ops(fx::ocr(vec![(page, ocr_layer(text))]), "ocr")
        .unwrap();
}

/// The page's /Contents as an array (a lone reference becomes one entry).
fn contents_of(pdf: &Pdf, page: ObjectId) -> Vec<Object> {
    let dict = pdf.get_dictionary(page).unwrap();
    match pdf.dereference(dict.get(b"Contents").unwrap()).unwrap().1 {
        Object::Array(a) => a.clone(),
        _ => vec![dict.get(b"Contents").unwrap().clone()],
    }
}

/// The stream `entry` names, decoded, as text.
fn stream_text(pdf: &Pdf, entry: &Object) -> String {
    let s = pdf
        .get_object(entry.as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    String::from_utf8_lossy(&s.decompressed_content().unwrap()).into_owned()
}

/// The fonts page `page` of `pdf` binds.
pub(crate) fn fonts_of(pdf: &Pdf, page: ObjectId) -> Dictionary {
    // Inline on the page, or referenced (possibly inherited).
    let (res, ids) = pdf.get_page_resources(page).unwrap();
    let res: Dictionary = match res {
        Some(d) => d.clone(),
        None => pdf.get_dictionary(ids[0]).unwrap().clone(),
    };
    pdf.dereference(res.get(b"Font").unwrap())
        .unwrap()
        .1
        .as_dict()
        .unwrap()
        .clone()
}

/// Whether `font`, as resources bind it, is the glyphless Type0 a layer
/// draws in.
pub(crate) fn is_glyphless(pdf: &Pdf, font: &Object) -> bool {
    let font = pdf.dereference(font).unwrap().1.as_dict().unwrap();
    let name = |key: &[u8]| font.get(key).and_then(Object::as_name).ok();
    name(b"Subtype") == Some(b"Type0") && name(b"BaseFont") == Some(b"GlyphLessFont")
}

/// How many glyphless Type0 fonts `pdf` holds.
fn glyphless_fonts(pdf: &Pdf) -> usize {
    (pdf.objects.values())
        .filter(|o| {
            o.as_dict().is_ok_and(|d| {
                let name = |key: &[u8]| d.get(key).and_then(Object::as_name).ok();
                name(b"Subtype") == Some(b"Type0") && name(b"BaseFont") == Some(b"GlyphLessFont")
            })
        })
        .count()
}

/// The block `dict`, a stream's dictionary, marks it as recognized text's,
/// by its layer's id: `/SheafBlock << /K /Ocr /B (b…) >>`.
pub(crate) fn ocr_block(dict: &Dictionary) -> Option<BlockId> {
    let marker = dict.get(b"SheafBlock").ok()?.as_dict().ok()?;
    (marker.get(b"K").and_then(Object::as_name).ok() == Some(b"Ocr"))
        .then(|| BlockId::parse(marker.get(b"B").ok()?.as_str().ok()?))
        .flatten()
}

/// Page `page`'s streams of `pdf` that are OCR blocks.
fn ocr_blocks(pdf: &Pdf, page: ObjectId) -> Vec<ObjectId> {
    (contents(pdf, page).into_iter())
        .filter(|&c| ocr_block(&stream(pdf, c).0).is_some())
        .collect()
}

/// "Hello" as a layer writes it in the glyphless font.
pub(crate) const HELLO_HEX: &str = "<00480065006C006C006F>";

/// The words each of page `page`'s streams draws, in the order they're
/// drawn, decoded through the page's fonts ([`text_bag`]); each stream's
/// in the bag's order.
fn words(pdf: &Pdf, page: ObjectId) -> Vec<String> {
    let lim = Limits::DEFAULT;
    let resources = pdf.get_dictionary(page).unwrap().get(b"Resources").ok();
    let fonts = FontDecoders::read(pdf, resources, &lim);
    (contents(pdf, page).into_iter())
        .flat_map(|c| {
            let data = stream(pdf, c).1;
            let bag = text_bag(&Concat::new(&[&data]), &ResCx::default(), &lim, &fonts).unwrap();
            let words: Vec<String> = bag.words().map(|(w, _)| w.to_string()).collect();
            words
        })
        .collect()
}

#[test]
fn rewrite_writes_invisible_text_layer() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 3, &[]);
    let mut model = open(&a, &[], Box::new(FixedCount(10)));
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    set_layer(&mut model, ids[1], "Hello");
    let id = (model.state().def(ids[1]).unwrap().content.ocr_layer())
        .unwrap()
        .id;
    let bytes = rewrite(&model).unwrap();
    assert_eq!(
        read_pages(&bytes),
        vec![("A 0".into(), 0), ("A 1".into(), 0), ("A 2".into(), 0)]
    );
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    // The page's content isolated, then the block.
    let contents = contents_of(&pdf, pages[1]);
    assert_eq!(contents.len(), 4);
    assert_eq!(stream_text(&pdf, &contents[0]), "q\n");
    assert!(stream_text(&pdf, &contents[1]).contains("MARK A 1"));
    assert_eq!(stream_text(&pdf, &contents[2]), "Q\n");
    let (dict, _) = stream(&pdf, contents[3].as_reference().unwrap());
    assert_eq!(ocr_block(&dict), Some(id));
    let layer = stream_text(&pdf, &contents[3]);
    assert!(layer.starts_with("/ShOCR /Sh"), "{layer}");
    assert!(layer.contains(" BDC\nq\nBT\n3 Tr\n"), "{layer}");
    assert!(layer.ends_with("ET\nQ\nEMC\n"), "{layer}");
    assert!(layer.contains(HELLO_HEX), "{layer}");
    // Inherited fonts survive next to ours, in resources of the page's
    // own; its list names the block.
    let fonts = fonts_of(&pdf, pages[1]);
    assert!(fonts.has(b"F1"));
    let (_, type0) = (fonts.iter()).find(|(_, f)| is_glyphless(&pdf, f)).unwrap();
    let type0 = pdf.get_dictionary(type0.as_reference().unwrap()).unwrap();
    let cid = pdf
        .get_dictionary(
            type0.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]
                .as_reference()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(cid.get(b"DW").unwrap().as_i64().unwrap(), 500);
    assert!(type0.has(b"ToUnicode"));
    let page = pdf.get_dictionary(pages[1]).unwrap();
    let res = pdf.dereference(page.get(b"Resources").unwrap()).unwrap().1;
    let lists = res.as_dict().unwrap().get(b"Properties").unwrap();
    let lists = pdf.dereference(lists).unwrap().1.as_dict().unwrap();
    let said = (lists.iter()).map(|(_, l)| pdf.dereference(l).unwrap().1.as_dict().unwrap());
    let said: Vec<&[u8]> = said
        .map(|l| l.get(b"SheafOcrBlock").unwrap().as_str().unwrap())
        .collect();
    assert_eq!(said, [id.to_string().as_bytes()]);
    // Other pages are untouched.
    for &p in [pages[0], pages[2]].iter() {
        assert_eq!(contents_of(&pdf, p).len(), 1);
        assert!(!fonts_of(&pdf, p).iter().any(|(_, f)| is_glyphless(&pdf, f)));
    }
}

#[test]
fn an_append_writes_a_layer_in_an_unchanged_order() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let original = std::fs::read(&a).unwrap();
    let mut model = open(&a, &[], Box::new(FixedCount(10)));
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    set_layer(&mut model, ids[0], "Hello");
    let bytes = How::Appended.write(&model);
    assert!(bytes.starts_with(&original), "original bytes preserved");
    assert_eq!(
        read_pages(&bytes),
        vec![("A 0".into(), 0), ("A 1".into(), 0)]
    );
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let contents = contents_of(&pdf, pages[0]);
    assert_eq!(contents.len(), 4);
    assert!(stream_text(&pdf, &contents[3]).contains(HELLO_HEX));
    // The rewritten page pulled its inherited attributes down, its
    // resources a dictionary of its own.
    let dict = pdf.get_dictionary(pages[0]).unwrap();
    assert!(dict.has(b"MediaBox"));
    assert!(dict.get(b"Resources").unwrap().as_reference().is_ok());
    let fonts = fonts_of(&pdf, pages[0]);
    assert!(fonts.has(b"F1") && fonts.iter().any(|(_, f)| is_glyphless(&pdf, f)));
    assert!(!pdf.get_dictionary(pages[1]).unwrap().has(b"Resources"));
    assert_eq!(glyphless_fonts(&pdf), 1);
    // Saved in place, as planned (a layer added removes nothing), the file
    // reopens with the layer.
    let report = save_with(
        &mut model,
        &a,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    );
    assert_eq!(report.unwrap().strategy, Some(StrategyKind::Append));
    let saved = Pdf::load(&a).unwrap();
    let p0 = saved.get_pages()[&1];
    assert_eq!(contents_of(&saved, p0).len(), 4);
}

#[test]
fn an_append_writes_a_layer_in_a_changed_tree() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 3, &[]);
    let original = std::fs::read(&a).unwrap();
    let mut model = open(&a, &[], Box::new(FixedCount(10)));
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    set_layer(&mut model, ids[2], "Hello");
    model
        .commit_ops(vec![fx::delete(&[ids[0]])], "del")
        .unwrap();
    let bytes = How::Appended.write(&model);
    assert!(bytes.starts_with(&original));
    assert_eq!(
        read_pages(&bytes),
        vec![("A 1".into(), 0), ("A 2".into(), 0)]
    );
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    assert_eq!(contents_of(&pdf, pages[0]).len(), 1);
    let contents = contents_of(&pdf, pages[1]);
    assert_eq!(contents.len(), 4);
    assert!(stream_text(&pdf, &contents[3]).contains(HELLO_HEX));
    assert!(fonts_of(&pdf, pages[1]).has(b"F1"));
}

/// Layers on every page draw in one font and are isolated by one `q` and
/// one `Q`, each written once a file; each page's block is its own.
#[test]
fn ocr_pages_share_one_font() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 3, &[]);
    let mut model = open(&a, &[], Box::new(FixedCount(10)));
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    model
        .commit_ops(
            fx::ocr(vec![
                (ids[0], ocr_layer("one")),
                (ids[1], ocr_layer("two")),
                (ids[2], ocr_layer("three")),
            ]),
            "ocr",
        )
        .unwrap();
    let bytes = rewrite(&model).unwrap();
    let pdf = Pdf::load_mem(&bytes).unwrap();
    assert_eq!(glyphless_fonts(&pdf), 1);
    let pages = pages(&pdf);
    let font = |page: ObjectId| {
        let fonts = fonts_of(&pdf, page);
        let (_, font) = (fonts.iter()).find(|(_, f)| is_glyphless(&pdf, f)).unwrap();
        font.clone()
    };
    assert!(pages.iter().all(|&p| font(p) == font(pages[0])));
    let isolation =
        |at: usize| -> Vec<ObjectId> { (pages.iter()).map(|&p| contents(&pdf, p)[at]).collect() };
    for shared in [isolation(0), isolation(2)] {
        assert!(shared.iter().all(|&s| s == shared[0]), "{shared:?}");
    }
    let blocks: std::collections::BTreeSet<ObjectId> =
        pages.iter().flat_map(|&p| ocr_blocks(&pdf, p)).collect();
    assert_eq!(blocks.len(), 3);
}

/// A page duplicated with its layer draws the same block, its layer's id
/// shared, turned once with it.
#[test]
fn duplicate_shares_layer_and_is_not_double_rotated() {
    let dir = tempdir();
    let a = make_pdf(&dir, "a.pdf", "A", 2, &[]);
    let mut model = open(&a, &[], Box::new(FixedCount(10)));
    let ids: Vec<PageId> = model.state().order().iter().copied().collect();
    model
        .commit_ops(vec![fx::rotate(&[ids[0]], Turn::Quarter)], "rot")
        .unwrap();
    set_layer(&mut model, ids[0], "Hello");
    model
        .commit_ops(vec![fx::duplicate(&[ids[0]])], "dup")
        .unwrap();
    let bytes = rewrite(&model).unwrap();
    assert_eq!(
        read_pages(&bytes),
        vec![("A 0".into(), 90), ("A 0".into(), 90), ("A 1".into(), 0)]
    );
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    for &p in &pages[..2] {
        let contents = contents_of(&pdf, p);
        assert_eq!(contents.len(), 4);
        assert!(stream_text(&pdf, &contents[3]).contains(HELLO_HEX));
    }
    assert_eq!(ocr_blocks(&pdf, pages[0]), ocr_blocks(&pdf, pages[1]));
}

crate::both_ways!(
    an_empty_layer_writes_nothing,
    an_empty_layer_writes_nothing_appended: empty_layer_writes_nothing
);

/// A layer with no words is none: nothing is drawn, no font written, and
/// a save as planned finds nothing to write.
fn empty_layer_writes_nothing(how: How) {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let p0 = doc.page(0).unwrap();
    let empty = Arc::new(crate::text_layer::from_engine(
        vec![],
        &crate::geom::Geom::read(None, None, 0),
        "test",
    ));
    let was = doc.current_version();
    doc.commit_ops(fx::ocr(vec![(p0, empty)]), "ocr").unwrap();
    assert_eq!(doc.current_version(), was, "no version");
    let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
    let page = pages(&pdf)[0];
    assert_eq!(contents(&pdf, page).len(), 1);
    assert_eq!(glyphless_fonts(&pdf), 0);
    let report = save_with(
        &mut doc,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, None, "{how:?}: nothing to write");
}

/// Three pages recognized and saved by an Append: the section holds a
/// block of each page's text, the `q` and the `Q` that isolate the pages'
/// content once each, and the glyphless font once.
#[test]
fn ocr_appends_one_block_a_page_and_one_font() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let layers: Vec<_> = (0..3)
        .map(|i| (doc.page(i).unwrap(), ocr_layer(&format!("word{i}"))))
        .collect();
    doc.commit_ops(fx::ocr(layers), "ocr").unwrap();
    let (was, section) = planned_in_place(&mut doc);
    let pdf = Pdf::load(&path).unwrap();
    let new = written(was.len(), &section, &path);
    let streams: Vec<Dictionary> = (new.iter())
        .filter_map(|&id| Some(pdf.get_object(id).ok()?.as_stream().ok()?.dict.clone()))
        .collect();
    let blocks = streams.iter().filter(|d| ocr_block(d).is_some()).count();
    assert_eq!(blocks, 3);
    let isolating: Vec<Vec<u8>> = (new.iter())
        .filter(|&&id| {
            let (dict, _) = stream_of(&pdf, id).unwrap_or_default();
            let marker = dict.get(b"SheafBlock").and_then(Object::as_dict);
            marker.is_ok_and(|m| m.get(b"K").and_then(Object::as_name).ok() == Some(b"Isolate"))
        })
        .map(|&id| stream(&pdf, id).1)
        .collect();
    assert_eq!(isolating.len(), 2, "{isolating:?}");
    assert!(isolating.contains(&b"q\n".to_vec()) && isolating.contains(&b"Q\n".to_vec()));
    let fonts = (new.iter())
        .filter(|&&id| {
            let font = Object::Reference(id);
            pdf.get_dictionary(id).is_ok() && is_glyphless(&pdf, &font)
        })
        .count();
    assert_eq!(fonts, 1);
}

/// Stream `id` of `pdf`, if it's one: its dictionary and data decoded.
fn stream_of(pdf: &Pdf, id: ObjectId) -> Option<(Dictionary, Vec<u8>)> {
    pdf.get_object(id).ok()?.as_stream().ok()?;
    Some(stream(pdf, id))
}

/// The files whose page holds recognized text sheaf wrote, each with its
/// name: an older sheaf's layer, a block of sheaf's in a stream of its
/// own, and a block of [`saved::layer`] this writer drew and saved (on
/// a page saying "page 0").
fn saved_text() -> Vec<(&'static str, Vec<u8>)> {
    let bytes = |mut pdf: Pdf| {
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        bytes
    };
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "ours.pdf", &text_file(1));
    let page = doc.page(0).unwrap();
    doc.commit_ops(fx::ocr([(page, Arc::new(saved::layer()))]), "ocr")
        .unwrap();
    let ours = rewrite(&doc).unwrap();
    vec![
        ("legacy", bytes(saved::legacy_page())),
        ("block", bytes(saved::block_page())),
        ("ours", ours),
    ]
}

/// What a save that took one page's recognized text out says why it
/// rewrote.
fn replaced_text() -> Vec<Why> {
    let removed = Removed {
        kind: RemovedKind::OcrLayers,
        count: 1,
    };
    vec![Why::ReplacesText(vec![removed])]
}

/// Whether the file `pdf` says the words of [`saved::layer`] anywhere: in
/// a string, a name or a stream's data, decoded.
fn says_old_words(pdf: &Pdf) -> bool {
    let all = everything(pdf);
    saved::WORDS_HEX.iter().any(|w| all.contains(w))
}

/// A layer a file holds, recognized over and saved as planned: a Rewrite,
/// for the text it replaces, after which the old words are nowhere in the
/// file and the page draws its own content and one block, the new one's.
/// Over an older sheaf's layer, a block of sheaf's, and a block this
/// writer saved, which saved again with nothing changed stays as it is.
#[test]
fn replacing_a_saved_layer_rewrites_without_the_old_words() {
    for (name, bytes) in saved_text() {
        let dir = scratch();
        let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
        let p0 = doc.page(0).unwrap();
        doc.commit_ops(fx::ocr([(p0, ocr_layer("Fresh"))]), "ocr")
            .unwrap();
        let id = (doc.state().def(p0).unwrap().content.ocr_layer())
            .unwrap()
            .id;
        let report = save_with(
            &mut doc,
            &path,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Rewrite), "{name}");
        assert_eq!(report.why, replaced_text(), "{name}");
        let pdf = Pdf::load(&path).unwrap();
        assert!(!says_old_words(&pdf), "{name}");
        let page = pages(&pdf)[0];
        let blocks = ocr_blocks(&pdf, page);
        assert_eq!(blocks.len(), 1, "{name}");
        assert_eq!(ocr_block(&stream(&pdf, blocks[0]).0), Some(id), "{name}");
        let said = words(&pdf, page);
        assert!(said.contains(&"Fresh".to_string()), "{name}: {said:?}");
        assert!(!saved::WORDS.iter().any(|w| said.contains(&w.to_string())));
        assert_eq!(depth(&pdf, page).1, Balance::default(), "{name}");
        // Opened again, it's what's saved: the block stays.
        let (_, doc) = opened(&dir, "b.pdf", &std::fs::read(&path).unwrap());
        let again = Pdf::load_mem(&rewrite(&doc).unwrap()).unwrap();
        assert_eq!(ocr_blocks(&again, pages(&again)[0]).len(), 1, "{name}");
    }
}

/// A layer a file holds, cleared and saved as planned: a Rewrite, for the
/// text it takes out, after which its words are nowhere in the file and
/// the page draws its own content as it was, sheaf's isolation set aside.
/// Over an older sheaf's layer, a block of sheaf's, one this writer saved,
/// and a block another tool merged into the page's one stream, cut out of
/// it, the rest of the stream as it was and the names only the block drew
/// with gone from the page's resources.
#[test]
fn clear_removes_a_saved_layer() {
    let mut merged = Vec::new();
    saved::merged_page().save_to(&mut merged).unwrap();
    let mut files = saved_text();
    files.push(("merged", merged));
    for (name, bytes) in files {
        let dir = scratch();
        let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
        let p0 = doc.page(0).unwrap();
        doc.commit_ops(vec![fx::clear_ocr(&[p0])], "clear").unwrap();
        let report = save_with(
            &mut doc,
            &path,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        )
        .unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Rewrite), "{name}");
        assert_eq!(report.why, replaced_text(), "{name}");
        let pdf = Pdf::load(&path).unwrap();
        assert!(!says_old_words(&pdf), "{name}");
        let page = pages(&pdf)[0];
        let streams = contents(&pdf, page);
        let [own] = streams[..] else {
            panic!("{name}: {streams:?}")
        };
        let data = stream(&pdf, own).1;
        match name {
            "ours" => assert_eq!(words(&pdf, page), ["0", "page"]),
            // What another tool merged around the block, byte for byte.
            "merged" => assert_eq!(data, [saved::BASE, b"\n"].concat()),
            _ => assert_eq!(data, saved::BASE, "{name}"),
        }
        let fonts = fonts_of(&pdf, page);
        assert!(!fonts.iter().any(|(_, f)| is_glyphless(&pdf, f)), "{name}");
        let res = pdf.get_dictionary(page).unwrap().get(b"Resources").unwrap();
        let res = pdf.dereference(res).unwrap().1.as_dict().unwrap();
        let lists = res.get(b"Properties").ok();
        let lists = lists.map(|l| pdf.dereference(l).unwrap().1.as_dict().unwrap());
        assert!(lists.is_none_or(|l| !l.has(saved::OCR_LIST)), "{name}");
    }
}

crate::both_ways!(
    removing_a_legacy_layer_keeps_q_and_q_balanced,
    removing_a_legacy_layer_keeps_q_and_q_balanced_appended: legacy_balance
);

/// An older sheaf's layer, whose page's own content leaves a `q` open,
/// taken out with the `q` before the content that its first `Q` ends:
/// cleared, the page balances as its own content does; recognized again,
/// or cleared under a header, the page's own content is isolated as its
/// balance asks, by one `q` and two `Q`s, and the page balances.
fn legacy_balance(how: How) {
    let base = Balance {
        net: 1,
        min_prefix: 0,
    };
    for what in ["cleared", "recognized", "cleared under a header"] {
        let dir = scratch();
        let mut bytes = Vec::new();
        saved::legacy_page().save_to(&mut bytes).unwrap();
        let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
        let p0 = doc.page(0).unwrap();
        let ops = match what {
            "cleared" => vec![fx::clear_ocr(&[p0])],
            "recognized" => fx::ocr([(p0, ocr_layer("Fresh"))]),
            _ => vec![
                fx::clear_ocr(&[p0]),
                fx::set_header(fx::header("Draft"), None),
            ],
        };
        doc.commit_ops(ops, what).unwrap();
        let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
        let page = pages(&pdf)[0];
        let streams = contents(&pdf, page);
        let data = |at: usize| stream(&pdf, streams[at]).1;
        if what == "cleared" {
            assert_eq!(streams.len(), 1, "{what} {how:?}");
            assert_eq!(depth(&pdf, page).1, base, "{what} {how:?}");
            continue;
        }
        assert_eq!(streams.len(), 4, "{what} {how:?}");
        assert_eq!((data(0), data(2)), (b"q\n".to_vec(), b"Q\nQ\n".to_vec()));
        assert_eq!(data(1), saved::BASE, "{what} {how:?}");
        assert_eq!(depth(&pdf, page).1, Balance::default(), "{what} {how:?}");
    }
}

/// The page's own text reads first, then the recognized words, then the
/// header (§11.4): its streams are its content, isolated, then the text's
/// block, then the header's.
#[test]
fn recognized_words_read_between_the_page_s_text_and_the_header() {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let p0 = doc.page(0).unwrap();
    let mut ops = fx::ocr([(p0, Arc::new(saved::layer()))]);
    ops.push(fx::set_header(fx::header("Draft"), None));
    doc.commit_ops(ops, "ocr and header").unwrap();
    for how in [How::Rewritten, How::Appended] {
        let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
        let page = pages(&pdf)[0];
        assert_eq!(
            words(&pdf, page),
            ["0", "page", "Saved", "words", "Draft"],
            "{how:?}"
        );
        let streams = contents(&pdf, page);
        assert!(ocr_block(&stream(&pdf, streams[3]).0).is_some(), "{how:?}");
    }
}

/// A layer on a page whose content can't be read is drawn as nothing else
/// is there: the page as it is, with no isolation, and a note says so.
#[test]
fn a_page_that_cant_be_read_is_saved_without_its_text() {
    let mut pdf = Pdf::load_mem(&text_file(1)).unwrap();
    let page = pdf.get_pages()[&1];
    let content = pdf.get_dictionary(page).unwrap().get(b"Contents").unwrap();
    let content = content.as_reference().unwrap();
    let bad = lopdf::Stream::new(Dictionary::new(), b"0 0 9 9 re f EMC".to_vec());
    pdf.objects.insert(content, Object::Stream(bad));
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let dir = scratch();
    let (_, doc) = opened(&dir, "a.pdf", &bytes);
    // No op puts a layer on such a page: one is there all the same.
    let p0 = doc.page(0).unwrap();
    let mut snap = doc.snapshot();
    let mut rec = crate::prim::Rec::new(snap.state.clone());
    let def = snap.state.def(p0).unwrap();
    let layer = crate::model::Layer {
        id: BlockId(7),
        body: crate::model::Static::OcrText(ocr_layer("Hidden")),
    };
    let content = def.content.with_ocr(Some(layer));
    let def = crate::model::PageDef::new(def.base.clone(), def.copy, content);
    let page = Some(crate::prim::Val::Page(Arc::new(def)));
    rec.put(crate::prim::Key::Page(p0), page);
    snap.state = rec.finish().0;
    let input = super::SaveInput::detached(&snap);
    let target = super::tests::target(super::Profile::default());
    let plan = super::plan::forced_rewrite(&input, &target);
    let out = super::materialize(&input, &plan, target, &NoProgress).unwrap();
    let notes: Vec<String> = out.notes.iter().map(|n| n.to_string()).collect();
    let note = "No recognized text, watermark, background, header or footer is drawn on page \
                1, whose content can't be read.";
    assert!(notes.iter().any(|n| n == note), "{notes:?}");
    let pdf = Pdf::load_mem(&out.bytes).unwrap();
    let page = pages(&pdf)[0];
    assert_eq!(contents(&pdf, page).len(), 1);
    assert_eq!(glyphless_fonts(&pdf), 0);
}

/// An older sheaf's layer with a second layer stream after it, as no
/// sheaf wrote: the `q` and the last layer go, as a pair; the other
/// layer's stream, its `Q` then the page's own, stays, and a note says
/// so.
#[test]
fn a_legacy_layer_without_its_q_stays() {
    let mut pdf = saved::legacy_page();
    let id = pdf.get_pages()[&1];
    let [q, base, layer] = crate::content::contents(pdf.get_dictionary(id).unwrap())[..] else {
        panic!("the legacy page draws three streams")
    };
    let copy = pdf.get_object(layer).unwrap().clone();
    let second = pdf.add_object(copy);
    let entries: Vec<Object> = [q, base, layer, second].map(Object::Reference).into();
    pdf.get_dictionary_mut(id).unwrap().set("Contents", entries);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    let commit = doc.commit_ops(vec![fx::clear_ocr(&[p0])], "clear").unwrap();
    // The save says it, not the commit as well.
    assert_eq!(commit.notices, []);
    let report = save_with(
        &mut doc,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    let notes: Vec<String> = report.notes.iter().map(|n| n.to_string()).collect();
    let note = "Recognized text sheaf wrote earlier stays on page 1: taking it out would change \
                what else the page draws.";
    assert!(notes.iter().any(|n| n == note), "{notes:?}");
    let pdf = Pdf::load(&path).unwrap();
    let page = pages(&pdf)[0];
    let streams = contents(&pdf, page);
    assert_eq!(streams.len(), 2);
    assert_eq!(stream(&pdf, streams[0]).1, saved::BASE);
    assert_eq!(stream(&pdf, streams[1]).1, saved::LEGACY_LAYER);
    assert_eq!(depth(&pdf, page).1, Balance::default());
}

/// An older sheaf's layer whose `q` another tool lost, the only text of
/// its kind on the page: a Clear can't take it out, so it changes nothing,
/// and says the layer stays, as a save says of one beside text it takes
/// out. Recognizing the page again says so too. A save writes it as it
/// was.
#[test]
fn a_lone_legacy_layer_without_its_q_stays_and_says_so() {
    let mut pdf = saved::legacy_page();
    let id = pdf.get_pages()[&1];
    let [_, base, layer] = crate::content::contents(pdf.get_dictionary(id).unwrap())[..] else {
        panic!("the legacy page draws three streams")
    };
    let entries: Vec<Object> = [base, layer].map(Object::Reference).into();
    pdf.get_dictionary_mut(id).unwrap().set("Contents", entries);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    let stays = "Recognized text sheaf wrote earlier stays on page 1: taking it out would change \
                 what else the page draws";
    let commit = doc.commit_ops(vec![fx::clear_ocr(&[p0])], "clear").unwrap();
    assert_eq!(commit.version, None, "nothing changed");
    assert_eq!(crate::notice_text(&commit.notices).as_deref(), Some(stays));
    let set = fx::ocr([(p0, ocr_layer("again"))]);
    let commit = doc.commit_ops(set, "ocr").unwrap();
    assert!(commit.version.is_some());
    assert_eq!(crate::notice_text(&commit.notices).as_deref(), Some(stays));
    doc.undo().unwrap();
    save_with(
        &mut doc,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    let pdf = Pdf::load(&path).unwrap();
    let page = pages(&pdf)[0];
    let streams = contents(&pdf, page);
    assert_eq!(streams.len(), 2);
    assert_eq!(stream(&pdf, streams[0]).1, saved::BASE);
    assert_eq!(stream(&pdf, streams[1]).1, saved::LEGACY_LAYER);
}

/// What each of page `page`'s streams of `pdf` is, by its marker:
/// `Isolate`, `Ocr` or `Gen`, or `-`, the page's own.
fn kinds(pdf: &Pdf, page: ObjectId) -> Vec<&'static str> {
    (contents(pdf, page).into_iter())
        .map(|c| {
            if isolates(pdf, c) {
                "Isolate"
            } else if ocr_block(&stream(pdf, c).0).is_some() {
                "Ocr"
            } else if block_of(pdf, c).is_some() {
                "Gen"
            } else {
                "-"
            }
        })
        .collect()
}

/// A file of [`text_file`]'s two pages, both recognized
/// ([`saved::layer`]), the first under a header saying "Draft", written
/// `how`: its bytes, and the first page's layer's id.
fn recognized_and_headed(dir: &std::path::Path, how: How) -> (Vec<u8>, BlockId) {
    let (_, mut doc) = opened(dir, "a.pdf", &text_file(2));
    let [p0, p1] = [0, 1].map(|i| doc.page(i).unwrap());
    let layer = Arc::new(saved::layer());
    let mut ops = fx::ocr([(p0, layer.clone()), (p1, layer)]);
    ops.push(fx::set_header(fx::header("Draft"), Some(&[p0])));
    doc.commit_ops(ops, "ocr and header").unwrap();
    let id = (doc.state().def(p0).unwrap().content.ocr_layer())
        .unwrap()
        .id;
    (how.write(&doc), id)
}

crate::both_ways!(
    a_reopened_ocr_page_is_intact,
    a_reopened_ocr_page_is_intact_appended: reopened_intact
);

/// Pages saved with recognized text, one under a header, hold them where
/// they're drawn: their own content isolated, then the text's block, then
/// the header's. Opened again, each block is found by its marker, and
/// each page is intact, with the header on it or with none: the realizer
/// renders it from its base page under the screen's inputs, and under
/// every consumer's says what an overlay of it alone writes; and a save
/// writes its content as it is.
fn reopened_intact(how: How) {
    let dir = scratch();
    let (bytes, id) = recognized_and_headed(&dir, how);
    let (_, again) = opened(&dir, "b.pdf", &bytes);
    let file = Pdf::load_mem(&bytes).unwrap();
    let [first, second] = pages(&file)[..] else {
        panic!("two pages")
    };
    assert_eq!(
        kinds(&file, first),
        ["Isolate", "-", "Isolate", "Ocr", "Gen"]
    );
    assert_eq!(kinds(&file, second), ["Isolate", "-", "Isolate", "Ocr"]);
    let src = again.primary();
    let text = contents(&file, first)[3];
    assert!(src.markers(0).contains(&(text, Marker::Ocr(id))), "{how:?}");
    let st = again.state();
    let screen = ViewInputs::screen();
    for (page, def, geom) in st.pages() {
        let index = def.base.index;
        let facts = src.page(index).unwrap();
        let on = content::generators_on(st, page);
        let intact = content::intact(st, page, &src.markers(index), &facts.contents, &on, |g| {
            content::model_layout_digest(st, g, page, geom)
        });
        assert!(intact, "{how:?}: page {index}");
        assert!(untouched(st, page, &screen), "{how:?}: page {index}");
        for inputs in crate::realize::fixture::every_input() {
            let emitted = emission_is_base(st, page, &inputs).unwrap();
            let at = format!("{how:?}: page {index} under {inputs:?}");
            assert_eq!(untouched(st, page, &inputs), emitted, "{at}");
        }
    }
    let written = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    for (a, b) in pages(&file).into_iter().zip(pages(&written)) {
        assert_eq!(contents(&file, a), contents(&written, b), "{how:?}");
    }
}

crate::both_ways!(
    nesting_never_grows,
    nesting_never_grows_appended: five_cycles
);

/// Five times opened, given a header saying something else and saved
/// `how`, a page holding sheaf's recognized text over its own content,
/// which leaves a `q` open ([`saved::block_page`], whose catalog names no
/// generator), keeps one stream of `q`s and one of `Q`s around that
/// content, as many as its balance asks, then the text's block as it was,
/// then the header's: nesting never grows, and the text reads where it
/// did.
fn five_cycles(how: How) {
    let dir = scratch();
    let mut bytes = Vec::new();
    saved::block_page().save_to(&mut bytes).unwrap();
    for cycle in 0..5 {
        let (_, mut doc) = opened(&dir, &format!("{cycle}.pdf"), &bytes);
        let said = format!("Cycle {cycle}");
        let set = fx::set_header(fx::header(&said), None);
        doc.commit_ops(vec![set], "header").unwrap();
        bytes = how.write(&doc);
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let page = pages(&pdf)[0];
        let at = format!("{how:?} {cycle}");
        assert_eq!(
            kinds(&pdf, page),
            ["Isolate", "-", "Isolate", "Ocr", "Gen"],
            "{at}"
        );
        let streams = contents(&pdf, page);
        let data = |at: usize| stream(&pdf, streams[at]).1;
        assert_eq!(data(0), b"q\n", "{at}");
        assert_eq!(data(1), saved::BASE, "{at}");
        assert_eq!(data(2), b"Q\nQ\n", "{at}");
        assert_eq!(data(3), saved::OCR_BLOCK, "{at}");
        assert_eq!(depth(&pdf, page), (2, Balance::default()), "{at}");
        let number = cycle.to_string();
        let want = ["Body", "text", "Saved", "words", &number, "Cycle"];
        assert_eq!(words(&pdf, page), want, "{at}");
    }
}

crate::both_ways!(
    an_ocr_only_file_gets_a_header_later,
    an_ocr_only_file_gets_a_header_later_appended: header_later
);

/// A file saved with recognized text and no generator, whose catalog so
/// names none, given a header when it's opened again and saved `how`:
/// its page's own content is isolated once, by the streams it was, then
/// the text's block, the file's own, then the header's.
fn header_later(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let p0 = doc.page(0).unwrap();
    doc.commit_ops(fx::ocr([(p0, Arc::new(saved::layer()))]), "ocr")
        .unwrap();
    let (_, mut again) = reopened(&dir, "b.pdf", &doc, how);
    assert!(!again.primary().marked(), "{how:?}");
    let was = Pdf::load_mem(again.primary().bytes()).unwrap();
    let was = contents(&was, pages(&was)[0]);
    again
        .commit_ops(vec![fx::set_header(fx::header("Draft"), None)], "header")
        .unwrap();
    let pdf = Pdf::load_mem(&how.write(&again)).unwrap();
    let page = pages(&pdf)[0];
    let kinds = kinds(&pdf, page);
    assert_eq!(kinds, ["Isolate", "-", "Isolate", "Ocr", "Gen"], "{how:?}");
    assert_eq!(contents(&pdf, page)[..4], was[..], "{how:?}");
    assert_eq!(depth(&pdf, page), (1, Balance::default()), "{how:?}");
    let want = ["0", "page", "Saved", "words", "Draft"];
    assert_eq!(words(&pdf, page), want, "{how:?}");
}

/// A header laid out otherwise over a page of a reopened file that holds
/// recognized text, saved as planned: an Append, which rebuilds the page
/// around its own content and keeps the text's block, the file's own
/// object, after it. The streams that isolated the content, which the
/// file has deeper than its balance asks, are left out, judged on what
/// they hold, `q`s and `Q`s alone: nothing is vetoed.
#[test]
fn an_append_over_a_saved_block_vetoes_nothing() {
    let dir = scratch();
    let (bytes, _) = recognized_and_headed(&dir, How::Rewritten);
    let mut pdf = Pdf::load_mem(&bytes).unwrap();
    let page = pages(&pdf)[0];
    let was = contents(&pdf, page);
    for (at, ops) in [(0, b"q\nq\n"), (2, b"Q\nQ\n")] {
        let mut isolating = pdf
            .get_object(was[at])
            .unwrap()
            .as_stream()
            .unwrap()
            .clone();
        isolating.dict.remove(b"Filter");
        isolating.set_content(ops.to_vec());
        pdf.objects.insert(was[at], isolating.into());
    }
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (path, mut doc) = opened(&dir, "b.pdf", &bytes);
    let mut bigger = fx::header("Draft");
    bigger.size = 12.0;
    let set = fx::set_header(bigger, Some(&[doc.page(0).unwrap()]));
    doc.commit_ops(vec![set], "header").unwrap();
    let before = removal::vetoes_here();
    let report = save_with(
        &mut doc,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    assert_eq!(removal::vetoes_here(), before);
    let pdf = Pdf::load(&path).unwrap();
    let now = contents(&pdf, page);
    assert_eq!(kinds(&pdf, page), ["Isolate", "-", "Isolate", "Ocr", "Gen"]);
    assert_eq!((now[1], now[3]), (was[1], was[3]));
    assert!(!now.contains(&was[0]) && !now.contains(&was[2]), "{now:?}");
    assert_eq!(depth(&pdf, page), (1, Balance::default()));
}
