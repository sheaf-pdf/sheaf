//! An Append's write-time checks (M10.5 slice 10, §9.2 and §11.9): one that
//! refers to objects the file hasn't got, or that would leave removed text
//! readable in it, is a Rewrite instead, or on a signed file asks first;
//! and what the removal check reads as text, of what a section unlinks and
//! of what it writes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, Stream, dictionary};
use sheaf_core::Rotation;

use super::append::{self, Refused};
use super::key::ObjKey;
use super::plan::{
    AppendAnyway, Ask, AskAnswers, Note, Onto, SaveStop, Strategy, Why, forced_append,
};
use super::removal::{self, Found, Mode, Section, Vetoes};
use super::{save, save_appended, write_file};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::content::ResId;
use crate::effects::{Removed, RemovedKind};
use crate::forms::fixture::{Form, row};
use crate::forms::sigs::{SigImpact, Verdict};
use crate::model::{Document, Edit, PageId, SourceId, SourceInfo};
use crate::serialize::fixture::holds;
use crate::source::Limits;
use crate::store::BlobKind;
use crate::text_layer::{OcrWord, TextLayer};

/// An empty directory of a test's own, removed when it's dropped, so a
/// test that fails (a property's shrinking runs, among them) leaves
/// nothing behind.
pub(super) struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An empty directory of the test's own ([`Scratch`]).
pub(super) fn scratch() -> Scratch {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("sheaf-removal-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

/// A file of `pages` pages, each saying its number in Helvetica.
pub(super) fn text_file(pages: usize) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let kids: Vec<Object> = (0..pages)
        .map(|n| {
            let text = format!("BT /F1 24 Tf 72 72 Td (page {n}) Tj ET");
            let content = pdf.add_object(Stream::new(dictionary! {}, text.into_bytes()));
            let page = dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
            pdf.add_object(page).into()
        })
        .collect();
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => pages as i64,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// `bytes` as `dir/name`, opened as the app opens it.
pub(super) fn opened(dir: &Path, name: &str, bytes: &[u8]) -> (PathBuf, Document) {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let doc = crate::open::file(&path, None).unwrap().doc;
    (path, doc)
}

/// What each page of the file at `path` says, in order.
pub(super) fn said(path: &Path) -> Vec<String> {
    let pdf = Pdf::load(path).unwrap();
    (pdf.get_pages().into_values())
        .map(|id| {
            let text = String::from_utf8_lossy(&pdf.get_page_content(id)).into_owned();
            let at = text.find("(page ").expect("its number");
            text[at + 1..].split(')').next().unwrap().to_string()
        })
        .collect()
}

/// `run`, and what it warned of on this thread, logged.
pub(super) fn warned<R>(run: impl FnOnce() -> R) -> (R, Vec<String>) {
    struct Capture;
    impl log::Log for Capture {
        fn enabled(&self, meta: &log::Metadata) -> bool {
            meta.level() <= log::Level::Warn
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                WARNED.with_borrow_mut(|w| w.as_mut().map(|w| w.push(record.args().to_string())));
            }
        }
        fn flush(&self) {}
    }
    thread_local! {
        static WARNED: std::cell::RefCell<Option<Vec<String>>> = const {
            std::cell::RefCell::new(None)
        };
    }
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        log::set_logger(&Capture).expect("no other logger in the tests");
        log::set_max_level(log::LevelFilter::Warn);
    });
    WARNED.set(Some(Vec::new()));
    let out = run();
    (out, WARNED.take().unwrap_or_default())
}

pub(super) fn rewrite_answers() -> AskAnswers {
    AskAnswers {
        break_signatures: true,
        drop_xfa_values: true,
        ..Default::default()
    }
}

/// `doc` saved over its file by an Append, whatever a plan would decide,
/// asking nothing, through [`write_file`] as any save is, its checks on,
/// with `answers`: how it was written, and the notes, the save recorded;
/// or why it stopped, the version unsealed.
fn forced(doc: &mut Document, answers: &AskAnswers) -> Result<(Strategy, Vec<Note>), SaveStop> {
    let path = doc.file().unwrap().0.to_path_buf();
    let v = doc.current_version();
    let input = doc.save_input(v).unwrap();
    let plan = forced_append(&input, Onto::Saved);
    match write_file(&input, &plan, doc.save_target(&path), answers, &NoProgress) {
        Ok(written) => {
            let done = (written.strategy.clone(), written.notes.clone());
            written.record_in(doc).unwrap();
            Ok(done)
        }
        Err(stop) => {
            doc.abandon_save(v);
            Err(stop)
        }
    }
}

/// What a Rewrite does to the one signature, of the field `name`.
fn broken(name: &str) -> Vec<SigImpact> {
    vec![SigImpact {
        signature: name.into(),
        verdict: Verdict::Invalid,
    }]
}

/// The key of the object a Rewrite was for, when the removal check made
/// it one.
fn vetoed(strategy: &Strategy) -> Option<ObjKey> {
    match strategy {
        Strategy::Rewrite { why } => match why[..] {
            [Why::RemovalCheck { key }] => Some(key),
            _ => None,
        },
        _ => None,
    }
}

/// A FreeText mark saying `contents`.
pub(super) fn note(contents: &str) -> Annot {
    Annot {
        id: AnnotId(1),
        kind: AnnotKind::FreeText {
            rect: Box2 {
                x: 100.0,
                y: 100.0,
                w: 200.0,
                h: 40.0,
            },
            size: 12.0,
        },
        color: [200, 30, 30],
        opacity: 1.0,
        stroke: 1.0,
        author: "me".into(),
        contents: contents.into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// `doc`'s one mark on page 0, changed by `change`: its id.
fn change_mark(doc: &mut Document, change: impl FnOnce(&mut Annot)) -> AnnotId {
    let page = doc.page(0).unwrap();
    let mut annot = doc.state().marks_on(page).next().unwrap().clone();
    change(&mut annot);
    let id = annot.id;
    doc.apply(Edit::AnnotUpdate { page, annot }, "change")
        .unwrap();
    id
}

/// OCR text saying `text`.
pub(super) fn layer(text: &str) -> Arc<TextLayer> {
    let word = OcrWord {
        text: text.into(),
        rect: Box2 {
            x: 72.0,
            y: 100.0,
            w: 60.0,
            h: 12.0,
        },
        confidence: 0.9,
    };
    let geom = crate::geom::Geom::read(None, None, 0);
    Arc::new(TextLayer::from_engine(vec![word], &geom, "test"))
}

/// Pages deleted, the file rewritten (pruning their objects), and the
/// delete undone: a plain Append would refer to what the file hasn't got,
/// so the save is a Rewrite for it, and the pages show again.
#[test]
fn pages_a_rewrite_pruned_come_back_by_a_rewrite() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let gone = doc.page(1).unwrap();
    doc.apply(Edit::DeletePages { ids: vec![gone] }, "delete")
        .unwrap();
    save(&mut doc, &path, &rewrite_answers(), &NoProgress).unwrap();
    assert_eq!(said(&path), ["page 0", "page 2"]);
    doc.undo().unwrap();
    let (strategy, notes) = forced(&mut doc, &AskAnswers::default()).unwrap();
    let why = vec![Why::BaseObjectsMissing];
    assert_eq!(strategy, Strategy::Rewrite { why });
    assert!(
        notes
            .iter()
            .any(|n| n.0.starts_with("Saved the whole file")),
        "{notes:?}"
    );
    assert!(!doc.is_dirty());
    assert_eq!(said(&path), ["page 0", "page 1", "page 2"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `bytes`, a form, registered in `doc` as `name`.
fn register(doc: &mut Document, bytes: &[u8], name: &str) -> SourceId {
    let kind = BlobKind::file(Path::new(name), None);
    let src = doc.store().open(bytes.into(), kind).unwrap();
    doc.add_source(SourceInfo {
        src,
        name: name.into(),
        derived: None,
    })
}

/// The one page of `source` inserted at the end of `doc`: the page.
fn insert(doc: &mut Document, source: SourceId) -> PageId {
    let insert = Edit::InsertPages {
        source,
        range: 0..1,
        before: None,
        outline: None,
        annots: Vec::new(),
    };
    doc.apply(insert, "insert").unwrap();
    *doc.state().order().last().unwrap()
}

/// A form with a filled text field.
fn small_form() -> Vec<u8> {
    let mut f = Form::new(1);
    let field = f.text("t", 0, row(0));
    f.fill(field, "value");
    f.bytes()
}

/// A form registered twice, the second registration's page inserted and
/// saved, then the first's: an instance's objects are copy 0 of its file's
/// only while no instance before it is held, so the file's are the first's
/// now, and the Append is refused; the save is a Rewrite.
#[test]
fn a_form_whose_copies_shift_refuses_the_append() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let form = small_form();
    let [a, b] = [
        register(&mut doc, &form, "a.pdf"),
        register(&mut doc, &form, "b.pdf"),
    ];
    insert(&mut doc, b);
    save(&mut doc, &path, &rewrite_answers(), &NoProgress).unwrap();
    insert(&mut doc, a);
    let input = doc.recovery_input();
    let base: Arc<[u8]> = std::fs::read(&path).unwrap().into();
    let plan = forced_append(&input, Onto::Saved);
    let e = append::append(&input, &plan, Arc::new(base), Mode::Veto, &NoProgress)
        .err()
        .unwrap();
    assert_eq!(
        e.downcast_ref::<Refused>(),
        Some(&Refused::FormShift),
        "{e:#}"
    );
    let (strategy, _) = forced(&mut doc, &AskAnswers::default()).unwrap();
    let why = vec![Why::BaseObjectsMissing];
    assert_eq!(strategy, Strategy::Rewrite { why });
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(back.page_count(), 3);
    assert_eq!(back.state().terminals().count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same on a signed file, appended to so its signature holds: the
/// save asks before it rewrites, never offering an Append, and writes
/// nothing until it's answered.
#[test]
fn missing_objects_on_a_signed_file_ask_and_write_nothing() {
    let dir = scratch();
    let mut f = Form::new(1);
    let sig = f.signature("sig", 0, row(1));
    f.sign(sig);
    let (path, mut doc) = opened(&dir, "signed.pdf", &f.bytes());
    let form = small_form();
    let [a, b] = [
        register(&mut doc, &form, "a.pdf"),
        register(&mut doc, &form, "b.pdf"),
    ];
    insert(&mut doc, b);
    save_appended(&mut doc, &path).unwrap();
    assert!(doc.saved_record().unwrap().signatures_hold);
    insert(&mut doc, a);
    let before = std::fs::read(&path).unwrap();
    let stop = forced(&mut doc, &AskAnswers::default()).err();
    let asks = Ask::Signatures {
        impact: broken("sig"),
        append_anyway: None,
        reason: Some(Why::BaseObjectsMissing),
    };
    assert_eq!(stop, Some(SaveStop::Ask(asks)));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(doc.is_dirty() && doc.sealed(doc.current_version()) == 0);
    // An Append anyway isn't offered: answered, it's still asked.
    let anyway = AskAnswers {
        append_anyway: true,
        ..Default::default()
    };
    assert!(matches!(
        forced(&mut doc, &anyway),
        Err(SaveStop::Ask(Ask::Signatures { .. }))
    ));
    let (strategy, _) = forced(&mut doc, &rewrite_answers()).unwrap();
    let why = vec![Why::BaseObjectsMissing];
    assert_eq!(strategy, Strategy::Rewrite { why });
    let _ = std::fs::remove_dir_all(&dir);
}

/// A form inserted twice, the first's page deleted (its field kept, with
/// no widget, so a Rewrite leaves it out), saved, and the delete undone:
/// the Append writes the field, whose widget is back, with the form's
/// dictionary, which lists it.
#[test]
fn a_field_whose_widget_comes_back_is_written_with_the_form() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let form = small_form();
    let [a, b] = [
        register(&mut doc, &form, "a.pdf"),
        register(&mut doc, &form, "b.pdf"),
    ];
    let first = insert(&mut doc, a);
    insert(&mut doc, b);
    doc.apply(Edit::DeletePages { ids: vec![first] }, "delete")
        .unwrap();
    save(&mut doc, &path, &rewrite_answers(), &NoProgress).unwrap();
    doc.undo().unwrap();
    let (strategy, _) = forced(&mut doc, &AskAnswers::default()).unwrap();
    assert_eq!(strategy, Strategy::Append { onto: Onto::Saved });
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        crate::forms::check(&bytes, None)
            .unwrap()
            .findings
            .is_empty()
    );
    let back = crate::open::file(&path, None).unwrap().doc;
    assert_eq!(back.page_count(), 3);
    assert_eq!(back.state().terminals().count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A mark deleted, and an Append forced as though the planner had judged
/// it one (a misclassification): the removal check finds the mark's text
/// in what the section unlinks, so the save is a Rewrite, which leaves it
/// out, and the misclassification is counted.
#[test]
fn a_forced_misclassification_is_vetoed_and_counted() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let page = doc.page(0).unwrap();
    let add = Edit::AnnotAdd {
        page,
        annot: note("a secret note"),
    };
    doc.apply(add, "add").unwrap();
    save(&mut doc, &path, &rewrite_answers(), &NoProgress).unwrap();
    assert!(holds(&std::fs::read(&path).unwrap(), b"a secret note"));
    let id = doc.state().marks_on(page).next().unwrap().id;
    doc.apply(Edit::AnnotDelete { page, id }, "delete").unwrap();
    let before = removal::vetoes_here();
    let (forced, warnings) = warned(|| forced(&mut doc, &AskAnswers::default()));
    let (strategy, _) = forced.unwrap();
    let key = ObjKey::Annot(id);
    let why = vec![Why::RemovalCheck { key }];
    assert_eq!(strategy, Strategy::Rewrite { why });
    assert_eq!(removal::vetoes_here().derived, before.derived + 1);
    let logged = removal::RemovalMisclassified { key }.to_string();
    assert_eq!(warnings, [logged]);
    assert!(!holds(&std::fs::read(&path).unwrap(), b"a secret note"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same veto on a file certified with `/P 2`, appended to so its
/// signature holds: the save asks, offering an Append anyway that says the
/// comment and the text the check found stay readable, and writes
/// nothing; answered so, the section appends after the signed bytes,
/// which the signature still covers, and the reader is told the deleted
/// text stays readable before it.
#[test]
fn the_veto_on_a_certified_file_asks_and_never_rewrites() {
    let dir = scratch();
    let mut f = Form::new(1);
    let sig = f.signature("sig", 0, row(1));
    f.certify(sig, 2);
    let (path, mut doc) = opened(&dir, "certified.pdf", &f.bytes());
    let page = doc.page(0).unwrap();
    let add = Edit::AnnotAdd {
        page,
        annot: note("a secret note"),
    };
    doc.apply(add, "add").unwrap();
    save_appended(&mut doc, &path).unwrap();
    let id = doc.state().marks_on(page).next().unwrap().id;
    doc.apply(Edit::AnnotDelete { page, id }, "delete").unwrap();
    let before = std::fs::read(&path).unwrap();
    let stop = forced(&mut doc, &AskAnswers::default()).err();
    let asks = Ask::Signatures {
        impact: broken("sig"),
        append_anyway: Some(AppendAnyway {
            stays_readable: vec![
                Removed {
                    kind: RemovedKind::Comments,
                    count: 1,
                },
                Removed {
                    kind: RemovedKind::Found,
                    count: 1,
                },
            ],
        }),
        reason: Some(Why::RemovalCheck {
            key: ObjKey::Annot(id),
        }),
    };
    assert_eq!(stop, Some(SaveStop::Ask(asks)));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(doc.is_dirty() && doc.sealed(doc.current_version()) == 0);
    let anyway = AskAnswers {
        append_anyway: true,
        stays_readable: true,
        ..Default::default()
    };
    let (strategy, notes) = forced(&mut doc, &anyway).unwrap();
    assert_eq!(strategy, Strategy::Append { onto: Onto::Saved });
    assert!(
        notes.contains(&Note(append::STAYS_READABLE.into())),
        "{notes:?}"
    );
    let now = std::fs::read(&path).unwrap();
    assert!(now.starts_with(&before));
    let findings = crate::forms::check(&now, None).unwrap().findings;
    assert!(
        !(findings.iter()).any(|f| matches!(f, crate::forms::Finding::StaleSignature { .. })),
        "{findings:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// How many of the streams of the page of the file at `path` are marked
/// as an OCR layer's: its `q` and its text, for one layer.
fn marked_in(path: &Path) -> usize {
    let pdf = Pdf::load(path).unwrap();
    let page = pdf.get_dictionary(pdf.get_pages()[&1]).unwrap();
    let contents = pdf.dereference(page.get(b"Contents").unwrap()).unwrap().1;
    let entries = match contents {
        Object::Array(a) => a.clone(),
        _ => vec![page.get(b"Contents").unwrap().clone()],
    };
    (entries.iter())
        .filter(|e| {
            (pdf.dereference(e).ok())
                .is_some_and(|(_, o)| o.as_stream().is_ok_and(|s| s.dict.has(b"SheafOcrLayer")))
        })
        .count()
}

/// A re-OCR over a layer saved in an earlier session: drawing the new
/// layer takes the file's out of the page's content, and the removal check
/// finds its words in what that unlinks, so the save is a Rewrite, with
/// one layer. Over a layer saved in this session, the same, judged on
/// the words the page's block was drawn from.
#[test]
fn a_re_ocr_over_a_saved_layer_is_a_rewrite() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let page = doc.page(0).unwrap();
    let ocr = |text: &str| Edit::SetTextLayers {
        layers: vec![(page, layer(text))],
    };
    doc.apply(ocr("first"), "ocr").unwrap();
    save(&mut doc, &path, &rewrite_answers(), &NoProgress).unwrap();
    assert_eq!(marked_in(&path), 2);
    // This session: the page's block.
    doc.apply(ocr("second"), "ocr").unwrap();
    let (strategy, _) = forced(&mut doc, &AskAnswers::default()).unwrap();
    let key = vetoed(&strategy);
    assert!(matches!(key, Some(ObjKey::Block(_))), "{strategy:?}");
    assert_eq!(marked_in(&path), 2);

    // An earlier session: the file's own streams.
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let page = doc.page(0).unwrap();
    let edit = Edit::SetTextLayers {
        layers: vec![(page, layer("third"))],
    };
    doc.apply(edit, "ocr").unwrap();
    let before = removal::vetoes_here();
    let (strategy, _) = forced(&mut doc, &AskAnswers::default()).unwrap();
    let key = vetoed(&strategy);
    assert!(matches!(key, Some(ObjKey::Base { .. })), "{strategy:?}");
    assert_eq!(
        removal::vetoes_here(),
        Vetoes {
            source: before.source + 1,
            ..before
        }
    );
    assert_eq!(marked_in(&path), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A mark's appearance written over with the same inputs (a new color)
/// appends; with other text, what it said is in what the section writes
/// over, and the save is a Rewrite.
#[test]
fn an_overwritten_appearance_is_judged_on_its_inputs() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let page = doc.page(0).unwrap();
    let add = Edit::AnnotAdd {
        page,
        annot: note("a note"),
    };
    doc.apply(add, "add").unwrap();
    save(&mut doc, &path, &rewrite_answers(), &NoProgress).unwrap();
    change_mark(&mut doc, |a| a.color = [10, 200, 10]);
    let (strategy, _) = forced(&mut doc, &AskAnswers::default()).unwrap();
    assert_eq!(strategy, Strategy::Append { onto: Onto::Saved });
    let id = change_mark(&mut doc, |a| a.contents = "another note".into());
    let (strategy, _) = forced(&mut doc, &AskAnswers::default()).unwrap();
    let key = vetoed(&strategy);
    let ours = |key| matches!(key, ObjKey::Annot(a) | ObjKey::AnnotPart(a, _) if a == id);
    assert!(key.is_some_and(ours), "{strategy:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A one-page file whose page draws form `/Fm0`, which shows `shows` in
/// `font` (a dictionary, its `/ToUnicode` the CMap `cmap` when there's
/// one), opened from `dir`: the document, and the numbers of its page and
/// of the form.
fn form_file(
    dir: &Path,
    font: Dictionary,
    cmap: Option<&str>,
    shows: &str,
) -> (PathBuf, Document, ObjectId, ObjectId) {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = with_cmap(&mut pdf, font, cmap);
    let form = pdf.add_object(form_stream(font, shows));
    let content = pdf.add_object(Stream::new(dictionary! {}, b"q /Fm0 Do Q".to_vec()));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => root, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "XObject" => dictionary! { "Fm0" => form } },
    });
    pdf.objects.insert(
        root,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (path, doc) = opened(dir, "form.pdf", &bytes);
    (path, doc, page, form)
}

/// `font` added to `pdf`, with `cmap` as its `/ToUnicode` when there's one.
fn with_cmap(pdf: &mut Pdf, mut font: Dictionary, cmap: Option<&str>) -> ObjectId {
    if let Some(cmap) = cmap {
        font.set("ToUnicode", pdf.add_object(to_unicode(cmap)));
    }
    pdf.add_object(font)
}

/// `cmap`'s mappings in a ToUnicode CMap stream.
fn to_unicode(cmap: &str) -> Stream {
    let cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n{cmap}\n\
         endcmap CMapName currentdict /CMap defineresource pop end end"
    );
    Stream::new(dictionary! {}, cmap.into_bytes())
}

/// A form that shows `shows`, a string as content writes it, in the font
/// at `font`.
fn form_stream(font: ObjectId, shows: &str) -> Stream {
    let dict = dictionary! {
        "Type" => "XObject", "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 300.into(), 100.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F" => font } },
    };
    let content = format!("BT /F 12 Tf 10 10 Td {shows} Tj ET");
    Stream::new(dict, content.into_bytes())
}

/// What the removal check finds when a section writes `doc`'s page, at
/// `page` in its file at `path`, over without its form, and `forms`: each a
/// new form showing a string in a new font, which the page draws instead.
fn checked(
    path: &Path,
    doc: &Document,
    page: ObjectId,
    forms: &[(Dictionary, Option<&str>, &str)],
) -> Option<Found> {
    let bytes = std::fs::read(path).unwrap();
    let old = LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap();
    let mut objects = BTreeMap::new();
    let mut next = old.xref().size;
    let mut add = |objects: &mut BTreeMap<ObjectId, (ObjKey, Object)>, obj: Object| {
        let at = (next, 0);
        let key = ObjKey::Res(ResId(u128::from(next)));
        next += 1;
        objects.insert(at, (key, obj));
        at
    };
    let mut xobjects = Dictionary::new();
    for (i, (font, cmap, shows)) in forms.iter().enumerate() {
        let mut font = font.clone();
        if let Some(cmap) = cmap {
            font.set("ToUnicode", add(&mut objects, to_unicode(cmap).into()));
        }
        let font = add(&mut objects, font.into());
        let form = add(&mut objects, form_stream(font, shows).into());
        xobjects.set(format!("Fm{}", i + 1), form);
    }
    let mut dict = old.get_dictionary(page).unwrap().clone();
    dict.set("Resources", dictionary! { "XObject" => xobjects });
    let id = doc.page(0).unwrap();
    objects.insert(page, (ObjKey::Page(id), dict.into()));
    let input = doc.recovery_input();
    let saved = input.saved.as_ref().unwrap();
    let catalog = saved.record.map.keys[&ObjKey::Catalog];
    let primary = input.sources[&input.primary].src.id();
    removal::check(&Section {
        old: &old,
        objects: &objects,
        trailer: &[catalog],
        saved,
        target: &input.target,
        primary,
        allowed: &[],
        lim: &Limits::DEFAULT,
    })
}

fn helvetica() -> Dictionary {
    dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    }
}

/// A form a page no longer draws is unlinked: the check reads its text,
/// through its font, and finds it gone, unless a form the section writes
/// shows the same words.
#[test]
fn an_unlinked_form_s_text_is_read() {
    let dir = scratch();
    let (path, doc, page, form) = form_file(&dir, helvetica(), None, "(hidden words)");
    let primary = doc.recovery_input();
    let primary = primary.sources[&primary.primary].src.id();
    let gone = Found {
        key: ObjKey::Base {
            blob: primary,
            obj: form,
            copy: 0,
        },
        derived: false,
    };
    assert_eq!(checked(&path, &doc, page, &[]), Some(gone));
    let again = [(helvetica(), None, "(words hidden)")];
    assert_eq!(checked(&path, &doc, page, &again), None);
    let fewer = [(helvetica(), None, "(hidden)")];
    assert_eq!(checked(&path, &doc, page, &fewer), Some(gone));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Text in codes a font doesn't say the text of is its bytes: the same
/// text in a font that does differs, and only the same bytes match.
#[test]
fn an_unknown_encoding_counts_as_differing() {
    let dir = scratch();
    let symbolic = dictionary! {
        "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+Wingdings",
    };
    let (path, doc, page, _) = form_file(&dir, symbolic.clone(), None, "(secret)");
    let known = [(helvetica(), None, "(secret)")];
    assert!(checked(&path, &doc, page, &known).is_some());
    let same = [(symbolic, None, "(secret)")];
    assert_eq!(checked(&path, &doc, page, &same), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same text in another subset of a font, its codes and CMap
/// another's, passes; other text doesn't.
#[test]
fn a_re_subset_font_with_the_same_text_passes() {
    let dir = scratch();
    let subset = |name: &str| {
        dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => name,
            "Encoding" => "Identity-H",
        }
    };
    let first = "1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                 3 beginbfchar <0001> <0048> <0002> <0069> <0003> <0021> endbfchar";
    let (path, doc, page, _) =
        form_file(&dir, subset("ABCDEF+Arial"), Some(first), "<000100020003>");
    let other = "1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                 3 beginbfchar <0007> <0048> <0005> <0069> <0009> <0021> endbfchar";
    let resubset = [(subset("GHIJKL+Arial"), Some(other), "<000700050009>")];
    assert_eq!(checked(&path, &doc, page, &resubset), None);
    let changed = [(subset("GHIJKL+Arial"), Some(other), "<00070009>")];
    assert!(checked(&path, &doc, page, &changed).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
