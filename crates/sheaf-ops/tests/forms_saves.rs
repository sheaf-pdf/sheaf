//! What a save takes out of a form, and asks first about (M10 slice 5).
//! A changed hybrid form loses its XFA copy, its tooltips kept in `/TU`
//! (bug 6); a rewrite drops Reader's usage rights, and `/SigFlags` when
//! nothing signed needs it (bug 7); an XFA copy holding values the
//! AcroForm lacks is found; a save that would rewrite a signed file asks,
//! and appending leaves its signed bytes as they were, an encrypted one's
//! too. A save, an edit and another save keep the form, the marks, other
//! tools' annotations and the bookmarks (M10.5 slice 8).
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::path::Path;

use lopdf::{
    Document as Pdf, EncryptionState, EncryptionVersion, Object, ObjectId, Permissions, dictionary,
};
use sheaf_doc::forms::fixture::{Form, note, outline_tree};
use sheaf_doc::forms::{Finding, Value, check};
use sheaf_doc::materialize::{Ask, AskAnswers, Note, SaveReport, SaveStop};
use sheaf_doc::{AnnotBody, AnnotId, Document, Edit, ObjRef, SourceInfo};

mod common;
use common::*;

/// A two-page hybrid form as Designer names one: `form1[0].x[0]` on page
/// 1 ("Ex") and `form1[0].y[0]` on page 2 ("Why"), with an XFA copy whose
/// tooltips the AcroForm lacks.
fn hybrid() -> Form {
    let mut f = Form::new(2);
    let x = blue(&mut f, "x[0]", 0, 0, "Ex");
    let y = blue(&mut f, "y[0]", 1, 0, "Why");
    f.group("form1[0]", &[x, y]);
    f.xfa();
    f
}

/// The form's `/XFA`, `/Perms` and `/SigFlags`, as there or not.
fn parts(bytes: &[u8]) -> (bool, bool, bool) {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let catalog = pdf.catalog().unwrap();
    let form = catalog
        .get(b"AcroForm")
        .ok()
        .and_then(|f| pdf.dereference(f).ok())
        .and_then(|(_, f)| f.as_dict().ok());
    (
        form.is_some_and(|f| f.has(b"XFA")),
        catalog.has(b"Perms"),
        form.is_some_and(|f| f.has(b"SigFlags")),
    )
}

/// Each field's own `/TU`, by full name.
fn tooltips(bytes: &[u8]) -> Vec<(String, Option<String>)> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    form_in(bytes)
        .fields
        .into_iter()
        .map(|f| {
            let tu = pdf
                .get_dictionary(f.object)
                .unwrap()
                .get(b"TU")
                .ok()
                .map(sheaf_doc::file::pdf_text);
            (f.name, tu)
        })
        .collect()
}

sheaf_doc::both_ways!(
    a_changed_hybrid_form_loses_its_xfa_copy,
    a_changed_hybrid_form_loses_its_xfa_copy_appended: changed_hybrid_loses_xfa
);

fn changed_hybrid_loses_xfa(how: How) {
    // Bug 6: a page deleted, so the XFA describes a field that's gone.
    let dir = scratch(&format!("saves-xfa-{how:?}"));
    let path = write(&dir, "form.pdf", &mut hybrid());
    let mut doc = open(&path);
    let second = doc.state().order()[1];
    doc.apply(Edit::DeletePages { ids: vec![second] }, "Delete")
        .unwrap();
    assert!(drops_xfa(&doc, &path));
    let written = (how.strategy(), written(&doc, how));
    for (strategy, bytes) in [written, saved(&mut doc, &path, how)] {
        let what = format!("{strategy:?}");
        if strategy == StrategyKind::Rewrite {
            clean(&bytes, &what);
        }
        assert!(!parts(&bytes).0, "{what}: the XFA copy stayed");
        // Its tooltip is the AcroForm's now. (A recovery sidecar can't
        // take `y` out: a save rewrites.)
        let tips = tooltips(&bytes);
        assert!(
            tips.contains(&("form1[0].x[0]".into(), Some("Tip for x".into()))),
            "{what}: {tips:?}"
        );
        if strategy == StrategyKind::Rewrite {
            assert_eq!(tips.len(), 1, "{what}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// An export that leaves out a hybrid form's widget page changes its form
/// as a page delete does: extracted pages and a printed copy drop the XFA
/// copy, its tooltip on the field kept in `/TU`, and the field left out
/// goes. Every page exported keeps the form as it was, XFA and all.
#[test]
fn an_export_without_a_hybrid_form_s_widget_page_drops_its_xfa() {
    let dir = scratch("export-xfa");
    let path = write(&dir, "form.pdf", &mut hybrid());
    let doc = open(&path);
    let snap = doc.snapshot();
    let order: Vec<_> = snap.state.order().iter().copied().collect();
    let extracted = sheaf_doc::save::extract_bytes(&snap, &order[..1]).unwrap();
    let printed = sheaf_doc::save::print_pdf(&snap, &order[..1], false).unwrap();
    for (bytes, what) in [(&extracted, "extracted"), (&printed, "printed")] {
        clean(bytes, what);
        assert!(!parts(bytes).0, "{what}: the XFA copy stayed");
        assert_eq!(
            tooltips(bytes),
            [("form1[0].x[0]".into(), Some("Tip for x".into()))],
            "{what}"
        );
    }
    let every = sheaf_doc::save::extract_bytes(&snap, &order).unwrap();
    clean(&every, "every page");
    assert!(parts(&every).0, "every page: the XFA copy went");
    assert_eq!(tooltips(&every).len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    an_unchanged_hybrid_form_keeps_its_xfa_copy,
    an_unchanged_hybrid_form_keeps_its_xfa_copy_appended: unchanged_hybrid_keeps_xfa
);

fn unchanged_hybrid_keeps_xfa(how: How) {
    // A page turned: the form is as it was.
    let dir = scratch(&format!("saves-xfa-kept-{how:?}"));
    let path = write(&dir, "form.pdf", &mut hybrid());
    let mut doc = open(&path);
    let first = doc.state().order()[0];
    doc.apply(
        Edit::RotatePages {
            ids: vec![first],
            by: sheaf_core::Rotation::R90,
        },
        "Rotate",
    )
    .unwrap();
    assert!(!drops_xfa(&doc, &path));
    let written = (how.strategy(), written(&doc, how));
    for (strategy, bytes) in [written, saved(&mut doc, &path, how)] {
        let what = format!("{strategy:?}");
        clean(&bytes, &what);
        assert!(parts(&bytes).0, "{what}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_form_brought_into_a_hybrid_one_is_saved_in_full() {
    // An insert changes the form: a save takes the XFA copy out.
    let dir = scratch("saves-xfa-insert");
    let path = write(&dir, "form.pdf", &mut hybrid());
    let other = write(&dir, "other.pdf", &mut Form::new(1));
    let mut doc = open(&path);
    {
        let mut f = Form::new(1);
        blue(&mut f, "z", 0, 0, "Zed");
        std::fs::write(&other, f.bytes()).unwrap();
    }
    let (info, _) = SourceInfo::read(doc.store(), &other, None).unwrap();
    let sid = doc.add_source(info);
    doc.apply(
        Edit::InsertPages {
            source: sid,
            range: 0..1,
            before: None,
            outline: None,
            annots: Vec::new(),
        },
        "Insert",
    )
    .unwrap();
    let drop_xfa = AskAnswers {
        drop_xfa_values: true,
        ..Default::default()
    };
    let report = save(&mut doc, &path, drop_xfa).unwrap();
    assert_eq!(report.strategy, StrategyKind::Rewrite);
    let removed = Note(sheaf_doc::forms::rules::XFA_REMOVED.into());
    assert!(report.notes.contains(&removed), "{:?}", report.notes);
    let bytes = std::fs::read(&path).unwrap();
    let report = clean(&bytes, "saved");
    assert_eq!(report.fields, 3);
    assert!(!parts(&bytes).0);
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    usage_rights_go_with_a_rewrite,
    usage_rights_go_with_a_rewrite_appended: usage_rights_go
);

fn usage_rights_go(how: How) {
    // Bug 7: they'd still sign the original's bytes.
    let dir = scratch(&format!("saves-rights-{how:?}"));
    let mut f = hybrid();
    f.usage_rights();
    let path = write(&dir, "form.pdf", &mut f);
    assert_eq!(parts(&std::fs::read(&path).unwrap()), (true, true, true));
    let mut doc = open(&path);
    let first = doc.state().order()[0];
    doc.apply(
        Edit::RotatePages {
            ids: vec![first],
            by: sheaf_core::Rotation::R90,
        },
        "Rotate",
    )
    .unwrap();
    // Saved as it is: an Append keeps them, still signing the original.
    let want = match how {
        // No signature needs `/SigFlags` now; the form is as it was.
        How::Rewritten => (true, false, false),
        How::Appended => (true, true, true),
    };
    let written = (how.strategy(), written(&doc, how));
    for (strategy, bytes) in [written, saved(&mut doc, &path, how)] {
        let what = format!("{strategy:?}");
        assert_eq!(strategy, how.strategy());
        clean(&bytes, &what);
        assert_eq!(parts(&bytes), want, "{what}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_xfa_copy_holding_values_the_acroform_lacks_is_found() {
    // `y` filled in the XFA's datasets only, as a tool that writes only XFA
    // leaves it.
    let mut f = hybrid();
    let y = f.fields()[0];
    let y = f.widgets_of(y)[1];
    f.xfa();
    f.dict(y).remove(b"V");
    let facts = form_in(&f.bytes()).facts;
    assert_eq!(facts.xfa_values, 1);
    // With its value back in the AcroForm, nothing's missing.
    f.dict(y).set("V", Object::string_literal("Why"));
    assert_eq!(form_in(&f.bytes()).facts.xfa_values, 0);
}

/// A two-page form with a signed signature field on page 1, and a text
/// field on each page.
fn signed() -> Form {
    let mut f = Form::new(2);
    blue(&mut f, "a", 0, 0, "Alpha");
    blue(&mut f, "b", 1, 0, "LEAKVALUE123");
    let sig = f.signature("sig", 0, row(2));
    f.sign(sig);
    f
}

/// Whether the file's signature fits it: its `/ByteRange` ends where a
/// revision ends.
fn signature_fits(bytes: &[u8]) -> bool {
    !check(bytes, None)
        .unwrap()
        .findings
        .iter()
        .any(|f| matches!(f, Finding::StaleSignature { .. }))
}

/// `doc` saved to `path` (`materialize::save`) with `answers`.
fn save(doc: &mut Document, path: &Path, answers: AskAnswers) -> Result<SaveReport, SaveStop> {
    sheaf_doc::materialize::save(doc, path, &answers, &sheaf_ops::NoProgress)
}

/// A save of a signed file asks first, Rewrite or Cancel (M10.5 §9.3):
/// asked, nothing's written and the document is as unsaved as it was;
/// rewritten, the signature breaks and the deleted page's value goes. An
/// unsigned file isn't asked about.
#[test]
fn a_save_that_would_rewrite_a_signed_file_asks() {
    let dir = scratch("saves-signed");
    let path = write(&dir, "signed.pdf", &mut signed());
    let original = std::fs::read(&path).unwrap();
    assert!(signature_fits(&original));
    let mut doc = open(&path);
    let second = doc.state().order()[1];
    doc.apply(Edit::DeletePages { ids: vec![second] }, "Delete")
        .unwrap();
    // Asked: nothing's written.
    let asks = SaveStop::Ask(Ask::Signatures {
        signatures: 1,
        append_anyway: None,
        reason: None,
    });
    assert_eq!(save(&mut doc, &path, AskAnswers::default()), Err(asks));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(doc.is_dirty());
    // Rewritten elsewhere: the signature breaks, and the value goes.
    let copy = dir.join("rewritten.pdf");
    let rewrite = AskAnswers {
        break_signatures: true,
        ..Default::default()
    };
    let report = save(&mut doc, &copy, rewrite).unwrap();
    assert_eq!(report.strategy, StrategyKind::Rewrite);
    assert!(!doc.is_dirty());
    let rewritten = std::fs::read(&copy).unwrap();
    assert!(!signature_fits(&rewritten));
    assert!(!holds(&rewritten, b"LEAKVALUE123"));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // Saved onto the file that Rewrite wrote, it isn't asked again: that
    // file has no signature left to break. Saved over the signed file
    // again, it is.
    let first = doc.state().order()[0];
    doc.apply(
        Edit::RotatePages {
            ids: vec![first],
            by: sheaf_core::Rotation::R90,
        },
        "Rotate",
    )
    .unwrap();
    save(&mut doc, &copy, AskAnswers::default()).unwrap();
    assert!(!doc.is_dirty());
    doc.apply(
        Edit::RotatePages {
            ids: vec![first],
            by: sheaf_core::Rotation::R90,
        },
        "Rotate",
    )
    .unwrap();
    let again = save(&mut doc, &path, AskAnswers::default()).unwrap_err();
    assert!(
        matches!(again, SaveStop::Ask(Ask::Signatures { .. })),
        "{again:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // An unsigned file isn't asked about.
    let plain = write(&dir, "plain.pdf", &mut hybrid());
    let mut doc = open(&plain);
    let second = doc.state().order()[1];
    doc.apply(Edit::DeletePages { ids: vec![second] }, "Delete")
        .unwrap();
    let drop_xfa = AskAnswers {
        drop_xfa_values: true,
        ..Default::default()
    };
    save(&mut doc, &plain, drop_xfa).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Appended, a signed file's bytes are as they were and its signature
/// fits; the deleted page's value stays in the earlier revision, as the
/// reader is told. Slice 10 brings the Append back.
#[test]
#[ignore = "slice 10"]
fn a_signed_file_appended_keeps_its_signature() {
    let dir = scratch("saves-signed-appended");
    let path = write(&dir, "signed.pdf", &mut signed());
    let original = std::fs::read(&path).unwrap();
    let mut doc = open(&path);
    let second = doc.state().order()[1];
    doc.apply(Edit::DeletePages { ids: vec![second] }, "Delete")
        .unwrap();
    let report = sheaf_doc::materialize::save_appended(&mut doc, &path).unwrap();
    assert_eq!(report.strategy, StrategyKind::Append);
    let appended = std::fs::read(&path).unwrap();
    assert!(appended.starts_with(&original));
    assert!(signature_fits(&appended));
    let report = check(&appended, None).unwrap();
    assert_eq!((report.pages, report.signed), (1, 1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Slice 10 brings the Append back.
#[test]
#[ignore = "slice 10"]
fn an_encrypted_signed_file_appends_and_keeps_its_signature() {
    // Opened with its empty user password, asked about, then appended to,
    // encrypted with the file's own key (M10.5 slice 7).
    let dir = scratch("saves-signed-encrypted");
    let mut f = signed();
    let id = Object::string_literal("0123456789abcdef");
    f.pdf.trailer.set("ID", vec![id.clone(), id]);
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &f.pdf,
        owner_password: "owner",
        user_password: "",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    f.pdf.encrypt(&state).unwrap();
    let path = write(&dir, "signed.pdf", &mut f);
    let original = std::fs::read(&path).unwrap();
    assert!(signature_fits(&original));
    let mut doc = open(&path);
    let [first, second] = [0, 1].map(|i| doc.state().order()[i]);
    let title = "APPENDEDTITLE123";
    doc.apply(
        Edit::OutlineInsert {
            parent: None,
            after: None,
            title: title.into(),
            target: Some(first),
            view: None,
        },
        "Bookmark",
    )
    .unwrap();
    doc.apply(Edit::DeletePages { ids: vec![second] }, "Delete")
        .unwrap();
    let asks = SaveStop::Ask(Ask::Signatures {
        signatures: 1,
        append_anyway: None,
        reason: None,
    });
    assert_eq!(save(&mut doc, &path, AskAnswers::default()), Err(asks));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // Appended: the signed bytes are as they were, and the signature fits.
    let report = sheaf_doc::materialize::save_appended(&mut doc, &path).unwrap();
    assert_eq!(report.strategy, StrategyKind::Append);
    let appended = std::fs::read(&path).unwrap();
    assert!(appended.starts_with(&original));
    assert!(signature_fits(&appended));
    let report = check(&appended, None).unwrap();
    assert_eq!((report.pages, report.signed), (1, 1));
    // Still encrypted: the bookmark's title is only there decrypted, and
    // the file opens with its empty user password.
    let section = &appended[original.len()..];
    assert!(!section.windows(title.len()).any(|w| w == title.as_bytes()));
    assert!(holds(&appended, title.as_bytes()));
    let (saved, _) = SourceInfo::read(doc.store(), &path, None).unwrap();
    assert!(saved.src.pdf().is_encrypted());
    assert_eq!(saved.src.page_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_certified_file_asks_too() {
    let dir = scratch("saves-certified");
    let mut f = Form::new(2);
    let sig = f.signature("cert", 0, row(0));
    f.certify(sig);
    let path = write(&dir, "certified.pdf", &mut f);
    let mut doc = open(&path);
    let second = doc.state().order()[1];
    doc.apply(Edit::DeletePages { ids: vec![second] }, "Delete")
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    let err = save(&mut doc, &path, AskAnswers::default()).unwrap_err();
    assert!(
        matches!(err, SaveStop::Ask(Ask::Signatures { signatures: 1, .. })),
        "{err:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn signed_fields_brought_in_are_kept_and_said_so() {
    let dir = scratch("saves-signed-in");
    let a = write(&dir, "a.pdf", &mut Form::new(1));
    let s = write(&dir, "signed.pdf", &mut signed());
    let mut doc = open(&a);
    let (info, _) = SourceInfo::read(doc.store(), &s, None).unwrap();
    let sid = doc.add_source(info);
    let out = doc
        .apply(
            Edit::InsertPages {
                source: sid,
                range: 0..1,
                before: None,
                outline: None,
                annots: Vec::new(),
            },
            "Insert",
        )
        .unwrap();
    assert_eq!(
        doc.form_notice(&out, false, false).as_deref(),
        Some("The signature from signed.pdf is no longer valid here")
    );
    let bytes = sheaf_doc::save::rewrite(&doc.snapshot()).unwrap();
    let report = check(&bytes, None).unwrap();
    assert_eq!(report.signed, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The corpus's `path`, or None (and the test skips) without a corpus.
fn corpus_path(path: &str) -> Option<std::path::PathBuf> {
    let corpus = sheaf_doc::forms::corpus::Corpus::or_skip()?;
    let entry = corpus.entry(path).unwrap().expect("in the manifest");
    Some(corpus.path(&entry))
}

/// The file at `path` opened, its page `index` deleted, and rewritten.
fn without_page(path: &Path, index: usize) -> (Document, Vec<u8>) {
    let mut doc = open(path);
    let id = doc.state().order()[index];
    doc.apply(Edit::DeletePages { ids: vec![id] }, "Delete")
        .unwrap();
    let bytes = sheaf_doc::save::rewrite(&doc.snapshot()).unwrap();
    (doc, bytes)
}

#[test]
fn corpus_a_1040_page_deleted_keeps_the_others_tooltips() {
    let Some(f1040) = corpus_path("forms/irs/f1040.pdf") else {
        return;
    };
    let (_, bytes) = without_page(&f1040, 1);
    // Bug 6: the XFA for two pages in a one-page file; bug 7: usage rights
    // over the original's bytes.
    assert_eq!(parts(&bytes), (false, false, false));
    clean(&bytes, "1040 without page 2");
    let tips = tooltips(&bytes);
    let with = tips.iter().filter(|(_, t)| t.is_some()).count();
    assert!(
        with * 10 >= tips.len() * 9,
        "{with} of {} have /TU",
        tips.len()
    );
    let (name, tip) = tips
        .iter()
        .find(|(n, _)| n == "topmostSubform[0].Page1[0].f1_01[0]")
        .unwrap();
    assert!(
        tip.as_deref().is_some_and(|t| t.starts_with("Page 1.")),
        "{name}: {tip:?}"
    );
}

#[test]
fn corpus_the_w9s_instructions_page_alone_has_no_form() {
    // Bug 6: its page 2, extracted, kept empty `/Fields` with the full XFA
    // and `/Perms`.
    let Some(fw9) = corpus_path("forms/irs/fw9.pdf") else {
        return;
    };
    let doc = open(&fw9);
    let second = doc.state().order()[1];
    let bytes = sheaf_doc::save::extract_bytes(&doc.snapshot(), &[second]).unwrap();
    let report = clean(&bytes, "W-9 page 2");
    assert_eq!(report.class(), "no form");
    assert_eq!(parts(&bytes), (false, false, false));
}

/// A two-page form, `top.a` ("Alpha") on page 1 and `top.b` ("Beta") on
/// page 2, under nested bookmarks, with one of sheaf's marks and another
/// tool's comment on page 1 and a link on page 2.
fn everything() -> Form {
    let mut f = Form::new(2);
    let a = blue(&mut f, "a", 0, 0, "Alpha");
    let b = blue(&mut f, "b", 1, 0, "Beta");
    f.group("top", &[a, b]);
    let mark = sheaf_doc::annot::to_pdf(&note(), &mut f.pdf).main;
    let mark = f.pdf.add_object(mark);
    let theirs = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text",
        "Rect" => vec![300.into(), 300.into(), 320.into(), 320.into()],
        "Contents" => Object::string_literal("Theirs"),
        "NM" => Object::string_literal("theirs-1"),
        "T" => Object::string_literal("Pat"),
    });
    let link = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![300.into(), 400.into(), 400.into(), 420.into()],
        "A" => dictionary! {
            "S" => "URI", "URI" => Object::string_literal("https://example.com/"),
        },
    });
    let mut first = f.annots(0);
    first.extend([mark, theirs]);
    f.set_annots(0, &first);
    let mut second = f.annots(1);
    second.push(link);
    f.set_annots(1, &second);
    let pages = f.pages.clone();
    let item = |title: &str, page: ObjectId| {
        dictionary! {
            "Title" => Object::string_literal(title),
            "Dest" => vec![page.into(), "Fit".into()],
        }
    };
    outline_tree(
        &mut f,
        vec![
            (0, item("Part", pages[0])),
            (1, item("First", pages[0])),
            (1, item("Second", pages[1])),
        ],
    );
    f
}

/// What a document holds, page by page in order: sheaf's marks (their
/// ids and text), other tools' comments and links (what they are and their
/// names), the fields (full names and values), and the bookmarks in
/// pre-order (depth, title and the page they go to).
#[derive(Debug, PartialEq)]
struct Held {
    marks: Vec<(usize, AnnotId, String)>,
    theirs: Vec<(usize, String, String)>,
    fields: Vec<(String, Option<Value>)>,
    bookmarks: Vec<(u16, String, Option<usize>)>,
}

fn held(doc: &Document) -> Held {
    let st = doc.state();
    let pages = || st.order().iter().enumerate();
    let mut fields: Vec<(String, Option<Value>)> = (st.terminals())
        .map(|(id, _, t)| (st.full_name(id).unwrap(), t.value.clone()))
        .collect();
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    Held {
        marks: pages()
            .flat_map(|(i, &p)| st.marks_on(p).map(move |a| (i, a.id, a.contents.clone())))
            .collect(),
        theirs: pages()
            .flat_map(|(i, &p)| {
                (st.foreign_on(p))
                    .map(move |(_, f)| (i, f.summary.subtype.clone(), f.summary.name.clone()))
            })
            .collect(),
        fields,
        bookmarks: (st.bookmarks_preorder().into_iter())
            .map(|(depth, _, b)| {
                (
                    depth,
                    b.title.to_string(),
                    b.page().and_then(|p| st.pos_of(p)),
                )
            })
            .collect(),
    }
}

/// Every record in `doc` that a file holds is the object the primary file
/// has it as: each mark, other tool's annotation, widget, field and
/// bookmark.
fn all_in_the_primary(doc: &Document, what: &str) {
    let st = doc.state();
    let primary = doc.source(doc.primary()).unwrap().src.id();
    let ours = |src: Option<&ObjRef>| src.is_some_and(|s| s.src.id() == primary);
    for (id, a) in st.annots() {
        let src = match &a.body {
            AnnotBody::Mark(m) => m.src.as_ref(),
            AnnotBody::Foreign(f) => f.src.as_ref(),
            AnnotBody::Widget(w) => w.src.as_ref(),
        };
        assert!(ours(src), "{what}: annotation {id:?}");
    }
    for (id, node) in st.fields() {
        assert!(ours(node.src.as_ref()), "{what}: field {id:?}");
    }
    for (_, id, b) in st.bookmarks_preorder() {
        assert!(ours(b.src.as_ref()), "{what}: bookmark {id:?}");
    }
}

/// `doc` saved to `path` as the app saves, by a Rewrite, or by an Append
/// when `incremental`. The document shows what it did.
fn save_by(doc: &mut Document, path: &Path, incremental: bool) {
    let report = match incremental {
        true => sheaf_doc::materialize::save_appended(doc, path),
        false => save(doc, path, AskAnswers::default()),
    };
    let want = [StrategyKind::Rewrite, StrategyKind::Append][usize::from(incremental)];
    assert_eq!(report.unwrap().strategy, want);
}

/// A save changes nothing in the document (M10.5 §9.1): every record is
/// still the object the file it opened has it as, and an edit and a second
/// save write them all again.
#[test]
fn a_save_an_edit_and_a_second_save_keep_everything() {
    saved_again(false);
}

#[test]
#[ignore = "slice 10"]
fn a_save_an_edit_and_a_second_save_keep_everything_appended() {
    saved_again(true);
}

fn saved_again(incremental: bool) {
    let dir = scratch(&format!("saves-again-{incremental}"));
    let what = if incremental {
        "incremental"
    } else {
        "rewrite"
    };
    let path = write(&dir, &format!("{what}.pdf"), &mut everything());
    let mut doc = open(&path);
    let opened = held(&doc);
    assert_eq!(opened.marks.len(), 1, "{what}");
    assert_eq!(opened.theirs.len(), 2, "{what}");
    assert_eq!(opened.fields.len(), 2, "{what}");
    assert_eq!(opened.bookmarks.len(), 3, "{what}");
    save_by(&mut doc, &path, incremental);
    assert_eq!(held(&doc), opened, "{what}: saved");
    all_in_the_primary(&doc, what);

    // Page 2 before page 1, a mark added, a bookmark renamed and the
    // form's first page duplicated.
    let [p1, p2] = [0, 1].map(|i| doc.state().order()[i]);
    let first = doc.state().bookmarks_preorder()[1].1;
    let edits = vec![
        Edit::MovePages {
            ids: vec![p2],
            before: Some(p1),
        },
        Edit::AnnotAdd {
            page: p2,
            annot: note(),
        },
        Edit::OutlineEdit {
            id: first,
            title: Some("Renamed".into()),
            target: None,
            color: None,
            view: None,
            style: None,
        },
        Edit::DuplicatePages { ids: vec![p1] },
    ];
    doc.apply_batch(edits, "Edit").unwrap();
    let edited = held(&doc);
    assert_eq!(edited.marks.len(), 3, "{what}");
    assert_eq!(edited.fields.len(), 3, "{what}: {:?}", edited.fields);
    save_by(&mut doc, &path, incremental);
    assert_eq!(held(&doc), edited, "{what}: saved again");
    let bytes = std::fs::read(&path).unwrap();
    clean(&bytes, what);
    assert_eq!(held(&open(&path)), edited, "{what}: reopened");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two fields named alike, as real forms have them, are each written at
/// its own object's number, not both at the first; so their page
/// duplicated and saved again brings both (M10.5 slice 8).
#[test]
fn fields_named_alike_stay_two_across_saves() {
    named_alike(false);
}

#[test]
#[ignore = "slice 10"]
fn fields_named_alike_stay_two_across_saves_appended() {
    named_alike(true);
}

fn named_alike(incremental: bool) {
    let dir = scratch(&format!("named-alike-{incremental}"));
    let what = if incremental {
        "incremental"
    } else {
        "rewrite"
    };
    let mut f = Form::new(1);
    blue(&mut f, "Name", 0, 0, "first");
    blue(&mut f, "Name", 0, 1, "second");
    let path = write(&dir, &format!("{what}.pdf"), &mut f);
    let mut doc = open(&path);
    save_by(&mut doc, &path, incremental);
    all_in_the_primary(&doc, what);
    // The fields at their objects' numbers, which a save keeps.
    let saved = form_in(&std::fs::read(&path).unwrap());
    let value_at = |object: ObjectId| {
        let f = saved.fields.iter().find(|f| f.object == object).unwrap();
        f.value.clone()
    };
    let st = doc.state();
    let objects: Vec<(ObjectId, Option<Value>)> = (st.terminals())
        .map(|(_, node, t)| (node.src.as_ref().unwrap().id, t.value.clone()))
        .collect();
    assert_ne!(objects[0].0, objects[1].0, "{what}");
    for (object, value) in &objects {
        assert_eq!(&value_at(*object), value, "{what}");
    }
    let page = st.order()[0];
    doc.apply(Edit::DuplicatePages { ids: vec![page] }, "Duplicate")
        .unwrap();
    save_by(&mut doc, &path, incremental);
    let mut values: Vec<Option<Value>> = (held(&open(&path)).fields.into_iter())
        .map(|(_, value)| value)
        .collect();
    values.sort_by_key(|v| format!("{v:?}"));
    let text = |s: &str| Some(Value::Text(s.into()));
    let want = [text("first"), text("first"), text("second"), text("second")];
    assert_eq!(values, want, "{what}");
    let _ = std::fs::remove_dir_all(&dir);
}
