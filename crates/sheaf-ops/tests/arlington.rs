//! What sheaf writes, held to the Arlington PDF Model (backlog F22): each
//! kind of object a save writes, on documents built in code, saved by a
//! Rewrite and by an Append (`sheaf_doc::both_ways!`), and every other
//! file sheaf writes: the exports (an extract, a clean copy, a flattened
//! copy, print's file with its markup and without, Remove Blanks'
//! rejects), Combine's file, the comment summary, pages made of images
//! and of a scan, blank pages, and the stamp library's stamps. Each is
//! checked with pdf-lint (sheaf-pdf/pdf-lint, 0.2.0+, run as a standalone
//! CLI binary), against the model it embeds, which it corrects where the
//! model contradicts ISO 32000-2, plus its content, font, color-space,
//! tree, page-tree, XMP and linearization checks. Each file is read as
//! pdf-lint reads one, from its bytes, decoding object streams,
//! cross-reference streams and content streams and decrypting what's
//! encrypted itself, so what a writer gets wrong below its objects is
//! found too (a `/Length` that doesn't end at `endstream`, a key written
//! twice). `SHEAF_PDF_LINT` names the binary, else `~/.local/share/sheaf-tools/pdf-lint/pdf-lint`
//! (`just setup-pdf-lint`), else `pdf-lint` on the `PATH`, and a run that
//! finds none at 0.2.0 or newer fails, saying so ([`lint::check`]). What
//! the version checked at deprecates is a finding, and a field merged with
//! its widget is checked as both tables ([`lint::options`]).
//!
//! What's held is what sheaf wrote ([`lint::written`]): an Append's
//! section, or the objects a Rewrite or an export derived, never a copy
//! of a file's object; a file sheaf makes whole of its own (blank pages,
//! an image's stamp) is held whole. Each object is built whole (a mark,
//! an appearance, a block and what it draws with: every key in it is
//! sheaf's) or merged over a source's dictionary (the catalog, a page, a
//! field), as its key says. A finding in a merged object that a file the
//! document read has too, in any of its objects, checked at the version of
//! the file written, is that file's, carried ([`answered`]); a reader's
//! finding in the file's own structure (its cross-references, header and
//! `%%EOF`) in what sheaf wrote is never carried ([`lint::own_structure`]).
//! An Append onto a file that was linearized as first written voids that
//! linearization by PDF's own rule (ISO 32000-2 F.1, Table F.1; `plan/between/`
//! item 4), which pdf-lint reports in the newest trailer (`Rule::Updated`);
//! `answered` excuses `Rule::Updated` on an Append and never on a Rewrite,
//! export or file made whole.
//! Any other fails its test unless `arlington_allowed.tsv` holds it for a
//! backlog entry, and a row there that no test needs fails too. Each test
//! names the tables it must have checked, so one that holds the wrong
//! objects fails too. A Rewrite or an export of a file that used object streams
//! (its cross-references a stream, or a hybrid's) packs objects into
//! object streams of its own, built whole and held, unless the encryption
//! it writes doesn't pack ([`packs`]): each test checks one as
//! `ObjectStream` exactly when what it saved packed. The encrypted forms
//! are written with a table and with a stream, so packed object streams
//! are checked encrypted too.
//!
//! The recovery sidecar isn't here: its record isn't kept, and the
//! emission that writes it is a save's, an Append's section or a
//! Rewrite's whole file, which the saves here check.
//!
//! Two files test the version: a PDF 1.3 file given what came later, and
//! a PDF 2.0 file given what 2.0 deprecates or asks more of.
//!
//! Every kind of button and choice, changed and its appearances made (M10
//! slice 8), is held on a form built in code ([`buttons_and_choices`]): a
//! check box's and a radio button's state dictionaries and the
//! ZapfDingbats font their marks are drawn in, a combo box's and a list
//! box's form, and a push button's `/N`, `/R` and `/D`.
//!
//! The corpus tier (ignored, as a corpus tier: `just tier-arlington`)
//! holds what sheaf writes on every form of Greg's corpus, never the
//! form's own objects: each is given a note, a header and footer and a
//! recognized word, saved as planned, then rewritten, and what each save
//! wrote is checked, a fault the form has carried as above. The header
//! draws a name in two fonts, so each save writes a subset of the
//! fallback font sheaf bundles, and its objects are checked too. A form
//! with check boxes or radio buttons whose file has no on state has a
//! save of its own, each of them pressed and the document rewritten, so
//! what's made of them is checked too ([`pressed_save`]). It prints what it
//! checked: objects by table, keys, the checks not made and why, the keys
//! the model doesn't list, and those the version deprecates or readers
//! ignore, and the widgets pressed and what was made of them; run on every
//! form, its counts of forms and saves are asserted ([`FORMS`]). Skips
//! when there's no corpus (see `sheaf_doc::forms::corpus`).

mod common;
mod lint;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use common::{How, StrategyKind, scratch};
use in_progress::NoProgress;
use lint::{
    Allowance, Allowances, Finding, Hold, Made, Options, Report, Rule, Unlisted, Version, check,
    file_version, options, own_structure, read, written,
};
use sheaf_doc::content::fixture::FALLBACK_TEXT;
use sheaf_doc::export::{Export, ExportFrom, Out, PRINT_ANSWERS, materialized};
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::forms::fixture::{Form, every_kind, every_mark_kind, mark_blobs, note};
use sheaf_doc::labels::LabelSpec;
use sheaf_doc::materialize::{Encryption, ObjKey};
use sheaf_doc::pdf::{LazyPdf, Object, ObjectId, Value, XrefKind, dictionary, literal};
use sheaf_doc::serialize::fixture as crypt;
use sheaf_doc::source::OpenError;
use sheaf_doc::store::{BlobKind, Origin};
use sheaf_doc::text_layer::fixture::layer;
use sheaf_doc::text_layer::from_engine;
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{
    Actor, AnnotKind, Blob, Box2, CommitOpts, Document, EditError, OcrWord, PageSize, Store,
};
use sheaf_edit::{
    BatesSpec, BlobSel, BookmarkPatch, Bring, GridPos, HeaderFooterSpec, LabelStyle, Margins, Op,
    PageRef, PageSource, Turn, WatermarkContent, WatermarkSpec, Z,
};
use sheaf_image::io::PageImage;
use sheaf_ops::combine::{CombineItem, CombineParams};
use sheaf_ops::comments::{CommentFilter, Layout};
use sheaf_ops::images::Paper;
use sheaf_ops::session::{SaveTo, Session};

/// The findings held for a backlog entry.
const ALLOWED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/arlington_allowed.tsv");

/// A file sheaf wrote, checked.
struct Checked {
    /// What wrote it, as its counts are printed.
    what: String,
    report: Report,
    /// The file's version, as it was checked at.
    version: Version,
    /// The checks of the files the document read, every object held as
    /// merged, at the version of the file written: what a merged object
    /// may carry ([`files_read`]).
    sources: Vec<Report>,
    /// Whether what wrote it packed objects into object streams, as a
    /// Rewrite or an export of a file that used them does ([`packs`]): its
    /// object streams are held, and checked as `ObjectStream`.
    packs: bool,
    /// Whether what wrote it was an incremental update (`StrategyKind::Append`),
    /// which intentionally voids a source's first-revision linearization.
    appended: bool,
}

/// Whether a Rewrite or an export of `doc`'s file, written encrypted as
/// `encryption` says, packs objects into object streams, as the object
/// streams plan has it: the file used them (decision 1: its
/// cross-references are a stream, or a hybrid's), and the encryption it
/// writes, if any, packs (decision 4: not an Identity `/StmF` under an
/// encrypting `/StrF`: `Crypt::packs`).
fn packs(doc: &Document, encryption: Encryption) -> bool {
    let (xref, crypt) = (doc.primary().xref(), doc.primary().crypt());
    let crypt = crypt.filter(|_| encryption == Encryption::Keep);
    (xref.kind == XrefKind::Stream || xref.hybrid) && crypt.is_none_or(|c| c.packs())
}

/// `bytes`, a file sheaf wrote, opened with `password` as sheaf opens one:
/// what its hold is read from ([`written`]).
fn reopened(bytes: &[u8], password: Option<&str>) -> Result<LazyPdf, String> {
    let limits = &sheaf_doc::source::Limits::DEFAULT.reader;
    LazyPdf::open(Arc::<[u8]>::from(bytes), password, limits)
        .map_err(|e| format!("the file written doesn't open: {e}"))
}

/// `bytes`, a file `what` wrote, opened with `password`, and checked,
/// holding what `hold` says of it; what a merged object may carry is what
/// the files `store` read have.
fn checked(
    what: &str,
    bytes: &[u8],
    password: Option<&str>,
    hold: impl FnOnce(&LazyPdf) -> Hold,
    store: &Store,
) -> Checked {
    let pdf = reopened(bytes, password).unwrap();
    let file = read(what, bytes, password);
    let mut report = check(&file, &hold(&pdf), &options());
    own_structure(&mut report, 0);
    let version = file_version(&file);
    Checked {
        what: what.into(),
        report,
        version,
        sources: files_read(store, version),
        packs: false,
        appended: false,
    }
}

/// Each file `store` read, checked with every object held as merged, at
/// `version`, a file's sheaf wrote: what a merged object of a document of
/// `store`'s, written in that file, may carry. What sheaf made in memory
/// carries nothing.
fn files_read(store: &Store, version: Version) -> Vec<Report> {
    let at = Options {
        version: Some(version),
        ..options()
    };
    let mut reports = Vec::new();
    for src in store.sources() {
        let BlobKind::Pdf {
            origin: Origin::File(path),
            ..
        } = src.blob().kind()
        else {
            continue;
        };
        let file = read(&path.display().to_string(), src.bytes(), src.password());
        reports.push(check(&file, &Hold::All(Made::Merged), &at));
    }
    reports
}

/// `doc` saved to `path` `how`, whatever the plan would decide, or as the
/// plan decides when it's None, and what the save wrote checked
/// ([`save_checked`]).
fn held(doc: &mut Document, how: Option<How>, path: &Path) -> Checked {
    let from = section_start(doc);
    let (strategy, bytes) = match how {
        Some(how) => common::saved(doc, path, how),
        None => common::planned(doc, path),
    };
    let (report, version) = save_checked(doc, from, strategy, &bytes).unwrap();
    Checked {
        what: format!("{strategy:?}"),
        report,
        version,
        sources: files_read(doc.store(), version),
        packs: strategy == StrategyKind::Rewrite && packs(doc, Encryption::Keep),
        appended: strategy == StrategyKind::Append,
    }
}

/// Where an Append of `doc` starts: its saved file's length, or, for a
/// document no save has read, its primary's, which an Append writes to
/// the path it saves to first.
fn section_start(doc: &Document) -> usize {
    match doc.saved_record() {
        Some(record) => record.stamp.len as usize,
        None => doc.primary().bytes().len(),
    }
}

/// What `doc`'s save just made, `strategy`, wrote in `bytes`, checked: an
/// Append's section, from byte `from` ([`section_start`] before the
/// save), or a Rewrite's derived objects, as the save's record says it was
/// written. And the file's version.
fn save_checked(
    doc: &Document,
    from: usize,
    strategy: StrategyKind,
    bytes: &[u8],
) -> Result<(Report, Version), String> {
    let record = doc.saved_record().ok_or("the save left no record")?;
    if record.strategy != strategy {
        return Err(format!(
            "saved as {strategy:?}, recorded as {:?}",
            record.strategy
        ));
    }
    let appended = (strategy == StrategyKind::Append).then_some((bytes, from));
    let password = doc.primary().password();
    let pdf = reopened(bytes, password)?;
    let hold = written(&record.map, doc.state(), doc.store(), &pdf, appended);
    let file = read(&format!("{strategy:?}"), bytes, password);
    let mut report = check(&file, &hold, &options());
    own_structure(&mut report, appended.map_or(0, |(_, from)| from));
    let Hold::Only(objects) = &hold else {
        return Err(format!("{strategy:?}: a save holds the objects it wrote"));
    };
    if objects.keys().all(|&id| Some(id) == report.trailer) {
        return Err(format!("{strategy:?}: nothing held but the trailer"));
    }
    Ok((report, file_version(&file)))
}

/// What `doc`'s last save wrote, a file of its own written whole, checked
/// as [`held`] checks a Rewrite: what a merged object may carry is what
/// the files `store` read have.
fn last_saved(what: &str, doc: &Document, store: &Store) -> Checked {
    let record = doc.saved_record().unwrap();
    assert_eq!(record.strategy, StrategyKind::Rewrite, "{what}");
    let bytes = std::fs::read(&*record.path).unwrap();
    let hold = |pdf: &LazyPdf| written(&record.map, doc.state(), doc.store(), pdf, None);
    Checked {
        packs: packs(doc, Encryption::Keep),
        ..checked(what, &bytes, doc.primary().password(), hold, store)
    }
}

/// The export `make` makes of `doc`'s current version, the user's, what
/// it asks answered as print answers it, and what it writes checked: the
/// objects it derived, never a copy of the document's file's objects,
/// which its numbering from 1 lists (as `ObjKey::Base`).
fn exported(what: &str, doc: &Document, make: impl FnOnce(ExportFrom) -> Export) -> Checked {
    let e = make(doc.export_from(Actor::User));
    let packs = packs(doc, e.profile.encryption);
    let (state, out) = materialized(e, &PRINT_ANSWERS, &NoProgress).unwrap();
    let hold = |pdf: &LazyPdf| written(&out.record.map, &state, doc.store(), pdf, None);
    let password = doc.primary().password();
    Checked {
        packs,
        ..checked(what, &out.bytes, password, hold, doc.store())
    }
}

/// The allowances file, read.
fn allowances() -> Allowances {
    Allowances::read(Path::new(ALLOWED)).unwrap()
}

/// `checked` found nothing sheaf answers for that no allowance holds, and
/// checked an object as each of `tables`, and as `ObjectStream` exactly
/// when what wrote it packed ([`Checked::packs`]). Its counts are printed.
fn clean(checked: &Checked, tables: &[&str]) {
    clean_against(checked, tables, &allowances());
}

/// [`clean`], against `allowances`.
fn clean_against(checked: &Checked, tables: &[&str], allowances: &Allowances) {
    let sources: Vec<&Report> = checked.sources.iter().collect();
    let (what, report) = (&checked.what, &checked.report);
    println!("{what}: {report}");
    let beyond = answered(report, &sources, checked.appended);
    let left = left(&beyond, checked.version, allowances);
    println!(
        "{} findings: {} carried from the files read, {} allowed",
        report.findings.len(),
        report.findings.len() - beyond.len(),
        beyond.len() - left.len()
    );
    assert!(
        left.is_empty(),
        "{what}: {} findings no allowance holds:\n{}",
        left.len(),
        left.join("\n")
    );
    let unchecked: Vec<&&str> = (tables.iter())
        .filter(|t| !report.checked.contains_key(**t))
        .collect();
    assert!(
        unchecked.is_empty(),
        "{what}: nothing checked as {unchecked:?}"
    );
    let streams = report.checked.contains_key("ObjectStream");
    assert_eq!(
        streams, checked.packs,
        "{what}: object streams checked: {streams}, packed: {}",
        checked.packs
    );
}

/// The findings of `report`, of what sheaf wrote, that sheaf answers for:
/// all of those in an object it built, and of those in one it merged over
/// a source's dictionary, any that no object of the files read, `sources`,
/// has a like one of (`Report::beyond`; matching each object to its own
/// source is backlog B46's), except `Rule::Updated` in the merged trailer
/// of an `Append` (`appended`), which by ISO 32000-2 F.1 Table F.1 voids a
/// source's first-revision linearization.
fn answered<'a>(report: &'a Report, sources: &[&Report], appended: bool) -> Vec<&'a Finding> {
    report
        .beyond(sources)
        .into_iter()
        .filter(|f| !(appended && f.made == Made::Merged && f.rule == Rule::Updated))
        .collect()
}

/// The findings of `beyond`, in a file of `version` a test made, that no
/// allowance of `allowances` holds.
fn left(beyond: &[&Finding], version: Version, allowances: &Allowances) -> Vec<String> {
    (beyond.iter())
        .filter(|f| allowances.allows(f, version, None).is_none())
        .map(ToString::to_string)
        .collect()
}

/// A form of `pages` pages, written to a new directory for test `name`,
/// and opened: its path, and the document.
fn opened(name: &str, pages: usize) -> (std::path::PathBuf, Document) {
    let dir = scratch(&format!("arlington-{name}"));
    let path = common::write(&dir, &format!("{name}.pdf"), &mut Form::new(pages));
    let doc = common::open(&path);
    (path, doc)
}

sheaf_doc::both_ways!(every_mark_kind_holds, every_mark_kind_holds_appended: every_mark);

/// Every kind of mark: each annotation, its appearance and what that
/// draws with, and the attachment's file.
fn every_mark(how: How) {
    clean(&marked(&format!("marks-{how:?}"), how), MARKS);
}

/// A form of two pages, written for test `name`, given one of every kind
/// of mark ([`every_mark_on`]) and saved `how`: what the save wrote,
/// checked.
fn marked(name: &str, how: How) -> Checked {
    let (path, mut doc) = opened(name, 2);
    let _held = mark_blobs(doc.store());
    every_mark_on(&mut doc);
    held(&mut doc, Some(how), &path)
}

/// The tables [`every_mark`] must check.
const MARKS: &[&str] = &[
    "AnnotText",
    "AnnotFreeText",
    "AnnotLine",
    "AnnotSquare",
    "AnnotCircle",
    "AnnotPolygon",
    "AnnotPolyLine",
    "AnnotHighlight",
    "AnnotUnderline",
    "AnnotSquiggly",
    "AnnotStrikeOut",
    "AnnotCaret",
    "AnnotStamp",
    "AnnotInk",
    "AnnotFileAttachment",
    "Appearance",
    "XObjectFormType1",
    "Resource",
    "GraphicsStateParameter",
    "FontType1",
    "FileSpecification",
    "EmbeddedFileStream",
];

/// One of every kind of mark ([`every_mark_kind`]) made on `doc`'s first
/// page, two pages or more, the stamp's page and the attachment's file
/// held in its store: a connector from it to the second page, and a reply
/// and a status on the note.
fn every_mark_on(doc: &mut Document) {
    let [p0, p1] = [0, 1].map(|i| doc.page(i).unwrap());
    let every = every_mark_kind();
    let mut ops = Vec::new();
    for a in every.iter().filter(|a| a.thread_parent().is_none()) {
        ops.push(match a.kind {
            AnnotKind::Connector { .. } => fx::connect((p0, a), (p1, a)),
            _ => fx::add(p0, a),
        });
    }
    doc.commit_ops(ops, "Marks").unwrap();
    let note = (doc.state().marks_on(p0))
        .find(|a| matches!(a.kind, AnnotKind::Note { .. }))
        .unwrap()
        .name();
    let threads = (every.iter().filter(|a| a.thread_parent().is_some()))
        .map(|a| {
            let mut a = a.clone();
            match &mut a.kind {
                AnnotKind::Reply { parent } | AnnotKind::Status { parent, .. } => {
                    *parent = note.clone()
                }
                other => panic!("{other:?} isn't in a thread"),
            }
            fx::add(p0, &a)
        })
        .collect();
    doc.commit_ops(threads, "Replies").unwrap();
}

sheaf_doc::both_ways!(generated_blocks_hold, generated_blocks_hold_appended: blocks);

/// A header with Bates numbers on every page, a text watermark over them
/// and an image under them: each block, the fonts and graphics states it
/// draws with, and the image.
fn blocks(how: How) {
    let (path, mut doc) = opened(&format!("blocks-{how:?}"), 2);
    let rgb = image::RgbImage::from_fn(8, 4, |x, _| image::Rgb([30 * x as u8, 90, 160]));
    let png = sheaf_image::encode::png(&rgb, 72).unwrap();
    let blob = (doc.store())
        .intern(png.as_slice().into(), BlobKind::image(&png).unwrap())
        .unwrap();
    let image = WatermarkSpec {
        content: WatermarkContent::Image {
            image: Some(BlobSel::Id(blob.id())),
            scale: 0.5,
            opacity: 0.6,
        },
        position: GridPos::BottomRight,
    };
    let bates = HeaderFooterSpec {
        bates: Some(BatesSpec {
            prefix: "SHEAF".into(),
            suffix: String::new(),
            digits: 6,
            start: 1,
        }),
        ..fx::header("<<bates>> of <<pages>>")
    };
    let ops = vec![
        fx::set_header(bates, None),
        fx::set_watermark(fx::watermark("Draft"), Z::Over, None),
        fx::set_watermark(image, Z::Under, None),
    ];
    doc.commit_ops(ops, "Blocks").unwrap();
    let checked = held(&mut doc, Some(how), &path);
    clean(
        &checked,
        &[
            "PageObject",
            "XObjectImage",
            "Resource",
            "GraphicsStateParameter",
            "FontType1",
        ],
    );
}

sheaf_doc::both_ways!(text_in_two_fonts_holds, text_in_two_fonts_holds_appended: two_fonts);

/// The tables [`two_fonts`] must check: the text box, and the subset of
/// the fallback font sheaf bundles, its Type 0 font, CIDFont, descriptor,
/// program, `/ToUnicode`, `/CIDSystemInfo` and widths (its `/CIDToGIDMap`
/// stream is checked as a `Stream`).
const TWO_FONTS: &[&str] = &[
    "AnnotFreeText",
    "FontType0",
    "FontCIDType2",
    "FontDescriptorCIDType2",
    "FontFile2",
    "ToUnicodeCMapStream",
    "CIDSystemInfo",
    "ArrayOfCIDGlyphMetricsW",
];

/// A text box and a header saying [`FALLBACK_TEXT`], which Helvetica draws
/// in part and the fallback font sheaf bundles the rest of (M10 slice 7):
/// the subset of it the save writes, each of its objects.
fn two_fonts(how: How) {
    let (path, mut doc) = opened(&format!("two-fonts-{how:?}"), 1);
    let page = doc.page(0).unwrap();
    let mut text = note();
    text.kind = AnnotKind::FreeText {
        rect: Box2 {
            x: 72.0,
            y: 300.0,
            w: 300.0,
            h: 40.0,
        },
        size: 12.0,
    };
    text.contents = FALLBACK_TEXT.into();
    let ops = vec![
        fx::add(page, &text),
        fx::set_header(fx::header(FALLBACK_TEXT), None),
    ];
    let c = doc.commit_ops(ops, "Two fonts").unwrap();
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    clean(&held(&mut doc, Some(how), &path), TWO_FONTS);
}

sheaf_doc::both_ways!(a_page_watermark_holds, a_page_watermark_holds_appended: page_watermark);

/// Another PDF's page drawn under every page: the form made of it.
fn page_watermark(how: How) {
    let (path, mut doc) = opened(&format!("page-watermark-{how:?}"), 2);
    let art = (doc.store())
        .open(
            Form::new(1).bytes().into(),
            BlobKind::file(Path::new("art.pdf"), None),
        )
        .unwrap();
    let spec = WatermarkSpec {
        content: WatermarkContent::Page {
            pdf: Some(BlobSel::Id(art.id())),
            page: 1,
            scale: 0.8,
            opacity: 0.5,
        },
        position: GridPos::Center,
    };
    doc.commit_ops(vec![fx::set_watermark(spec, Z::Under, None)], "Art")
        .unwrap();
    let checked = held(&mut doc, Some(how), &path);
    clean(
        &checked,
        &["PageObject", "XObjectFormType1", "GraphicsStateParameter"],
    );
}

sheaf_doc::both_ways!(recognized_text_holds, recognized_text_holds_appended: recognized);

/// Recognized text on a page: its block and the glyphless font it draws
/// with, its descendant, descriptor and program.
fn recognized(how: How) {
    let (path, mut doc) = opened(&format!("ocr-{how:?}"), 1);
    let p0 = doc.page(0).unwrap();
    doc.commit_ops(fx::ocr([(p0, Arc::new(layer()))]), "OCR")
        .unwrap();
    let checked = held(&mut doc, Some(how), &path);
    clean(
        &checked,
        &[
            "PageObject",
            "FontType0",
            "FontCIDType2",
            "FontDescriptorCIDType2",
            "FontFile2",
        ],
    );
}

sheaf_doc::both_ways!(outline_and_labels_hold, outline_and_labels_hold_appended: outline_and_labels);

/// Bookmarks added, one under another and one renamed, and page labels:
/// the outline's root and items, and the labels' number tree.
fn outline_and_labels(how: How) {
    let (path, mut doc) = opened(&format!("outline-{how:?}"), 3);
    let [p0, p1, p2] = [0, 1, 2].map(|i| doc.page(i).unwrap());
    let ops = vec![
        fx::bookmark(None, None, "First", Some(p0)),
        fx::labels(&[
            LabelSpec {
                start: p0,
                style: LabelStyle::LowerRoman,
                prefix: String::new(),
                first: 1,
            },
            LabelSpec {
                start: p1,
                style: LabelStyle::Decimal,
                prefix: "A-".into(),
                first: 1,
            },
        ]),
    ];
    doc.commit_ops(ops, "Outline").unwrap();
    let first = doc.state().bookmarks_preorder()[0].1;
    let ops = vec![
        fx::bookmark(Some(first), None, "Under", Some(p2)),
        fx::bookmark(None, Some(first), "Second", Some(p1)),
    ];
    doc.commit_ops(ops, "More").unwrap();
    let renamed = BookmarkPatch {
        title: Some("Renamed".into()),
        ..Default::default()
    };
    doc.commit_ops(vec![fx::patch_bookmark(first, renamed)], "Rename")
        .unwrap();
    let checked = held(&mut doc, Some(how), &path);
    clean(
        &checked,
        &[
            "Catalog",
            "Outline",
            "OutlineItem",
            "number-tree",
            "PageLabel",
        ],
    );
}

sheaf_doc::both_ways!(pages_hold, pages_hold_appended: pages);

/// Pages turned, cropped, duplicated and moved, another file's page
/// inserted and a blank page made in memory: each page, the page tree,
/// and another tool's comment and its pop-up, copied with the page
/// duplicated.
fn pages(how: How) {
    let dir = scratch(&format!("arlington-pages-{how:?}"));
    let mut f = Form::new(3);
    let comment = f.pdf.new_object_id();
    let popup = f.pdf.add_object(dictionary! {
        "Type" => Object::name("Annot"), "Subtype" => Object::name("Popup"), "Parent" => comment,
        "Rect" => vec![300.into(), 600.into(), 500.into(), 700.into()],
    });
    let page = f.pages[0];
    f.pdf.objects.insert(
        comment,
        dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Text"), "P" => page, "Popup" => popup,
            "Rect" => vec![72.into(), 600.into(), 92.into(), 620.into()],
            "Contents" => literal("Theirs"),
            "NM" => literal("theirs"),
        }
        .into(),
    );
    f.set_annots(0, &[comment, popup]);
    let path = common::write(&dir, "pages.pdf", &mut f);
    let other = common::write(&dir, "other.pdf", &mut Form::new(1));
    let mut doc = common::open(&path);
    let other = doc.store().read_file(&other, None).unwrap().0;
    let size = PageSize {
        width: 612.0,
        height: 792.0,
    };
    let blank = sheaf_doc::memory::blank(doc.store(), size, 1).unwrap();
    let [p0, p1, p2] = [0, 1, 2].map(|i| doc.page(i).unwrap());
    let margins = Margins {
        top: 36.0,
        bottom: 36.0,
        left: 18.0,
        right: 18.0,
    };
    let ops = vec![
        fx::rotate(&[p0], Turn::Quarter),
        fx::crop(&[p1], margins),
        fx::duplicate(&[p0]),
        fx::move_pages(&[p2], Some(p0)),
        fx::insert(&other, [0], "other.pdf", None),
        fx::insert(&blank, [0], "blank", Some(p1)),
    ];
    doc.commit_ops(ops, "Pages").unwrap();
    let checked = held(&mut doc, Some(how), &path);
    clean(
        &checked,
        &["PageObject", "PageTreeNodeRoot", "AnnotText", "AnnotPopup"],
    );
}

sheaf_doc::both_ways!(buttons_and_choices_hold, buttons_and_choices_hold_appended: buttons_and_choices);

/// The tables [`buttons_and_choices`] must check: each kind of field and
/// its widget, the `/AP` dictionaries and their state dictionaries, the
/// forms made, and the ZapfDingbats font the marks are drawn in. A radio
/// field apart from its widgets is checked only in an Append, which writes
/// it, its dictionary as its file has it, for its value (`/V` isn't written
/// before M10 slice 9); a Rewrite copies it.
const BUTTONS: &[&str] = &[
    "FieldBtnCheckbox",
    "FieldChoice",
    "FieldBtnPush",
    "AnnotWidget",
    "Appearance",
    "AppearanceSubDict",
    "XObjectFormType1",
    "FontType1",
];

/// Every kind of button and choice changed as no op changes one before M10
/// slice 9 (`tx::fixture`), each widget's appearances made by the save
/// (M10 slice 8): a check box, one object with its widget, and a radio
/// group of two kid widgets, under `/Opt`, each with no appearances,
/// pressed (`/N` and `/D` state dictionaries, `/AS`, the marks in
/// ZapfDingbats); a combo box and a list box given values (`/N`); and a
/// push button with `/H /P` given three captions (`/N`, `/R` and `/D`).
/// The check box is required (`/Ff 2`), as [`apart`] says why.
fn buttons_and_choices(how: How) {
    use sheaf_doc::forms::fixture::{Button, RADIO, row};
    use sheaf_doc::materialize::key::Part;
    let dir = scratch(&format!("arlington-buttons-{how:?}"));
    let mut f = Form::new(1);
    let check = f.check_box("check", 0, row(0), false);
    f.dict(check).remove(b"AP");
    f.dict(check).set("Ff", 2);
    let off = |i| Button::at(i, None, Some("Off"));
    let (pick, _) = f.buttons("pick", RADIO, &[off(1), off(2)]);
    f.dict(pick).set("Opt", vec![literal("a"), literal("b")]);
    f.choice("combo", 0, row(3), &["alpha", "bravo"], true);
    f.choice("list", 0, row(4), &["alpha", "bravo", "charlie"], false);
    let push = f.push_button("push", 0, row(5), "Go");
    f.dict(push).set("H", Object::name("P"));
    let path = common::write(&dir, "buttons.pdf", &mut f);
    let mut doc = common::open(&path);
    let widgets = |doc: &Document, name: &str| {
        let st = doc.state();
        let id = st.field_named(name).expect("the field");
        let term = st.field(id).and_then(|f| f.term.clone());
        let widgets = term.expect("a terminal").widgets.clone();
        widgets.into_iter().collect::<Vec<_>>()
    };
    let check = widgets(&doc, "check")[0];
    fx::press(&mut doc, check);
    let second = widgets(&doc, "pick")[1];
    fx::press(&mut doc, second);
    fx::choose(&mut doc, "combo", &["bravo"]);
    fx::choose(&mut doc, "list", &["charlie"]);
    let push = widgets(&doc, "push")[0];
    fx::caption(&mut doc, push, Some("Stop"), Some("Over"), Some("Down"));
    let checked = held(&mut doc, Some(how), &path);
    let mut tables = BUTTONS.to_vec();
    if how == How::Appended {
        tables.push("FieldBtnRadio");
    }
    clean(&checked, &tables);
    // The forms the save made, each widget's: the check box's on state's
    // normal and down ones and `Off`'s down one (the pressed gray; its
    // normal `Off` paints nothing, with no `/MK`), the radio group's
    // pressed widget's the same and the other's none (its state didn't
    // change), a form each choice, and the push button's three.
    let record = doc.saved_record().expect("saved");
    let made: Vec<usize> = (["check", "pick", "combo", "list", "push"].iter())
        .flat_map(|name| widgets(&doc, name))
        .map(|aid| {
            (Part::ALL.into_iter())
                .filter(|&part| (record.map.keys).contains_key(&ObjKey::AnnotPart(aid, part)))
                .count()
        })
        .collect();
    assert_eq!(made, [3, 0, 3, 1, 1, 3]);
}

/// A page deleted, saved as the plan decides: a Rewrite, strict removal
/// leaving nothing of the page, which holds the page tree it writes.
#[test]
fn a_page_deleted_holds() {
    let (path, mut doc) = opened("deleted", 3);
    let p1 = doc.page(1).unwrap();
    doc.commit_ops(vec![fx::delete(&[p1])], "Delete").unwrap();
    let checked = held(&mut doc, None, &path);
    assert_eq!(checked.what, "Rewrite", "something's gone");
    clean(&checked, &["Catalog", "PageTreeNodeRoot", "PageObject"]);
}

sheaf_doc::both_ways!(merged_fields_hold, merged_fields_hold_appended: merged_fields);

/// Every kind of field, two other forms' with their pages, inserted into a
/// form's document: one whose fields are each one object with their
/// widget, and one whose fields are apart from them. The form, each field
/// and widget merged into it, and the appearances they bring. A field one
/// object with its widget is held to both tables at once, the widget's and
/// the field's by its `/FT` and `/Ff`, from the page's `/Annots` or the
/// form's `/Fields` alike, as pdf-lint holds one by default: the objects
/// held as two tables are counted, and so pinned (the Rewrite's 12, the
/// Append's 8), so a check that took one table fails.
fn merged_fields(how: How) {
    let dir = scratch(&format!("arlington-fields-{how:?}"));
    let path = common::write(&dir, "form.pdf", &mut common::two_page_form());
    let mut doc = common::open(&path);
    let forms = [
        ("merged.pdf", every_kind()),
        ("apart.pdf", apart(every_kind(), "form2[0]")),
    ];
    let srcs = forms.map(|(name, mut form)| {
        let file = common::write(&dir, name, &mut form);
        (name, doc.store().read_file(&file, None).unwrap().0)
    });
    let inserts = (srcs.iter())
        .map(|(name, src)| fx::insert(src, [0, 1], name, None))
        .collect();
    doc.commit_ops(inserts, "Insert").unwrap();
    let checked = held(&mut doc, Some(how), &path);
    clean(
        &checked,
        &[
            "InteractiveForm",
            "Field",
            "FieldTx",
            "FieldBtnCheckbox",
            "FieldBtnRadio",
            "FieldBtnPush",
            "FieldChoice",
            "FieldSig",
            "AnnotWidget",
            "AppearanceCharacteristics",
        ],
    );
    let two_tables = match how {
        How::Rewritten => 12,
        How::Appended => 8,
    };
    assert_eq!(checked.report.two_tables, two_tables, "{}", checked.report);
}

/// `f` with each field that's one object with its widget made two, the
/// widget the field's kid (ISO 32000-1 12.7.3.1), and its top field
/// renamed `to`. Its check boxes are required (`/Ff 2`): the model's
/// tables of buttons differ only in `/Ff`'s bits, so a check box without
/// one ties a push button's table, which is listed first, and is held to
/// that, as TestGrammar holds it (the tie counted).
fn apart(mut f: Form, to: &str) -> Form {
    const WIDGET: [&[u8]; 9] = [
        b"Type", b"Subtype", b"Rect", b"P", b"AP", b"AS", b"MK", b"F", b"BS",
    ];
    for page in 0..f.pages.len() {
        let mut annots = f.annots(page);
        for at in annots.iter_mut() {
            let field = f.dict(*at);
            if !field.has(b"T") {
                continue;
            }
            let mut widget = dictionary! { "Parent" => *at };
            for key in WIDGET {
                if let Some(value) = field.remove(key) {
                    widget.set(key, value);
                }
            }
            let widget = f.pdf.add_object(widget);
            let field = f.dict(*at);
            field.set("Kids", vec![widget.into()]);
            let button = field.get(b"FT").and_then(Object::as_name) == Some(b"Btn");
            if button && !field.has(b"Ff") {
                field.set("Ff", 2);
            }
            *at = widget;
        }
        f.set_annots(page, &annots);
    }
    let top = f.fields()[0];
    f.dict(top).set("T", literal(to));
    f
}

sheaf_doc::both_ways!(encrypted_files_hold, encrypted_files_hold_appended: encrypted_files);

/// A form encrypted with RC4 (40- and 128-bit), AES-128 and AES-256, its
/// cross-references a table and a stream, each given a note: the trailer
/// and its `/ID`, and the encryption dictionary a Rewrite writes as the
/// file stores it, which an Append leaves where it is; a stream file's
/// Rewrite packs, each object stream encrypted as a stream.
fn encrypted_files(how: How) {
    let dir = scratch(&format!("arlington-encrypted-{how:?}"));
    let plain = crypt::form();
    let states =
        std::iter::once(crypt::rc4_40(&plain)).chain(crypt::rc4_aes_128_and_aes_256(&plain));
    for (name, state) in states {
        let written = crypt::encrypted(&plain, &state, |_| {});
        for ((kind, trailer), bytes) in [("table", "FileTrailer"), ("stream", "XRefStream")]
            .into_iter()
            .zip(written)
        {
            let path = dir.join(format!("{name}-{kind}.pdf"));
            std::fs::write(&path, bytes).unwrap();
            let mut doc = sheaf_doc::open::file(&path, Some(crypt::USER)).unwrap().doc;
            let p0 = doc.page(0).unwrap();
            doc.commit_ops(vec![fx::add(p0, &note())], "Note").unwrap();
            let checked = held(&mut doc, Some(how), &path);
            let mut tables = vec![trailer, "TrailerIDArray", "AnnotText"];
            if how == How::Rewritten {
                tables.push("EncryptionStandard");
            }
            println!("{name}, {kind}:");
            clean(&checked, &tables);
        }
    }
}

sheaf_doc::both_ways!(a_pdf_1_3_file_holds, a_pdf_1_3_file_holds_appended: pdf_1_3);

/// A PDF 1.3 file, its cross-references a table as a 1.3 file's are,
/// given what came later: a header and footer, marked content whose
/// property lists came in 1.4, and recognized text in a CIDFont, 1.2's,
/// which the save says the file needs; and a note, whose `/NM`, `/CA` and
/// appearance's transparency, 1.4's, the header's version covers, but
/// whose `/CreationDate` came in 1.5, which no mark raises the version
/// for yet (backlog B42: marks will, when the save has no signatures to
/// keep).
fn pdf_1_3(how: How) {
    clean(&old(&format!("1.3-{how:?}"), how), PDF_1_3);
}

/// The tables [`pdf_1_3`] must check.
const PDF_1_3: &[&str] = &[
    "FileTrailer",
    "AnnotText",
    "FontType0",
    "FontCIDType2",
    "FontType1",
];

/// [`pdf_1_3`]'s file, written for test `name` and saved `how`: what the
/// save wrote, checked.
fn old(name: &str, how: How) -> Checked {
    let dir = scratch(&format!("arlington-{name}"));
    let mut f = Form::new(1);
    f.pdf.version = "1.3".into();
    f.pdf.xref_kind = XrefKind::Table;
    let path = common::write(&dir, "old.pdf", &mut f);
    let mut doc = common::open(&path);
    let p0 = doc.page(0).unwrap();
    let mut ops = fx::ocr([(p0, Arc::new(layer()))]);
    ops.extend([
        fx::set_header(fx::header("Page <<page>>"), None),
        fx::add(p0, &note()),
    ]);
    doc.commit_ops(ops, "Later").unwrap();
    held(&mut doc, Some(how), &path)
}

sheaf_doc::both_ways!(a_pdf_2_0_file_holds, a_pdf_2_0_file_holds_appended: pdf_2_0);

/// A PDF 2.0 file given a header and footer, a text watermark, a note and
/// recognized text: what 2.0 deprecates, and asks for that earlier
/// versions didn't, as the widths and descriptor of the standard 14 fonts
/// sheaf draws with, which it doesn't write (backlog B43).
fn pdf_2_0(how: How) {
    clean(&new(&format!("2.0-{how:?}"), how), PDF_2_0);
}

/// The tables [`pdf_2_0`] must check.
const PDF_2_0: &[&str] = &[
    "AnnotText",
    "Appearance",
    "FontType0",
    "FontType1",
    "GraphicsStateParameter",
];

/// [`pdf_2_0`]'s file, written for test `name` and saved `how`: what the
/// save wrote, checked.
fn new(name: &str, how: How) -> Checked {
    let dir = scratch(&format!("arlington-{name}"));
    let mut f = Form::new(1);
    f.pdf.version = "2.0".into();
    let path = common::write(&dir, "new.pdf", &mut f);
    let mut doc = common::open(&path);
    let p0 = doc.page(0).unwrap();
    let mut ops = fx::ocr([(p0, Arc::new(layer()))]);
    ops.extend([
        fx::set_header(fx::header("Page <<page>>"), None),
        fx::set_watermark(fx::watermark("Draft"), Z::Over, None),
        fx::add(p0, &note()),
    ]);
    doc.commit_ops(ops, "Later").unwrap();
    held(&mut doc, Some(how), &path)
}

/// Every row of the allowances for any file is needed by a default test:
/// the marks', the PDF 1.3 file's and the PDF 2.0 file's checks, each
/// against the one set of allowances, use them all. A row no longer
/// needed (the model or sheaf fixed) fails here, and goes.
#[test]
fn every_allowance_for_any_file_is_needed() {
    let allowances = allowances();
    let rewritten = How::Rewritten;
    for (checked, tables) in [
        (marked("needed-marks", rewritten), MARKS),
        (old("needed-1.3", rewritten), PDF_1_3),
        (new("needed-2.0", rewritten), PDF_2_0),
    ] {
        clean_against(&checked, tables, &allowances);
    }
    let unneeded: Vec<String> = (allowances.unused().into_iter())
        .filter(|a| a.file.is_none())
        .map(named)
        .collect();
    assert!(
        unneeded.is_empty(),
        "rows no default test needs:\n{}",
        unneeded.join("\n")
    );
}

/// Every allowance's reason starts with the backlog entry it's held for
/// (`B12: …`, or `F12: …`), and the entry is open: a file
/// `plan/backlog/B12.md` (a closed entry's moves to
/// `plan/complete/backlog-closed.md`).
#[test]
fn every_allowance_names_an_open_backlog_entry() {
    let backlog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plan/backlog");
    let open = |a: &&Allowance| {
        let entry = entry(a);
        let number = (entry.strip_prefix('B')).or_else(|| entry.strip_prefix('F'));
        number.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            && backlog.join(format!("{entry}.md")).is_file()
    };
    let unknown: Vec<String> = (allowances().rows())
        .filter(|a| !open(a))
        .map(named)
        .collect();
    assert!(
        unknown.is_empty(),
        "rows whose reason doesn't start with an entry open in plan/backlog/:\n{}",
        unknown.join("\n")
    );
}

/// The carried rule ([`answered`]) matches a merged object's finding
/// against every object of the sources, not the one it was emitted over
/// (backlog B46): a page sheaf wrote without its `/MediaBox` is carried
/// when any page of a source lacks one. Matching each object to its own
/// source is B46's, and flips this.
#[test]
fn a_merged_objects_finding_is_carried_from_any_object_of_a_source() {
    let missing = |n: u32, made: Made| Finding {
        object: Some(ObjectId::new(n, 0)),
        made,
        path: format!("Root/Pages/Kids[{n}]"),
        table: "PageObject".to_string(),
        key: "MediaBox".to_string(),
        rule: Rule::Missing,
        value: "absent".to_string(),
        at: None,
    };
    let written = Report {
        findings: vec![missing(3, Made::Merged), missing(4, Made::Built)],
        ..Report::default()
    };
    let another_page = Report {
        findings: vec![missing(9, Made::Merged)],
        ..Report::default()
    };
    let answered = answered(&written, &[&another_page], false);
    assert_eq!(answered, [&missing(4, Made::Built)]);
}

/// An Append onto a file that was linearized as first written voids its
/// linearization (`Rule::Updated` in the merged trailer), which `answered`
/// excuses only on an Append; a Rewrite clears `/Linearized` and would fail
/// if `Rule::Updated` appeared in it.
#[test]
fn an_append_voids_first_revision_linearization_and_a_rewrite_never_excuses_it() {
    let voided = Finding {
        object: None,
        made: Made::Merged,
        path: "trailer".to_string(),
        table: "FileTrailer".to_string(),
        key: "-".to_string(),
        rule: Rule::Updated,
        value: "the file was linearized as first written, its first 1024 bytes; the update after it, its section at byte 1024, voids that: it's read as an ordinary file (ISO 32000-2, F.1, Table F.1)".to_string(),
        at: None,
    };
    let report = Report {
        findings: vec![voided.clone()],
        ..Report::default()
    };
    assert!(answered(&report, &[], true).is_empty());
    assert_eq!(answered(&report, &[], false), [&voided]);
}

/// A fault in the file's own structure that sheaf wrote is sheaf's even
/// when the file it saved over has one like it, where pdf-lint holds a
/// reader's finding of no object as merged ([`own_structure`]): each
/// cross-reference row cut to 19 bytes, in an Append's section and in a
/// Rewrite's file, over a form whose rows are 19 bytes too. The form's
/// own, before the Append's section, is carried.
#[test]
fn a_fault_in_the_structure_sheaf_writes_is_never_carried() {
    for how in [How::Appended, How::Rewritten] {
        let dir = scratch(&format!("arlington-rows-{how:?}"));
        let mut f = Form::new(1);
        f.pdf.xref_kind = XrefKind::Table;
        let path = dir.join("form.pdf");
        std::fs::write(&path, misshapen(&f.bytes(), 0)).unwrap();
        let mut doc = common::open(&path);
        let p0 = doc.page(0).unwrap();
        doc.commit_ops(vec![fx::add(p0, &note())], "Note").unwrap();
        let from = section_start(&doc);
        let (strategy, bytes) = common::saved(&mut doc, &path, how);
        let append = strategy == StrategyKind::Append;
        let from = if append { from } else { 0 };
        let bytes = misshapen(&bytes, from);
        let (report, version) = save_checked(&doc, from, strategy, &bytes).unwrap();
        let rows = |report: &Report| {
            (report.findings.iter())
                .filter(|f| f.rule == Rule::Reader && f.value.contains("20 bytes"))
                .count()
        };
        let sources = files_read(doc.store(), version);
        assert_eq!(
            sources.iter().map(rows).sum::<usize>(),
            1,
            "the form's rows"
        );
        let sources: Vec<&Report> = sources.iter().collect();
        let answered = Report {
            findings: answered(&report, &sources, append)
                .into_iter()
                .cloned()
                .collect(),
            ..Report::default()
        };
        assert_eq!(rows(&answered), 1, "{strategy:?}: {report}");
        let carried = rows(&report) - rows(&answered);
        assert_eq!(carried, usize::from(append), "{strategy:?}: {report}");
    }
}

/// `bytes` with each cross-reference table's rows from byte `from` on
/// cut to 19 bytes, the last before their end of line dropped and the
/// end of line a lone `\n`, as some writers write them. The tables'
/// `xref` keywords, which `startxref` and `/Prev` name, don't move.
fn misshapen(bytes: &[u8], from: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(at) = (from.max(i)..bytes.len()).find(|&j| bytes[j..].starts_with(b"\nxref")) {
        let mut j = at + b"\nxref".len();
        while matches!(bytes[j], b'\r' | b'\n') {
            j += 1;
        }
        out.extend_from_slice(&bytes[i..j]);
        loop {
            let end = j + bytes[j..].iter().position(|&b| b == b'\n').unwrap() + 1;
            let header: Vec<&str> = (std::str::from_utf8(&bytes[j..end]).unwrap_or(""))
                .split_whitespace()
                .collect();
            let [_, count] = header[..] else {
                break;
            };
            out.extend_from_slice(&bytes[j..end]);
            j = end;
            for _ in 0..count.parse::<usize>().unwrap() {
                out.extend_from_slice(&bytes[j..j + 18]);
                out.push(b'\n');
                j += 20;
            }
        }
        i = j;
    }
    out.extend_from_slice(&bytes[i..]);
    out
}

/// B43's rows hold the standard 14 fonts' widths only from PDF 2.0, as
/// they're asked for only from then: a PDF 1.7 file's simple font that
/// isn't one of them, without its widths, still fails, and a PDF 2.0
/// file's standard 14 font is held.
#[test]
fn the_fonts_widths_are_allowed_only_where_pdf_2_0_asks_for_them() {
    for (version, base_font, held) in [
        ("1.7", "NotAStandardFont", false),
        ("2.0", "NotAStandardFont", true),
        ("2.0", "Helvetica", true),
    ] {
        let mut pdf = sheaf_doc::pdf::Document::with_version(version);
        let pages = pdf.new_object_id();
        let font = pdf.add_object(dictionary! {
            "Type" => Object::name("Font"),
            "Subtype" => Object::name("Type1"),
            "BaseFont" => Object::name(base_font),
        });
        let content = pdf.add_object(sheaf_doc::pdf::Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf (A) Tj ET".to_vec(),
        ));
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"),
            "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            "Contents" => content,
        });
        pdf.objects.insert(
            pages,
            Object::from(dictionary! {
                "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages });
        pdf.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        let checked = checked(
            &format!("{base_font} in a {version} file"),
            &bytes,
            None,
            |_| Hold::All(Made::Built),
            &Store::new(),
        );
        let fonts: Vec<&Finding> = (answered(&checked.report, &[], false).into_iter())
            .filter(|f| f.table == "FontType1")
            .collect();
        assert_eq!(fonts.len(), 4, "{}: {}", checked.what, checked.report);
        let left = left(&fonts, checked.version, &allowances());
        assert_eq!(left.is_empty(), held, "{}: {left:?}", checked.what);
    }
}

/// [`common::two_page_form`], written to a new directory for test `name`
/// and opened, with something on it for every export to make something
/// of: every kind of mark on its first page, a header on every page, a
/// bookmark to each page, and page labels. The directory, the document,
/// and the blobs its marks name.
fn everything(name: &str) -> (std::path::PathBuf, Document, [Blob; 2]) {
    let dir = scratch(&format!("arlington-{name}"));
    let path = common::write(&dir, "form.pdf", &mut common::two_page_form());
    let mut doc = common::open(&path);
    let held = mark_blobs(doc.store());
    every_mark_on(&mut doc);
    let [p0, p1] = [0, 1].map(|i| doc.page(i).unwrap());
    let label = LabelSpec {
        start: p0,
        style: LabelStyle::UpperRoman,
        prefix: String::new(),
        first: 1,
    };
    let ops = vec![
        fx::set_header(fx::header("Page <<page>> of <<pages>>"), None),
        fx::bookmark(None, None, "First", Some(p0)),
        fx::bookmark(None, None, "Second", Some(p1)),
        fx::labels(&[label]),
    ];
    doc.commit_ops(ops, "Everything").unwrap();
    (dir, doc, held)
}

/// Acrobat's Extract Pages of the first page of two: the page, its marks
/// and their appearances, the form keeping only its fields, and the
/// bookmark and label it keeps. The widgets are the file's, copied.
#[test]
fn an_extract_holds() {
    let (_dir, doc, _held) = everything("extract");
    let first = doc.page(0).unwrap();
    let checked = exported("Extract", &doc, |from| {
        Export::extract(from, &[first], Out::Bytes)
    });
    clean(
        &checked,
        &[
            "Catalog",
            "PageTreeNodeRoot",
            "PageObject",
            "AnnotText",
            "XObjectFormType1",
            "InteractiveForm",
            "Outline",
            "OutlineItem",
            "PageLabel",
        ],
    );
}

/// Export Clean Copy: the pages without the marks, and the bookmarks and
/// labels kept. The form is the file's, copied.
#[test]
fn a_clean_copy_holds() {
    let (_dir, doc, _held) = everything("clean-copy");
    let checked = exported("Clean copy", &doc, |from| {
        Export::clean_copy(from, Out::Bytes)
    });
    clean(
        &checked,
        &[
            "Catalog",
            "PageObject",
            "Outline",
            "OutlineItem",
            "PageLabel",
        ],
    );
}

/// Export Flattened: every mark and widget drawn into its page as a form,
/// with what it draws with.
#[test]
fn a_flattened_copy_holds() {
    let (_dir, doc, _held) = everything("flattened");
    let checked = exported("Flattened", &doc, |from| {
        Export::flattened(from, Out::Bytes)
    });
    clean(
        &checked,
        &[
            "Catalog",
            "PageObject",
            "XObjectFormType1",
            "Resource",
            "GraphicsStateParameter",
        ],
    );
}

#[test]
fn a_print_file_holds() {
    printed(true);
}

#[test]
fn a_print_file_without_markup_holds() {
    printed(false);
}

/// The file sent to a printer, of both pages: with `markup`, every
/// printed mark and widget drawn into its page; without, the marks left
/// out, the header drawn, and the form, the file's, printing as it is.
/// Print's Save as PDF is the same export to a file.
fn printed(markup: bool) {
    let (_dir, doc, _held) = everything(&format!("print-{markup}"));
    let pages: Vec<_> = doc.state().order().iter().copied().collect();
    let what = format!("Print, markup {markup}");
    let checked = exported(&what, &doc, |from| Export::print(from, &pages, markup));
    let tables: &[&str] = match markup {
        true => &["Catalog", "PageObject", "XObjectFormType1", "Resource"],
        false => &["Catalog", "PageObject", "Resource", "FontType1"],
    };
    clean(&checked, tables);
}

/// Remove Blanks' second file: the page it took out, with a bookmark of
/// its own and its label, and the connector that ends on it.
#[test]
fn rejects_hold() {
    let (_dir, doc, _held) = everything("rejects");
    let second = doc.page(1).unwrap();
    let titled = [(second, "Blank page 2".to_string())];
    let checked = exported("Rejects", &doc, |from| {
        Export::rejects(from, &titled, Out::Bytes)
    });
    clean(
        &checked,
        &[
            "Catalog",
            "PageObject",
            "Outline",
            "OutlineItem",
            "PageLabel",
            "AnnotLink",
        ],
    );
}

/// Combine of a form and a file of blank pages, a bookmark for each file:
/// the combined file's catalog, page tree and bookmarks, the second
/// file's pages it brings, and the form's widgets, which its Rewrite
/// writes over the file's dictionaries.
#[test]
fn a_combined_file_holds() {
    let dir = scratch("arlington-combine");
    let form = common::write(&dir, "form.pdf", &mut common::two_page_form());
    let blank = dir.join("blank.pdf");
    let pages = sheaf_doc::memory::blank_pages(612.0, 792.0, 2).unwrap();
    std::fs::write(&blank, pages).unwrap();
    let items = [CombineItem::new(&form), CombineItem::new(&blank)];
    let params = CombineParams::default();
    let out = dir.join("combined.pdf");
    let (session, _) =
        sheaf_ops::combine::combine(&items, &params, &out, &PRINT_ANSWERS, &NoProgress).unwrap();
    let doc = &session.doc;
    let checked = last_saved("Combine", doc, doc.store());
    clean(
        &checked,
        &[
            "Catalog",
            "PageTreeNodeRoot",
            "PageObject",
            "AnnotWidget",
            "Outline",
            "OutlineItem",
        ],
    );
}

#[test]
fn a_comment_summary_holds() {
    summary(Layout::CommentsOnly);
}

#[test]
fn a_summary_of_pages_holds() {
    summary(Layout::Pages);
}

/// The comment summary of every kind of mark, laid out as `layout` says,
/// saved. Its pages, its text and the fonts it's set in, made in memory
/// and held as built; its pictures of the document's pages, forms drawing
/// the printed pages with their resources, held as merged; and the `/Info`
/// naming it, made in memory too. What a merged object may carry is the
/// document's file's.
fn summary(layout: Layout) {
    let (dir, doc, _held) = everything(&format!("summary-{layout:?}"));
    let from = doc.export_from(Actor::User);
    let cf = CommentFilter::default();
    let mut made = sheaf_ops::comments::summary(&from, &cf, "", layout, "Summary").unwrap();
    let to = SaveTo::File {
        path: dir.join("summary.pdf"),
        append: false,
    };
    made.save(to, &PRINT_ANSWERS, &NoProgress).unwrap();
    let what = format!("Summary, {layout:?}");
    summary_held(&made.doc, layout);
    let checked = last_saved(&what, &made.doc, doc.store());
    let mut tables = vec!["Catalog", "PageObject", "Resource", "FontType1", "DocInfo"];
    if layout == Layout::Pages {
        tables.push("XObjectFormType1");
    }
    clean(&checked, &tables);
}

/// What the save of `summary`, a comment summary laid out as `layout`
/// says, holds: its pages' content and its fonts as built, and its
/// pictures of the document's pages, forms, as merged, of which a summary
/// of the comments alone has none.
fn summary_held(summary: &Document, layout: Layout) {
    let record = summary.saved_record().unwrap();
    let bytes = std::fs::read(&*record.path).unwrap();
    let pdf = reopened(&bytes, None).unwrap();
    let hold = written(&record.map, summary.state(), summary.store(), &pdf, None);
    let Hold::Only(held) = hold else {
        panic!("a save holds the objects it wrote");
    };
    let named = |id: &ObjectId, key: &[u8], value: &[u8]| {
        let Ok(obj) = pdf.get(*id) else {
            return false;
        };
        let Some(dict) = obj.as_dict() else {
            return false;
        };
        dict.get(key).and_then(Object::as_name) == Some(value)
    };
    let forms: Vec<ObjectId> = pdf
        .ids()
        .filter(|id| named(id, b"Subtype", b"Form"))
        .collect();
    assert_eq!(
        forms.is_empty(),
        layout == Layout::CommentsOnly,
        "{forms:?}"
    );
    for id in forms {
        assert_eq!(held.get(&id), Some(&Made::Merged), "the picture {id:?}");
    }
    let page = record
        .map
        .get(&ObjKey::Page(summary.page(0).unwrap()))
        .unwrap();
    let contents = pdf.get_dictionary(page).unwrap().get(b"Contents").unwrap();
    let mut own: Vec<ObjectId> = match contents.value() {
        Value::Ref(id) => vec![*id],
        Value::Array(streams) => streams.iter().filter_map(Object::as_reference).collect(),
        other => panic!("the page's content is {other:?}"),
    };
    if layout == Layout::CommentsOnly {
        own.extend(pdf.ids().filter(|id| named(id, b"Type", b"Font")));
    }
    assert!(!own.is_empty(), "{own:?}");
    for id in own {
        assert_eq!(
            held.get(&id),
            Some(&Made::Built),
            "{id:?}, the summary's own"
        );
    }
}

/// Images made pages, as a scan and Images to PDF make them: a JPEG, an
/// image in color, one in gray, and one in black and white.
fn page_images() -> Vec<PageImage> {
    let rgb = image::RgbImage::from_fn(16, 8, |x, y| image::Rgb([16 * x as u8, 30 * y as u8, 90]));
    let jpeg = sheaf_image::encode::jpeg(&rgb, 80, 72).unwrap();
    let gray = image::GrayImage::from_fn(16, 8, |x, _| image::Luma([16 * x as u8]));
    let ink = image::GrayImage::from_fn(16, 8, |x, y| image::Luma([255 * ((x + y) % 2) as u8]));
    let mut pages = sheaf_image::io::read_bytes(jpeg).unwrap();
    pages.extend([
        PageImage::from_image(rgb.into(), 72.0),
        PageImage::from_image(gray.into(), 72.0),
        PageImage::from_image(ink.into(), 72.0),
    ]);
    pages
}

/// Images to PDF's body (`sheaf_ops::images::images_to_pdf`, whose session
/// isn't returned): the images made pages in memory, each its own size as
/// Images to PDF makes them unless told otherwise, inserted into a
/// document of no pages, saved. The pages, made whole by sheaf, and their
/// images.
#[test]
fn images_made_pages_hold() {
    let dir = scratch("arlington-images");
    let mut session = Session::new_empty();
    let fit = Paper::default().fit(0.0);
    let src = sheaf_ops::intake::batch_source(session.doc.store(), &page_images(), fit).unwrap();
    session.hold(src.blob().clone());
    let insert = Op::InsertPages {
        from: PageSource::File {
            file: BlobSel::Id(src.id()),
            name: "images".into(),
        },
        pages: None,
        before: None,
        bring: Bring::default(),
        locals: Vec::new(),
    };
    session.commit(vec![insert], "Images to PDF").unwrap();
    let to = SaveTo::File {
        path: dir.join("images.pdf"),
        append: false,
    };
    session.save(to, &PRINT_ANSWERS, &NoProgress).unwrap();
    let checked = last_saved("Images", &session.doc, &Store::new());
    clean(
        &checked,
        &["Catalog", "PageTreeNodeRoot", "PageObject", "XObjectImage"],
    );
}

/// A scan landed in a document of no pages as a scan lands its batches,
/// each image fitted to a Letter page and given the words read on it,
/// saved: the pages, their images, and the text laid over them.
#[test]
fn a_scan_holds() {
    let dir = scratch("arlington-scan");
    let mut session = Session::new_empty();
    let pages = page_images();
    let words: Vec<Vec<OcrWord>> = pages.iter().map(|_| layer().words).collect();
    let batch = sheaf_ops::intake::Batch {
        pages,
        words: Some(words),
        fit: Paper::Letter.fit(36.0),
        engine: "test 1.0".into(),
    };
    let mut landing = sheaf_ops::intake::SessionLanding::new(&mut session, "Scan");
    sheaf_ops::intake::Landing::land(&mut landing, batch, &NoProgress).unwrap();
    landing.finish();
    let to = SaveTo::File {
        path: dir.join("scan.pdf"),
        append: false,
    };
    session.save(to, &PRINT_ANSWERS, &NoProgress).unwrap();
    let checked = last_saved("Scan", &session.doc, &Store::new());
    clean(
        &checked,
        &[
            "PageObject",
            "XObjectImage",
            "FontType0",
            "FontCIDType2",
            "FontDescriptorCIDType2",
        ],
    );
}

/// Files sheaf makes whole, every object in them its own: blank pages
/// (held whole, including catalog and page tree), and the `/Info` object of
/// the internal zero-page seed `memory::titled` that the comment summary
/// starts from before inserting pages.
#[test]
fn files_made_whole_hold() {
    let blank = sheaf_doc::memory::blank_pages(612.0, 792.0, 2).unwrap();
    let blank_checked = checked(
        "Blank pages",
        blank.as_slice(),
        None,
        |_| Hold::All(Made::Built),
        &Store::new(),
    );
    clean(
        &blank_checked,
        &["Catalog", "PageTreeNodeRoot", "PageObject", "Resource"],
    );

    let titled = sheaf_doc::memory::titled(&Store::new(), "Title").unwrap();
    let titled_checked = checked(
        "Titled",
        titled.bytes(),
        None,
        |pdf| {
            let info = pdf.trailer().get(b"Info").unwrap().as_reference().unwrap();
            Hold::Only(BTreeMap::from([(info, Made::Built)]))
        },
        &Store::new(),
    );
    clean(&titled_checked, &["DocInfo"]);
}

/// A picture with transparency added to the stamp library (Add from
/// Image…): the collection sheaf writes, and the stamp alone, as one
/// placed is made, each wholly sheaf's; and the stamp placed on a page,
/// saved, its appearance drawing the stamp's page.
#[test]
fn an_image_stamp_holds() {
    let dir = scratch("arlington-image-stamp");
    let seal = dir.join("seal.png");
    let rgba = image::RgbaImage::from_fn(8, 4, |x, _| image::Rgba([200, 30, 30, 30 * x as u8]));
    rgba.save(&seal).unwrap();
    let (collection, _) = sheaf_ops::stamps::add_image(&dir, "Mine", &seal).unwrap();
    let stamp = sheaf_ops::stamps::stamp_pdf(&collection, 0).unwrap();
    let files = [
        ("Collection", std::fs::read(&collection).unwrap()),
        ("Stamp", stamp.clone()),
    ];
    for (what, bytes) in files {
        let checked = checked(
            what,
            &bytes,
            None,
            |_| Hold::All(Made::Built),
            &Store::new(),
        );
        let tables = [
            "Catalog",
            "PageObject",
            "XObjectImage",
            "XObjectImageSoftMask",
        ];
        clean(&checked, &tables);
    }
    let path = common::write(&dir, "placed.pdf", &mut Form::new(1));
    let mut doc = common::open(&path);
    let blob = doc.store().intern(stamp.into(), BlobKind::stamp()).unwrap();
    let mut placed = every_mark_kind()
        .into_iter()
        .find(|a| matches!(a.kind, AnnotKind::PageStamp { .. }))
        .unwrap();
    if let AnnotKind::PageStamp { stamp, .. } = &mut placed.kind {
        *stamp = blob.id().stem();
    }
    let p0 = doc.page(0).unwrap();
    doc.commit_ops(vec![fx::add(p0, &placed)], "Place").unwrap();
    let checked = held(&mut doc, Some(How::Rewritten), &path);
    clean(
        &checked,
        &[
            "AnnotStamp",
            "XObjectFormType1",
            "XObjectImage",
            "XObjectImageSoftMask",
        ],
    );
}

/// A page of another file added to the stamp library (Add from PDF
/// Page…), the stamp alone: the page copied, so held as merged, carrying
/// what the file has.
#[test]
fn a_page_stamp_holds() {
    let dir = scratch("arlington-page-stamp");
    let art = common::write(&dir, "art.pdf", &mut Form::new(1));
    let (collection, _) = sheaf_ops::stamps::add_pdf_page(&dir, "Pages", &art, 0, None).unwrap();
    let stamp = sheaf_ops::stamps::stamp_pdf(&collection, 0).unwrap();
    let store = Store::new();
    let kind = BlobKind::file(&art, None);
    let _art = store
        .open(std::fs::read(&art).unwrap().into(), kind)
        .unwrap();
    let merged = |_: &LazyPdf| Hold::All(Made::Merged);
    let checked = checked("Page stamp", &stamp, None, merged, &store);
    clean(&checked, &["Catalog", "PageTreeNodeRoot", "PageObject"]);
}

/// Corpus paths, comma-separated, to run
/// [`what_sheaf_writes_on_every_form_holds_to_arlington`] on alone
/// (`SHEAF_ARLINGTON_ONLY=forms/irs/f1040.pdf,…`): for looking at a few
/// files by hand. Unset, it takes every file.
const ARLINGTON_ONLY: &str = "SHEAF_ARLINGTON_ONLY";

/// A file [`what_sheaf_writes_on_every_form_holds_to_arlington`] writes
/// each save's counts to, a line each (`SHEAF_ARLINGTON_SAVES=saves.tsv`):
/// for setting one run beside another by hand. Its columns are the corpus
/// file, the save, the objects checked (the sum of the counts by table, in
/// which one checked as two tables counts as each), the keys, the
/// findings, those carried, allowed and left, and the objects checked by
/// table (`table=n`, space-separated). Unset, it writes none.
const ARLINGTON_SAVES: &str = "SHEAF_ARLINGTON_SAVES";

/// The note's text on every corpus file's first page, and the word
/// recognized there.
const WORD: &str = "SHEAFARLINGTON";

/// How many of the most common checks not made, and keys the model
/// doesn't list, the corpus tier prints.
const TOP: usize = 20;

/// The corpus tier's counts on the corpus they were counted on (its
/// manifest's length), unnarrowed: the forms that open, those that don't
/// and why (three whose cross-reference tables don't parse, which no tier
/// opens), and what the saves were, a form's check boxes and radio buttons
/// with no on state pressed among them ([`pressed_save`], M10 slice 8). A
/// change that moves one says why, and moves it. A save of none isn't
/// counted.
const FORMS: FormsBaseline = FormsBaseline {
    manifest: 4392,
    files: 4389,
    unopened: [(UNREAD, 3), (PASSWORD, 0)],
    saves: [
        ("as planned, Append", 2406),
        ("as planned, Rewrite", 1983),
        ("a page deleted, as planned, Rewrite", 2901),
        ("of one page, a Rewrite to a copy", 1488),
        (PRESSED, 47),
    ],
};

struct FormsBaseline {
    manifest: usize,
    files: usize,
    unopened: [(&'static str, usize); 2],
    saves: [(&'static str, usize); 5],
}

/// What [`pressed_save`]'s save is.
const PRESSED: &str = "toggles pressed, a Rewrite";

/// Why a form didn't open.
const UNREAD: &str = "its file doesn't read";
const PASSWORD: &str = "it needs a password";

/// What the corpus tier made of a form.
enum Outcome {
    /// It didn't open, for this reason ([`UNREAD`], [`PASSWORD`]).
    Unopened(&'static str),
    Checked(FormChecked),
    /// It couldn't be copied, or a save failed.
    Failed(String),
}

/// What the corpus tier's saves of a form wrote, each checked.
struct FormChecked {
    saves: Vec<SaveChecked>,
    /// Why its first page took no recognized text, when it didn't.
    untexted: Option<&'static str>,
    /// What its pressed save pressed and made, where it had one
    /// ([`pressed_save`]).
    pressed: Option<Pressed>,
    /// Why its pressed save failed, where it did: the form's other saves
    /// are counted all the same.
    failed: Option<String>,
}

/// What [`pressed_save`] pressed of a form's check boxes and radio
/// buttons, and what the save made of them, each by its kind (`check box`,
/// `radio button`).
#[derive(Default)]
struct Pressed {
    /// The widgets pressed.
    pressed: BTreeMap<&'static str, usize>,
    /// The widgets whose state the presses left other than their file's
    /// and whose file has no on state: those whose appearances the save
    /// makes (`appearance::stale`).
    stale: BTreeMap<&'static str, usize>,
    /// Of those, the widgets the save made appearances for.
    made: BTreeMap<&'static str, usize>,
    /// Of those, the widgets it made none for, by kind and why
    /// (`appearance::unmade_kind`).
    unmade: BTreeMap<String, usize>,
}

/// One save's check, its findings sorted out.
struct SaveChecked {
    /// The save, and what it was saved as.
    what: String,
    /// Its counts; the findings are sorted out into what follows.
    report: Report,
    /// Findings carried from the form, by rule.
    carried: BTreeMap<Rule, usize>,
    /// First-revision linearizations voided by an Append (`Rule::Updated`).
    voided: usize,
    /// Findings an allowance holds, each named by its allowance.
    allowed: Vec<String>,
    /// Findings neither carried nor allowed: failures.
    left: Vec<String>,
}

impl SaveChecked {
    /// This save of the corpus file `file`, as a line of the file
    /// [`ARLINGTON_SAVES`] names.
    fn line(&self, file: &str) -> String {
        let r = &self.report;
        let objects: usize = r.checked.values().sum();
        let carried: usize = self.carried.values().sum();
        let (allowed, left) = (self.allowed.len(), self.left.len());
        let findings = carried + self.voided + allowed + left;
        let tables: Vec<String> = (r.checked.iter())
            .map(|(table, n)| format!("{table}={n}"))
            .collect();
        let (what, keys, tables) = (&self.what, r.keys, tables.join(" "));
        format!(
            "{file}\t{what}\t{objects}\t{keys}\t{findings}\t{carried}\t{allowed}\t{left}\t{tables}\n"
        )
    }
}

/// Every corpus form given what sheaf writes most and saved as planned,
/// then rewritten, and what each save wrote held to the Arlington model:
/// a note and a recognized word on its first page and a header and footer
/// with Bates numbers on every page, in one version, saved as planned (an
/// Append where the form takes one); then its last page deleted, saved as
/// planned, which strict removal makes a Rewrite, or a form of one page
/// rewritten to a copy. And, on a copy of its own, its check boxes and
/// radio buttons whose file has no on state pressed, and rewritten
/// ([`pressed_save`]). A finding no allowance holds fails, unless it's in
/// an object merged over the form's that the form has too. The counts are
/// printed: saves by what they were, objects checked by table, keys, the
/// checks not made and what stood in their way, the keys the model doesn't
/// list, those the version deprecates and those readers ignore, the
/// widgets pressed and those made and not by why, the findings carried and
/// allowed, and the allowances that allowed nothing here. A save that held
/// nothing but its trailer fails. Run on every
/// form, the counts of forms opened and not, and of saves, are asserted
/// against their baseline ([`FORMS`]), so a form that stops opening fails
/// too; and each allowance for one corpus file must have allowed a
/// finding. Each save's counts go to the file [`ARLINGTON_SAVES`] names,
/// when it names one.
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test arlington -- --ignored --nocapture"]
fn what_sheaf_writes_on_every_form_holds_to_arlington() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let allowances = Allowances::read(Path::new(ALLOWED)).unwrap();
    let dir = scratch("arlington-corpus");
    let entries = common::narrowed(ARLINGTON_ONLY, corpus.forms().unwrap(), |e| &e.path);
    let outcomes = each_entry(&entries, |entry| {
        written_on(entry, &corpus, &dir, &allowances)
    });
    let mut totals = Totals::default();
    let mut failed = BTreeMap::new();
    let mut lines = String::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        match outcome {
            Outcome::Unopened(why) => *totals.unopened.entry(why).or_default() += 1,
            Outcome::Checked(form) => {
                let left: Vec<&str> = (form.saves.iter())
                    .flat_map(|save| save.left.iter().map(String::as_str))
                    .chain(form.failed.as_deref())
                    .collect();
                if !left.is_empty() {
                    failed.insert(entry.path.clone(), left.join("\n  "));
                }
                for save in &form.saves {
                    lines.push_str(&save.line(&entry.path));
                }
                totals.add(form);
            }
            Outcome::Failed(e) => {
                totals.files += 1;
                failed.insert(entry.path.clone(), e);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(to) = std::env::var_os(ARLINGTON_SAVES) {
        std::fs::write(to, lines).unwrap();
    }
    totals.print(entries.len(), &allowances);
    for (path, why) in &failed {
        println!("{path}:\n  {why}");
    }
    assert!(failed.is_empty(), "{} files failed", failed.len());
    if entries.len() == FORMS.manifest {
        assert_eq!(totals.files, FORMS.files, "forms opened");
        let unopened: BTreeMap<&str, usize> = (FORMS.unopened.iter().copied())
            .filter(|&(_, n)| n > 0)
            .collect();
        assert_eq!(totals.unopened, unopened, "forms not opened");
        let saves: BTreeMap<String, usize> = (FORMS.saves.iter())
            .filter(|&&(_, n)| n > 0)
            .map(|&(what, n)| (what.to_string(), n))
            .collect();
        assert_eq!(totals.saves, saves, "saves");
        let unneeded: Vec<String> = (allowances.unused().into_iter())
            .filter(|a| a.file.is_some())
            .map(named)
            .collect();
        assert!(
            unneeded.is_empty(),
            "rows for a corpus file that allowed nothing:\n{}",
            unneeded.join("\n")
        );
    }
}

/// A copy of `entry`'s file, in a directory of its own under `dir`, given
/// and saved what the corpus tier gives it ([`saves_of`]), and checked
/// against `allowances`.
fn written_on(entry: &Entry, corpus: &Corpus, dir: &Path, allowances: &Allowances) -> Outcome {
    let dir = dir.join(entry.path.replace('/', "-"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("form.pdf");
    let outcome = match std::fs::copy(corpus.path(entry), &path) {
        Err(e) => Outcome::Failed(format!("not copied: {e}")),
        Ok(_) => match sheaf_doc::open::file(&path, None) {
            Ok(opened) => {
                let mut doc = opened.doc;
                match saves_of(&mut doc, (&path, &entry.path), allowances) {
                    Ok(mut form) => {
                        let pressed = dir.join("pressed.pdf");
                        let from = corpus.path(entry);
                        let saved = pressed_save(&from, &pressed, entry, allowances);
                        match saved {
                            Ok(Some((save, pressed))) => {
                                form.saves.push(save);
                                form.pressed = Some(pressed);
                            }
                            Ok(None) => {}
                            Err(e) => form.failed = Some(e),
                        }
                        Outcome::Checked(form)
                    }
                    Err(e) => Outcome::Failed(e),
                }
            }
            Err(e) if OpenError::is_password(&e) => Outcome::Unopened(PASSWORD),
            Err(_) => Outcome::Unopened(UNREAD),
        },
    };
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

/// [`written_on`]'s saves of `doc`, opened from `path`, a copy of the
/// corpus file `file`, each checked.
fn saves_of(
    doc: &mut Document,
    (path, file): (&Path, &str),
    allowances: &Allowances,
) -> Result<FormChecked, String> {
    let untexted = given(doc)?;
    let mut sources = BTreeMap::new();
    let mut saves = Vec::new();
    let from = section_start(doc);
    let (strategy, bytes) = common::planned_or_why(doc, path)?;
    let (report, version) = save_checked(doc, from, strategy, &bytes)?;
    let what = format!("as planned, {strategy:?}");
    let held = (&*doc, file, allowances);
    saves.push(sorted_out(
        what,
        (report, version, strategy == StrategyKind::Append),
        held,
        &mut sources,
    ));
    let from = section_start(doc);
    let pages = doc.state().page_count();
    let (what, bytes) = if pages > 1 {
        let last = doc.page(pages - 1).ok_or("no last page")?;
        (doc.commit_ops(vec![fx::delete(&[last])], "Delete"))
            .map_err(|e| format!("the last page not deleted: {e}"))?;
        let (strategy, bytes) = common::planned_or_why(doc, path)?;
        if strategy != StrategyKind::Rewrite {
            return Err(format!("a page deleted, saved as {strategy:?}"));
        }
        ("a page deleted, as planned, Rewrite", bytes)
    } else {
        let copy = path.with_file_name("rewritten.pdf");
        sheaf_doc::materialize::save_rewritten(doc, &copy)
            .map_err(|stop| format!("not rewritten to a copy: {stop}"))?;
        let bytes = std::fs::read(&copy).map_err(|e| format!("reading the copy: {e}"))?;
        ("of one page, a Rewrite to a copy", bytes)
    };
    let (report, version) = save_checked(doc, from, StrategyKind::Rewrite, &bytes)?;
    let held = (&*doc, file, allowances);
    saves.push(sorted_out(
        what.into(),
        (report, version, false),
        held,
        &mut sources,
    ));
    Ok(FormChecked {
        saves,
        untexted,
        pressed: None,
        failed: None,
    })
}

/// Where the corpus file `entry` has check box or radio widgets whose
/// normal appearance has no on state, a save of its own (M10 slice 8): the
/// file copied from `from` to `path`, opened as a document of its own, so
/// the other saves don't move, each such widget pressed once
/// (`tx::fixture::press`, as M10 slice 9's lowering will press one), in
/// page order, then rewritten and checked. Their state changed, and their
/// file having no appearance to show it, the save makes their appearances
/// ([`appearance::stale`]): which widgets it made them for, and which it
/// couldn't and why, are counted ([`Pressed`]). A widget the group doesn't
/// hold, or another record of one already pressed (a page the tree lists
/// twice), isn't pressed. None where the file has no such widget.
///
/// A Rewrite, not a save as planned: the Rewrite holds every object sheaf
/// writes anew for the widgets pressed, which is what this save is for,
/// where a planned Append of a field whose own dictionary is a widget
/// beside its kids (the corpus's toggles with no on state are most often
/// such a dictionary, of no size) stops at the save's own check, as a text
/// field of that shape filled does (an Append writes the widget apart from
/// the field and not its page's `/Annots`).
///
/// [`appearance::stale`]: sheaf_doc::forms::appearance::stale
fn pressed_save(
    from: &Path,
    path: &Path,
    entry: &Entry,
    allowances: &Allowances,
) -> Result<Option<(SaveChecked, Pressed)>, String> {
    use sheaf_doc::forms::FieldKind;
    use sheaf_doc::forms::{appearance, buttons};
    use sheaf_doc::materialize::{SaveHow, key::Part};
    std::fs::copy(from, path).map_err(|e| format!("not copied to press: {e}"))?;
    let mut doc = (sheaf_doc::open::file(path, None))
        .map_err(|e| format!("not opened to press: {e}"))?
        .doc;
    let kind_of = |doc: &Document, aid| {
        let st = doc.state();
        let w = st.annot(aid)?.widget()?;
        match st.field(w.field)?.term.as_deref()?.kind {
            FieldKind::CheckBox => Some(("check box", w.field)),
            FieldKind::Radio => Some(("radio button", w.field)),
            _ => None,
        }
    };
    let st = doc.state();
    let mut seen = std::collections::HashSet::new();
    let mut presses = Vec::new();
    for &page in st.order() {
        for (aid, w) in st.widgets_on(page) {
            let Some((kind, field)) = kind_of(&doc, aid) else {
                continue;
            };
            let Some(r) = &w.src else {
                continue;
            };
            let read = r.src.form().widget_by_object(r.id);
            if read.is_none_or(|read| read.on_state.is_some()) {
                continue;
            }
            let held = buttons(st, field).is_some_and(|group| group.on_state(aid).is_some());
            if held && seen.insert((r.src.id(), r.id)) {
                presses.push((aid, kind));
            }
        }
    }
    if presses.is_empty() {
        return Ok(None);
    }
    let mut pressed = Pressed::default();
    for &(aid, kind) in &presses {
        fx::press(&mut doc, aid);
        *pressed.pressed.entry(kind).or_default() += 1;
    }
    // Which widgets the save should make appearances for, and why one
    // can't be: asked before it, as a save takes what it wrote for its
    // file's.
    let st = doc.state();
    let no_font = |_: Option<&[u8]>| Err(sheaf_doc::pdf::forms::Unmade::FontNotBindable);
    let stale: Vec<(sheaf_doc::AnnotId, &str, String)> = (st.annots())
        .filter(|&(aid, _)| appearance::stale(st, aid))
        .filter_map(|(aid, _)| {
            let (kind, _) = kind_of(&doc, aid)?;
            let made = appearance::look(st, aid).and_then(|look| appearance::make(&look, no_font));
            let why = match made {
                Ok(_) => "made, not written".to_string(),
                Err(why) => appearance::unmade_kind(&why),
            };
            Some((aid, kind, format!("{kind}, {why}")))
        })
        .collect();
    let (strategy, bytes) = common::saved_or_why(&mut doc, path, SaveHow::Rewrite)?;
    let record = doc
        .saved_record()
        .ok_or("the pressed save left no record")?;
    for (aid, kind, unmade) in stale {
        *pressed.stale.entry(kind).or_default() += 1;
        match (record.map.keys).contains_key(&ObjKey::AnnotPart(aid, Part::Ap)) {
            true => *pressed.made.entry(kind).or_default() += 1,
            false => *pressed.unmade.entry(unmade).or_default() += 1,
        }
    }
    let (report, version) = save_checked(&doc, 0, strategy, &bytes)?;
    let held = (&doc, entry.path.as_str(), allowances);
    let save = sorted_out(
        PRESSED.into(),
        (report, version, false),
        held,
        &mut BTreeMap::new(),
    );
    Ok(Some((save, pressed)))
}

/// `doc` given, in one version, a note and a recognized word on its first
/// page, and the corpus tiers' header and footer, with Bates numbers, on
/// every page, its header's right [`FALLBACK_TEXT`], which Helvetica draws
/// in part and the fallback font sheaf bundles the rest of (M10 slice 7),
/// so every save writes a subset of it: why the page took no word, when it
/// didn't (its content can't be read, or it shows nothing to read a word
/// on).
fn given(doc: &mut Document) -> Result<Option<&'static str>, String> {
    let first = doc.page(0).ok_or("no pages")?;
    let snap = doc.snapshot();
    let mut tx = Document::tx_at(&snap, doc.lower_cx(Actor::User));
    (tx.apply(fx::add(first, &common::note(WORD)), &NoProgress))
        .map_err(|e| format!("no note: {e}"))?;
    let mut header = common::corpus_header();
    header.texts[2] = FALLBACK_TEXT.into();
    (tx.apply(fx::set_header(header, None), &NoProgress))
        .map_err(|e| format!("no header and footer: {e}"))?;
    let geom = snap.geom(first);
    let layer = from_engine(vec![common::word_on(&geom, WORD)], &geom, "corpus");
    let untexted = match layer.words.is_empty() {
        true => Some("its first page shows nothing to read a word on"),
        false => {
            let set = Op::SetOcrText {
                page: PageRef::Id(first),
                layer: Arc::new(layer),
            };
            match tx.apply(set, &NoProgress) {
                Ok(_) => None,
                Err(EditError::Content { .. }) => Some("its first page's content can't be read"),
                Err(e) => return Err(format!("no recognized text: {e}")),
            }
        }
    };
    doc.commit(tx, CommitOpts::labelled("Arlington"))
        .map_err(|e| format!("not committed: {e}"))?;
    Ok(untexted)
}

/// `report`, of what `doc`'s save `what` wrote, a file of `version`, its
/// findings sorted out: those carried from the files `doc` read, those
/// `allowances` allow in the corpus file `file`, and the rest. The files
/// read are checked at `version` only once an object merged over theirs
/// has a finding (a built one's are never carried), and kept in `sources`
/// by version.
fn sorted_out(
    what: String,
    (mut report, version, appended): (Report, Version, bool),
    (doc, file, allowances): (&Document, &str, &Allowances),
    sources: &mut BTreeMap<Version, Vec<Report>>,
) -> SaveChecked {
    let merged = report.findings.iter().any(|f| f.made == Made::Merged);
    let sources: Vec<&Report> = match merged {
        true => (sources.entry(version))
            .or_insert_with(|| files_read(doc.store(), version))
            .iter()
            .collect(),
        false => Vec::new(),
    };
    let mut carried: BTreeMap<Rule, usize> = BTreeMap::new();
    for finding in &report.findings {
        *carried.entry(finding.rule).or_default() += 1;
    }
    let uncarried = report.beyond(&sources);
    let mut voided = 0usize;
    let (mut allowed, mut left) = (Vec::new(), Vec::new());
    for finding in uncarried {
        *carried.entry(finding.rule).or_default() -= 1;
        if appended && finding.made == Made::Merged && finding.rule == Rule::Updated {
            voided += 1;
            continue;
        }
        match allowances.allows(finding, version, Some(file)) {
            Some(allowance) => allowed.push(named(allowance)),
            None => left.push(format!("{what}: {finding}")),
        }
    }
    carried.retain(|_, n| *n > 0);
    report.findings = Vec::new();
    SaveChecked {
        what,
        report,
        carried,
        voided,
        allowed,
        left,
    }
}

/// The corpus tier's counts, over every file.
#[derive(Default)]
struct Totals {
    /// Files that opened, and their saves, by what they were; files that
    /// didn't, by why.
    files: usize,
    saves: BTreeMap<String, usize>,
    unopened: BTreeMap<&'static str, usize>,
    /// Files whose first page took no word, by why.
    untexted: BTreeMap<&'static str, usize>,
    /// Files with a pressed save, and what those pressed and made, summed
    /// ([`pressed_save`]).
    pressed_files: usize,
    pressed: Pressed,
    checked: BTreeMap<String, usize>,
    keys: usize,
    /// Checks not made, by what stood in the way ([`in_the_way`]), and by
    /// table, key, column and what stood in the way.
    unchecked: BTreeMap<String, usize>,
    unchecked_at: BTreeMap<String, usize>,
    unlisted: BTreeMap<Unlisted, usize>,
    /// Keys the version deprecates, and keys the model says readers
    /// ignore, by table and key.
    deprecated: BTreeMap<(String, String), usize>,
    ignored: BTreeMap<(String, String), usize>,
    carried: BTreeMap<Rule, usize>,
    voided: usize,
    allowed: BTreeMap<String, usize>,
    no_table: usize,
    ties: usize,
    truncated: usize,
    two_contexts: usize,
    two_tables: usize,
}

impl Totals {
    fn add(&mut self, form: FormChecked) {
        self.files += 1;
        if let Some(why) = form.untexted {
            *self.untexted.entry(why).or_default() += 1;
        }
        if let Some(pressed) = form.pressed {
            self.pressed_files += 1;
            let sums = &mut self.pressed;
            for (kind, n) in pressed.pressed {
                *sums.pressed.entry(kind).or_default() += n;
            }
            for (kind, n) in pressed.stale {
                *sums.stale.entry(kind).or_default() += n;
            }
            for (kind, n) in pressed.made {
                *sums.made.entry(kind).or_default() += n;
            }
            for (why, n) in pressed.unmade {
                *sums.unmade.entry(why).or_default() += n;
            }
        }
        for save in form.saves {
            let r = save.report;
            *self.saves.entry(save.what).or_default() += 1;
            for (table, n) in r.checked {
                *self.checked.entry(table).or_default() += n;
            }
            self.keys += r.keys;
            for (u, n) in r.unchecked {
                *self.unchecked.entry(in_the_way(&u.why).into()).or_default() += n;
                let at = format!("{}/{} ({} {})", u.table, u.key, u.column, u.why);
                *self.unchecked_at.entry(at).or_default() += n;
            }
            for (u, n) in r.unlisted {
                *self.unlisted.entry(u).or_default() += n;
            }
            for (at, n) in r.deprecated {
                *self.deprecated.entry(at).or_default() += n;
            }
            for (at, n) in r.ignored {
                *self.ignored.entry(at).or_default() += n;
            }
            for (rule, n) in save.carried {
                *self.carried.entry(rule).or_default() += n;
            }
            self.voided += save.voided;
            for allowance in save.allowed {
                *self.allowed.entry(allowance).or_default() += 1;
            }
            self.no_table += r.no_table;
            self.ties += r.ties;
            self.truncated += r.truncated;
            self.two_contexts += r.two_contexts;
            self.two_tables += r.two_tables;
        }
    }

    /// The counts, of `of` files, and the allowances of `allowances` that
    /// allowed nothing. The objects are counted once each, as pdf-lint's
    /// `Report::objects` counts them, though one checked as two tables is
    /// counted as each by table.
    fn print(&self, of: usize, allowances: &Allowances) {
        let objects = self.checked.values().sum::<usize>() - self.two_tables;
        let unchecked: usize = self.unchecked.values().sum();
        let unlisted: usize = self.unlisted.values().sum();
        let deprecated: usize = self.deprecated.values().sum();
        let ignored: usize = self.ignored.values().sum();
        println!(
            "{} of {of} files given a note, a header and footer and a recognized word, saved as \
             planned, then rewritten; {objects} objects sheaf wrote checked as {} tables, {} \
             keys; {unchecked} checks not made for predicates that can't be told; {unlisted} \
             keys the model doesn't list; {deprecated} deprecated; {ignored} that readers \
             ignore; {} with no table, {} ties, {} truncated, {} in two contexts, {} as two \
             tables",
            self.files,
            self.checked.len(),
            self.keys,
            self.no_table,
            self.ties,
            self.truncated,
            self.two_contexts,
            self.two_tables
        );
        for (why, n) in &self.unopened {
            println!("  not opened, {why}: {n}");
        }
        for (what, n) in &self.saves {
            println!("  saved {what}: {n}");
        }
        for (why, n) in &self.untexted {
            println!("  no word recognized, {why}: {n}");
        }
        let p = &self.pressed;
        match self.pressed_files {
            0 => println!(
                "  no form has a check box or radio button whose file has no on state: no toggle \
                 was pressed, and the pressed saves prove nothing of made toggles"
            ),
            n => println!("  forms whose toggles with no on state were pressed: {n}"),
        }
        for (kind, n) in &p.pressed {
            println!("  pressed, {kind}: {n}");
        }
        for (kind, n) in &p.stale {
            println!("  pressed, to be made, {kind}: {n}");
        }
        for (kind, n) in &p.made {
            println!("  pressed, made, {kind}: {n}");
        }
        for (why, n) in &p.unmade {
            println!("  pressed, not made, {why}: {n}");
        }
        for (table, n) in largest(&self.checked, usize::MAX) {
            println!("  checked as {table}: {n}");
        }
        for (why, n) in largest(&self.unchecked, usize::MAX) {
            println!("  not checked, for {why}: {n}");
        }
        for (at, n) in largest(&self.unchecked_at, TOP) {
            println!("  not checked, {at}: {n}");
        }
        for (u, n) in largest(&self.unlisted, TOP) {
            println!("  unlisted: {}/{} ({}): {n}", u.table, u.key, u.class);
        }
        for ((table, key), n) in largest(&self.deprecated, TOP) {
            println!("  deprecated: {table}/{key}: {n}");
        }
        for ((table, key), n) in largest(&self.ignored, TOP) {
            println!("  ignored: {table}/{key}: {n}");
        }
        for (rule, n) in &self.carried {
            println!("  carried from the form, {rule}: {n}");
        }
        if self.voided > 0 {
            println!("  voided linearization on Append: {}", self.voided);
        }
        for (allowance, n) in &self.allowed {
            println!("  allowed by {allowance}: {n}");
        }
        for allowance in allowances.unused() {
            println!("  allowed nothing here: {}", named(allowance));
        }
    }
}

/// `allowance` as the corpus tier names it: its entry, what it allows,
/// and its line.
fn named(allowance: &Allowance) -> String {
    let a = allowance;
    let file = (a.file.as_ref()).map_or(String::new(), |file| format!(" in {file}"));
    let (table, key, rule, line) = (&a.table, &a.key, a.rule, a.line);
    format!("{} {table}/{key} {rule}{file} (line {line})", entry(a))
}

/// The backlog entry `allowance` is held for: its reason up to the first
/// colon (`B40: The model has…`), or none.
fn entry(allowance: &Allowance) -> &str {
    (allowance.reason.split_once(": ")).map_or("", |(entry, _)| entry)
}

/// What stood in the way of a check not made, as the corpus tier counts
/// it: the function the evaluator doesn't support, arithmetic, or a value
/// that can't be read (`Unchecked::why`).
fn in_the_way(why: &str) -> &str {
    match why {
        "+" | "-" | "*" | "mod" => "arithmetic",
        function if function.starts_with("fn:") => function,
        _ => "a value that can't be read",
    }
}

/// `counts`' `n` largest, largest first.
fn largest<K>(counts: &BTreeMap<K, usize>, n: usize) -> Vec<(&K, usize)> {
    let mut all: Vec<(&K, usize)> = counts.iter().map(|(k, &n)| (k, n)).collect();
    all.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    all.truncate(n);
    all
}
