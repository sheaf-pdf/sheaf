//! Recognized text as `SetOcrText` and `ClearOcrText` lower it (M10.5
//! slice 18): a layer with a block id of its own, which a layer replacing
//! it unsaved keeps; the recognized text sheaf wrote in the file filtered
//! out, whatever shape it was written in; and a page whose content can't
//! be read, or cut as the filter would, failing the op.

use std::collections::BTreeSet;

use in_progress::NoProgress;
use lopdf::Document as Pdf;

use super::fixture as fx;
use super::stale_tests::{on, planned};
use super::tests::{commit, doc_of, opened};
use super::*;
use crate::content::BlockId;
use crate::effects::{Flags, RemovedKind, judge};
use crate::model::{ContentStack, Document, Filter, MarkedSel, StreamSel};
use crate::text_layer::fixture as saved;
use crate::text_layer::{OcrWord, TextLayer};

/// Recognized text saying `text`.
fn layer(text: &str) -> Arc<TextLayer> {
    let word = OcrWord {
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

/// Recognized text of no words.
fn empty() -> Arc<TextLayer> {
    Arc::new(crate::text_layer::from_engine(
        Vec::new(),
        &crate::geom::Geom::read(None, None, 0),
        "test",
    ))
}

/// `pdf` opened, as a file named `name` is.
fn open(mut pdf: Pdf, name: &str) -> Document {
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    opened(bytes, name)
}

/// Page `page`'s filters.
fn filters(d: &Document, page: PageId) -> Vec<Filter> {
    let def = d.state().def(page).unwrap();
    def.content.filters.iter().cloned().collect()
}

/// Page `page`'s recognized text, by its words.
fn words(d: &Document, page: PageId) -> Option<Vec<String>> {
    let text = d.state().ocr_text(page)?;
    Some(text.words.iter().map(|w| w.text.clone()).collect())
}

/// Page `page`'s layer's block id.
fn block(d: &Document, page: PageId) -> Option<BlockId> {
    let def = d.state().def(page)?;
    def.content.ocr_layer().map(|l| l.id)
}

/// The recognized text a file holds, in each shape sheaf wrote it, is
/// filtered out by a re-OCR and by Clear: a block in a stream of its own
/// dropped whole, a block merged into
/// another stream cut out. Either is replaced text, once a page; set again
/// or cleared again, the page keeps the one filter.
#[test]
fn set_and_clear_filter_out_the_text_a_file_holds() {
    let streams = Filter::DropStreams(StreamSel::SheafOcr);
    let merged = Filter::DropMarked(MarkedSel::SheafOcr);
    let shapes = [
        (saved::block_page as fn() -> Pdf, &streams),
        (saved::merged_page, &merged),
    ];
    let replaced = Flags::CONTENT | Flags::REMOVES | Flags::REPLACES_TEXT;
    for (pdf, filter) in shapes {
        let mut d = open(pdf(), "saved.pdf");
        let page = d.page(0).unwrap();
        let before = d.state().clone();
        let c = commit(&mut d, Actor::User, fx::ocr([(page, layer("new"))])).unwrap();
        assert_eq!(filters(&d, page), std::slice::from_ref(filter));
        assert_eq!(words(&d, page), Some(vec!["new".into()]));
        assert_eq!(c.effects.effects.flags, replaced, "{filter:?}");
        let keys = BTreeSet::from([Key::Page(page)]);
        let path = judge(&before, d.state(), &keys);
        let removed: Vec<(RemovedKind, usize)> =
            path.removed.iter().map(|r| (r.kind, r.count)).collect();
        assert_eq!(removed, [(RemovedKind::OcrLayers, 1)]);
        commit(&mut d, Actor::User, fx::ocr([(page, layer("again"))])).unwrap();
        assert_eq!(filters(&d, page), std::slice::from_ref(filter));
        commit(&mut d, Actor::User, vec![fx::clear_ocr(&[page])]).unwrap();
        assert_eq!(
            (filters(&d, page), words(&d, page)),
            (vec![filter.clone()], None)
        );

        // Cleared at once.
        let mut d = open(pdf(), "saved.pdf");
        let page = d.page(0).unwrap();
        let c = commit(&mut d, Actor::User, vec![fx::clear_ocr(&[page])]).unwrap();
        assert_eq!(
            (filters(&d, page), words(&d, page)),
            (vec![filter.clone()], None)
        );
        assert_eq!(c.effects.effects.flags, replaced);
        assert_eq!(c.lines[0].text, "Remove recognized text from page 1");
    }
}

/// An unsaved layer is taken off by Clear with no filter, the page
/// sharing the empty stack again; a page with no recognized text, or an
/// empty layer on one, makes no version.
#[test]
fn clear_takes_an_unsaved_layer_off_and_nothing_is_no_change() {
    let mut d = doc_of(2, &[]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    commit(&mut d, Actor::User, fx::ocr([(p0, layer("a"))])).unwrap();
    let c = commit(&mut d, Actor::User, vec![fx::clear_ocr(&[p0])]).unwrap();
    assert!(ContentStack::is_shared_empty(
        &d.state().def(p0).unwrap().content
    ));
    let flags = Flags::CONTENT | Flags::REMOVES | Flags::REPLACES_TEXT;
    assert_eq!(c.effects.effects.flags, flags);
    let n = d.version_count();
    for ops in [vec![fx::clear_ocr(&[p0, p1])], fx::ocr([(p1, empty())])] {
        let c = commit(&mut d, Actor::User, ops).unwrap();
        assert_eq!((c.version, d.version_count()), (None, n));
    }
    // Over an unsaved layer, an empty one takes it off.
    commit(&mut d, Actor::User, fx::ocr([(p1, layer("b"))])).unwrap();
    commit(&mut d, Actor::User, fx::ocr([(p1, empty())])).unwrap();
    assert!(ContentStack::is_shared_empty(
        &d.state().def(p1).unwrap().content
    ));
}

/// A page whose content can't be read fails Set and Clear, naming it as
/// the document counts it, and so does one whose merged block a splice
/// can't take out alone; nothing changes.
#[test]
fn a_page_whose_content_can_t_be_cut_fails_the_op() {
    // Its text state set outside a `q`: cut, it would stay set for what's
    // drawn after.
    let unsafe_block: &[u8] = b"/ShOCR << /SHEF_OcrBlock (b2a) >> BDC BT 3 Tr ET EMC 0 0 9 9 re f";
    let contents: [&[u8]; 3] = [b"0 0 9 9 re f", b"BT (never ends Tj ET", unsafe_block];
    let mut d = open(saved::pages_drawing(&contents), "pages.pdf");
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let (at, st, n) = (d.current_version(), d.state().clone(), d.version_count());
    let cases = [
        (fx::ocr([(p1, layer("a"))]), "set-ocr-text", 1),
        (vec![fx::clear_ocr(&[p0, p1])], "clear-ocr-text", 1),
        (fx::ocr([(p2, layer("a"))]), "set-ocr-text", 2),
        (vec![fx::clear_ocr(&[p2])], "clear-ocr-text", 2),
    ];
    for (ops, name, pos) in cases {
        let e = commit(&mut d, Actor::User, ops).unwrap_err();
        let EditError::Content { op, page, why } = &e else {
            panic!("{e}");
        };
        assert_eq!((*op, *page), (name, pos), "{e}");
        let said = match pos {
            1 => "page 2's content can't be read; nothing was changed",
            _ => {
                "page 3's content can't be changed without changing what else it draws; \
                  nothing was changed"
            }
        };
        assert_eq!(why, said);
        assert_eq!(e.to_string(), format!("{name}: {said}"));
        assert_eq!((d.current_version(), d.version_count()), (at, n));
        assert_eq!(d.state(), &st);
    }
    commit(&mut d, Actor::User, fx::ocr([(p0, layer("a"))])).unwrap();
    assert_eq!(filters(&d, p0), []);
}

/// A layer's block id is drawn in [1, 2^53) when it's made, kept by a
/// layer that replaces it unsaved, and by undo and redo; a duplicate's
/// layer is the page's; and a transaction lowered again draws the id it
/// drew.
#[test]
fn a_layer_s_block_id_is_kept() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    commit(&mut d, Actor::User, fx::ocr([(p0, layer("a"))])).unwrap();
    let id = block(&d, p0).unwrap();
    assert!((1..1 << 53).contains(&id.0), "{id}");
    commit(&mut d, Actor::User, fx::ocr([(p0, layer("b"))])).unwrap();
    assert_eq!(block(&d, p0), Some(id));
    assert_eq!(words(&d, p0), Some(vec!["b".into()]));
    d.undo().unwrap();
    assert_eq!(
        (block(&d, p0), words(&d, p0)),
        (Some(id), Some(vec!["a".into()]))
    );
    d.redo().unwrap();
    assert_eq!(
        (block(&d, p0), words(&d, p0)),
        (Some(id), Some(vec!["b".into()]))
    );
    commit(&mut d, Actor::User, vec![fx::duplicate(&[p0])]).unwrap();
    let copy = d.page(1).unwrap();
    let layer_of = |d: &Document, p| d.state().def(p).unwrap().content.ocr_layer().cloned();
    assert_eq!(layer_of(&d, copy), layer_of(&d, p0));

    // Cleared and set again, it's a new layer, with an id of its own.
    commit(&mut d, Actor::User, vec![fx::clear_ocr(&[p0])]).unwrap();
    commit(&mut d, Actor::User, fx::ocr([(p0, layer("c"))])).unwrap();
    assert_ne!(block(&d, p0), Some(id));

    let (v, mut tx) = planned(&d);
    tx.apply(fx::ocr([(p1, layer("d"))]).remove(0), &NoProgress)
        .unwrap();
    let drawn = tx.state().def(p1).unwrap().content.ocr_layer().unwrap().id;
    d.commit_ops(vec![fx::rotate(&[p2], sheaf_edit::Turn::Quarter)], "Rotate")
        .unwrap();
    d.commit(tx, on(v, Stale::Relower)).unwrap();
    assert_eq!(block(&d, p1), Some(drawn), "pinned");
}
