//! Every way out of a document (M10 proposal 7, M10.5 §12 and slice 19),
//! each on a filled hybrid form: the 1040 from Greg's corpus, or, without
//! the corpus, a hybrid form of two pages made here (the test says it
//! skipped the corpus). The ways: a save in place and a Save As, each by a
//! Rewrite and by an Append; the recovery sidecar, as sections after the
//! file and as the whole document; the clean copy, the flattened copy, an
//! extract and a split's part; Combine, of the file and a blank page, on
//! a session; print, with its markup and without, and print's Save as PDF;
//! a stamp made of a page; and the comment summary, which shows the
//! pages. Of each:
//! - the form check finds nothing in what it wrote that it didn't find in
//!   the file (`nothing_found`);
//! - none replaces the open file, which every export refuses;
//! - a clean copy holds none of sheaf's private data, what marks another
//!   file's generator's watermark among it;
//! - an extract keeps the Bates numbers the document was given, unsaved;
//! - a flatten draws what's shown, and print's what prints, never a
//!   hidden annotation, and a widget's look only by its `/AS`;
//! - an extract that leaves out pages of the form's widgets drops its XFA
//!   part, which a save that leaves the form alone keeps;
//! - nothing is written to the temp, cache or home directories: the test
//!   runs in a process of its own, whose are new and stay empty
//!   (`own_dirs`).
//!
//! And a corpus tier, ignored: every form's extract of its first page,
//! clean copy and flattened copy, each checked by the form check against
//! the file, holding none of sheaf's private data where it's stripped, and
//! opening again ([`export_round_trips`]).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{How, holds, nothing_found, scratch, told};
use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::export::{
    Export, ExportError, ExportFrom, Gate, Out, PRINT_ANSWERS, preflight, run,
};
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::forms::fixture::{Form, append_revision, note, row};
use sheaf_doc::forms::{Report, check};
use sheaf_doc::materialize::{AskAnswers, SaveRecord};
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{Actor, Document, PageId};
use sheaf_edit::{BatesSpec, HeaderFooterSpec};
use sheaf_ops::combine::{CombineItem, CombineParams};
use sheaf_ops::comments::{self, CommentFilter, DataFormat, Layout};
use sheaf_ops::session::SaveTo;
use sheaf_ops::split::SplitBy;

/// The field filled in, on the form's first page: the 1040's, and the
/// one of the form made here.
const F1040_FIELD: &str = "topmostSubform[0].Page1[0].f1_14[0]";
const FIXTURE_FIELD: &str = "form1[0].a[0]";

/// What each annotation added to the first page draws: the color its
/// appearance fills with, which says where it's drawn. One hidden (and
/// printed), one shown but not printed, one shown and printed, and a
/// check box's `/Yes`, which has no `/AS` to say it's the state shown.
const HIDDEN: &[u8] = b"0.111 0.222 0.333 rg";
const UNPRINTED: &[u8] = b"0.444 0.555 0.666 rg";
const PRINTED: &[u8] = b"0.777 0.888 0.999 rg";
const CHECKED: &[u8] = b"0.123 0.456 0.789 rg";

/// The file every way starts from: the 1040, filled in, or the form made
/// here when there's no corpus.
fn base() -> (Vec<u8>, &'static str) {
    if let Some(f1040) = common::corpus_form("forms/irs/f1040.pdf") {
        return (f1040, F1040_FIELD);
    }
    // Two pages of fields, and an XFA copy that names them.
    let mut f = Form::new(2);
    f.text(FIXTURE_FIELD, 0, row(0));
    f.text("form1[0].b[0]", 1, row(0));
    f.check_box("form1[0].c[0]", 1, row(1), true);
    f.xfa();
    (f.bytes(), FIXTURE_FIELD)
}

/// `bytes` with `field` filled in, as Reader saves a fill (`filled`), and
/// then, in a revision of its own, another file's generator's watermark
/// merged into the first page's content, its list marked as the
/// generator's ([`MERGED_WATERMARK`]); and on that page the three other
/// tools' squares and the check box [`HIDDEN`] and the rest name.
fn prepared(bytes: &[u8], field: &str) -> Vec<u8> {
    let bytes = common::filled(bytes, field, "Filled In");
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let page = pdf.get_pages()[&1];
    let catalog = pdf.catalog().unwrap();
    let form = catalog.get(b"AcroForm").unwrap().as_reference().unwrap();
    let mut ids = vec![page, form];
    let page_dict = pdf.get_dictionary(page).unwrap();
    let annots_at = page_dict
        .get(b"Annots")
        .ok()
        .and_then(|a| a.as_reference().ok());
    ids.extend(annots_at);
    append_revision(&bytes, &ids, |pdf| {
        // What the watermark draws with: its own names alone (the page has
        // an `/F1` of its own).
        let bold = pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold",
        });
        let gs = pdf.add_object(dictionary! { "Type" => "ExtGState", "ca" => 0.3, "CA" => 0.3 });
        let drawn_with = dictionary! {
            "Font" => dictionary! { "WmF" => bold },
            "ExtGState" => dictionary! { "WmGs" => gs },
        };
        let watermark = pdf.add_object(Stream::new(dictionary! {}, MERGED_WATERMARK.to_vec()));
        let appearance = |pdf: &mut Pdf, fill: &[u8]| {
            let mut content = fill.to_vec();
            content.extend_from_slice(b" 0 0 20 20 re f");
            let bbox = vec![0.into(), 0.into(), 20.into(), 20.into()];
            let stream = Stream::new(
                dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => bbox },
                content,
            );
            pdf.add_object(stream)
        };
        let rect = |i: i64| {
            vec![
                (100 + 40 * i).into(),
                300.into(),
                (120 + 40 * i).into(),
                320.into(),
            ]
        };
        let mut added: Vec<Object> = Vec::new();
        for (i, (flags, fill)) in [(6, HIDDEN), (0, UNPRINTED), (4, PRINTED)]
            .into_iter()
            .enumerate()
        {
            let ap = appearance(pdf, fill);
            let square = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Square", "Rect" => rect(i as i64),
                "F" => flags, "C" => vec![1.into(), 0.into(), 0.into()],
                "AP" => dictionary! { "N" => ap },
            });
            added.push(square.into());
        }
        let (yes, off) = (appearance(pdf, CHECKED), appearance(pdf, b"1 g"));
        let check = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn",
            "T" => Object::string_literal("sheafcheck"), "Rect" => rect(3), "F" => 4, "P" => page,
            "V" => "Yes", "DA" => Object::string_literal("/ZaDb 0 Tf 0 g"),
            "AP" => dictionary! { "N" => dictionary! { "Yes" => yes, "Off" => off } },
        });
        added.push(check.into());
        let d = pdf.get_dictionary_mut(page).unwrap();
        let mut contents = Vec::new();
        match d.get(b"Contents").unwrap() {
            Object::Array(a) => contents.extend(a.iter().cloned()),
            c => contents.push(c.clone()),
        }
        contents.push(watermark.into());
        d.set("Contents", contents);
        let resources = d.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
        for (kind, names) in drawn_with.iter() {
            let mut have: Dictionary = (resources.get(kind).and_then(Object::as_dict))
                .cloned()
                .unwrap_or_default();
            for (name, object) in names.as_dict().unwrap().iter() {
                have.set(name.clone(), object.clone());
            }
            resources.set(kind.clone(), have);
        }
        let annots = match annots_at {
            Some(at) => pdf.get_object_mut(at).unwrap(),
            None => {
                let d = pdf.get_dictionary_mut(page).unwrap();
                if !d.has(b"Annots") {
                    d.set("Annots", Vec::<Object>::new());
                }
                d.get_mut(b"Annots").unwrap()
            }
        };
        annots.as_array_mut().unwrap().extend(added);
        let acroform = pdf.get_dictionary_mut(form).unwrap();
        acroform
            .get_mut(b"Fields")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(check.into());
    })
}

/// A watermark another file's generator drew, merged into a page's
/// content: "DRAFT" in red, its list marked as generator `g1a`'s.
const MERGED_WATERMARK: &[u8] = b"/Artifact <</Type/Pagination/Subtype/Watermark/SHEF_Gen (g1a)>> \
    BDC\nq\n/WmGs gs 0.7071 0.7071 -0.7071 0.7071 200.000 300.000 cm BT 0.784 0.118 0.118 rg \
    /WmF 72.00 Tf 0 0 Td (DRAFT) Tj ET\nQ\nEMC\n";

/// The Bates numbers every page is given, `SHEAF000001` on the first.
fn numbered() -> HeaderFooterSpec {
    HeaderFooterSpec {
        bates: Some(BatesSpec {
            prefix: "SHEAF".into(),
            suffix: String::new(),
            digits: 6,
            start: 1,
        }),
        ..fx::header("<<bates>>")
    }
}

/// The prepared file copied to `dir` as `name`, opened, and edited, not
/// saved: a note on its first page, and Bates numbers on every page. Its
/// path, and the document.
fn edited(prepared: &[u8], dir: &Path, name: &str) -> (PathBuf, Document) {
    let path = dir.join(name);
    std::fs::write(&path, prepared).unwrap();
    let mut doc = common::open(&path);
    let first = doc.page(0).unwrap();
    let ops = vec![fx::add(first, &note()), fx::set_header(numbered(), None)];
    doc.commit_ops(ops, "Note and number").unwrap();
    (path, doc)
}

/// What a save is told when it asks, as print answers: break signatures,
/// drop an XFA part's values.
const ANSWERS: AskAnswers = PRINT_ANSWERS;

/// The ways out of a document (M10 proposal 7's list).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Way {
    /// Saved in place, by a Rewrite or by an Append, whatever the plan
    /// would decide.
    Save(How),
    /// Saved to another file: written whole, or the file copied and
    /// appended to, whatever the plan would decide.
    SaveAs(How),
    /// The recovery sidecar: sections after the file (an Append's), or
    /// the whole document (a Rewrite's), when none can follow it.
    Sidecar(How),
    CleanCopy,
    Flattened,
    /// The second page alone.
    Extract,
    /// The first of a split's parts of a page each.
    SplitPart,
    /// The file, and a blank page after it, combined on a session.
    Combine,
    Print {
        markup: bool,
    },
    /// Print's Save as PDF, with the markup.
    SaveAsPdf,
    /// A stamp made of the first page, as Add from PDF Page… makes one.
    PageStamp,
    /// The comment summary, of the pages and their comments.
    Summary,
}

const WAYS: [Way; 16] = [
    Way::Save(How::Rewritten),
    Way::Save(How::Appended),
    Way::SaveAs(How::Rewritten),
    Way::SaveAs(How::Appended),
    Way::Sidecar(How::Appended),
    Way::Sidecar(How::Rewritten),
    Way::CleanCopy,
    Way::Flattened,
    Way::Extract,
    Way::SplitPart,
    Way::Combine,
    Way::Print { markup: true },
    Way::Print { markup: false },
    Way::SaveAsPdf,
    Way::PageStamp,
    Way::Summary,
];

/// The file `way` writes of the prepared file, edited (`edited`), in
/// `dir`: where it is.
fn way_out(way: Way, prepared: &[u8], dir: &Path) -> PathBuf {
    let name = format!("{way:?}").replace([' ', '{', '}', ':', '(', ')'], "");
    let (path, mut doc) = edited(prepared, dir, &format!("{name}.pdf"));
    let out = dir.join(format!("{name} out.pdf"));
    let from = || doc.export_from(Actor::User);
    let file = || Out::File {
        path: out.clone(),
        overwrite: false,
    };
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    let written = |bytes: &[u8]| std::fs::write(&out, bytes).unwrap();
    match way {
        Way::Save(how) => {
            how.save(&mut doc, &path);
            return path;
        }
        Way::SaveAs(how) => {
            how.save(&mut doc, &out);
        }
        Way::Sidecar(how) => {
            let (file, base) = doc.file().map(|(f, b)| (f.to_path_buf(), *b)).unwrap();
            let mut input = doc.recovery_input();
            if how == How::Rewritten {
                // A record no update can follow: the sidecar is the whole
                // document.
                let saved = input.saved.as_mut().unwrap();
                let record = SaveRecord {
                    sound: false,
                    ..SaveRecord::clone(&saved.record)
                };
                saved.record = Arc::new(record);
            }
            sheaf_doc::recovery::write(&input, &file, &base, &NoProgress).unwrap();
            let rec = sheaf_doc::recovery::find(&file).expect("a sidecar");
            let mut bytes = match &rec.pending.bytes {
                Some(_) => std::fs::read(&file).unwrap(),
                None => Vec::new(),
            };
            assert_eq!(rec.pending.bytes.is_some(), how == How::Appended, "{way:?}");
            bytes.extend_from_slice(&rec.bytes);
            written(&bytes);
            sheaf_doc::recovery::discard(&file);
        }
        Way::CleanCopy => exported(Export::clean_copy(from(), file())),
        Way::Flattened => exported(Export::flattened(from(), file())),
        Way::Extract => exported(Export::extract(from(), &pages[1..2], file())),
        Way::SplitPart => {
            let parts = dir.join(format!("{name} parts"));
            let by = SplitBy::EveryN(1);
            let parts =
                sheaf_ops::split::split(&from(), &by, &parts, "part", false, &ANSWERS, &NoProgress)
                    .unwrap();
            return parts[0].clone();
        }
        Way::Combine => {
            let blank = dir.join(format!("{name} blank.pdf"));
            let page = sheaf_doc::memory::blank_pages(612.0, 792.0, 1).unwrap();
            std::fs::write(&blank, page).unwrap();
            let items = [CombineItem::new(&path), CombineItem::new(&blank)];
            let params = CombineParams::default();
            sheaf_ops::combine::combine(&items, &params, &out, &ANSWERS, &NoProgress).unwrap();
        }
        Way::Print { markup } => {
            written(&sheaf_doc::export::printed(from(), &pages, markup, &NoProgress).unwrap())
        }
        Way::SaveAsPdf => exported(Export {
            out: file(),
            ..Export::print(from(), &pages, true)
        }),
        Way::PageStamp => {
            let stamps = dir.join(format!("{name} stamps"));
            let (collection, _) =
                sheaf_ops::stamps::add_pdf_page(&stamps, "Pages", &path, 0, None).unwrap();
            written(&sheaf_ops::stamps::stamp_pdf(&collection, 0).unwrap());
        }
        Way::Summary => {
            let cf = CommentFilter::default();
            let mut made =
                sheaf_ops::comments::summary(&from(), &cf, "", Layout::Pages, "Summary").unwrap();
            let to = SaveTo::File {
                path: out.clone(),
                append: false,
            };
            made.save(to, &ANSWERS, &NoProgress).unwrap();
        }
    }
    out
}

/// `e` written, what it asks answered as print answers.
fn exported(e: Export) {
    run(e, &ANSWERS, &NoProgress).unwrap();
}

/// The forms placed on page `index` (from 0) of `bytes`, as their
/// content, decoded: what a flatten drew there.
fn drawn_on(bytes: &[u8], index: u32) -> Vec<Vec<u8>> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let page = pdf.get_pages()[&(index + 1)];
    let resources = pdf.get_dictionary(page).unwrap().get(b"Resources").unwrap();
    let resources = pdf.dereference(resources).unwrap().1.as_dict().unwrap();
    let Ok(xobjects) = resources.get(b"XObject") else {
        return Vec::new();
    };
    let xobjects = pdf.dereference(xobjects).unwrap().1.as_dict().unwrap();
    (xobjects.iter())
        .filter_map(|(_, x)| {
            let stream = pdf.dereference(x).ok()?.1.as_stream().ok()?;
            Some(
                stream
                    .decompressed_content()
                    .unwrap_or(stream.content.clone()),
            )
        })
        .collect()
}

/// Which of the first page's added annotations `bytes` draws there.
fn placed_marks(bytes: &[u8]) -> Vec<&'static str> {
    let forms = drawn_on(bytes, 0);
    let named = [
        ("hidden", HIDDEN),
        ("unprinted", UNPRINTED),
        ("printed", PRINTED),
        ("checked", CHECKED),
    ];
    (named.into_iter())
        .filter(|(_, fill)| {
            forms
                .iter()
                .any(|f| f.windows(fill.len()).any(|w| w == *fill))
        })
        .map(|(name, _)| name)
        .collect()
}

/// Whether the file's form has an XFA part.
fn has_xfa(bytes: &[u8]) -> bool {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let catalog = pdf.catalog().unwrap();
    let Ok(form) = catalog.get(b"AcroForm") else {
        return false;
    };
    let form = pdf.dereference(form).unwrap().1.as_dict().unwrap();
    form.has(b"XFA")
}

/// The Bates numbers `doc`'s header gives its pages, in order.
fn bates_numbers(doc: &Document) -> Vec<Option<u64>> {
    let st = doc.state();
    let header = st.header_footer().expect("a header");
    (st.order().iter())
        .map(|&p| header.frozen.bates_number(p))
        .collect()
}

#[test]
fn every_way_out_keeps_the_form_and_its_rules() {
    common::own_dirs("every_way_out_keeps_the_form_and_its_rules", every_way_out);
}

fn every_way_out() {
    let dir = scratch("every-way-out");
    let (base, field) = base();
    let prepared = prepared(&base, field);
    let was = check(&prepared, None).unwrap();
    assert!(
        holds(&prepared, b"/SHEF_Gen (g1a)"),
        "another file's generator's watermark"
    );
    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    let mut outs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for way in WAYS {
        let out = way_out(way, &prepared, &dir);
        if let Err(e) = nothing_found(Some(&was), &out, |_| false) {
            failed.insert(format!("{way:?}"), e);
        }
        outs.insert(format!("{way:?}"), std::fs::read(&out).unwrap());
    }
    let out = |way: Way| &outs[&format!("{way:?}")];

    // A clean copy holds none of sheaf's private data, what marks another
    // file's generator's watermark among it, which a save keeps.
    let clean = Pdf::load_mem(out(Way::CleanCopy)).unwrap();
    assert_eq!(sheaf_doc::private::found(&clean), []);
    assert!(!holds(out(Way::CleanCopy), b"SHEF_Gen"));
    assert!(holds(out(Way::Save(How::Rewritten)), b"/SHEF_Gen (g1a)"));

    // The extract of the second page keeps its Bates number, set and not
    // saved, drawn and as the file says it: not renumbered from 1.
    let extract = out(Way::Extract);
    assert!(holds(extract, b"SHEAF000002") && !holds(extract, b"SHEAF000001"));
    let reopened = common::opened(extract.clone(), "extract.pdf", None);
    assert_eq!(bates_numbers(&reopened), [Some(2)]);

    // A flatten draws what's shown, print's what prints, never a hidden
    // annotation, and a check box with no `/AS` not at all.
    assert_eq!(placed_marks(out(Way::Flattened)), ["unprinted", "printed"]);
    for printed in [Way::Print { markup: true }, Way::SaveAsPdf] {
        assert_eq!(placed_marks(out(printed)), ["printed"], "{printed:?}");
    }
    // Printed without markup, nothing's drawn of it, and like a clean copy
    // it holds none of sheaf's private data.
    let unmarked = out(Way::Print { markup: false });
    assert_eq!(placed_marks(unmarked), Vec::<&str>::new());
    let unmarked = Pdf::load_mem(unmarked).unwrap();
    assert_eq!(sheaf_doc::private::found(&unmarked), []);

    // The form's XFA part goes with the pages of some of its widgets, and
    // stays when the form is left as it is.
    assert!(has_xfa(&prepared));
    assert!(has_xfa(out(Way::Save(How::Rewritten))));
    for part in [Way::Extract, Way::SplitPart] {
        assert!(!has_xfa(out(part)), "{part:?}");
    }

    // Every export refuses the open file, a split whose part it would be
    // and the comment exports too: the file is as it was.
    let (path, doc) = edited(&prepared, &dir, "own_Part_1.pdf");
    let own = || Out::File {
        path: path.clone(),
        overwrite: true,
    };
    let from = || doc.export_from(Actor::User);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    let rejects: Vec<(PageId, String)> = vec![(pages[0], "Blank".into())];
    let exports = [
        Export::whole(from(), own()),
        Export::extract(from(), &pages[..1], own()),
        Export::flattened(from(), own()),
        Export::clean_copy(from(), own()),
        Export::rejects(from(), &rejects, own()),
        Export {
            out: own(),
            ..Export::print(from(), &pages, true)
        },
    ];
    for e in exports {
        let gate = Gate::OwnFile { path: path.clone() };
        assert_eq!(preflight(&e), [gate]);
        let refused = run(e, &ANSWERS, &NoProgress).unwrap_err();
        assert!(
            matches!(refused, ExportError::Refused(Gate::OwnFile { .. })),
            "{refused}"
        );
    }
    let by = SplitBy::EveryN(1);
    let split = sheaf_ops::split::split(&from(), &by, &dir, "own", true, &ANSWERS, &NoProgress);
    let mut refusals = vec![split.unwrap_err()];
    // The comment exports, which write through writers of their own: the
    // summary, and a data file.
    let all = (&CommentFilter::default(), "");
    let summary = comments::summary_to(&from(), all, Layout::Pages, "own", &path);
    refusals.push(summary.unwrap_err());
    let data = comments::export_data(&doc.snapshot(), &[], DataFormat::Xfdf).unwrap();
    refusals.push(comments::write_to(&doc.snapshot(), &path, &data).unwrap_err());
    for refused in refusals {
        let refused = format!("{refused:#}");
        assert!(refused.contains("can't replace the open file"), "{refused}");
    }
    assert_eq!(std::fs::read(&path).unwrap(), prepared);

    // And the form check finds nothing in any that it didn't in the file.
    assert!(failed.is_empty(), "{failed:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ----- The exports over the corpus ------------------------------------------

/// Corpus paths, comma-separated, to run [`export_round_trips`] on alone
/// (`SHEAF_EXPORT_ONLY=forms/irs/f1040.pdf,…`): for looking at a few files
/// by hand. Unset, it takes every file.
const EXPORT_ONLY: &str = "SHEAF_EXPORT_ONLY";

/// Every corpus form's extract of its first page, clean copy and
/// flattened copy, each answered as print answers what it asks: the form
/// check finds nothing in it the file didn't have, but what breaking a
/// signature and dropping an XFA part's values leave (`told`); the clean
/// copy holds none of sheaf's private data; and each opens again. A file
/// that doesn't open without a password is passed over.
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test exports -- --ignored --nocapture"]
fn export_round_trips() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("export-round-trips");
    let entries = common::narrowed(EXPORT_ONLY, corpus.forms().unwrap(), |e| &e.path);
    let outcomes = each_entry(&entries, |entry| exported_three_ways(entry, &corpus, &dir));
    let mut files = 0;
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        match outcome {
            None => {}
            Some(Ok(())) => files += 1,
            Some(Err(e)) => {
                files += 1;
                failed.insert(entry.path.clone(), e);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "{files} of {} files extracted, clean-copied and flattened, and opened again; {} failed",
        entries.len(),
        failed.len()
    );
    for (path, why) in &failed {
        println!("{path}: {why}");
    }
    // Its radio group's chosen button is on the second page: the extract
    // of the first keeps the group's value, which none of the buttons left
    // shows, as a delete of the second page does (slice 19's notes, Found).
    let chosen_left_out = (
        "forms/safedocs/0070616.pdf",
        r#"extract: the form check finds [NoAppearance(["Gruppieren4"])]"#,
    );
    assert!(
        (failed.iter()).all(|(path, why)| (path.as_str(), why.as_str()) == chosen_left_out),
        "{failed:#?}"
    );
}

/// [`export_round_trips`] of one file: None when it doesn't open.
fn exported_three_ways(entry: &Entry, corpus: &Corpus, dir: &Path) -> Option<Result<(), String>> {
    let path = corpus.path(entry);
    let doc = sheaf_doc::open::file(&path, None).ok()?.doc;
    let was: Option<Report> = std::fs::read(&path).ok().and_then(|b| check(&b, None).ok());
    let tag = entry.path.replace('/', "-");
    let first = doc.page(0)?;
    type Make = fn(ExportFrom, Out, PageId) -> Export;
    let ways: [(&str, Make); 3] = [
        ("extract", |from, out, first| {
            Export::extract(from, &[first], out)
        }),
        ("clean copy", |from, out, _| Export::clean_copy(from, out)),
        ("flattened", |from, out, _| Export::flattened(from, out)),
    ];
    let outcome = (|| {
        for (what, make) in ways {
            let out = dir.join(format!("{what} {tag}"));
            let file = Out::File {
                path: out.clone(),
                overwrite: true,
            };
            let e = make(doc.export_from(Actor::User), file, first);
            run(e, &ANSWERS, &NoProgress).map_err(|e| format!("{what}: {e}"))?;
            nothing_found(was.as_ref(), &out, told).map_err(|e| format!("{what}: {e}"))?;
            if what == "clean copy" {
                let pdf = Pdf::load_mem(&std::fs::read(&out).unwrap()).unwrap();
                let found = sheaf_doc::private::found(&pdf);
                if !found.is_empty() {
                    return Err(format!("{what} holds {found:?}"));
                }
            }
            sheaf_doc::open::file(&out, None).map_err(|e| format!("{what} opens: {e:#}"))?;
            let _ = std::fs::remove_file(&out);
        }
        Ok(())
    })();
    Some(outcome)
}
