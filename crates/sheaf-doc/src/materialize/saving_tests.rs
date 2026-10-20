//! Saves as outputs (M10.5 §9.1, slices 9 and 10): what a save does to the
//! document, which is nothing but which version is saved; edits made while
//! one is written; the file another program changed; Save As; a restored
//! document's first save; Appends in place, one after another, and torn;
//! what the reader's answers make of a plan; and an Unchanged plan.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use in_progress::{Level, NoProgress, Progress, Unit};
use lopdf::{Document as Pdf, Object};
use sheaf_edit::{Op, Rotation, Turn};

use super::{
    Ask, AskAnswers, Onto, SavePlan, SaveStop, Strategy, StrategyKind, Why, forced_append, plan,
    save, save_appended, save_rewritten, write_file,
};
use crate::forms::fixture::{Form, row};
use crate::model::{DocState, Document, GroupKind, PageId, VersionId};
use crate::serialize::durable::FileStamp;
use crate::text_layer::{OcrWord, TextLayer};
use crate::tx::fixture as fx;

/// An empty directory of the test's own.
fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("sheaf-saving-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A form of `pages` pages, a text field on each, as `dir/name`.
fn form_file(dir: &Path, name: &str, pages: usize) -> PathBuf {
    let mut f = Form::new(pages);
    for page in 0..pages {
        let field = f.text(&format!("t{page}"), page, row(0));
        f.fill(field, &format!("value {page}"));
    }
    let path = dir.join(name);
    std::fs::write(&path, f.bytes()).unwrap();
    path
}

/// The file at `path`, opened as the app opens it, with its record.
fn opened(path: &Path) -> Document {
    crate::open::file(path, None).unwrap().doc
}

fn turn(d: &mut Document, index: usize) {
    let page = d.page(index).unwrap();
    d.commit_ops(vec![fx::rotate(&[page], Turn::Quarter)], "Rotate")
        .unwrap();
}

/// OCR text saying `text`.
fn layer(text: &str) -> std::sync::Arc<TextLayer> {
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
    std::sync::Arc::new(crate::text_layer::from_engine(vec![word], &geom, "test"))
}

fn ocr(page: PageId, text: &str) -> Vec<Op> {
    fx::ocr([(page, layer(text))])
}

/// Each page of the file at `path`: its `/Rotate`, and whether its
/// content has an OCR layer.
fn pages_of(path: &Path) -> Vec<(i64, bool)> {
    let pdf = Pdf::load(path).unwrap();
    (pdf.get_pages().into_values())
        .map(|id| {
            let page = pdf.get_dictionary(id).unwrap();
            let rotate = page.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0);
            let contents = pdf.dereference(page.get(b"Contents").unwrap()).unwrap().1;
            let entries = match contents {
                Object::Array(a) => a.clone(),
                _ => vec![page.get(b"Contents").unwrap().clone()],
            };
            let layered = entries.iter().any(|e| {
                (pdf.dereference(e).ok())
                    .is_some_and(|(_, o)| o.as_stream().is_ok_and(|s| s.dict.has(b"SheafOcrLayer")))
            });
            (rotate, layered)
        })
        .collect()
}

/// The names of the files in `dir`.
fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A progress that holds a write at its first check: it says so on
/// `started`, runs `meanwhile`, then waits for `go`.
struct Gate {
    started: Mutex<Option<Sender<()>>>,
    go: Mutex<Option<Receiver<()>>>,
    meanwhile: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl Gate {
    fn new(started: Option<Sender<()>>, go: Option<Receiver<()>>) -> Gate {
        Gate {
            started: Mutex::new(started),
            go: Mutex::new(go),
            meanwhile: Mutex::new(None),
        }
    }
}

impl Progress for Gate {
    fn set_total(&self, _: Option<u64>, _: Unit) {}
    fn set(&self, _: u64) {}
    fn inc(&self, _: u64) {}
    fn message(&self, _: Level, _: &str) {}
    fn is_cancelled(&self) -> bool {
        if let Some(started) = self.started.lock().unwrap().take() {
            started.send(()).unwrap();
        }
        if let Some(meanwhile) = self.meanwhile.lock().unwrap().take() {
            meanwhile();
        }
        if let Some(go) = self.go.lock().unwrap().take() {
            go.recv().unwrap();
        }
        false
    }
    fn child(&self, _: &str, _: f64) -> Box<dyn Progress> {
        Box::new(NoProgress)
    }
}

/// A save in flight seals its version (M10.5 §9.1): an edit and an OCR
/// flush made while it's written (a group's next commit, which would amend
/// the group's version otherwise) each make a version after it. The save records
/// the version it wrote, the edits stay unsaved, and the next save writes
/// them.
#[test]
fn an_edit_and_an_ocr_flush_during_a_slow_save_land_after_it() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 2);
    let mut d = opened(&path);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    turn(&mut d, 0);
    let g = d.begin_group(GroupKind::Ocr);
    d.commit_ops_in_group(g, ocr(p0, "first"), "Recognize text")
        .unwrap();
    let v = d.current_version();
    let input = d.save_input(v).unwrap();
    let sealed = input.target.clone();
    let target = d.save_target(&path);
    let plan = plan(&input, &target);
    let (started, has_started) = channel();
    let (go, wait) = channel();
    let gate = Gate::new(Some(started), Some(wait));
    let mut flushed = None;
    let written = std::thread::scope(|s| {
        let writing = s.spawn(|| {
            let answers = AskAnswers::default();
            write_file(&input, &plan, target, &answers, &gate)
        });
        has_started.recv().unwrap();
        // While it's written: the group's next commit, and an edit.
        d.commit_ops_in_group(g, ocr(p1, "second"), "Recognize text")
            .unwrap();
        flushed = Some(d.current_version());
        turn(&mut d, 1);
        go.send(()).unwrap();
        writing.join().unwrap()
    })
    .unwrap();
    let flushed = flushed.unwrap();
    assert_ne!(flushed, v, "the flush didn't amend the version being saved");
    let after: (VersionId, DocState) = (d.current_version(), d.state().clone());
    d.record_save(written.record).unwrap();
    // Nothing changed but which version is saved: the one sealed, as it
    // was sealed.
    assert!(d.current_version() == after.0 && *d.state() == after.1);
    assert_eq!(d.saved_record().unwrap().version, v);
    assert!(d.is_dirty());
    assert_eq!(pages_of(&path), [(90, true), (0, false)]);
    // The flush and the edit are the versions after it.
    assert_eq!(
        d.undo().unwrap().map(|c| c.label).as_deref(),
        Some("Rotate")
    );
    assert_eq!(d.current_version(), flushed);
    assert!(d.is_dirty());
    d.undo().unwrap();
    assert_eq!(d.current_version(), v);
    assert!(*d.state() == sealed && !d.is_dirty());
    d.redo().unwrap();
    d.redo().unwrap();
    assert_eq!(d.current_version(), after.0);
    // The next save writes them.
    save(&mut d, &path, &AskAnswers::default(), &NoProgress).unwrap();
    assert!(!d.is_dirty());
    assert_eq!(pages_of(&path), [(90, true), (90, true)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Undo and redo across saves are exact: a save changes no version or
/// state, so each version is the state it was made as, and only the saved
/// one is clean.
#[test]
fn undo_across_saves_is_exact() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 2);
    let mut d = opened(&path);
    let mut made = vec![(d.current_version(), d.state().clone())];
    for i in 0..4 {
        turn(&mut d, i % 2);
        made.push((d.current_version(), d.state().clone()));
        if i == 1 || i == 2 {
            // A turn removes nothing: it appends.
            let report = save(&mut d, &path, &AskAnswers::default(), &NoProgress).unwrap();
            assert_eq!(report.strategy, Some(StrategyKind::Append));
            let (v, st) = made.last().unwrap();
            assert!(d.current_version() == *v && d.state() == st);
        }
    }
    let saved = made[3].0;
    let check = |d: &Document, k: usize| {
        let (v, st) = &made[k];
        assert_eq!(d.current_version(), *v, "{k}");
        assert!(d.state() == st, "{k}");
        assert_eq!(d.is_dirty(), *v != saved, "{k}");
    };
    check(&d, 4);
    for k in (0..4).rev() {
        d.undo().unwrap();
        check(&d, k);
    }
    for k in 1..=4 {
        d.redo().unwrap();
        check(&d, k);
    }
    // The file is the saved version's.
    assert_eq!(pages_of(&path), [(180, false), (90, false)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Another program changed the file since it was opened (bug 2): a save
/// in place writes nothing and asks, though the change kept the file's
/// length, and the document is as unsaved as it was. Save As still saves,
/// leaving theirs; Overwrite replaces theirs, and the next save asks
/// nothing.
#[test]
fn a_file_changed_since_open_asks() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 2);
    let original = std::fs::read(&path).unwrap();
    let [mut a, mut b] = [opened(&path), opened(&path)];
    turn(&mut a, 0);
    turn(&mut b, 0);
    let mut theirs = original.clone();
    *theirs.last_mut().unwrap() ^= 0x20;
    std::fs::write(&path, &theirs).unwrap();
    let asks = Err(SaveStop::Ask(Ask::FileChanged {
        path: path.as_path().into(),
    }));
    for d in [&mut a, &mut b] {
        let v = d.current_version();
        assert_eq!(save(d, &path, &AskAnswers::default(), &NoProgress), asks);
        assert!(d.is_dirty());
        // Unsealed: the next edit of a job amends it again.
        assert_eq!(d.sealed(v), 0);
    }
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert_eq!(names_in(&dir), ["a.pdf"], "nothing was started beside it");
    // Saved as another file: written there, and theirs left.
    let copy = dir.join("copy.pdf");
    save(&mut a, &copy, &AskAnswers::default(), &NoProgress).unwrap();
    assert!(!a.is_dirty());
    assert_eq!(pages_of(&copy), [(90, false), (0, false)]);
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    // Overwritten: the document, with the edit, replaces theirs.
    let overwrite = AskAnswers {
        overwrite: true,
        ..Default::default()
    };
    save(&mut b, &path, &overwrite, &NoProgress).unwrap();
    assert_eq!(pages_of(&path), [(90, false), (0, false)]);
    turn(&mut b, 1);
    save(&mut b, &path, &AskAnswers::default(), &NoProgress).unwrap();
    assert_eq!(pages_of(&path), [(90, false), (90, false)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The check just before the rename: a file another program changed
/// while the save was writing stops it with the same ask, and is left as
/// they left it, with nothing beside it.
#[test]
fn a_file_changed_while_saving_asks() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 1);
    let original = std::fs::read(&path).unwrap();
    let mut d = opened(&path);
    turn(&mut d, 0);
    // The same length, a new tail, while the save writes.
    let mut theirs = original.clone();
    *theirs.last_mut().unwrap() ^= 0x20;
    let gate = Gate::new(None, None);
    *gate.meanwhile.lock().unwrap() = Some(Box::new({
        let (path, theirs) = (path.clone(), theirs.clone());
        move || std::fs::write(&path, &theirs).unwrap()
    }));
    let asks = Err(SaveStop::Ask(Ask::FileChanged {
        path: path.as_path().into(),
    }));
    let v = d.current_version();
    assert_eq!(save(&mut d, &path, &AskAnswers::default(), &gate), asks);
    assert_eq!(std::fs::read(&path).unwrap(), theirs);
    assert_eq!(names_in(&dir), ["a.pdf"], "no temp file left");
    assert!(d.is_dirty());
    assert_eq!(d.sealed(v), 0, "unsealed");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Save As is a save: its file is the document's from then on, which its
/// record says, so a save to it checks it, and Save asks nothing more. An
/// export can't replace it, and the recovery sidecar is written beside
/// it; the file the document was is another file now.
#[test]
fn save_as_makes_its_file_the_document_s() {
    let dir = scratch();
    let a = form_file(&dir, "a.pdf", 2);
    let b = dir.join("b.pdf");
    let mut d = opened(&a);
    turn(&mut d, 0);
    assert_eq!(&**d.file().unwrap().0, a.as_path());
    let report = save(&mut d, &b, &AskAnswers::default(), &NoProgress).unwrap();
    assert_eq!(&*report.path, b.as_path());
    let (file, stamp) = d.file().unwrap();
    assert_eq!(
        (&**file, *stamp),
        (b.as_path(), FileStamp::read(&b).unwrap())
    );
    let to_b = d.save_target(&b);
    assert!(to_b.same_file && to_b.stamp == Some(*stamp));
    assert!(!d.save_target(&a).same_file);
    let to_b = plan(&d.recovery_input(), &to_b);
    assert_eq!(to_b.asks, []);
    // An export can't replace it, and can replace what it was.
    let snap = d.snapshot();
    let err = crate::save::export_clean_copy(&snap, &b).unwrap_err();
    assert!(
        err.to_string().contains("can't replace the open file"),
        "{err}"
    );
    crate::save::export_flattened(&snap, &b).unwrap_err();
    crate::save::export_clean_copy(&snap, &a).unwrap();
    // The sidecar of an edit after it, beside it.
    turn(&mut d, 1);
    let (file, base) = d.file().map(|(f, s)| (f.clone(), *s)).unwrap();
    crate::recovery::write(&d.recovery_input(), &file, &base, &NoProgress).unwrap();
    assert!(crate::recovery::find(&b).is_some());
    assert!(crate::recovery::find(&a).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A document restored from its sidecar has its file's record, with the
/// sidecar's sections pending: it isn't what the file holds, and its first
/// save to it appends them, then its own, while the file is still the
/// sidecar's base: it asks when another program changed it, and
/// otherwise writes, its record the file's from then on.
#[test]
fn a_restored_document_s_first_save_checks_the_base() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 2);
    let original = std::fs::read(&path).unwrap();
    let mut d = opened(&path);
    turn(&mut d, 1);
    let (file, base) = d.file().map(|(f, s)| (f.clone(), *s)).unwrap();
    crate::recovery::write(&d.recovery_input(), &file, &base, &NoProgress).unwrap();
    let rec = crate::recovery::find(&path).unwrap();
    assert_eq!(rec.base, base);
    let restored = || crate::recovery::tests::restored(&path, &rec, None);
    let [mut first, mut second] = [restored(), restored()];
    let record = first.saved_record().unwrap();
    assert!(record.pending_tail.is_some() && first.is_dirty());
    assert_eq!(
        first.file().map(|(f, s)| (&**f, *s)),
        Some((path.as_path(), base))
    );
    let target = first.save_target(&path);
    assert!(target.same_file && target.stamp == Some(base));
    let planned = plan(&first.recovery_input(), &target);
    assert_eq!(planned.strategy, Strategy::Append { onto: Onto::Saved });
    assert_eq!(planned.asks, []);
    // The first writes, its record the file it wrote.
    let report = save(&mut first, &path, &AskAnswers::default(), &NoProgress).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    assert!(!first.is_dirty());
    let record = first.saved_record().unwrap();
    assert!(record.pending_tail.is_none());
    assert_eq!(record.stamp, FileStamp::read(&path).unwrap());
    let written = std::fs::read(&path).unwrap();
    assert!(written.starts_with(&original));
    assert_eq!(&written[original.len()..], &*rec.bytes);
    assert_eq!(pages_of(&path), [(0, false), (90, false)]);
    // The second's base is gone: it asks, and writes nothing.
    let asks = Err(SaveStop::Ask(Ask::FileChanged {
        path: path.as_path().into(),
    }));
    assert_eq!(
        save(&mut second, &path, &AskAnswers::default(), &NoProgress),
        asks
    );
    assert_eq!(std::fs::read(&path).unwrap(), written);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plan of `strategy` for a save of `input`, asking nothing and changing
/// nothing of the form.
fn planned(strategy: Strategy) -> SavePlan {
    SavePlan {
        strategy,
        asks: Vec::new(),
        transforms: Vec::new(),
        notes: Vec::new(),
    }
}

/// Three Appends in a row, each after the last: the file keeps every byte
/// it had, each section reads as the file's last, and the file reopens
/// with every page as saved, through `LazyPdf` and lopdf. The records say
/// what each file is, and its checksum, which an opened file's isn't
/// known, is carried on once a Rewrite made it: the checksum of the whole
/// file each time. Signatures hold until the Rewrite.
#[test]
fn appends_one_after_another_reopen() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 3);
    let original = std::fs::read(&path).unwrap();
    let mut d = opened(&path);
    let opened_record = d.saved_record().unwrap().clone();
    assert_eq!(opened_record.sound, d.primary().base_sound());
    assert!(opened_record.signatures_hold && opened_record.content_hash.is_none());
    for i in 0..3 {
        turn(&mut d, i);
        let was = std::fs::read(&path).unwrap();
        let report = save_appended(&mut d, &path).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let now = std::fs::read(&path).unwrap();
        assert!(now.len() > was.len() && now.starts_with(&was), "{i}");
        let record = d.saved_record().unwrap();
        assert_eq!(record.stamp, FileStamp::read(&path).unwrap(), "{i}");
        assert!(
            record.signatures_hold && record.content_hash.is_none(),
            "{i}"
        );
        assert!(!d.is_dirty());
    }
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(&original));
    let lazy = lopdf::LazyPdf::open(
        std::sync::Arc::<[u8]>::from(bytes.as_slice()),
        None,
        &crate::source::Limits::DEFAULT.reader,
    )
    .unwrap();
    assert!(lazy.base_sound());
    assert_eq!(lazy.xref().sections, 4);
    assert_eq!(pages_of(&path), [(90, false); 3]);
    let again = opened(&path);
    assert_eq!(again.state().order().len(), 3);
    for i in 0..3 {
        assert_eq!(again.geom(again.page(i).unwrap()).rotate, Rotation::R90);
    }

    // After a Rewrite, its checksum, carried over each section.
    turn(&mut d, 0);
    save_rewritten(&mut d, &path).unwrap();
    let record = d.saved_record().unwrap().clone();
    assert_eq!(record.strategy, StrategyKind::Rewrite);
    assert!(record.sound && !record.signatures_hold);
    let sum = |d: &Document| {
        d.saved_record()
            .unwrap()
            .content_hash
            .as_ref()
            .unwrap()
            .finish()
    };
    assert_eq!(
        sum(&d),
        sheaf_util::checksum(&std::fs::read(&path).unwrap())
    );
    for i in 1..3 {
        turn(&mut d, i);
        save_appended(&mut d, &path).unwrap();
        assert_eq!(
            sum(&d),
            sheaf_util::checksum(&std::fs::read(&path).unwrap()),
            "{i}"
        );
        assert!(!d.saved_record().unwrap().signatures_hold);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Another program changed the file before an Append: a byte appended, or
/// one of its last cross-reference section changed, 64 KiB and more from
/// its end, its length and time kept. Nothing is written, the version
/// stays sealed for the reader's answer, and Overwrite rewrites it.
#[test]
fn a_file_changed_before_an_append_asks_and_overwrite_rewrites() {
    let dir = scratch();
    let path = dir.join("a.pdf");
    let blank = crate::serialize::fixture::blank_table(3500);
    let (at, _) = crate::serialize::durable::startxref(&blank[blank.len() - 64..]).unwrap();
    assert!(
        blank.len() as u64 - at > 64 * 1024,
        "the section is past the tail"
    );
    for how in ["appended", "its last section"] {
        std::fs::write(&path, &blank).unwrap();
        let mut d = opened(&path);
        turn(&mut d, 0);
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut theirs = blank.clone();
        match how {
            "appended" => theirs.push(b'\n'),
            _ => theirs[at as usize + 7] = b'1',
        }
        std::fs::write(&path, &theirs).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(mtime).unwrap();
        drop(file);
        let v = d.current_version();
        let input = d.save_input(v).unwrap();
        let target = d.save_target(&path);
        let plan = planned(Strategy::Append { onto: Onto::Saved });
        let stop = write_file(
            &input,
            &plan,
            target.clone(),
            &AskAnswers::default(),
            &NoProgress,
        );
        let asks = SaveStop::Ask(Ask::FileChanged {
            path: path.as_path().into(),
        });
        assert_eq!(stop.err(), Some(asks), "{how}");
        assert_eq!(std::fs::read(&path).unwrap(), theirs, "{how}");
        assert_eq!(names_in(&dir), ["a.pdf"], "{how}: no journal left");
        assert_eq!(d.sealed(v), 1, "{how}");
        let overwrite = AskAnswers {
            overwrite: true,
            ..Default::default()
        };
        let written = write_file(&input, &plan, target, &overwrite, &NoProgress).unwrap();
        assert_eq!(written.report().strategy, Some(StrategyKind::Rewrite));
        written.record_in(&mut d).unwrap();
        assert!(!d.is_dirty() && d.sealed(v) == 0);
        let saved = std::fs::read(&path).unwrap();
        assert!(!saved.starts_with(&theirs), "{how}");
        assert_eq!(pages_of(&path)[0], (90, false), "{how}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the reader's answers make of a plan: an Append or an Unchanged
/// plan answered Overwrite is a Rewrite, because the file changed, which
/// asks before it breaks signatures that hold, and not once a Rewrite broke
/// them. Any other plan, or no Overwrite, is as it was.
#[test]
fn overwrite_makes_an_append_a_rewrite() {
    let dir = scratch();
    let mut f = Form::new(1);
    let field = f.text("t", 0, row(0));
    f.fill(field, "value");
    let sig = f.signature("sig", 0, row(1));
    f.sign(sig);
    let path = dir.join("signed.pdf");
    std::fs::write(&path, f.bytes()).unwrap();
    let mut d = opened(&path);
    turn(&mut d, 0);
    let overwrite = AskAnswers {
        overwrite: true,
        ..Default::default()
    };
    let rewrite = Strategy::Rewrite {
        why: vec![Why::FileChanged],
    };
    let target = d.save_target(&path);
    for strategy in [Strategy::Append { onto: Onto::Saved }, Strategy::Unchanged] {
        let input = d.recovery_input();
        let plan = planned(strategy.clone());
        assert_eq!(plan.answered(&input, &target, &AskAnswers::default()), plan);
        let answered = plan.answered(&input, &target, &overwrite);
        assert_eq!(answered.strategy, rewrite, "{strategy:?}");
        assert!(
            matches!(
                &answered.asks[..],
                [Ask::Signatures {
                    impact,
                    append_anyway: None,
                    reason: Some(Why::FileChanged)
                }] if impact.len() == 1
            ),
            "{:?}",
            answered.asks
        );
    }
    let plan = planned(rewrite.clone());
    assert_eq!(
        plan.answered(&d.recovery_input(), &target, &overwrite),
        plan
    );
    // Once a Rewrite wrote the file, it has no signature that holds.
    let answers = AskAnswers {
        break_signatures: true,
        ..Default::default()
    };
    save(&mut d, &path, &answers, &NoProgress).unwrap();
    turn(&mut d, 0);
    let plan = planned(Strategy::Append { onto: Onto::Saved });
    let answered = plan.answered(&d.recovery_input(), &target, &overwrite);
    assert_eq!((answered.strategy, answered.asks), (rewrite, Vec::new()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An Unchanged plan, the document what its file holds by another route
/// (a turn and its turn back), writes nothing: the file's bytes and time
/// are as they were, the version is the saved one, clean, and no save is
/// noted.
#[test]
fn an_unchanged_plan_writes_nothing() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 2);
    let before = std::fs::read(&path).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let mut d = opened(&path);
    let saves = |d: &Document| -> usize { d.timeline().iter().map(|v| v.saves.len()).sum() };
    let noted = saves(&d);
    let first = d.page(0).unwrap();
    for by in [Turn::Quarter, Turn::Back] {
        d.commit_ops(vec![fx::rotate(&[first], by)], "Rotate")
            .unwrap();
    }
    assert!(d.is_dirty());
    let v = d.current_version();
    let input = d.save_input(v).unwrap();
    let target = d.save_target(&path);
    let plan = planned(Strategy::Unchanged);
    let written = write_file(&input, &plan, target, &AskAnswers::default(), &NoProgress).unwrap();
    assert!(!written.wrote);
    assert_eq!(written.report().strategy, None);
    assert_eq!(written.record.version, v);
    written.record_in(&mut d).unwrap();
    assert!(!d.is_dirty() && d.sealed(v) == 0);
    assert_eq!(d.saved_record().unwrap().version, v);
    assert_eq!(saves(&d), noted);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An Append a crash cuts short (a test hook tears it) leaves the file
/// with part of its section and the journal: the next open cuts it back
/// to the file saved before, which is what the document opens, and says
/// so. The save that tore asks nothing of the file it left.
#[test]
fn a_torn_append_is_undone_when_the_file_opens() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 2);
    let before = std::fs::read(&path).unwrap();
    let mut d = opened(&path);
    turn(&mut d, 1);
    crate::serialize::append::tear_next(30);
    assert!(save_appended(&mut d, &path).is_err());
    assert!(d.is_dirty());
    let torn = std::fs::read(&path).unwrap();
    assert_eq!(torn.len(), before.len() + 30);
    assert!(crate::serialize::append::journal_path(&path).exists());
    let reopened = crate::open::file(&path, None).unwrap();
    assert!(
        matches!(reopened.torn, Some(crate::serialize::append::Torn::Undone { len }) if len == before.len() as u64)
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(names_in(&dir), ["a.pdf"]);
    assert_eq!(pages_of(&path), [(0, false), (0, false)]);
    // The file is the one the document saved last: an Append goes after it.
    save_appended(&mut d, &path).unwrap();
    assert_eq!(pages_of(&path), [(0, false), (90, false)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A forced Append's plan has save policy's transforms, as "Append anyway"
/// writes them, and asks nothing.
#[test]
fn a_forced_append_writes_the_transforms_and_asks_nothing() {
    let dir = scratch();
    let path = form_file(&dir, "a.pdf", 1);
    let mut d = opened(&path);
    turn(&mut d, 0);
    let input = d.recovery_input();
    let rewrite = plan(&input, &d.save_target(&path));
    let forced = forced_append(&input, Onto::CopyOf);
    assert_eq!(forced.strategy, Strategy::Append { onto: Onto::CopyOf });
    assert!(forced.asks.is_empty());
    assert_eq!(forced.transforms, rewrite.transforms);
    let _ = std::fs::remove_dir_all(&dir);
}
