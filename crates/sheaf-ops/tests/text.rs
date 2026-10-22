//! A document's text (M10.5 §10.5, slice 14): each page's characters read
//! from its realization, as it draws under the caller's inputs. An
//! untouched page, an inserted file's (turned, its text read upright), a
//! duplicate, a crop and a page with recognized text and a header not yet
//! saved each have their words, the header and the recognized words only
//! under inputs that draw them, and a page edited where its content alone
//! isn't has its base page's characters where PDFium puts them. A source
//! PDFium refuses has its text from its groups, even a page asked of
//! before the refusal was known, as the saved file has it. Characters are
//! kept by render key: asked again, or of a duplicate, they're at hand,
//! and an edit reads them again. A search walks the pages in the
//! document's order and hits each where it draws, a refused source's
//! through its groups. A plugin reads each page as the file will draw it,
//! recognized text and a header not yet saved with it, but no field's
//! value. Recognized text sheaf saved over a page whose content leaves
//! its graphics state changed stays on its words under a header set
//! later, on the screen and saved (slice 18). Over the corpus's turned or
//! offset pages, a duplicate's characters, read from its group's
//! realization, are its source page's within 1/16 pt.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself; the
//! corpus test skips without the corpus.

mod common;

use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use futures::executor::block_on;
use lopdf::xref::XrefType;
use lopdf::{Document as Pdf, Stream, dictionary};
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::geom::Geom;
use sheaf_doc::materialize::How;
use sheaf_doc::realize::{EngineKey, GroupKey, ViewInputs};
use sheaf_doc::serialize::fixture::revision_cut_out;
use sheaf_doc::store::BlobKind;
use sheaf_doc::text_layer::fixture as saved;
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{
    Box2, DocState, Document, OcrWord, PageChars, PageId, Query, Store, TextOpts, TextService,
};
use sheaf_edit::{Margins, Rotation};
use sheaf_ops::text::{Found, Text};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Budget, OpenError, PageIndex, Renderer};

use common::{file, opened, ready, views, written};

/// A Letter page the file turns 90°, its crop box off the origin, with
/// "Inserted upright" drawn up it: upright as the page shows.
fn turned() -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.5");
    let pages = pdf.new_object_id();
    let font = dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
    let content = b"BT /F1 18 Tf 0 1 -1 0 300 200 Tm (Inserted upright) Tj ET".to_vec();
    let content = pdf.add_object(Stream::new(dictionary! {}, content));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "CropBox" => vec![36.into(), 48.5.into(), 560.into(), 740.into()],
        "Rotate" => 90,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "Contents" => content,
    });
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// Page `page` of `d`'s characters under `inputs`, through the text
/// service.
fn read(text: &Text, d: &Document, page: PageId, inputs: &ViewInputs) -> Arc<PageChars> {
    let o = TextOpts {
        inputs: inputs.clone(),
    };
    TextService::chars(text, &d.snapshot(), page, &o).unwrap()
}

/// The words of `chars`, in reading order.
fn words(chars: &PageChars) -> Vec<String> {
    chars.words().into_iter().map(|(w, ..)| w).collect()
}

/// The words of `chars`, sorted.
fn sorted(chars: &PageChars) -> Vec<String> {
    let mut words = words(chars);
    words.sort();
    words
}

/// Page `index` of `bytes`, read by PDFium.
fn pdfium_s(bytes: &[u8], index: u32) -> PageChars {
    let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
    let chars = engine.page_chars(PageIndex(index)).unwrap();
    PageChars::from_engine(chars.into_iter().map(|c| (c.ch, c.rect, c.angle)))
}

/// Whether two boxes' edges are within 1/16 pt.
fn near(a: Box2, b: Box2) -> bool {
    let by = 1.0 / 16.0;
    (a.x - b.x).abs() <= by
        && (a.y - b.y).abs() <= by
        && (a.right() - b.right()).abs() <= by
        && (a.bottom() - b.bottom()).abs() <= by
}

/// Whether `a` is inside `b`, but for `by` points past any edge.
fn inside(a: Box2, b: Box2, by: f32) -> bool {
    a.x >= b.x - by
        && a.y >= b.y - by
        && a.right() <= b.right() + by
        && a.bottom() <= b.bottom() + by
}

/// Where two pages' characters first differ: in a character, its line or
/// its box by more than 1/16 pt, or in how many there are or which way
/// they read. None when they don't.
fn differ(ours: &PageChars, theirs: &PageChars) -> Option<String> {
    if (ours.len(), ours.upright) != (theirs.len(), theirs.upright) {
        return Some(format!(
            "{} characters read {:?}, theirs {} read {:?}",
            ours.len(),
            ours.upright,
            theirs.len(),
            theirs.upright
        ));
    }
    let at = (ours.glyphs.iter().zip(&theirs.glyphs)).position(|(g, h)| {
        let boxed = match (g.rect, h.rect) {
            (Some(r), Some(s)) => near(r, s),
            (r, s) => r.is_none() && s.is_none(),
        };
        g.ch != h.ch || g.line != h.line || !boxed
    })?;
    let (g, h) = (&ours.glyphs[at], &theirs.glyphs[at]);
    Some(format!("character {at}: {g:?}, theirs {h:?}"))
}

/// Crops `page` of `d` by a margin off each side, as Document > Crop Pages
/// does.
fn crop(d: &mut Document, page: PageId) {
    let margins = Margins {
        top: 40.0,
        right: 30.0,
        bottom: 50.0,
        left: 20.0,
    };
    common::crop(d, &[page], margins);
}

fn word(text: &str, x: f32, y: f32, w: f32) -> OcrWord {
    OcrWord {
        text: text.into(),
        rect: Box2 { x, y, w, h: 14.0 },
        confidence: 0.9,
    }
}

/// Every page's characters come from its realization (§10.5): under the
/// screen's inputs an untouched page, an inserted file's page (turned 90°,
/// its text drawn upright as it shows, so reading up PageSpace), a
/// duplicate, a crop, and a page with recognized text and a header not yet
/// saved each have their words, the recognized ones inside the words'
/// boxes. With the page's content alone, the header and the recognized
/// text aren't there, and a page realized for an edit has its base page's
/// characters within 1/16 pt.
#[test]
fn chars_come_from_each_page_s_realization() {
    let mut d = opened(file(4, XrefType::CrossReferenceTable), "a.pdf", None);
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let kind = BlobKind::file(Path::new("turned.pdf"), None);
    let inserted = d.store().open(turned().into(), kind).unwrap();
    let ops = vec![
        fx::insert(&inserted, 0..1, "turned.pdf", None),
        fx::duplicate(&[p1]),
    ];
    d.commit_ops(ops, "Insert").unwrap();
    // Words as OCR saw them on the page as it shows, upright.
    let seen = vec![
        word("Total", 300.0, 600.0, 50.0),
        word("42", 360.0, 600.0, 20.0),
    ];
    let layer = sheaf_doc::text_layer::from_engine(seen, &d.geom(p3), "test");
    let mut ops = fx::ocr([(p3, Arc::new(layer.clone()))]);
    ops.push(fx::set_header(fx::header("Draft"), Some(&[p3])));
    d.commit_ops(ops, "OCR").unwrap();
    crop(&mut d, p2);
    let st = d.state();
    let copy = (st.pages().find(|(_, def, _)| def.copy > 0)).unwrap().0;
    let turned = (st.pages())
        .find(|(_, def, _)| def.base.src.id() == inserted.id())
        .unwrap()
        .0;
    let views = views();
    let text = Text::new(views.clone(), Budget::Screen);
    let (screen, content) = (ViewInputs::screen(), ViewInputs::content_only());
    let engine = |p, inputs: &ViewInputs| views.view(st, p, inputs).unwrap().engine;
    assert!(matches!(engine(p3, &screen), EngineKey::Group(..)));
    assert!(matches!(engine(copy, &content), EngineKey::Group(..)));
    assert!(matches!(engine(p3, &content), EngineKey::Base(_)));
    // The crop is its file's page, its box changed: read from its group.
    let primary = d.primary().id();
    assert_eq!(st.def(p2).unwrap().base.src.id(), primary);
    let cropped = engine(p2, &content);
    let in_group =
        matches!(cropped, EngineKey::Group(GroupKey { source, .. }, _) if source == primary);
    assert!(in_group, "{cropped:?}");
    assert!(matches!(engine(turned, &screen), EngineKey::Base(_)));

    for (page, want) in [
        (p0, "page 0"),
        (p1, "page 1"),
        (copy, "page 1"),
        (p2, "page 2"),
    ] {
        for inputs in [&screen, &content] {
            let chars = read(&text, &d, page, inputs);
            assert_eq!(
                words(&chars),
                want.split(' ').collect::<Vec<_>>(),
                "{page:?}"
            );
        }
    }
    let up = read(&text, &d, turned, &screen);
    assert_eq!(words(&up), ["Inserted", "upright"]);
    assert_eq!(up.upright, Rotation::R90);
    assert_eq!(up.rects(0, up.len()).len(), 1, "one line");

    let headed = read(&text, &d, p3, &screen);
    assert_eq!(sorted(&headed), ["3", "42", "Draft", "Total", "page"]);
    // PDFium's box for the layer's font reaches a fifth of a point above
    // a word's (its glyphs' ascent, not the font's).
    let found = headed.words();
    for w in &layer.words {
        let (_, _, r) = (found.iter().find(|(t, ..)| *t == w.text)).unwrap();
        assert!(
            inside(*r, w.rect, 0.25),
            "{}: {r:?} in {:?}",
            w.text,
            w.rect
        );
    }
    let alone = read(&text, &d, p3, &content);
    assert_eq!(words(&alone), ["page", "3"]);

    let primary = d.primary().bytes().clone();
    for (page, index) in [(copy, 1), (p2, 2), (p3, 3)] {
        let ours = read(&text, &d, page, &content);
        let theirs = pdfium_s(&primary, index);
        assert_eq!(differ(&ours, &theirs), None, "{page:?}");
    }
}

/// A source PDFium refuses (as it does the corpus's 4422502) has its text
/// from its groups, as it's drawn: a page left alone, asked of before
/// the refusal is known, is read from its group once the base engine's
/// failed open has told the source; and every page, the one with a header
/// too, reads as the saved file's page does.
#[test]
fn a_refused_source_s_text_comes_from_its_groups() {
    let bytes = revision_cut_out(file(3, XrefType::CrossReferenceTable));
    let refused = PdfiumEngine::open_bytes(bytes.clone().into(), None).err();
    assert!(OpenError::is_refused(&refused.expect("PDFium refuses it")));
    let mut d = opened(bytes, "refused.pdf", None);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let header = fx::set_header(fx::header("Draft"), Some(&[p1]));
    d.commit_ops(vec![header], "Header").unwrap();
    let views = views();
    let text = Text::new(views.clone(), Budget::Screen);
    let screen = ViewInputs::screen();
    let view = views.view(d.state(), p0, &screen).unwrap();
    assert!(matches!(view.engine, EngineKey::Base(_)));
    assert!(!d.primary().base_refused());
    let first = read(&text, &d, p0, &screen);
    assert!(d.primary().base_refused());
    assert_eq!(words(&first), ["page", "0"]);
    let saved = sheaf_doc::save::rewrite(&d.snapshot()).unwrap();
    for (at, page) in [p0, p1, p2].into_iter().enumerate() {
        let view = views.view(d.state(), page, &screen).unwrap();
        assert!(matches!(view.engine, EngineKey::Group(..)), "{at}");
        let ours = read(&text, &d, page, &screen);
        assert_eq!(differ(&ours, &pdfium_s(&saved, at as u32)), None, "{at}");
    }
    assert_eq!(
        sorted(&read(&text, &d, p1, &screen)),
        ["1", "Draft", "page"]
    );
    let alone = read(&text, &d, p1, &ViewInputs::content_only());
    assert_eq!(words(&alone), ["page", "1"]);
}

/// What a search of `st` for `q` under `inputs` found, page by page: each
/// page's position, id and matches' text, once it's done. Every position
/// is told in order, how far the search got after each turn, and last that
/// it's done; every match lies on its page's visible box, in PageSpace.
fn searched(
    text: &Text,
    st: &DocState,
    q: Query,
    inputs: &ViewInputs,
) -> Vec<(usize, PageId, Vec<String>)> {
    let told: Vec<Found> = block_on(text.search(st, q, inputs).collect());
    let (mut pages, mut scanned) = (Vec::new(), 0);
    for found in &told {
        match found {
            Found::Hits {
                position,
                page,
                hits,
            } => {
                assert!(*position >= scanned, "{position} after {scanned}");
                assert_eq!(st.page_at(*position), Some(*page));
                let v = st.geom(*page).expect("a page").visible();
                for r in hits.iter().flat_map(|h| &h.rects) {
                    let on = r.x >= v.x0 && r.y >= v.y0 && r.right() <= v.x1 && r.bottom() <= v.y1;
                    assert!(on, "{r:?} on {v:?}");
                }
                let found = hits.iter().map(|h| h.text.clone()).collect();
                pages.push((*position, *page, found));
            }
            Found::Scanned(n) => {
                assert!(
                    *n > scanned || *n == st.order().len(),
                    "{n} after {scanned}"
                );
                scanned = *n;
            }
            Found::Done => assert_eq!(scanned, st.order().len()),
        }
    }
    assert!(matches!(told.last(), Some(Found::Done)));
    pages
}

fn literal(query: &str) -> Query {
    Query::literal(query, false).unwrap()
}

/// A search walks the document's pages in its order, each read from its
/// own realization (§10.5): Find hits an untouched page, an inserted
/// file's page turned 90°, a duplicate, a crop and a page with recognized
/// text and a header not yet saved, each at its position, its matches on
/// its visible box; a pattern finds the numbers on every page, recognized
/// ones among them; and the header and the recognized words, and a phrase
/// running from the page's own line onto theirs, are found on their page
/// under the screen's inputs, and not with the page's content alone. A
/// source PDFium refuses is searched through its groups.
#[test]
fn a_search_walks_the_document_s_order() {
    let mut d = opened(file(4, XrefType::CrossReferenceTable), "a.pdf", None);
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let kind = BlobKind::file(Path::new("turned.pdf"), None);
    let inserted = d.store().open(turned().into(), kind).unwrap();
    let ops = vec![
        fx::insert(&inserted, 0..1, "turned.pdf", Some(p1)),
        fx::duplicate(&[p1]),
    ];
    d.commit_ops(ops, "Insert").unwrap();
    let seen = vec![
        word("Total", 300.0, 600.0, 50.0),
        word("42", 360.0, 600.0, 20.0),
    ];
    let layer = sheaf_doc::text_layer::from_engine(seen, &d.geom(p3), "test");
    let mut ops = fx::ocr([(p3, Arc::new(layer))]);
    ops.push(fx::set_header(fx::header("Draft"), Some(&[p3])));
    d.commit_ops(ops, "OCR").unwrap();
    crop(&mut d, p2);
    let st = d.state().clone();
    let at = |page| st.order().index_of(&page).unwrap();
    let [turned, copy] = [1, 3].map(|i| st.page_at(i).unwrap());
    assert_eq!(st.def(turned).unwrap().base.src.id(), inserted.id());
    assert!(st.def(copy).unwrap().copy > 0);
    let text = Text::new(views(), Budget::Screen);
    let (screen, content) = (ViewInputs::screen(), ViewInputs::content_only());

    let pages = [p0, p1, copy, p2, p3];
    let want: Vec<_> = (pages.iter())
        .map(|&p| (at(p), p, vec!["page".to_string()]))
        .collect();
    assert_eq!(searched(&text, &st, literal("PAGE"), &screen), want);
    let up = searched(&text, &st, literal("inserted UPRIGHT"), &screen);
    assert_eq!(up, [(1, turned, vec!["Inserted upright".to_string()])]);
    let numbers = Query::Pattern(Arc::new(regex::Regex::new(r"\d+").unwrap()));
    let found = searched(&text, &st, numbers.clone(), &screen);
    let want: Vec<_> = [(p0, "0"), (p1, "1"), (copy, "1"), (p2, "2")]
        .map(|(p, n)| (at(p), p, vec![n.to_string()]))
        .into_iter()
        .chain([(at(p3), p3, vec!["3".to_string(), "42".to_string()])])
        .collect();
    let mut sorted = found.clone();
    sorted.iter_mut().for_each(|(.., n)| n.sort());
    assert_eq!(sorted, want);
    // A phrase from the page's own line onto the recognized words' finds
    // them across the line break.
    for (q, word) in [
        ("draft", "Draft"),
        ("total", "Total"),
        ("3 TOTAL", "3\nTotal"),
    ] {
        let found = searched(&text, &st, literal(q), &screen);
        assert_eq!(found, [(at(p3), p3, vec![word.to_string()])]);
        assert_eq!(searched(&text, &st, literal(q), &content), []);
    }
    let alone = searched(&text, &st, numbers, &content);
    assert_eq!(alone.last(), Some(&(at(p3), p3, vec!["3".to_string()])));

    // The refused source's first page is viewed on its base engine, as
    // nothing has tried it yet, and read from its group (the walk's
    // fallback); the pages after it view their groups.
    let mut d = opened(
        revision_cut_out(file(3, XrefType::CrossReferenceTable)),
        "refused.pdf",
        None,
    );
    let p: Vec<PageId> = (0..3).map(|i| d.page(i).unwrap()).collect();
    d.commit_ops(
        vec![fx::set_header(fx::header("Draft"), Some(&[p[1]]))],
        "Header",
    )
    .unwrap();
    let (views, st) = (views(), d.state().clone());
    let view = views.view(&st, p[0], &screen).unwrap();
    assert!(matches!(view.engine, EngineKey::Base(_)));
    let text = Text::new(views, Budget::Screen);
    let found = searched(&text, &st, literal("page"), &screen);
    let want: Vec<_> = (0..3)
        .map(|i| (i, p[i], vec!["page".to_string()]))
        .collect();
    assert_eq!(found, want);
    assert!(d.primary().base_refused());
    let found = searched(&text, &st, literal("draft"), &screen);
    assert_eq!(found, [(1, p[1], vec!["Draft".to_string()])]);
}

/// Characters are kept by render key: asked again, they're answered at
/// once, the same characters, with no query; a duplicate draws as its
/// original does and has them too. An edit to the page changes its key,
/// so it's read again, and the page beside it keeps its own; an undo back
/// to what was read finds it kept.
#[test]
fn chars_are_cached_by_render_key() {
    let mut d = opened(file(2, XrefType::CrossReferenceTable), "a.pdf", None);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let screen = ViewInputs::screen();
    let text = Text::new(views(), Budget::Screen);
    let st = d.state().clone();
    assert!(text.cached(&st, p0, &screen).is_none());
    let first = block_on(text.chars(&st, p0, &screen)).unwrap().unwrap();
    let beside = block_on(text.chars(&st, p1, &screen)).unwrap().unwrap();
    let kept = text.cached(&st, p0, &screen).expect("kept");
    assert!(Arc::ptr_eq(&kept, &first));
    let mut again = text.chars(&st, p0, &screen);
    let again = again.try_recv().unwrap().expect("answered at once");
    assert!(Arc::ptr_eq(&again.unwrap(), &first));

    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate")
        .unwrap();
    let copy = d.page(1).unwrap();
    let copied = text
        .cached(d.state(), copy, &screen)
        .expect("its original's");
    assert!(Arc::ptr_eq(&copied, &first));

    let header = fx::set_header(fx::header("Draft"), Some(&[p0]));
    d.commit_ops(vec![header], "Header").unwrap();
    let st = d.state().clone();
    assert!(text.cached(&st, p0, &screen).is_none());
    let kept = text.cached(&st, p1, &screen).expect("the page beside");
    assert!(Arc::ptr_eq(&kept, &beside));
    let edited = block_on(text.chars(&st, p0, &screen)).unwrap().unwrap();
    assert_eq!(sorted(&edited), ["0", "Draft", "page"]);
    d.undo().unwrap();
    let back = text.cached(d.state(), p0, &screen).expect("read before");
    assert!(Arc::ptr_eq(&back, &first));
}

/// A plugin's text is each page as the file will draw it (§14.3): under
/// its inputs, a page with recognized text not yet saved and one with a
/// header not yet saved have those words beside their own, read from
/// their groups, and every page reads as the saved file's does. A source
/// PDFium refuses has a plugin's text too, from its groups, a page asked
/// of before the refusal is known among them.
#[test]
fn plugin_text_reads_the_page_as_the_file_will_draw_it() {
    let mut d = opened(file(3, XrefType::CrossReferenceTable), "a.pdf", None);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let seen = vec![
        word("Total", 300.0, 600.0, 50.0),
        word("42", 360.0, 600.0, 20.0),
    ];
    let layer = sheaf_doc::text_layer::from_engine(seen, &d.geom(p0), "test");
    let mut ops = fx::ocr([(p0, Arc::new(layer))]);
    ops.push(fx::set_header(fx::header("Draft"), Some(&[p1])));
    d.commit_ops(ops, "OCR").unwrap();
    let views = views();
    let text = Text::new(views.clone(), Budget::Background);
    let plugin = ViewInputs::plugin_text();
    for page in [p0, p1] {
        let view = views.view(d.state(), page, &plugin).unwrap();
        assert!(matches!(view.engine, EngineKey::Group(..)), "{page:?}");
    }
    let recognized = read(&text, &d, p0, &plugin);
    assert_eq!(sorted(&recognized), ["0", "42", "Total", "page"]);
    let headed = read(&text, &d, p1, &plugin);
    assert_eq!(sorted(&headed), ["1", "Draft", "page"]);
    assert_eq!(words(&read(&text, &d, p2, &plugin)), ["page", "2"]);
    let saved = sheaf_doc::save::rewrite(&d.snapshot()).unwrap();
    for (at, page) in [p0, p1, p2].into_iter().enumerate() {
        let ours = read(&text, &d, page, &plugin);
        assert_eq!(differ(&ours, &pdfium_s(&saved, at as u32)), None, "{at}");
    }

    let mut d = opened(
        revision_cut_out(file(2, XrefType::CrossReferenceTable)),
        "refused.pdf",
        None,
    );
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let header = fx::set_header(fx::header("Draft"), Some(&[p1]));
    d.commit_ops(vec![header], "Header").unwrap();
    let text = Text::new(views, Budget::Background);
    assert!(!d.primary().base_refused());
    assert_eq!(words(&read(&text, &d, p0, &plugin)), ["page", "0"]);
    assert!(d.primary().base_refused());
    let headed = read(&text, &d, p1, &plugin);
    assert_eq!(sorted(&headed), ["1", "Draft", "page"]);
}

/// A plugin's text leaves fields' values out (§14.3's privacy in API
/// 0.4): a filled field's value is drawn on its page, but the page's text
/// as a plugin reads it is the page's own, whether the page is read from
/// its file or, with a header on it, from its group, and with its widgets
/// drawn too.
#[test]
fn plugin_text_leaves_field_values_out() {
    use sheaf_doc::forms::fixture::{Form, row};
    use sheaf_image::raster::PixelFormat;
    use sheaf_render::Job;
    let mut form = Form::new(2);
    let at = row(5);
    for page in 0..2 {
        let field = form.text(&format!("ssn{page}"), page, at);
        form.fill(field, "SECRET 123-45-6789");
    }
    let mut d = opened(form.bytes(), "form.pdf", None);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let header = fx::set_header(fx::header("Draft"), Some(&[p1]));
    d.commit_ops(vec![header], "Header").unwrap();
    let views = views();
    let text = Text::new(views.clone(), Budget::Background);
    let plugin = ViewInputs::plugin_text();
    let engine = |page| views.view(d.state(), page, &plugin).unwrap().engine;
    assert!(matches!(engine(p0), EngineKey::Base(_)));
    assert!(matches!(engine(p1), EngineKey::Group(..)));
    assert_eq!(words(&read(&text, &d, p0, &plugin)), ["Page", "1"]);
    assert_eq!(
        sorted(&read(&text, &d, p1, &plugin)),
        ["2", "Draft", "Page"]
    );
    // PDFium never reads a widget's appearance as text, so the page drawn
    // with its widgets, as a plugin's render draws it, reads the same.
    for page in [p0, p1] {
        let drawn = read(&text, &d, page, &ViewInputs::plugin());
        assert_eq!(drawn, read(&text, &d, page, &plugin), "{page:?}");
    }
    // The value is drawn all the same: ink in the field, at a pixel a
    // point.
    for page in [p0, p1] {
        let job = Job::Page {
            width: 612,
            rotation: Rotation::R0,
        };
        let rx = views.render(
            d.state(),
            page,
            &ViewInputs::plugin(),
            job,
            PixelFormat::Rgba,
            0,
            Budget::Background,
        );
        let raster = block_on(rx).unwrap().unwrap().raster;
        let (x0, x1) = (at[0] as u32, at[2] as u32);
        let (y0, y1) = ((792.0 - at[3]) as u32, (792.0 - at[1]) as u32);
        let inked = (y0..y1)
            .any(|y| (x0..x1).any(|x| raster.data[((y * raster.width + x) * 4) as usize] < 128));
        assert!(inked, "{page:?}: the field's value isn't drawn");
    }
}

sheaf_doc::both_ways!(
    a_saved_block_over_an_unbalanced_base_stays_in_place,
    a_saved_block_over_an_unbalanced_base_stays_in_place_appended: block_in_place
);

/// Recognized text sheaf saved over a page whose own content leaves a `q`
/// open, moved by a `cm` (`text_layer::fixture::block_page`), with a
/// header set over it: each word of the text and of the page's own is
/// where PDFium found it before, within 1/16 pt, as the screen reads the
/// page before it's saved and as PDFium reads it saved `how`, with the
/// header's word beside them.
fn block_in_place(how: How) {
    let mut bytes = Vec::new();
    saved::block_page().save_to(&mut bytes).unwrap();
    let boxes = |chars: &PageChars| -> Vec<(String, Box2)> {
        (chars.words().into_iter())
            .map(|(word, _, at)| (word, at))
            .collect()
    };
    let was = boxes(&pdfium_s(&bytes, 0));
    let said: Vec<&str> = was.iter().map(|(w, _)| w.as_str()).collect();
    assert_eq!(said, ["Body", "text", "Saved", "words"]);
    let mut d = opened(bytes, "block.pdf", None);
    let p0 = d.page(0).unwrap();
    let header = fx::set_header(fx::header("Draft"), None);
    d.commit_ops(vec![header], "Header").unwrap();
    let text = Text::new(views(), Budget::Screen);
    let screen = boxes(&read(&text, &d, p0, &ViewInputs::screen()));
    let saved = boxes(&pdfium_s(&written(&d, how), 0));
    for (read, now) in [("the screen", screen), ("saved", saved)] {
        for (word, at) in &was {
            let found = now.iter().find(|(w, _)| w == word);
            let (_, now_at) = found.unwrap_or_else(|| panic!("{how:?}, {read}: no {word}"));
            assert!(
                near(*now_at, *at),
                "{how:?}, {read}: {word} at {now_at:?}, was {at:?}"
            );
        }
        let draft = now.iter().filter(|(w, _)| w == "Draft").count();
        assert_eq!((now.len(), draft), (5, 1), "{how:?}, {read}: {now:?}");
    }
}

/// Whether a page's frame is more than the plain one, a y flip below the
/// height: its file turns it, or its visible box is off the origin.
fn moved(geom: &Geom) -> bool {
    let v = geom.visible();
    geom.rotate != Rotation::R0 || v.x0 != 0.0 || v.y0 != 0.0
}

/// How many characters with boxes a corpus file's duplicated pages have,
/// each of its pages whose frame is more than the plain one duplicated,
/// and where a duplicate's characters, read from its group's realization,
/// differ from its source page's as PDFium reads the file, or why the
/// file couldn't be made ready and duplicated. None for a file sheaf's
/// store doesn't open, or PDFium refuses.
fn realized_glyphs(corpus: &Corpus, entry: &Entry) -> Option<(usize, Vec<String>)> {
    let path = corpus.path(entry);
    let bytes: Arc<[u8]> = std::fs::read(&path).ok()?.into();
    let store = Store::new();
    let primary = store
        .open(bytes.clone(), BlobKind::file(&path, None))
        .ok()?;
    let wanted: Vec<u32> = (0..primary.page_count())
        .filter(|&i| moved(&primary.geom(i)))
        .collect();
    if wanted.is_empty() {
        return Some((0, Vec::new()));
    }
    let engine = PdfiumEngine::open_bytes(bytes, None).ok()?;
    let name = entry.path.as_str();
    let mut d = match ready(store, primary, name) {
        Ok(d) => d,
        Err(e) => return Some((0, vec![format!("opening: {e:#}")])),
    };
    let pages: Vec<PageId> = (d.state().pages())
        .filter(|(_, def, _)| wanted.contains(&def.base.index))
        .map(|(id, ..)| id)
        .collect();
    if let Err(e) = d.commit_ops(vec![fx::duplicate(&pages)], "Duplicate") {
        return Some((0, vec![format!("duplicating: {e}")]));
    }
    let text = Text::new(views(), Budget::Background);
    let (snap, o) = (
        d.snapshot(),
        TextOpts {
            inputs: ViewInputs::screen(),
        },
    );
    let (mut chars, mut differ_at) = (0, Vec::new());
    let copies = (snap.state.pages()).filter(|(_, def, _)| def.copy > 0);
    for (copy, def, _) in copies {
        let index = def.base.index;
        let Ok(theirs) = engine.page_chars(PageIndex(index)) else {
            continue;
        };
        let theirs = PageChars::from_engine(theirs.into_iter().map(|c| (c.ch, c.rect, c.angle)));
        match TextService::chars(&text, &snap, copy, &o) {
            Ok(ours) => {
                chars += ours.glyphs.iter().filter(|g| g.rect.is_some()).count();
                if let Some(how) = differ(&ours, &theirs) {
                    differ_at.push(format!("page {index}: {how}"));
                }
            }
            Err(e) => differ_at.push(format!("page {index}: {e:#}")),
        }
    }
    Some((chars, differ_at))
}

/// The characters [`realized_glyphs`] compares over the corpus of 4,392
/// files (slice 14): fewer is a regression, a file lost to the check,
/// unless the slice notes say why it's right.
const REALIZED_GLYPHS: (usize, usize) = (4392, 780_238);

/// A duplicate's characters, read from its group's realization, are its
/// source page's as PDFium reads the file, within 1/16 pt: on every
/// corpus page whose frame is more than the plain one (slice 14's check
/// that the realization keeps PageSpace, beside sheaf-render's that the
/// engine's boxes are PDFium's).
#[test]
fn realized_glyphs_lie_within_a_sixteenth_of_a_point_of_the_source_s() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.manifest().unwrap();
    let outcomes = each_entry(&entries, |entry| realized_glyphs(&corpus, entry));
    let compared: usize = outcomes.iter().flatten().map(|o| o.0).sum();
    eprintln!("{} files: {compared} characters compared", entries.len());
    assert!(compared > 0, "no characters were compared");
    let (files, at_least) = REALIZED_GLYPHS;
    if entries.len() == files {
        assert!(compared >= at_least, "{compared} compared, from {at_least}");
    }
    let differ: Vec<(&str, &[String])> = (entries.iter().zip(&outcomes))
        .filter_map(|(entry, o)| {
            let found = &o.as_ref()?.1;
            (!found.is_empty()).then(|| (entry.path.as_str(), &found[..found.len().min(3)]))
        })
        .collect();
    assert!(differ.is_empty(), "{differ:#?}");
}
