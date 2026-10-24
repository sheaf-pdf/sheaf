//! The comment summary and exports, from a file with sheaf's marks and
//! another tool's comments: what the summary PDF says, where it draws the
//! pages and their numbers (read back with PDFium), and the CSV.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

// A test crate of its own, whose scratch files go in the temp directory
// clippy.toml bans outside tests.
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};

use lopdf::{Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::export::Export;
use sheaf_doc::materialize::AskAnswers;
use sheaf_doc::save::text_string;
use sheaf_doc::tx::fixture::add;
use sheaf_doc::{Actor, Annot, AnnotId, AnnotKind, Box2, Pt};
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_ops::NoProgress;
use sheaf_ops::comments::{self, Column, CommentFilter, Layout};
use sheaf_ops::session::SaveTo;
use sheaf_render::PageIndex;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, Renderer};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-comments-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Three Letter pages, each with a black square: page 1 plain; page 2
/// with Ann's note and Bo's reply to it; page 3 with `/Rotate 90`.
fn pdf(dir: &Path) -> PathBuf {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let mut kids = Vec::new();
    for n in 0..3 {
        // Page 3's square is off its middle, to show which way it turned.
        let square: &[u8] = if n == 2 {
            b"0 0 0 rg 100 500 100 100 re f"
        } else {
            b"0 0 0 rg 300 300 100 100 re f"
        };
        let content = pdf.add_object(Stream::new(dictionary! {}, square.to_vec()));
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        if n == 2 {
            page.set("Rotate", 90);
        }
        let page_id = pdf.add_object(page);
        if n == 1 {
            let note = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Text", "P" => page_id,
                "Rect" => vec![100.into(), 600.into(), 120.into(), 620.into()],
                "Contents" => text_string("Check “this” (please)"),
                "T" => Object::string_literal("Ann"),
                "NM" => Object::string_literal("ann-1"),
                "M" => Object::string_literal("D:20261001120000"),
                "C" => vec![0.into(), 0.into(), 1.into()],
            });
            let reply = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Text", "P" => page_id,
                "Rect" => vec![100.into(), 600.into(), 120.into(), 620.into()],
                "Contents" => Object::string_literal("Done"),
                "T" => Object::string_literal("Bo"),
                "NM" => Object::string_literal("bo-1"),
                "IRT" => note,
                "M" => Object::string_literal("D:20261002120000"),
            });
            pdf.get_dictionary_mut(page_id)
                .unwrap()
                .set("Annots", vec![note.into(), reply.into()]);
        }
        kids.push(Object::Reference(page_id));
    }
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 3 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let path = dir.join("review.pdf");
    pdf.save(&path).unwrap();
    path
}

fn mark(kind: AnnotKind, color: [u8; 3], contents: &str) -> Annot {
    Annot {
        id: AnnotId(0),
        kind,
        color,
        opacity: 1.0,
        stroke: 1.0,
        author: "Greg".into(),
        contents: contents.into(),
        modified: "D:20261005090000".into(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// `a`, placed where page `i` of `doc` shows it, as the model keeps it:
/// in PageSpace, reading upright as the page shows.
fn placed(doc: &sheaf_doc::Document, i: usize, a: Annot) -> Annot {
    a.to_page(&doc.geom(doc.page(i).unwrap()))
}

/// The file, with a red note of ours on page 1 at (100, 100) and a green
/// tickmark on page 3 at (50, 60), as they show, and the other tools'
/// comments in it.
fn opened(dir: &Path) -> sheaf_doc::Document {
    let mut doc = comments::open(&pdf(dir), None).unwrap().doc;
    let pages: Vec<_> = doc.state().order().iter().copied().collect();
    let note = mark(
        AnnotKind::Note {
            pos: Pt { x: 100.0, y: 100.0 },
        },
        [255, 0, 0],
        "First note (with parens)",
    );
    let note = placed(&doc, 0, note);
    doc.commit_ops(vec![add(pages[0], &note)], "note").unwrap();
    let tick = mark(
        AnnotKind::Stamp {
            rect: Box2 {
                x: 50.0,
                y: 60.0,
                w: 20.0,
                h: 20.0,
            },
            glyph: "✓".into(),
        },
        [0, 160, 0],
        "",
    );
    let tick = placed(&doc, 2, tick);
    doc.commit_ops(vec![add(pages[2], &tick)], "tick").unwrap();
    doc
}

fn engine(bytes: Vec<u8>) -> PdfiumEngine {
    PdfiumEngine::open_bytes(bytes.into(), None).unwrap()
}

fn render(e: &PdfiumEngine, page: u32) -> Raster {
    e.render(
        PageIndex(page),
        Job::Page {
            width: 792,
            rotation: Rotation::R0,
        },
        PixelFormat::Rgba,
    )
    .unwrap()
}

fn px(r: &Raster, x: f32, y: f32) -> [u8; 3] {
    let i = ((y.round() as u32 * r.width + x.round() as u32) * 4) as usize;
    [r.data[i], r.data[i + 1], r.data[i + 2]]
}

/// `doc`'s comments, as `cf` lists them, summarized as `layout` lays them
/// out and saved to `path` as the reader saves the summary: its bytes.
fn summary(doc: &sheaf_doc::Document, cf: &CommentFilter, layout: Layout, path: &Path) -> Vec<u8> {
    let from = doc.export_from(Actor::User);
    let mut made = comments::summary(&from, cf, "", layout, "review.pdf").unwrap();
    let to = SaveTo::File {
        path: path.into(),
        append: false,
    };
    made.save(to, &AskAnswers::default(), &NoProgress).unwrap();
    std::fs::read(path).unwrap()
}

/// `bytes`, a summary, with its `/CreationDate`'s digits zeroed: the one
/// thing in it that's the time's.
fn undated(mut bytes: Vec<u8>) -> Vec<u8> {
    let key = b"/CreationDate(D:";
    let at = bytes
        .windows(key.len())
        .position(|w| w == key)
        .expect("the summary is dated")
        + key.len();
    for b in bytes[at..].iter_mut().take_while(|b| b.is_ascii_digit()) {
        *b = b'0';
    }
    bytes
}

/// The same summary gives the same file, twice and on two threads, byte
/// for byte but for its `/CreationDate`: it's made in memory and saved,
/// and nothing else in it is the time's. Compared whole, writes either
/// side of a second's turn differed.
#[test]
fn the_summary_writes_the_same_pages_every_time() {
    let dir = scratch("same");
    let doc = opened(&dir);
    let write = |n: usize| {
        let path = dir.join(format!("summary {n}.pdf"));
        let bytes = summary(&doc, &CommentFilter::default(), Layout::Pages, &path);
        undated(bytes)
    };
    let first = write(0);
    assert_eq!(write(1), first);
    std::thread::scope(|s| {
        let threads: Vec<_> = (2..4).map(|n| s.spawn(move || write(n))).collect();
        for t in threads {
            assert_eq!(t.join().unwrap(), first);
        }
    });
}

#[test]
fn the_summary_lists_every_comment_and_thread() {
    let dir = scratch("only");
    let doc = opened(&dir);
    let all = dir.join("all.pdf");
    let bytes = summary(&doc, &CommentFilter::default(), Layout::CommentsOnly, &all);
    let e = engine(bytes);
    assert_eq!(e.page_count(), 1);
    let text = e.page_text(PageIndex(0)).unwrap();
    for want in [
        "review.pdf",
        "3 comments",
        "Page 1",
        "First note (with parens)",
        "Page 2",
        "Check “this” (please)",
        "2.1",
        "Done",
        "Page 3",
        "Tickmark ✔",
        "Page 1 of 1",
    ] {
        assert!(text.contains(want), "no {want:?} in:\n{text}");
    }
    // Filtered to page 2, the rest are gone.
    let cf = CommentFilter {
        pages: Some((1, 1)),
        ..Default::default()
    };
    let bytes = summary(&doc, &cf, Layout::CommentsOnly, &dir.join("page 2.pdf"));
    let text = engine(bytes).page_text(PageIndex(0)).unwrap();
    assert!(
        text.contains("Filtered: page 2") && text.contains("Done"),
        "{text}"
    );
    assert!(!text.contains("First note"), "{text}");
}

#[test]
fn the_pages_layout_draws_each_page_with_its_numbers() {
    let dir = scratch("pages");
    let doc = opened(&dir);
    let pages = dir.join("pages.pdf");
    let bytes = summary(&doc, &CommentFilter::default(), Layout::Pages, &pages);
    let e = engine(bytes);
    assert_eq!(e.page_count(), 3);
    let text = e.page_text(PageIndex(1)).unwrap();
    assert!(
        text.contains("Page 2") && text.contains("2.1") && text.contains("Done"),
        "{text}"
    );

    // Page 1 fills the left half: 351 wide at 0.5735, from (36, 36) down.
    let r = render(&e, 0);
    let k = 351.0 / 612.0;
    let (left, top) = (36.0, 36.0);
    // Its square's middle, user (350, 350), shows at (350, 442).
    assert_eq!(px(&r, left + 350.0 * k, top + 442.0 * k), [0, 0, 0]);
    // The note's number, a red disc at its corner.
    let (cx, cy) = (left + 100.0 * k, top + 100.0 * k);
    for (dx, dy) in [(-5.0, 0.0), (5.0, 0.0), (0.0, -5.0), (0.0, 5.0)] {
        let [red, g, b] = px(&r, cx + dx, cy + dy);
        assert!(
            red > 200 && g < 80 && b < 80,
            "{:?} at {dx},{dy}",
            [red, g, b]
        );
    }

    // Page 3 turned a quarter: 792 × 612 shown, at 351 / 792.
    let r = render(&e, 2);
    let k = 351.0 / 792.0;
    // Its square's middle, user (150, 550), shows at (550, 150).
    assert_eq!(px(&r, left + 550.0 * k, top + 150.0 * k), [0, 0, 0]);
    let (cx, cy) = (left + 50.0 * k, top + 60.0 * k);
    for (dx, dy) in [(-5.0, 0.0), (5.0, 0.0), (0.0, -5.0), (0.0, 5.0)] {
        let [red, g, b] = px(&r, cx + dx, cy + dy);
        assert!(
            red < 60 && g > 120 && b < 60,
            "{:?} at {dx},{dy}",
            [red, g, b]
        );
    }
}

/// Comments brought in from the same XFDF twice, through a session, as
/// `sheaf comments import` brings them (M10.5 §6.8): the second time they
/// update the marks the first made, matched by `/NM`, rather than add
/// them again; each keeps its own name, dates and author, and the reply
/// still threads under its comment by name, in the saved file too.
#[test]
fn importing_the_same_xfdf_twice_updates_it() {
    let dir = scratch("import-twice");
    let xfdf = r#"<?xml version="1.0" encoding="UTF-8"?>
<xfdf xmlns="http://ns.adobe.com/xfdf/" xml:space="preserve"><annots>
<text page="0" rect="100,600,120,620" name="cy-1" title="Cy" date="D:20261003100000" creationdate="D:20261002090000"><contents>Look here</contents></text>
<text page="0" rect="100,600,120,620" name="di-1" title="Di" inreplyto="cy-1" date="D:20261004110000"><contents>Seen</contents></text>
</annots></xfdf>"#;
    let mut session = comments::open(&pdf(&dir), None).unwrap();
    let said = comments::import(&mut session, xfdf.as_bytes(), None).unwrap();
    assert_eq!(said, "2 added, 0 updated");
    let said = comments::import(&mut session, xfdf.as_bytes(), None).unwrap();
    assert_eq!(said, "0 added, 2 updated");
    let theirs = |doc: &sheaf_doc::Document| {
        let st = doc.state();
        let p0 = st.order()[0];
        let marks: Vec<(String, String, String, String, Option<String>)> = (st.marks_on(p0))
            .map(|a| {
                let parent = a.thread_parent().map(str::to_string);
                (
                    a.name(),
                    a.author.clone(),
                    a.created.clone(),
                    a.modified.clone(),
                    parent,
                )
            })
            .collect();
        marks
    };
    let want = vec![
        (
            "cy-1".to_string(),
            "Cy".to_string(),
            "D:20261002090000".to_string(),
            "D:20261003100000".to_string(),
            None,
        ),
        (
            "di-1".to_string(),
            "Di".to_string(),
            String::new(),
            "D:20261004110000".to_string(),
            Some("cy-1".to_string()),
        ),
    ];
    assert_eq!(theirs(&session.doc), want);
    let path = dir.join("imported.pdf");
    let to = SaveTo::File {
        path: path.clone(),
        append: false,
    };
    session
        .save(to, &AskAnswers::default(), &NoProgress)
        .unwrap();
    let back = comments::open(&path, None).unwrap();
    assert_eq!(theirs(&back.doc), want);
    let listing = comments::list(back.doc.state(), &CommentFilter::default(), "");
    let thread: Vec<&str> = (listing
        .threads()
        .find(|t| t.page == 0)
        .unwrap()
        .entries
        .iter())
    .map(|e| &*e.comment.name)
    .collect();
    assert_eq!(thread, ["cy-1", "di-1"]);
}

#[test]
fn the_csv_numbers_replies_under_their_comment() {
    let dir = scratch("csv");
    let doc = opened(&dir);
    let listing = comments::list(doc.state(), &CommentFilter::default(), "");
    let out = comments::csv(
        &listing,
        None,
        &[
            Column::Number,
            Column::Page,
            Column::Author,
            Column::Text,
            Column::ReplyTo,
        ],
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "Number,Page,Author,Text,Reply to",
            "1,1,Greg,First note (with parens),",
            "2,2,Ann,Check “this” (please),",
            "2.1,2,Bo,Done,2",
            "3,3,Greg,Tickmark ✓,",
        ]
    );
}

#[test]
fn a_measurement_shows_its_value_and_lists_it() {
    use sheaf_doc::measure::{Measured, Scale};
    let dir = scratch("measure");
    let mut doc = opened(&dir);
    let p0 = doc.state().order()[0];
    let feet = Scale {
        page: 1.0,
        page_unit: "in".into(),
        real: 10.0,
        unit: "ft".into(),
        precision: 2,
    };
    let m = mark(
        AnnotKind::Measure {
            what: Measured::Distance,
            points: vec![Pt { x: 100.0, y: 300.0 }, Pt { x: 244.0, y: 300.0 }],
            scale: Box::new(feet),
        },
        [0, 0, 255],
        "",
    );
    doc.commit_ops(vec![add(p0, &m)], "measure").unwrap();
    // Two inches at 1 in = 10 ft: drawn on the page with its value.
    let printed = doc.export_bytes(|f, _| Export::print(f, &[p0], true));
    let text = engine(printed).page_text(PageIndex(0)).unwrap();
    assert!(text.contains("20.00 ft"), "{text}");
    let listing = comments::list(doc.state(), &CommentFilter::default(), "");
    let out = comments::csv(
        &listing,
        None,
        &[Column::Type, Column::Measurement, Column::Scale],
    );
    assert!(out.contains("Distance,20.00 ft,1 in = 10 ft"), "{out}");
}

/// Another tool's link is one of the page's records, as its comments are,
/// the object it is, but it's no comment: the panel doesn't list it, and
/// a save keeps it.
#[test]
fn links_are_foreign_records() {
    let dir = scratch("links");
    let mut file = Pdf::load(pdf(&dir)).unwrap();
    let page = file.page_iter().next().unwrap();
    let link = file.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![300.into(), 300.into(), 400.into(), 400.into()],
        "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com") },
    });
    file.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![link.into()]);
    let path = dir.join("linked.pdf");
    file.save(&path).unwrap();
    let doc = comments::open(&path, None).unwrap().doc;
    let st = doc.state();
    let p0 = st.order()[0];
    let theirs: Vec<(String, Option<lopdf::ObjectId>)> = (st.foreign_on(p0))
        .map(|(_, f)| (f.summary.subtype.clone(), f.src.as_ref().map(|o| o.id)))
        .collect();
    assert_eq!(theirs, [("Link".to_string(), Some(link))]);
    // Ann's note, with Bo's reply, on page 2: nothing of page 1's.
    let listing = comments::list(st, &CommentFilter::default(), "");
    let pages: Vec<usize> = listing.threads().map(|t| t.page).collect();
    assert_eq!(pages, [1]);
    let out = sheaf_doc::save::rewrite(&doc).unwrap();
    let back = Pdf::load_mem(&out).unwrap();
    let page = back.page_iter().next().unwrap();
    let list = back.get_dictionary(page).unwrap().get(b"Annots").unwrap();
    let kept = list.as_array().unwrap().iter().any(|e| {
        let d = back.get_dictionary(e.as_reference().unwrap()).unwrap();
        d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Link".as_slice())
    });
    assert!(kept, "the link is kept");
}

/// Every corpus file's comments, as the panel lists them, are what the
/// file holds, as `file::annots_in` reads it: on each page, sheaf's marks
/// and other tools' comments, but for the parts grouped under a comment
/// on the page and any of theirs a mark has the name of (made editable);
/// their links aren't comments; and what it lists comes to the totals
/// measured. Skips without the corpus.
#[test]
fn the_comments_panel_lists_what_the_file_holds() {
    use std::collections::{BTreeMap, HashSet};

    use sheaf_doc::file::Found;
    use sheaf_doc::forms::corpus::{Corpus, each_entry};
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    // Each page's marks, by count, and other tools' comments, by name.
    type Page = (usize, Vec<String>);
    let entries = corpus.forms().unwrap();
    let outcomes = each_entry(&entries, |entry| {
        let doc = comments::open(&corpus.path(entry), None).ok()?.doc;
        let mut listed: BTreeMap<usize, Page> = BTreeMap::new();
        let listing = comments::list(doc.state(), &CommentFilter::default(), "");
        for t in listing.threads() {
            for e in &t.entries {
                let page = listed.entry(t.page).or_default();
                match e.comment.item {
                    comments::Item::Ours(..) => page.0 += 1,
                    comments::Item::Foreign(_) => page.1.push(e.comment.name.clone()),
                }
            }
        }
        let src = &doc.primary();
        let pages = (src.pages().iter()).map(|page| (page.id, page.geom.rotate));
        let read = sheaf_doc::file::annots_in(src.pdf(), pages);
        let mut held: BTreeMap<usize, Page> = BTreeMap::new();
        for (&index, list) in &read.pages {
            let ours: HashSet<String> = (list.iter())
                .filter_map(|a| match &a.found {
                    Found::Mark { annot, .. } => Some(annot.name()),
                    Found::Foreign(_) => None,
                })
                .collect();
            let theirs: Vec<_> = (list.iter())
                .filter_map(|a| match &a.found {
                    Found::Foreign(f) if !f.is_link() && !ours.contains(&f.name) => Some(f),
                    _ => None,
                })
                .collect();
            let names: HashSet<&str> = (ours.iter().map(String::as_str))
                .chain(theirs.iter().map(|f| f.name.as_str()))
                .collect();
            let shown = (theirs.iter())
                .filter(|f| !(f.grouped && f.parent.as_deref().is_some_and(|p| names.contains(p))));
            let page = (ours.len(), shown.map(|f| f.name.clone()).collect());
            if page != Page::default() {
                held.insert(index as usize, page);
            }
        }
        let comments = (listed.values())
            .map(|(ours, theirs)| ours + theirs.len())
            .sum();
        for page in listed.values_mut().chain(held.values_mut()) {
            page.1.sort();
        }
        Some((listed == held, listing.count, comments))
    });
    let opened: Vec<(bool, usize, usize)> = outcomes.iter().flatten().copied().collect();
    let differ: Vec<&str> = (entries.iter().zip(&outcomes))
        .filter(|(_, o)| o.is_some_and(|(same, ..)| !same))
        .map(|(e, _)| e.path.as_str())
        .collect();
    assert!(differ.is_empty(), "{differ:#?}");
    // What the frozen corpus lists, as measured at M10.5 slice 8, so a
    // change in what the reader finds shows too: files, those that open,
    // those with comments, threads and comments.
    let totals = (
        entries.len(),
        opened.len(),
        opened.iter().filter(|(_, threads, _)| *threads > 0).count(),
        opened.iter().map(|(_, threads, _)| threads).sum::<usize>(),
        opened.iter().map(|(.., comments)| comments).sum::<usize>(),
    );
    assert_eq!(
        totals,
        (4_392, 4_389, 233, 898, 943),
        "a corpus with files added needs these measured again"
    );
}
