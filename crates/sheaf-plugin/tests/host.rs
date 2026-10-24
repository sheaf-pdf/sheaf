//! Host integration tests against a real plugin component. The fixture in
//! `tests/fixtures/test-plugin` is built for `wasm32-wasip2` on first use
//! (rustup installs the target from `rust-toolchain.toml`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context as _, Result};
use arc_swap::ArcSwap;
use futures::executor::block_on;
use sheaf_doc::prim::Key;
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::Identity;
use sheaf_doc::tx::fixture;
use sheaf_doc::{
    AnnotKind, Box2, ChangeSet, Commit, CommitOpts, DestView, Document, EditError, FixedCount,
    FormRef, PageChars, PageId, Published, Snapshot, Store, TextOpts, TextService, Tx, ViewInputs,
};
use sheaf_edit::{Cap, Failure, FailureKind, FlattenSet, Rotation, Turn};
use sheaf_plugin::ui::{
    BuiltinPanel, FieldAnswer, FieldKind, FieldValue, Form, PanelRow, ShowPanel, Ui,
};
use sheaf_plugin::{
    Call, CallContext, CallOutput, ChangeInfo, Committed, Committer, DocContext, DocEvent, Door,
    Grants, Landed, Landing, Limits, LoadOptions, Manifest, Plugin, PluginError, Runtime, Status,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Build a plugin crate for wasm32-wasip2 and return its `.wasm`.
fn build_plugin(crate_dir: &Path, wasm_name: &str) -> PathBuf {
    let target = workspace_root().join("target/plugin-fixtures");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "--manifest-path",
        ])
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("RUSTFLAGS")
        .status()
        .expect("running cargo");
    assert!(status.success(), "building {} failed", crate_dir.display());
    target.join("wasm32-wasip2/release").join(wasm_name)
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-plugin")
}

/// Build the fixture once per test run and return its `.wasm`.
fn fixture_wasm() -> &'static Path {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| build_plugin(&fixture_dir(), "test_plugin.wasm"))
}

/// An installed plugin directory (manifest, wasm, icons) for one test.
fn install(name: &str, src: &Path, wasm: &Path) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("plugins-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(src.join("sheaf-plugin.toml"), dir.join("sheaf-plugin.toml")).unwrap();
    std::fs::copy(wasm, dir.join("plugin.wasm")).unwrap();
    if let Ok(icons) = std::fs::read_dir(src.join("icons")) {
        std::fs::create_dir_all(dir.join("icons")).unwrap();
        for icon in icons {
            let icon = icon.unwrap();
            std::fs::copy(icon.path(), dir.join("icons").join(icon.file_name())).unwrap();
        }
    }
    dir
}

fn plugin_dir(name: &str) -> PathBuf {
    install(name, &fixture_dir(), fixture_wasm())
}

fn load(name: &str, limits: Limits) -> Plugin {
    let dir = plugin_dir(name);
    let manifest = Manifest::load(&dir).unwrap();
    Runtime::new(limits)
        .unwrap()
        .load(dir, manifest, LoadOptions::default())
}

/// The bundled plugin in `plugins/<name>`, built and installed as `test`.
fn bundled(name: &str, test: &str, opts: LoadOptions) -> Plugin {
    let src = workspace_root().join("plugins").join(name);
    let wasm = build_plugin(&src, &format!("{}.wasm", name.replace('-', "_")));
    let dir = install(test, &src, &wasm);
    let manifest = Manifest::load(&dir).unwrap();
    Runtime::new(Limits::default())
        .unwrap()
        .load(dir, manifest, opts)
}

/// Pages' text as a fake engine draws it: a page's from `text`, by its
/// index in its source; its line n at y = 10 + 100n of the page as the
/// file shows it, character k at x = k, a point wide and 10 tall, upright
/// there. Handed over in PageSpace, as the text service hands it, and what
/// each read was under kept.
struct FakeText {
    text: Box<dyn Fn(u32) -> String + Send + Sync>,
    asked: Mutex<Vec<TextOpts>>,
}

impl FakeText {
    /// "Form 1040" and a second line on the first page, "Form 1041" on
    /// the next, and so on.
    fn forms() -> FakeText {
        FakeText::new(|index| format!("Form {}\nsecond line", 1040 + index))
    }

    /// `pages`, in order; past them, none.
    fn pages(pages: Vec<&'static str>) -> FakeText {
        FakeText::new(move |index| pages.get(index as usize).copied().unwrap_or("").into())
    }

    fn new(text: impl Fn(u32) -> String + Send + Sync + 'static) -> FakeText {
        FakeText {
            text: Box::new(text),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// What each read so far was under.
    fn asked(&self) -> Vec<TextOpts> {
        self.asked.lock().unwrap().clone()
    }
}

impl TextService for FakeText {
    fn chars(&self, snap: &Snapshot, page: PageId, o: &TextOpts) -> Result<Arc<PageChars>> {
        self.asked.lock().unwrap().push(o.clone());
        let base = &snap.state.def(page).context("a page of the document")?.base;
        let frame = base.src.geom(base.index);
        let (from_view, angle) = (frame.from_view(), frame.rotate.degrees() as f32);
        let text = (self.text)(base.index);
        let mut chars = Vec::new();
        for (n, line) in text.split('\n').enumerate() {
            for (k, ch) in line.chars().enumerate() {
                let shown = Box2 {
                    x: k as f32,
                    y: 10.0 + 100.0 * n as f32,
                    w: 1.0,
                    h: 10.0,
                };
                chars.push((ch, Some(from_view.apply_box(shown)), angle));
            }
        }
        Ok(Arc::new(PageChars::from_engine(chars)))
    }
}

fn doc(pages: u32) -> Document {
    let store = Store::new();
    let primary = sheaf_doc::store::blank(&store, pages);
    let policy = Box::new(FixedCount(50));
    Document::new(store, primary, "return.pdf".into(), None, &[], policy)
}

/// The top-level bookmarks' titles, in order.
fn top_level(d: &Document) -> Vec<String> {
    let st = d.state();
    (st.outline_roots().iter())
        .map(|&id| st.bookmark(id).unwrap().title.to_string())
        .collect()
}

/// A blue square of the user's at (10, 10), 20 points across.
fn square() -> sheaf_doc::Annot {
    sheaf_doc::Annot {
        id: sheaf_doc::AnnotId(0),
        kind: AnnotKind::Square {
            rect: Box2 {
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 20.0,
            },
        },
        color: [0, 0, 255],
        opacity: 1.0,
        stroke: 1.0,
        author: "Greg".into(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// `bytes`, opened as file `name`.
fn opened(bytes: &[u8], name: &str) -> Document {
    let store = Store::new();
    let kind = BlobKind::file(Path::new(name), None);
    let primary = store.open(bytes.into(), kind).unwrap();
    let policy = Box::new(FixedCount(50));
    Document::new(store, primary, name.into(), None, &[], policy)
}

/// Two pages: the first `/Rotate 90` with its CropBox off the origin,
/// the second plain Letter.
fn turned_doc() -> Document {
    let mut f = sheaf_doc::forms::fixture::Form::new(2);
    let first = f.pages[0];
    f.dict(first).set("Rotate", 90);
    let crop: Vec<lopdf::Object> = vec![20.into(), 30.into(), 600.into(), 770.into()];
    f.dict(first).set("CropBox", crop);
    opened(&f.bytes(), "turned.pdf")
}

/// One page, with another tool's comment on it: a square by Pat Reviewer
/// saying "From review".
fn commented() -> Document {
    use lopdf::{Object, dictionary};
    let mut f = sheaf_doc::forms::fixture::Form::new(1);
    let rect: Vec<Object> = vec![40.into(), 40.into(), 80.into(), 80.into()];
    let square = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "Rect" => rect,
        "NM" => Object::string_literal("review-1"),
        "T" => Object::string_literal("Pat Reviewer"),
        "Contents" => Object::string_literal("From review"),
    });
    f.set_annots(0, &[square]);
    opened(&f.bytes(), "commented.pdf")
}

/// A document as the app holds one for plugin calls: published to the
/// cell a call reads at every change, and the landing of the plugin call
/// that's running (plugin `plugin`'s, unless [`Shared::calling`] says
/// another's), whose commits (`edits.commit`) land in its group and whose
/// result lands as the app lands it ([`Shared::land`]).
#[derive(Clone)]
struct Shared(Arc<Inner>);

struct Inner {
    calls: Mutex<Calls>,
    cell: Arc<ArcSwap<Published>>,
    plugin: String,
}

/// The document, the running call's landing, and the changes plugin calls
/// made to it, oldest first.
struct Calls {
    doc: Document,
    landing: Landing,
    made: Vec<ChangeSet>,
}

/// The document as plugin calls land on it, keeping each change they make.
struct Told<'a> {
    doc: &'a mut Document,
    made: &'a mut Vec<ChangeSet>,
}

impl Door for Told<'_> {
    fn model(&mut self) -> &mut Document {
        self.doc
    }

    fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError> {
        let commit = self.doc.commit(tx, opts)?;
        self.made.push(commit.change.clone());
        Ok(commit)
    }

    fn follow(&mut self, change: &ChangeSet) {
        self.made.push(change.clone());
    }
}

impl Shared {
    fn new(d: Document, plugin: &str) -> Shared {
        let cell = Arc::new(ArcSwap::from_pointee(d.published()));
        let calls = Calls {
            doc: d,
            landing: Landing::new(plugin),
            made: Vec::new(),
        };
        Shared(Arc::new(Inner {
            calls: Mutex::new(calls),
            cell,
            plugin: plugin.into(),
        }))
    }

    /// The fixture's document.
    fn of(d: Document) -> Shared {
        Shared::new(d, "test-plugin")
    }

    /// `f` on the document, as the user reads or changes it, published
    /// after.
    fn with<T>(&self, f: impl FnOnce(&mut Document) -> T) -> T {
        let mut calls = self.0.calls.lock().unwrap();
        let out = f(&mut calls.doc);
        self.0.cell.store(Arc::new(calls.doc.published()));
        out
    }

    /// The next call is plugin `plugin`'s.
    fn calling(&self, plugin: &str) {
        self.0.calls.lock().unwrap().landing = Landing::new(plugin);
    }

    /// A call's `result` landed as the app lands it, the call over: what
    /// it left committed, or its edits taken back.
    fn land(&self, result: Result<CallOutput, PluginError>) -> Landed {
        let mut calls = self.0.calls.lock().unwrap();
        let Calls { doc, landing, made } = &mut *calls;
        let landing = std::mem::replace(landing, Landing::new(self.0.plugin.as_str()));
        let landed = landing.finish(&mut Told { doc, made }, result, false);
        self.0.cell.store(Arc::new(doc.published()));
        landed
    }

    /// The changes plugin calls made since this was last asked, oldest
    /// first.
    fn made(&self) -> Vec<ChangeSet> {
        std::mem::take(&mut self.0.calls.lock().unwrap().made)
    }

    /// What a call reads, `text` its pages' text.
    fn context(&self, path: &str, text: Arc<dyn TextService>) -> DocContext {
        DocContext {
            path: Some(path.into()),
            cell: self.0.cell.clone(),
            selection: Vec::new(),
            current_page: None,
            text,
            ocr: None,
            render: None,
            committer: Some(Arc::new(self.clone())),
        }
    }
}

impl Committer for Shared {
    fn commit(&self, tx: Tx, label: &str, depth: u8) -> Result<Committed, EditError> {
        let mut calls = self.0.calls.lock().unwrap();
        let Calls { doc, landing, made } = &mut *calls;
        landing.commit(&mut Told { doc, made }, tx, label, depth)?;
        let base = Arc::new(doc.published());
        self.0.cell.store(base.clone());
        Ok(Committed {
            version: doc.current_version(),
            instance: doc.instance(),
            base,
        })
    }
}

/// A call on `d`, its second page selected and its first current, its
/// pages' text [`FakeText::forms`].
fn ctx(d: &Shared) -> CallContext {
    let (first, second) = d.with(|d| (d.page(0).unwrap(), d.page(1)));
    let doc = DocContext {
        selection: second.into_iter().collect(),
        current_page: Some(first),
        ..d.context("return.pdf", Arc::new(FakeText::forms()))
    };
    CallContext {
        doc: Some(doc),
        ..CallContext::default()
    }
}

fn run(
    p: &Plugin,
    cmd: &str,
    args: Option<&str>,
    c: CallContext,
) -> Result<CallOutput, PluginError> {
    block_on(p.run_command(cmd, args.map(String::from), c)).expect("reply")
}

/// What `out`'s transaction decided by.
fn decided(out: &CallOutput) -> BTreeSet<Key> {
    out.tx.as_ref().map(Tx::decided).unwrap_or_default()
}

/// A call lands as one undo step, called as its first submit: its ops
/// lowered in order, each bookmark appended after the one before.
#[test]
fn command_returns_edits_that_apply_as_one_step() {
    let p = load("edits", Limits::default());
    let d = Shared::of(doc(3));
    let out = run(&p, "bookmark", None, ctx(&d)).unwrap();
    assert_eq!(p.status(), Status::Ready);
    assert_eq!(out.label.as_deref(), Some("Bookmark pages"));
    assert_eq!(out.depth, 0);
    let landed = d.land(Ok(out));
    assert!(landed.failed.is_none(), "{landed:?}");
    d.with(|d| {
        assert_eq!(top_level(d), ["Form 1040", "Form 1041", "Form 1042"]);
        assert_eq!(d.undo_label(), Some("Bookmark pages"));
        d.undo().unwrap();
        assert!(top_level(d).is_empty(), "one undo reverts them all");
    });
}

/// A call's submits go into its one transaction: one version, called as
/// the first.
#[test]
fn a_call_s_submits_are_one_undo_step() {
    let p = load("submits", Limits::default());
    let d = Shared::of(doc(1));
    let versions = d.with(|d| d.timeline().len());
    let out = run(&p, "two-submits", None, ctx(&d)).unwrap();
    assert_eq!(out.label.as_deref(), Some("one"));
    assert!(d.land(Ok(out)).failed.is_none());
    d.with(|d| {
        assert_eq!(top_level(d), ["one", "two"]);
        assert_eq!(
            (d.timeline().len(), d.undo_label()),
            (versions + 1, Some("one"))
        );
    });
}

#[test]
fn plugin_sees_the_call_context() {
    let p = load("context", Limits::default());
    let d = Shared::of(doc(2));
    let err = run(&p, "context", Some(r#"{"x":1}"#), ctx(&d)).unwrap_err();
    let PluginError::Failed(msg) = err else {
        panic!("{err:?}")
    };
    let (p0, p1, v) = d.with(|d| {
        let at = d.current_version().0;
        (d.page(0).unwrap().0, d.page(1).unwrap().0, at)
    });
    assert_eq!(
        msg,
        format!(
            r#"path=Some("return.pdf") version=Some({v}) selection=[{p1}] current=Some({p0}) args=Some("{{\"x\":1}}")"#
        )
    );
    // A document with no file yet: no path, and its pages as they are.
    let mut untitled = ctx(&d);
    untitled.doc.as_mut().unwrap().path = None;
    let err = run(&p, "context", None, untitled).unwrap_err();
    assert_eq!(
        err,
        PluginError::Failed(format!(
            "path=None version=Some({v}) selection=[{p1}] current=Some({p0}) args=None"
        ))
    );
    // No document: empty answers, not errors.
    let err = run(&p, "context", None, CallContext::default()).unwrap_err();
    assert_eq!(
        err,
        PluginError::Failed("path=None version=None selection=[] current=None args=None".into())
    );
}

#[test]
fn a_returned_error_keeps_the_plugin_loaded() {
    let p = load("fail", Limits::default());
    assert_eq!(
        run(&p, "fail", None, CallContext::default()).unwrap_err(),
        PluginError::Failed("nope".into())
    );
    assert_eq!(p.status(), Status::Ready);
    assert!(run(&p, "early", None, CallContext::default()).is_ok());
}

#[test]
fn a_trap_unloads_the_plugin() {
    let p = load("trap", Limits::default());
    let err = run(&p, "trap", None, CallContext::default()).unwrap_err();
    assert!(matches!(err, PluginError::Trapped(_)), "{err:?}");
    assert!(matches!(p.status(), Status::Failed(_)));
    let again = run(&p, "early", None, CallContext::default()).unwrap_err();
    assert!(matches!(again, PluginError::Unavailable(_)), "{again:?}");
}

#[test]
fn an_infinite_loop_times_out() {
    let p = load(
        "spin",
        Limits {
            call_timeout: Duration::from_millis(300),
            ..Limits::default()
        },
    );
    let t = std::time::Instant::now();
    let err = run(&p, "spin", None, CallContext::default()).unwrap_err();
    assert_eq!(err, PluginError::Trapped("took longer than 300ms".into()));
    assert!(t.elapsed() < Duration::from_secs(10));
}

#[test]
fn memory_is_limited() {
    let p = load(
        "alloc",
        Limits {
            memory_bytes: 64 << 20,
            ..Limits::default()
        },
    );
    let err = run(&p, "alloc", None, CallContext::default()).unwrap_err();
    assert!(matches!(err, PluginError::Trapped(_)), "{err:?}");
}

#[test]
fn files_are_out_of_reach_without_permission() {
    let p = load("files", Limits::default());
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let err = run(
        &p,
        "read-file",
        Some(target.to_str().unwrap()),
        CallContext::default(),
    )
    .unwrap_err();
    let PluginError::Failed(msg) = err else {
        panic!("{err:?}")
    };
    assert!(msg.starts_with("denied"), "{msg}");
}

#[test]
fn a_missing_wasm_fails_to_load() {
    let dir = plugin_dir("missing");
    std::fs::remove_file(dir.join("plugin.wasm")).unwrap();
    let manifest = Manifest::load(&dir).unwrap();
    let p = Runtime::new(Limits::default())
        .unwrap()
        .load(dir, manifest, LoadOptions::default());
    let err = run(&p, "bookmark", None, CallContext::default()).unwrap_err();
    assert!(matches!(err, PluginError::Unavailable(m) if m.contains("plugin.wasm")));
    assert!(matches!(p.status(), Status::Failed(_)));
}

/// A plugin built for a newer minor of the edits than this sheaf reads
/// isn't loaded: its manifest is refused as it's read, so it's found as
/// a plugin that can't load, and never runs.
#[test]
fn a_plugin_built_for_newer_edits_is_refused_at_load() {
    let dir = plugin_dir("newer-edits");
    let manifest = dir.join("sheaf-plugin.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace("edit-schema = \"1.0\"", "edit-schema = \"1.1\""),
    )
    .unwrap();
    let refused = format!("{:#}", Manifest::load(&dir).unwrap_err());
    assert!(
        refused.contains("built for edits 1.1; this sheaf reads up to 1.0"),
        "{refused}"
    );
    let found = sheaf_plugin::discover(dir.parent().unwrap());
    let ours = found.iter().find_map(|f| match f {
        Err((at, e)) if *at == dir => Some(format!("{e:#}")),
        _ => None,
    });
    assert_eq!(ours, Some(refused));
}

#[test]
fn discover_finds_plugins_and_reports_bad_ones() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("discover");
    let _ = std::fs::remove_dir_all(&root);
    for (name, manifest) in [
        (
            "a",
            "id = \"alpha\"\nname = \"A\"\nversion = \"1.0.0\"\napi = \"^0.4\"\n",
        ),
        (
            "b",
            "id = \"alpha\"\nname = \"B\"\nversion = \"1.0.0\"\napi = \"^0.4\"\n",
        ),
        (
            "c",
            "id = \"Bad\"\nname = \"C\"\nversion = \"1.0.0\"\napi = \"^0.4\"\n",
        ),
    ] {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("sheaf-plugin.toml"), manifest).unwrap();
    }
    std::fs::create_dir_all(root.join("not-a-plugin")).unwrap();
    let found = sheaf_plugin::discover(&root);
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].as_ref().unwrap().manifest.name, "A");
    assert!(format!("{:#}", found[1].as_ref().err().unwrap().1).contains("already has id"));
    assert!(found[2].is_err());
    assert!(sheaf_plugin::discover(&root.join("nope")).is_empty());
}

/// Tax forms are bookmarked at the end of the top level, in page order,
/// after what's there.
#[test]
fn tax_form_bookmarks_example() {
    let p = bundled(
        "tax-form-bookmarks",
        "tax-form-bookmarks",
        LoadOptions::default(),
    );
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Form 1040 U.S. Individual Income Tax Return",
        "Form 1040 (2025) Page 2",
        "SCHEDULE C\n(Form 1040)",
        "PAYER'S name\nJPMorgan Chase Bank\nForm 1099-INT",
        "Notes to self",
    ]));
    let d = Shared::new(doc(5), "tax-form-bookmarks");
    let cover = d.with(|d| fixture::bookmark(None, None, "Cover letter", d.page(4)));
    d.with(|d| d.commit_ops(vec![cover], "Bookmark")).unwrap();
    let ctx = |d: &Shared| CallContext {
        doc: Some(d.context("binder.pdf", text.clone())),
        ..CallContext::default()
    };
    let out = run(&p, "scan", None, ctx(&d)).unwrap();
    assert_eq!(out.label.as_deref(), Some("Bookmark 3 tax forms"));
    assert!(d.land(Ok(out)).failed.is_none());
    let got: Vec<(String, Option<usize>)> = d.with(|d| {
        let st = d.state();
        (st.outline_roots().iter())
            .map(|&id| {
                let b = st.bookmark(id).unwrap();
                (b.title.to_string(), b.page().and_then(|t| d.index_of(t)))
            })
            .collect()
    });
    assert_eq!(
        got,
        [
            ("Cover letter".to_string(), Some(4)),
            ("Form 1040".to_string(), Some(0)),
            ("Schedule C".to_string(), Some(2)),
            ("1099-INT — JPMorgan Chase Bank".to_string(), Some(3)),
        ]
    );
    // Running again adds nothing.
    assert_eq!(
        run(&p, "scan", None, ctx(&d)).unwrap_err(),
        PluginError::Failed("no new tax forms found".into())
    );
}

/// Records UI calls; answers forms from a script.
#[derive(Default)]
struct FakeUi {
    answer: std::sync::Mutex<Option<Vec<FieldAnswer>>>,
    /// How long "the user" takes to answer.
    delay: Duration,
    /// What "the user" does while the next form is open, before answering.
    meanwhile: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    forms: std::sync::Mutex<Vec<Form>>,
    notices: std::sync::Mutex<Vec<String>>,
    panels: std::sync::Mutex<Vec<(String, Vec<PanelRow>)>>,
    shown: std::sync::Mutex<Vec<ShowPanel>>,
}

impl Ui for FakeUi {
    fn show_form(&self, form: Form) -> Option<Vec<FieldAnswer>> {
        self.forms.lock().unwrap().push(form);
        std::thread::sleep(self.delay);
        if let Some(edit) = self.meanwhile.lock().unwrap().take() {
            edit();
        }
        self.answer.lock().unwrap().take()
    }

    fn notice(&self, message: String) {
        self.notices.lock().unwrap().push(message);
    }

    fn set_panel(&self, panel: String, rows: Vec<PanelRow>) {
        self.panels.lock().unwrap().push((panel, rows));
    }

    fn show_panel(&self, panel: ShowPanel) {
        self.shown.lock().unwrap().push(panel);
    }
}

fn with_ui(d: &Shared, ui: &Arc<FakeUi>) -> CallContext {
    CallContext {
        ui: Some(ui.clone()),
        ..ctx(d)
    }
}

impl FakeUi {
    /// A user who does `edit` to `d` while the next form is open,
    /// published before answering it.
    fn editing(d: &Shared, edit: impl FnOnce(&mut Document) + Send + 'static) -> Arc<FakeUi> {
        let d = d.clone();
        let ui = FakeUi::default();
        *ui.meanwhile.lock().unwrap() = Some(Box::new(move || d.with(edit)));
        Arc::new(ui)
    }

    /// The last notice.
    fn told(&self) -> String {
        self.notices
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or_default()
    }
}

#[test]
fn forms_round_trip_and_waiting_is_free() {
    // The user takes longer than the plugin's whole time limit.
    let p = load(
        "form",
        Limits {
            call_timeout: Duration::from_millis(200),
            ..Limits::default()
        },
    );
    let d = Shared::of(doc(3));
    let third = d.with(|d| d.page(2).unwrap());
    let ui = Arc::new(FakeUi {
        delay: Duration::from_millis(500),
        ..FakeUi::default()
    });
    *ui.answer.lock().unwrap() = Some(vec![
        FieldAnswer {
            id: "title".into(),
            value: FieldValue::Text("Engagement letter".into()),
        },
        FieldAnswer {
            id: "pages".into(),
            value: FieldValue::PageRange(vec![third]),
        },
    ]);
    let out = run(&p, "form", None, with_ui(&d, &ui)).unwrap();
    {
        let forms = ui.forms.lock().unwrap();
        let form = &forms[0];
        assert_eq!(form.title, "Bookmark");
        assert_eq!(form.submit_label, "OK");
        assert!(
            matches!(&form.fields[0].kind, FieldKind::Text { default, .. } if default == "Untitled")
        );
        assert_eq!(form.fields[1].kind, FieldKind::PageRange(String::new()));
    }
    assert!(d.land(Ok(out)).failed.is_none());
    d.with(|d| {
        let st = d.state();
        let b = st.bookmark(st.outline_roots()[0]).unwrap();
        assert_eq!(
            (b.title.as_ref(), b.page()),
            ("Engagement letter", Some(third))
        );
    });
    // Cancelled (and no UI at all) come back as none.
    let err = run(&p, "form", None, with_ui(&d, &ui)).unwrap_err();
    assert_eq!(err, PluginError::Failed("cancelled".into()));
    let err = run(&p, "form", None, ctx(&d)).unwrap_err();
    assert_eq!(err, PluginError::Failed("cancelled".into()));
}

/// A call's reads are the slots it decided by, recorded in its
/// transaction as it read them (M10.5 §14.3): listing the pages reads
/// their order, their labels, what each shows and its boxes, and a page's
/// text what of the page the file will draw as text (what it shows, and no
/// annotation, a mark on it neither); a page's annotations read its list
/// of them and each mark shown; the outline its top level and each
/// bookmark.
#[test]
fn a_call_s_reads_are_the_slots_it_decided_by() {
    let p = load("reads", Limits::default());
    let d = Shared::of(doc(2));
    let (first, pages) = d.with(|d| (d.page(0).unwrap(), d.state().order().clone()));
    let listed = || -> BTreeSet<Key> {
        let each = pages.iter().flat_map(|&p| [Key::Page(p), Key::Geom(p)]);
        [Key::Order, Key::Labels].into_iter().chain(each).collect()
    };
    let out = run(&p, "bookmark", None, ctx(&d)).unwrap();
    assert_eq!(decided(&out), listed(), "the pages, and each one's text");
    let base = out.tx.as_ref().map(|tx| tx.base());
    assert_eq!(base, Some(d.with(|d| d.current_version())));
    d.land(Ok(out));

    let out = run(&p, "reposition", Some("10,20"), ctx(&d)).unwrap();
    let outline: BTreeSet<Key> = d.with(|d| {
        let roots = d.state().outline_roots().clone();
        let each = roots.into_iter().map(Key::Bookmark);
        std::iter::once(Key::OutlineRoots).chain(each).collect()
    });
    assert_eq!(outline.len(), 3);
    let read = decided(&out);
    assert!(read.is_superset(&outline), "{read:?}");
    assert_eq!(read, &outline | &listed());

    let made = d.with(|d| d.commit_ops(vec![fixture::add(first, &square())], "Add"));
    let mark = made.unwrap().created.annots().next().unwrap();
    let out = run(&p, "marks", None, ctx(&d)).unwrap();
    let mut on_first = listed();
    on_first.extend([Key::PageAnnots(first), Key::Annot(mark)]);
    assert_eq!(decided(&out), on_first);
    let out = run(&p, "bookmark", None, ctx(&d)).unwrap();
    assert_eq!(decided(&out), listed(), "text reads no mark");
}

/// A user who turns a page while a plugin's form is open: the document
/// moves on before the form is answered.
struct TurnedMeanwhile {
    cell: Arc<ArcSwap<Published>>,
    turned: Arc<Published>,
}

impl Ui for TurnedMeanwhile {
    fn show_form(&self, _: Form) -> Option<Vec<FieldAnswer>> {
        self.cell.store(self.turned.clone());
        Some(Vec::new())
    }

    fn notice(&self, _: String) {}

    fn set_panel(&self, _: String, _: Vec<PanelRow>) {}

    fn show_panel(&self, _: ShowPanel) {}
}

/// A call's transaction is made on the document as it is at the call's
/// first read or submit, not when the call was queued or its form shown
/// (M10.5 §14.3): the bookmark it submits after its form is made on the
/// page the user turned meanwhile. A submit decides by nothing it didn't
/// read.
#[test]
fn a_call_s_base_is_the_document_at_its_first_read() {
    let p = load("base", Limits::default());
    let d = Shared::of(doc(3));
    let c = ctx(&d);
    let cell = c.doc.as_ref().unwrap().cell.clone();
    let shown = d.with(|d| d.current_version());
    let shown_cell = cell.load_full();
    let turned = d.with(|d| {
        let page = d.page(0).unwrap();
        d.commit_ops(vec![fixture::rotate(&[page], Turn::Quarter)], "Rotate")
            .unwrap();
        Arc::new(d.published())
    });
    // As the call was queued: before the turn.
    cell.store(shown_cell);
    let ui = TurnedMeanwhile {
        cell,
        turned: turned.clone(),
    };
    let c = CallContext {
        ui: Some(Arc::new(ui)),
        ..c
    };
    let out = run(&p, "form", None, c).unwrap();
    let base = out.tx.as_ref().map(|tx| tx.base());
    assert_ne!(base, Some(shown));
    assert_eq!(base, Some(turned.snap.version));
    assert!(decided(&out).is_empty(), "{:?}", decided(&out));
}

#[test]
fn panels_notices_and_panel_actions() {
    let p = load("panel", Limits::default());
    let d = Shared::of(doc(2));
    let ui = Arc::new(FakeUi::default());
    run(&p, "panel", Some(r"\d{4}"), with_ui(&d, &ui)).unwrap();
    let panels = ui.panels.lock().unwrap();
    assert_eq!(panels.len(), 1, "undeclared panels are dropped");
    let (id, rows) = &panels[0];
    assert_eq!(id, "matches");
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["1040", "1041"]);
    assert_eq!(rows[1].page, d.with(|d| d.page(1)));
    // Where it shows, through the page's view matrix.
    assert_eq!(rows[0].detail, "5,10");
    assert_eq!(*ui.notices.lock().unwrap(), ["2 matches"]);
    assert_eq!(
        *ui.shown.lock().unwrap(),
        [
            ShowPanel::Plugin("matches".into()),
            ShowPanel::Builtin(BuiltinPanel::Comments)
        ],
        "unknown panels are dropped"
    );

    let err = block_on(p.call(
        Call::PanelAction {
            panel: "matches".into(),
            row: rows[0].id.clone(),
            action: "pick".into(),
        },
        ctx(&d),
    ))
    .unwrap()
    .unwrap_err();
    assert_eq!(
        err,
        PluginError::Failed(format!("matches/{}/pick", rows[0].id))
    );
}

#[test]
fn bad_regex_is_an_error_not_a_trap() {
    let p = load("bad-regex", Limits::default());
    let d = Shared::of(doc(1));
    let err = run(&p, "panel", Some("(unclosed"), ctx(&d)).unwrap_err();
    assert!(
        matches!(&err, PluginError::Failed(m) if m.contains("unclosed")),
        "{err:?}"
    );
    assert_eq!(p.status(), Status::Ready);
}

/// What Find Matching Amounts' form is answered with: a $100 minimum,
/// `pages`, round hundreds skipped.
fn crossref_answers(pages: Vec<PageId>) -> Option<Vec<FieldAnswer>> {
    Some(vec![
        FieldAnswer {
            id: "min".into(),
            value: FieldValue::Number(100.0),
        },
        FieldAnswer {
            id: "pages".into(),
            value: FieldValue::PageRange(pages),
        },
        FieldAnswer {
            id: "round".into(),
            value: FieldValue::Checkbox(true),
        },
    ])
}

#[test]
fn amount_crossref_bundled_plugin() {
    let p = bundled("amount-crossref", "amount-crossref", LoadOptions::default());
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Schedule K-1 box 1 ordinary income 12,345.67",
        "Form 1040 line 8 $12,345.67 and fee 99.00 and 2,000.00",
        "Workpaper total (12,345.67) memo 12,345.67 rounded 2,000.00",
    ]));
    let d = Shared::new(doc(3), "amount-crossref");
    let ui = Arc::new(FakeUi::default());
    let ctx = |d: &Shared, ui: &Arc<FakeUi>| CallContext {
        doc: Some(d.context("binder.pdf", text.clone())),
        ui: Some(ui.clone()),
        ..CallContext::default()
    };
    let all: Vec<_> = d.with(|d| (0..3).map(|i| d.page(i).unwrap()).collect());
    *ui.answer.lock().unwrap() = crossref_answers(all);
    let out = run(&p, "find", None, ctx(&d, &ui)).unwrap();
    assert!(out.tx.is_none());
    let rows = ui.panels.lock().unwrap().last().unwrap().1.clone();
    let summary: Vec<(String, String)> = rows
        .iter()
        .map(|r| (r.label.clone(), r.detail.clone()))
        .collect();
    assert_eq!(
        summary,
        [
            ("$12,345.67".to_string(), "page 1 ⇄ page 2".to_string()),
            ("$12,345.67".to_string(), "page 2 ⇄ page 3".to_string()),
        ],
        "99.00 is under the minimum, 2,000.00 is round, (12,345.67) is negative"
    );
    assert_eq!(
        ui.notices.lock().unwrap().last().unwrap(),
        "2 matching pairs — see Amount Matches"
    );

    // Link the first pair from the panel.
    let out = block_on(p.call(
        Call::PanelAction {
            panel: "pairs".into(),
            row: rows[0].id.clone(),
            action: "link".into(),
        },
        ctx(&d, &ui),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(out.label.as_deref(), Some("Link $12,345.67"));
    assert!(d.land(Ok(out)).failed.is_none());
    let labels: Vec<(usize, String)> = d.with(|d| {
        (0..3)
            .flat_map(|i| {
                let id = d.page(i).unwrap();
                d.state()
                    .marks_on(id)
                    .filter_map(|a| match &a.kind {
                        AnnotKind::Connector { label, partner, .. } => {
                            assert!(partner.is_some(), "the host pairs the ends");
                            Some((i, label.clone()))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    });
    assert_eq!(labels, [(0, "1".to_string()), (1, "1".to_string())]);
    // The panel now holds just the other pair.
    let rows = ui.panels.lock().unwrap().last().unwrap().1.clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].detail, "page 2 ⇄ page 3");
}

fn load_with(name: &str, opts: LoadOptions) -> Plugin {
    let dir = plugin_dir(name);
    let manifest = Manifest::load(&dir).unwrap();
    Runtime::new(Limits::default())
        .unwrap()
        .load(dir, manifest, opts)
}

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn answer(p: &Plugin, cmd: &str, args: Option<&str>) -> String {
    match run(p, cmd, args, CallContext::default()) {
        Err(PluginError::Failed(m)) => m,
        other => panic!("{other:?}"),
    }
}

#[test]
fn storage_survives_a_restart() {
    let data = scratch("storage-data");
    let opts = LoadOptions {
        data_dir: Some(data.clone()),
        ..LoadOptions::default()
    };
    let p = load_with("storage", opts.clone());
    run(&p, "store", Some("client=Acme"), CallContext::default()).unwrap();
    drop(p);
    let p = load_with("storage", opts);
    assert_eq!(answer(&p, "store", Some("client")), r#"Some("Acme")"#);
    assert_eq!(answer(&p, "settings", None), "{}");
    std::fs::write(data.join("settings.json"), r#"{"x":1}"#).unwrap();
    assert_eq!(answer(&p, "settings", None), r#"{"x":1}"#);
}

/// Events reach `on-event` as the API says them: a change by its versions
/// and scopes, and a refusal by its reason.
#[test]
fn events_arrive_through_on_event() {
    let p = load("events", Limits::default());
    let mut d = doc(1);
    let (page, from) = (d.page(0).unwrap(), d.current_version());
    let made = d.commit_ops(vec![fixture::add(page, &square())], "Add");
    let change = ChangeInfo::of(&made.unwrap().change, d.instance());
    for e in [
        DocEvent::Opened,
        DocEvent::Changed(change),
        DocEvent::Closed,
        DocEvent::EditRefused("the user undid this".into()),
    ] {
        block_on(p.call(Call::Event(e), CallContext::default()))
            .unwrap()
            .unwrap();
    }
    let to = d.current_version();
    assert_eq!(
        answer(&p, "events", None),
        format!(
            "opened,changed:{}>{} annots 1,closed,refused:the user undid this",
            from.0, to.0
        )
    );
    let m = p.manifest();
    assert!(m.events.contains(&sheaf_plugin::EventKind::Changed));
    assert!(m.events.contains(&sheaf_plugin::EventKind::EditRefused));
    assert!(!m.events.contains(&sheaf_plugin::EventKind::Saved));
}

#[test]
fn granted_directories_are_reachable_and_only_as_granted() {
    let ro = scratch("grant-ro");
    let rw = scratch("grant-rw");
    let outside = scratch("grant-outside");
    for d in [&ro, &rw, &outside] {
        std::fs::write(d.join("f.txt"), "hello").unwrap();
    }
    let p = load_with(
        "grants",
        LoadOptions {
            grants: Grants {
                read: vec![ro.clone()],
                write: vec![rw.clone()],
                ..Grants::default()
            },
            ..LoadOptions::default()
        },
    );
    let path = |d: &Path| d.join("f.txt").to_string_lossy().replace('\\', "/");
    assert_eq!(answer(&p, "read-file", Some(&path(&ro))), "read 5 bytes");
    assert_eq!(answer(&p, "read-file", Some(&path(&rw))), "read 5 bytes");
    assert!(answer(&p, "read-file", Some(&path(&outside))).starts_with("denied"));
    assert!(answer(&p, "write-file", Some(&path(&ro))).starts_with("denied"));
    assert_eq!(answer(&p, "write-file", Some(&path(&rw))), "wrote");
    assert!(answer(&p, "write-file", Some(&path(&outside))).starts_with("denied"));
    assert_eq!(std::fs::read_to_string(rw.join("f.txt")).unwrap(), "x");
}

/// A plugin's text is the page as the file will draw it (M10.5 §14.3),
/// read under the plugin's inputs, in PageSpace. Through the page's view
/// matrices it's where it shows: on a page the file turns 90°, its crop
/// box off the origin, where the fake engine drew it there, and a region
/// given as it shows takes the text in it alone; turned a quarter more in
/// sheaf, where it shows then. Search folds case.
#[test]
fn text_comes_as_the_file_draws_it_in_page_space() {
    let p = load("find", Limits::default());
    let d = Shared::of(turned_doc());
    let page = d.with(|d| d.page(0).unwrap());
    let text = Arc::new(FakeText::forms());
    let ctx = |d: &Shared| {
        let mut c = ctx(d);
        c.doc.as_mut().unwrap().text = text.clone();
        c
    };
    let want = PluginError::Failed(
        "hits Form@0,10 4x10; words Form@0,10 4x10 1040@5,10 4x10; boxes 0,10 9x10".into(),
    );
    assert_eq!(run(&p, "find", Some("FORM"), ctx(&d)).unwrap_err(), want);
    // In PageSpace, where the service hands it, the first line runs up the
    // page from (30, 30).
    let o = TextOpts {
        inputs: ViewInputs::plugin_text(),
    };
    let chars = text.chars(&d.with(|d| d.snapshot()), page, &o).unwrap();
    let f = chars.glyphs[0].rect.unwrap();
    assert_eq!((f.x, f.y, f.w, f.h), (30.0, 30.0, 10.0, 1.0));
    d.with(|d| d.commit_ops(vec![fixture::rotate(&[page], Turn::Quarter)], "Rotate"))
        .unwrap();
    // Turned a quarter more, its lines run down the page, the second to
    // the left of the first.
    let turned = PluginError::Failed(
        "hits Form@560,0 10x4; words Form@560,0 10x4 1040@560,5 10x4 second@460,0 10x6 \
         line@460,7 10x4; boxes 560,0 10x9+460,0 10x11"
            .into(),
    );
    assert_eq!(run(&p, "find", Some("FORM"), ctx(&d)).unwrap_err(), turned);
    let asked = text.asked();
    assert_eq!(asked.len(), 7);
    assert!(asked.iter().all(|a| *a == o), "{asked:?}");
}

struct FakeOcr;

impl sheaf_plugin::OcrSource for FakeOcr {
    fn recognize(
        &self,
        snap: &Snapshot,
        page: PageId,
        language: Option<String>,
        _dpi: Option<u32>,
    ) -> Result<sheaf_doc::TextLayer> {
        // Slow, like a real engine: longer than the plugin's time limit.
        std::thread::sleep(Duration::from_millis(400));
        let base = &snap.state.def(page).unwrap().base;
        Ok(sheaf_doc::text_layer::from_engine(
            vec![sheaf_doc::OcrWord {
                text: format!("page{}", base.index),
                rect: sheaf_doc::Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 40.0,
                    h: 10.0,
                },
                confidence: 0.9,
            }],
            &snap.geom(page),
            &format!("fake-{}", language.unwrap_or_default()),
        ))
    }
}

#[test]
fn ocr_runs_on_the_host_and_isnt_charged_to_the_plugin() {
    let p = load(
        "ocr",
        Limits {
            call_timeout: Duration::from_millis(200),
            ..Limits::default()
        },
    );
    let d = Shared::of(doc(1));
    let mut c = ctx(&d);
    c.doc.as_mut().unwrap().ocr = Some(Arc::new(FakeOcr));
    let err = run(&p, "ocr", None, c).unwrap_err();
    assert_eq!(err, PluginError::Failed("page0 by fake-deu".into()));
    // Without an OCR source, it says so.
    let err = run(&p, "ocr", None, ctx(&d)).unwrap_err();
    assert_eq!(err, PluginError::Failed("OCR isn't available".into()));
}

#[test]
fn bookmark_from_pattern_bundled_plugin() {
    let p = bundled(
        "bookmark-from-pattern",
        "bookmark-from-pattern",
        LoadOptions::default(),
    );
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Invoice 101\nfor services",
        "nothing here",
        "Invoice 102\nsecond line\nInvoice 103",
    ]));
    let d = Shared::new(doc(3), "bookmark-from-pattern");
    let ui = Arc::new(FakeUi::default());
    let ctx = |d: &Shared, ui: &Arc<FakeUi>| CallContext {
        doc: Some(d.context("invoices.pdf", text.clone())),
        ui: Some(ui.clone()),
        presets: vec![sheaf_plugin::Preset {
            name: "Invoice".into(),
            regex: r"Invoice (\d+)".into(),
        }],
        ..CallContext::default()
    };
    let all: Vec<_> = d.with(|d| (0..3).map(|i| d.page(i).unwrap()).collect());
    let answer = |source: usize, template: &str, placement: usize| {
        vec![
            FieldAnswer {
                id: "source".into(),
                value: FieldValue::Choice(source),
            },
            FieldAnswer {
                id: "regex".into(),
                value: FieldValue::Text(r"Invoice (?P<n>\d+)".into()),
            },
            FieldAnswer {
                id: "template".into(),
                value: FieldValue::Text(template.into()),
            },
            FieldAnswer {
                id: "placement".into(),
                value: FieldValue::Choice(placement),
            },
            FieldAnswer {
                id: "heading".into(),
                value: FieldValue::Text("Invoices".into()),
            },
            FieldAnswer {
                id: "pages".into(),
                value: FieldValue::PageRange(all.clone()),
            },
        ]
    };

    // Custom pattern, a named group in the title, under a new heading.
    *ui.answer.lock().unwrap() = Some(answer(0, "Inv #{n}", 2));
    let out = run(&p, "bookmark", None, ctx(&d, &ui)).unwrap();
    {
        let forms = ui.forms.lock().unwrap();
        let FieldKind::Choice { options, .. } = &forms[0].fields[0].kind else {
            panic!("{:?}", forms[0].fields[0])
        };
        assert_eq!(options[1], "Invoice", "the user's presets are offered");
    }
    assert_eq!(out.label.as_deref(), Some("Bookmark 3 matches"));
    assert!(d.land(Ok(out)).failed.is_none());
    let got: Vec<(u16, String, Option<usize>, Option<f32>)> = d.with(|d| {
        let st = d.state();
        (st.bookmarks_preorder().into_iter())
            .map(|(depth, _, b)| {
                let index = b.page().and_then(|t| d.index_of(t));
                // Held in PageSpace: where it lands as the page shows.
                let top = match (b.view(), b.page()) {
                    (Some(DestView::Xyz { left, top, .. }), Some(t)) => {
                        d.geom(t).to_view().apply_axes(*left, *top).1
                    }
                    _ => None,
                };
                (depth, b.title.to_string(), index, top)
            })
            .collect()
    });
    assert_eq!(
        got,
        [
            (0, "Invoices".to_string(), Some(0), Some(0.0)),
            (1, "Inv #101".to_string(), Some(0), Some(0.0)),
            (1, "Inv #102".to_string(), Some(2), Some(0.0)),
            // Two lines down on its page: the bookmark lands there.
            (1, "Inv #103".to_string(), Some(2), Some(198.0)),
        ]
    );
    d.with(|d| d.undo().unwrap());

    // A preset, the whole match, top level; then a re-run adds nothing.
    *ui.answer.lock().unwrap() = Some(answer(1, "{0}", 0));
    let out = run(&p, "bookmark", None, ctx(&d, &ui)).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    assert_eq!(
        d.with(|d| top_level(d)),
        ["Invoice 101", "Invoice 102", "Invoice 103"]
    );
    *ui.answer.lock().unwrap() = Some(answer(1, "{0}", 0));
    let out = run(&p, "bookmark", None, ctx(&d, &ui)).unwrap();
    assert!(out.tx.is_none());
    assert_eq!(
        ui.notices.lock().unwrap().last().unwrap(),
        "All 3 matches are already bookmarked"
    );
    // The form remembers the last run.
    *ui.answer.lock().unwrap() = None;
    run(&p, "bookmark", None, ctx(&d, &ui)).unwrap();
    let forms = ui.forms.lock().unwrap();
    let FieldKind::Choice { default, .. } = &forms.last().unwrap().fields[0].kind else {
        panic!()
    };
    assert_eq!(*default, 1, "last time used the Invoice preset");
}

/// Subtotal sums the numbers in a region as the page shows, on a page the
/// file turns as on a plain one; it reads a page's numbers again after a
/// change to its text, and not after one to its marks alone.
#[test]
fn subtotal_bundled_plugin() {
    let p = bundled("subtotal", "subtotal", LoadOptions::default());
    let events = &p.manifest().events;
    assert!(events.contains(&sheaf_plugin::EventKind::Region));
    assert!(events.contains(&sheaf_plugin::EventKind::Changed));
    // The fake engine puts line n at y = 10 + 100n, character k at x = k,
    // as the page shows; the first page's text can change.
    let first = Arc::new(Mutex::new(
        "Rent $1,200.00 W-2\nFees (34.50) 2025\nTotal 9,999".to_string(),
    ));
    let text: Arc<dyn TextService> = {
        let first = first.clone();
        Arc::new(FakeText::new(move |index| match index {
            0 => first.lock().unwrap().clone(),
            _ => String::new(),
        }))
    };
    let ui = Arc::new(FakeUi::default());
    for d in [doc(1), turned_doc()] {
        let d = Shared::new(d, "subtotal");
        let (page, from_view) = d.with(|d| {
            let page = d.page(0).unwrap();
            (page, d.geom(page).from_view())
        });
        let ctx = || CallContext {
            doc: Some(d.context("ledger.pdf", text.clone())),
            ui: Some(ui.clone()),
            ..CallContext::default()
        };
        // Given as the page shows; a region event is PageSpace.
        let region = |x: f32, y: f32, w: f32, h: f32| {
            Call::Event(DocEvent::Region {
                page,
                rect: from_view.apply_box(Box2 { x, y, w, h }),
            })
        };
        let call = |c| block_on(p.call(c, ctx())).unwrap().unwrap();
        // Lines 0 and 1: 1,200.00 + (34.50) + 2025.
        call(region(0.0, 0.0, 100.0, 150.0));
        assert_eq!(ui.told(), "Sum 3,190.50 · 3 numbers");
        // Just the first line's amount.
        call(region(0.0, 0.0, 100.0, 50.0));
        assert_eq!(ui.told(), "Sum 1,200.00 · 1 number");
        // Nothing there.
        call(region(500.0, 500.0, 10.0, 10.0));
        assert_eq!(ui.told(), "Subtotal: no numbers in the selection");
        call(Call::Event(DocEvent::RegionCleared));
        assert_eq!(ui.told(), "");

        // The page's text is read again only when a change to it says so:
        // a mark changes no number, and a flatten draws into the page.
        *first.lock().unwrap() = "Rent $1,500.00".into();
        let changed = |op| {
            let made = d.with(|d| d.commit_ops(vec![op], "Edit")).unwrap();
            let instance = d.with(|d| d.instance());
            Call::Event(DocEvent::Changed(ChangeInfo::of(&made.change, instance)))
        };
        call(changed(fixture::add(page, &square())));
        call(region(0.0, 0.0, 100.0, 50.0));
        assert_eq!(ui.told(), "Sum 1,200.00 · 1 number", "as it was read");
        call(changed(fixture::flatten(&[page], FlattenSet::All)));
        call(region(0.0, 0.0, 100.0, 50.0));
        assert_eq!(ui.told(), "Sum 1,500.00 · 1 number");
        *first.lock().unwrap() = "Rent $1,200.00 W-2\nFees (34.50) 2025\nTotal 9,999".into();
    }
}

/// A tiny HTTP server answering Frankfurter-style rate requests with EUR→USD
/// 1.1 and GBP→USD 1.25 on 2026-10-09. Returns its port and a request count.
fn rate_server() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hits = count.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let rate = if req.contains("from=GBP") {
                "1.25"
            } else {
                "1.1"
            };
            let body = format!(
                r#"{{"amount":1.0,"base":"X","date":"2026-10-09","rates":{{"USD":{rate}}}}}"#
            );
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (port, count)
}

#[test]
fn currency_convert_bundled_plugin() {
    let src = workspace_root().join("plugins/currency-convert");
    let wasm = build_plugin(&src, "currency_convert.wasm");
    let dir = install("currency-convert", &src, &wasm);
    let manifest = Manifest::load(&dir).unwrap();
    assert_eq!(manifest.permissions.network, ["api.frankfurter.dev"]);
    let (port, requests) = rate_server();
    let data = scratch("currency-data");
    std::fs::write(
        data.join("settings.json"),
        format!(r#"{{"base_url":"http://127.0.0.1:{port}"}}"#),
    )
    .unwrap();
    let load = |network: Vec<String>| {
        Runtime::new(Limits::default()).unwrap().load(
            dir.clone(),
            manifest.clone(),
            LoadOptions {
                data_dir: Some(data.clone()),
                grants: Grants {
                    network,
                    ..Grants::default()
                },
            },
        )
    };
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Invoice total €1.234,56 and fee GBP 20.00",
        "Already dollars: $500.00",
    ]));
    let d = Shared::new(doc(2), "currency-convert");
    let ui = Arc::new(FakeUi::default());
    let ctx = |d: &Shared| CallContext {
        doc: Some(d.context("statement.pdf", text.clone())),
        ui: Some(ui.clone()),
        ..CallContext::default()
    };
    let answer = |date: &str| {
        Some(vec![
            FieldAnswer {
                id: "target".into(),
                value: FieldValue::Choice(0), // USD
            },
            FieldAnswer {
                id: "date".into(),
                value: FieldValue::Text(date.into()),
            },
        ])
    };

    // Not granted: the request is refused before it leaves.
    let p = load(Vec::new());
    *ui.answer.lock().unwrap() = answer("2026-10-10");
    let err = run(&p, "convert", None, ctx(&d)).unwrap_err();
    assert!(
        matches!(&err, PluginError::Failed(m) if m.contains("denied")),
        "{err:?}"
    );
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);

    // Granted the test server.
    let p = load(vec![format!("127.0.0.1:{port}")]);
    *ui.answer.lock().unwrap() = answer("2026-10-10");
    let out = run(&p, "convert", None, ctx(&d)).unwrap();
    assert_eq!(out.label.as_deref(), Some("Convert 2 amounts to USD"));
    assert!(d.land(Ok(out)).failed.is_none());
    let notes: Vec<String> = d.with(|d| {
        let first = d.page(0).unwrap();
        (d.state().marks_on(first))
            .map(|a| a.contents.clone())
            .collect()
    });
    assert_eq!(
        notes,
        [
            "≈ USD 1,358.02 (EUR 1,234.56 × 1.1, ECB 2026-10-09)",
            "≈ USD 25.00 (GBP 20.00 × 1.25, ECB 2026-10-09)",
        ]
    );
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 2);

    // Again: already converted, and no new requests for cached rates.
    *ui.answer.lock().unwrap() = answer("2026-10-10");
    let out = run(&p, "convert", None, ctx(&d)).unwrap();
    assert!(out.tx.is_none());
    assert_eq!(
        ui.notices.lock().unwrap().last().unwrap(),
        "No amounts to convert to USD"
    );
    // Undone, then again: the rates come from storage, not the network.
    d.with(|d| d.undo().unwrap());
    *ui.answer.lock().unwrap() = answer("2026-10-10");
    let out = run(&p, "convert", None, ctx(&d)).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    let first = d.with(|d| d.state().marks_on(d.page(0).unwrap()).count());
    assert_eq!(first, 2);
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 2);

    // On a page the file turns, each highlight lies over its amount as the
    // page shows it, reading upright there.
    let d = Shared::new(turned_doc(), "currency-convert");
    *ui.answer.lock().unwrap() = answer("2026-10-10");
    let out = run(&p, "convert", None, ctx(&d)).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    let shown: Vec<(Box2, Rotation)> = d.with(|d| {
        let page = d.page(0).unwrap();
        let to_view = d.geom(page).to_view();
        (d.state().marks_on(page))
            .map(|a| match &a.kind {
                AnnotKind::Highlight { rects } => (to_view.apply_box(rects[0]), a.turn),
                k => panic!("{k:?}"),
            })
            .collect()
    });
    // "€1.234,56" from 14 across and "GBP 20.00" from 32, 9 wide each.
    let at = |x: f32| {
        let b = Box2 {
            x,
            y: 10.0,
            w: 9.0,
            h: 10.0,
        };
        (b, Rotation::R90)
    };
    assert_eq!(shown, [at(14.0), at(32.0)]);
}

/// Pixels whose gray value is (x + y) % 256 in page-pixel coordinates.
struct FakeRender;

impl sheaf_plugin::render::RenderSource for FakeRender {
    fn render(
        &self,
        _snap: &Snapshot,
        _page: PageId,
        _inputs: &ViewInputs,
        _page_w: u32,
        _page_h: u32,
        (x0, y0, w, h): (u32, u32, u32, u32),
    ) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let v = ((x + y) % 256) as u8;
                out.extend([v, v, v, 255]);
            }
        }
        Ok(out)
    }
}

#[test]
fn render_handles_crop_map_cap_and_expire() {
    let p = load("render", Limits::default());
    let d = Shared::of(doc(1));
    let with_render = |d: &Shared| {
        let mut c = ctx(d);
        c.doc.as_mut().unwrap().render = Some(Arc::new(FakeRender));
        c
    };
    let err = run(&p, "render", None, with_render(&d)).unwrap_err();
    assert_eq!(
        err,
        PluginError::Failed("612x792 -> 30x40 first [30] at 10,20 rows 60".into())
    );
    // The image the plugin kept is gone after its call.
    let err = run(&p, "kept-width", None, with_render(&d)).unwrap_err();
    assert_eq!(err, PluginError::Failed("0".into()));
    let err = run(&p, "render-many", None, with_render(&d)).unwrap_err();
    assert!(
        matches!(&err, PluginError::Failed(m) if m.starts_with("16 then: at most 16 images")),
        "{err:?}"
    );
    // Without a renderer it says so.
    let err = run(&p, "render", None, ctx(&d)).unwrap_err();
    assert_eq!(err, PluginError::Failed("rendering isn't available".into()));
}

/// Blocky noise per page: pages with the same seed look the same.
struct SeededRender(Vec<u64>);

impl sheaf_plugin::render::RenderSource for SeededRender {
    fn render(
        &self,
        snap: &Snapshot,
        page: PageId,
        _inputs: &ViewInputs,
        page_w: u32,
        page_h: u32,
        (x0, y0, w, h): (u32, u32, u32, u32),
    ) -> Result<Vec<u8>> {
        let seed = self.0[snap.state.pos_of(page).unwrap()];
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                // Blocks scale with the page so any dpi gives the same picture.
                let (bx, by) = (x * 16 / page_w, y * 16 / page_h);
                let mut v = seed ^ (bx as u64 * 0x9e37_79b9) ^ (by as u64 * 0x85eb_ca6b);
                v ^= v >> 13;
                v = v.wrapping_mul(0xc2b2_ae35);
                let g = (v >> 24) as u8;
                out.extend([g, g, g, 255]);
            }
        }
        Ok(out)
    }
}

#[test]
fn duplicate_pages_bundled_plugin() {
    let opts = LoadOptions {
        data_dir: Some(scratch("duplicates-data")),
        ..LoadOptions::default()
    };
    let p = bundled("duplicate-pages", "duplicate-pages", opts);
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Statement 101 total 5,000",
        "Invoice 7",
        "Statement 101 total 5,000",
        // Same layout, next month.
        "Statement 102 total 6,000",
        "W-2 Acme 2025",
    ]));
    let render: Arc<dyn sheaf_plugin::render::RenderSource> =
        Arc::new(SeededRender(vec![1, 2, 1, 1, 3]));
    let d = Shared::new(doc(5), "duplicate-pages");
    let ui = Arc::new(FakeUi::default());
    let ctx = |d: &Shared| CallContext {
        doc: Some(DocContext {
            render: Some(render.clone()),
            ..d.context("binder.pdf", text.clone())
        }),
        ui: Some(ui.clone()),
        ..CallContext::default()
    };
    let tolerant = || {
        Some(vec![FieldAnswer {
            id: "match".into(),
            value: FieldValue::Choice(0),
        }])
    };
    *ui.answer.lock().unwrap() = tolerant();
    run(&p, "find", None, ctx(&d)).unwrap();
    let rows = ui.panels.lock().unwrap().last().unwrap().1.clone();
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["Pages 1, 3", "Page 1", "Page 3"]);
    assert_eq!(rows[0].detail, "100% alike");

    // Keep the first copy: page 3 goes, in one step.
    let third = d.with(|d| d.page(2).unwrap());
    let out = block_on(p.call(
        Call::PanelAction {
            panel: "duplicates".into(),
            row: rows[0].id.clone(),
            action: "keep-first".into(),
        },
        ctx(&d),
    ))
    .unwrap()
    .unwrap();
    assert_eq!(out.label.as_deref(), Some("Delete 1 duplicate page"));
    assert!(d.land(Ok(out)).failed.is_none());
    d.with(|d| {
        assert_eq!(d.state().order().len(), 4);
        assert_eq!(d.index_of(third), None);
        d.undo().unwrap();
    });
    assert!(ui.panels.lock().unwrap().last().unwrap().1.is_empty());

    // "Not duplicates" is remembered across runs.
    *ui.answer.lock().unwrap() = tolerant();
    run(&p, "find", None, ctx(&d)).unwrap();
    let rows = ui.panels.lock().unwrap().last().unwrap().1.clone();
    block_on(p.call(
        Call::PanelAction {
            panel: "duplicates".into(),
            row: rows[0].id.clone(),
            action: "not-duplicates".into(),
        },
        ctx(&d),
    ))
    .unwrap()
    .unwrap();
    *ui.answer.lock().unwrap() = tolerant();
    run(&p, "find", None, ctx(&d)).unwrap();
    assert_eq!(
        ui.notices.lock().unwrap().last().unwrap(),
        "No duplicate pages found"
    );
}

/// A command's progress tasks report under the call's tracker, and
/// cancelling it stops the command at its next check.
#[test]
fn progress_reports_and_cancels() {
    let p = load("progress", Limits::default());
    let d = Shared::of(doc(1));
    // Runs to the end without a tracker.
    run(&p, "count", None, ctx(&d)).unwrap();

    // And with one: its tasks are freed, so the step goes back to the
    // call's own, which counts nothing, and the message stays.
    let tracker = in_progress::Tracker::new();
    let mut c = ctx(&d);
    c.progress = Some(tracker.clone());
    run(&p, "count", None, c).unwrap();
    let s = tracker.snapshot();
    assert_eq!(
        (s.fraction, s.step.as_str(), s.total, s.message_count),
        (Some(1.0), "", None, 1)
    );

    // A task left short of its total, which the host frees, fills its part.
    let tracker = in_progress::Tracker::new();
    let mut c = ctx(&d);
    c.progress = Some(tracker.clone());
    run(&p, "early", None, c).unwrap();
    let s = tracker.snapshot();
    assert_eq!((s.fraction, s.step.as_str()), (Some(1.0), ""));

    let tracker = in_progress::Tracker::new();
    let mut c = ctx(&d);
    c.progress = Some(tracker.clone());
    let rx = p.run_command("count", None, c);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    // Wait until it's under way, then stop it.
    loop {
        let s = tracker.snapshot();
        if s.done >= 20 {
            assert_eq!(
                s.message,
                Some((in_progress::Level::Info, "ten done".into()))
            );
            assert!(s.fraction.unwrap() > 0.0);
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no progress: {s:?}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    tracker.cancel();
    let out = block_on(rx).expect("reply");
    match out {
        Err(PluginError::Failed(m)) => {
            let n: u64 = m.trim_start_matches("Stopped at ").parse().unwrap();
            assert!((20..400).contains(&n), "{m}");
        }
        other => panic!("expected a stop, got {other:?}"),
    }
    // A stopped task keeps its place.
    let s = tracker.snapshot();
    assert!(s.cancelled && s.fraction.unwrap() < 1.0, "{s:?}");
    assert_eq!(p.status(), Status::Ready);
}

/// Tasks past the cap on live ones get trackers nobody sees: none of them
/// moves the bar, takes the step or shows a message. Stopping the call
/// still reaches them.
#[test]
fn tasks_past_the_cap_leave_the_bar_alone() {
    let p = load("many-steps", Limits::default());
    let d = Shared::of(doc(1));
    let tracker = in_progress::Tracker::new();
    let mut c = ctx(&d);
    c.progress = Some(tracker.clone());
    let rx = p.run_command("many-steps", None, c);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let s = tracker.snapshot();
        if s.message.as_ref().is_some_and(|(_, m)| m == "made") {
            assert_eq!(
                (s.fraction, s.step.as_str(), s.message_count),
                (Some(0.0), "Many", 1)
            );
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no message: {s:?}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    tracker.cancel();
    match block_on(rx).expect("reply") {
        Err(PluginError::Failed(m)) => assert_eq!(m, "Stopped"),
        other => panic!("expected a stop, got {other:?}"),
    }
}

/// A plugin is shown every kind of mark, as it's kept, and its
/// preferences; the mark it adds is its own, its tokens left as it wrote
/// them (M10.5 §6.8).
#[test]
fn every_kind_is_shown_and_the_preferences() {
    use sheaf_doc::measure::{Measured, Scale};
    use sheaf_doc::{Annot, AnnotId, Pt};
    let p = load("kinds", Limits::default());
    let d = Shared::of(doc(1));
    let page = d.with(|d| d.page(0).unwrap());
    let mark = |kind| Annot {
        id: AnnotId(0),
        kind,
        color: [0, 0, 255],
        opacity: 1.0,
        stroke: 1.0,
        author: "Greg".into(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: true,
        cloudy: false,
        turn: sheaf_edit::Rotation::R0,
        name: String::new(),
    };
    let pts = vec![
        Pt { x: 0.0, y: 0.0 },
        Pt { x: 9.0, y: 0.0 },
        Pt { x: 9.0, y: 9.0 },
    ];
    for annot in [
        mark(AnnotKind::Polygon {
            points: pts.clone(),
        }),
        mark(AnnotKind::Measure {
            what: Measured::Area,
            points: pts,
            scale: Box::new(Scale::default()),
        }),
    ] {
        d.with(|d| d.commit_ops(vec![fixture::add(page, &annot)], "add"))
            .unwrap();
    }
    let out = run(&p, "marks", None, ctx(&d)).unwrap();
    assert_eq!(
        answer(&p, "store", Some("kinds")),
        r#"Some("polygon:dashed,measure:dashed")"#
    );
    assert!(d.land(Ok(out)).failed.is_none());
    d.with(|d| {
        let added = d.state().marks_on(page).last().unwrap().clone();
        assert!(matches!(added.kind, AnnotKind::Polygon { .. }));
        assert_eq!((added.fill, added.dashed), (Some([255, 0, 0]), true));
        assert_eq!(added.author, "test-plugin", "the plugin's");
        // Never named to a plugin not granted it.
        assert_eq!(added.contents, "Checked by <<initials>>");
    });
    let c = CallContext {
        preferences: r#"{"rate":2}"#.into(),
        ..CallContext::default()
    };
    match run(&p, "prefs", None, c) {
        Err(PluginError::Failed(m)) => assert_eq!(m, r#"{"rate":2}"#),
        other => panic!("{other:?}"),
    }
    assert_eq!(answer(&p, "prefs", None), "{}");
}

/// A plugin places things where the user sees them through its page's
/// view matrices (`wit/sheaf.wit`): on a `/Rotate 90` page with its
/// CropBox off the origin, a square and a bookmark placed at a point as
/// the page shows are kept in PageSpace there, and read back through the
/// matrices as placed; a bookmark's position moves to a point as its own
/// page shows. Text is found where it shows, the turned page as the plain
/// one.
#[test]
fn a_plugin_places_through_the_page_s_view_matrices() {
    let p = load("frames", Limits::default());
    let d = Shared::of(turned_doc());
    let (page, geom) = d.with(|d| {
        let page = d.page(0).unwrap();
        (page, d.geom(page))
    });
    assert_eq!(geom.rotate, Rotation::R90);
    // As shown, (x, y) is PageSpace (20 + y, 30 + x) on this page.
    let shown = Box2 {
        x: 100.0,
        y: 200.0,
        w: 40.0,
        h: 20.0,
    };
    let kept = Box2 {
        x: 220.0,
        y: 130.0,
        w: 20.0,
        h: 40.0,
    };
    assert_eq!(geom.from_view().apply_box(shown), kept);

    let out = run(&p, "place", Some("100,200"), ctx(&d)).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    d.with(|d| {
        let square = d.state().marks_on(page).next().unwrap().clone();
        assert_eq!(square.kind, AnnotKind::Square { rect: kept });
        // It reads upright as the page shows.
        assert_eq!(square.turn, Rotation::R90);
        let st = d.state();
        let both = st.bookmark(st.outline_roots()[0]).unwrap();
        let at = DestView::Xyz {
            left: Some(220.0),
            top: Some(130.0),
            zoom: None,
        };
        assert_eq!((both.page(), both.view()), (Some(page), Some(&at)));
    });
    let back = run(&p, "where", None, ctx(&d)).unwrap_err();
    assert_eq!(
        back,
        PluginError::Failed("square 100,200 40x20; both Some(220.0),Some(130.0)".into())
    );
    // A new position, as the bookmark's own page shows.
    let out = run(&p, "reposition", Some("150,250"), ctx(&d)).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    let back = run(&p, "where", None, ctx(&d)).unwrap_err();
    assert_eq!(
        back,
        PluginError::Failed("square 100,200 40x20; both Some(270.0),Some(180.0)".into())
    );

    // Text, which the text service hands over in PageSpace, is found
    // where it shows: where the fake engine drew it, on the turned page as
    // on the plain one.
    let ui = Arc::new(FakeUi::default());
    run(&p, "panel", Some(r"\d{4}"), with_ui(&d, &ui)).unwrap();
    let panels = ui.panels.lock().unwrap();
    let details: Vec<&str> = panels[0].1.iter().map(|r| r.detail.as_str()).collect();
    assert_eq!(details, ["5,10", "5,10"]);
}

/// A region event is PageSpace, as sheaf keeps it.
#[test]
fn a_region_arrives_in_page_space() {
    let p = load("frame-region", Limits::default());
    let d = Shared::of(turned_doc());
    let page = d.with(|d| d.page(0).unwrap());
    let event = Call::Event(DocEvent::Region {
        page,
        rect: Box2 {
            x: 220.0,
            y: 130.0,
            w: 20.0,
            h: 40.0,
        },
    });
    block_on(p.call(event, ctx(&d))).unwrap().unwrap();
    assert_eq!(
        answer(&p, "events", None),
        format!("region:{}@220,130 20x40", page.0)
    );
}

/// `page-info` is a page's turn as it shows, file's and sheaf's
/// together, its label, and boxes whose view matrices give the size it
/// shows.
#[test]
fn page_info_is_the_turn_shown_the_size_shown_and_the_label() {
    let p = load("frame-pages", Limits::default());
    let d = Shared::of(turned_doc());
    assert_eq!(
        run(&p, "pages", None, ctx(&d)).unwrap_err(),
        PluginError::Failed("90 740x580 1; 0 612x792 2".into())
    );
    d.with(|d| {
        let ids = [d.page(0).unwrap(), d.page(1).unwrap()];
        d.commit_ops(vec![fixture::rotate(&ids, Turn::Quarter)], "Rotate")
            .unwrap();
    });
    assert_eq!(
        run(&p, "pages", None, ctx(&d)).unwrap_err(),
        PluginError::Failed("180 580x740 1; 90 792x612 2".into())
    );
}

/// On a page the file turns, a match's bookmark lands a little above it as
/// the page shows, keeping the viewer's place across: PageSpace's x is
/// what runs down the page.
#[test]
fn bookmark_from_pattern_lands_above_matches_as_turned_pages_show() {
    let p = bundled(
        "bookmark-from-pattern",
        "pattern-turned",
        LoadOptions::default(),
    );
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Invoice 101\nfor services\nInvoice 103",
        "Invoice 102",
    ]));
    let d = Shared::new(turned_doc(), "bookmark-from-pattern");
    let all: Vec<_> = d.with(|d| d.state().order().iter().copied().collect());
    let ui = Arc::new(FakeUi::default());
    let text_answer = |id: &str, t: &str| FieldAnswer {
        id: id.into(),
        value: FieldValue::Text(t.into()),
    };
    *ui.answer.lock().unwrap() = Some(vec![
        FieldAnswer {
            id: "source".into(),
            value: FieldValue::Choice(0),
        },
        text_answer("regex", r"Invoice \d+"),
        text_answer("template", "{0}"),
        FieldAnswer {
            id: "placement".into(),
            value: FieldValue::Choice(0),
        },
        FieldAnswer {
            id: "pages".into(),
            value: FieldValue::PageRange(all),
        },
    ]);
    let c = CallContext {
        doc: Some(d.context("invoices.pdf", text)),
        ui: Some(ui.clone()),
        ..CallContext::default()
    };
    let out = run(&p, "bookmark", None, c).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    // Where each lands as its page shows: the shown x left to the viewer.
    type Shown = (Option<f32>, Option<f32>);
    let got: Vec<(String, Option<usize>, Shown)> = d.with(|d| {
        let st = d.state();
        (st.bookmarks_preorder().into_iter())
            .map(|(_, _, b)| {
                let t = b.page().unwrap();
                let Some(DestView::Xyz { left, top, .. }) = b.view() else {
                    panic!("{:?}", b.view())
                };
                let shown = d.geom(t).to_view().apply_axes(*left, *top);
                (b.title.to_string(), d.index_of(t), shown)
            })
            .collect()
    });
    assert_eq!(
        got,
        [
            ("Invoice 101".to_string(), Some(0), (None, Some(0.0))),
            ("Invoice 103".to_string(), Some(0), (None, Some(198.0))),
            ("Invoice 102".to_string(), Some(1), (None, Some(0.0))),
        ]
    );
}

/// On a page the file turns, a connector goes just right of its amount as
/// the page shows, as it does on a plain one.
#[test]
fn amount_crossref_links_beside_amounts_as_their_pages_show() {
    let p = bundled("amount-crossref", "crossref-turned", LoadOptions::default());
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Total 12,345.67",
        "Form 1040 line 8 $12,345.67",
    ]));
    let d = Shared::new(turned_doc(), "amount-crossref");
    let ui = Arc::new(FakeUi::default());
    let c = |d: &Shared| CallContext {
        doc: Some(d.context("binder.pdf", text.clone())),
        ui: Some(ui.clone()),
        ..CallContext::default()
    };
    let all: Vec<_> = d.with(|d| d.state().order().iter().copied().collect());
    *ui.answer.lock().unwrap() = crossref_answers(all);
    run(&p, "find", None, c(&d)).unwrap();
    let out = run(&p, "link-all", None, c(&d)).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    let shown: Vec<Box2> = d.with(|d| {
        (d.state().order().iter())
            .flat_map(|&page| {
                let to_view = d.geom(page).to_view();
                (d.state().marks_on(page))
                    .map(|a| match &a.kind {
                        AnnotKind::Connector { rect, .. } => to_view.apply_box(*rect),
                        k => panic!("{k:?}"),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    });
    // "12,345.67" is 9 points wide from 6 across, and "$12,345.67" 10 from
    // 17, both 10 down and 10 tall: each connector is 3 points right, its
    // middle level with the amount's.
    let square = |x: f32| Box2 {
        x,
        y: 4.0,
        w: 22.0,
        h: 22.0,
    };
    assert_eq!(shown, [square(18.0), square(30.0)]);
}

/// Pairs found in a document aren't linked in the same file opened again
/// (M10.5 §14.3): page ids name pages of one instance only, and the new
/// one's may be the same numbers. It says to find again, and changes
/// nothing.
#[test]
fn amount_crossref_refuses_cached_ids_after_a_reopen() {
    let p = bundled("amount-crossref", "crossref-reopen", LoadOptions::default());
    let text: Arc<dyn TextService> = Arc::new(FakeText::pages(vec![
        "Total 12,345.67",
        "Form 1040 line 8 $12,345.67",
    ]));
    let bytes = sheaf_doc::forms::fixture::Form::new(2).bytes();
    let first = Shared::new(opened(&bytes, "binder.pdf"), "amount-crossref");
    let again = Shared::new(opened(&bytes, "binder.pdf"), "amount-crossref");
    let ids = |d: &Shared| d.with(|d| d.state().order().iter().copied().collect::<Vec<_>>());
    assert_eq!(
        ids(&first),
        ids(&again),
        "the same ids, of another instance"
    );
    let ui = Arc::new(FakeUi::default());
    let c = |d: &Shared| CallContext {
        doc: Some(d.context("binder.pdf", text.clone())),
        ui: Some(ui.clone()),
        ..CallContext::default()
    };
    *ui.answer.lock().unwrap() = crossref_answers(ids(&first));
    run(&p, "find", None, c(&first)).unwrap();
    assert_eq!(ui.told(), "1 matching pair — see Amount Matches");
    let versions = again.with(|d| d.timeline().len());
    assert_eq!(
        run(&p, "link-all", None, c(&again)).unwrap_err(),
        PluginError::Failed("the document was opened again; find again".into())
    );
    assert_eq!(again.with(|d| d.timeline().len()), versions);
    // In the document they were found in, they still link.
    let out = run(&p, "link-all", None, c(&first)).unwrap();
    assert!(first.land(Ok(out)).failed.is_none());
    assert_eq!(
        first.with(|d| d.undo_label().map(String::from)),
        Some("Link 1 amount".into())
    );
}

/// What sheaf answered a fixture's submit and the fixture told back: why
/// it failed.
fn failure(told: &str) -> Failure {
    serde_json::from_str(told).unwrap_or_else(|e| panic!("{told}: {e}"))
}

/// One submit makes a note and a reply to it, the reply naming the note by
/// its local; a second changes the note by the id the first answered with
/// (M10.5 §14.3). They land under those ids, as one undo step called as
/// the first submit.
#[test]
fn a_submit_s_ids_are_what_its_edits_keep() {
    let p = load("thread", Limits::default());
    let d = Shared::of(doc(1));
    let (page, versions) = d.with(|d| (d.page(0).unwrap(), d.timeline().len()));
    let ui = Arc::new(FakeUi::default());
    let out = run(&p, "thread", None, with_ui(&d, &ui)).unwrap();
    assert_eq!(out.label.as_deref(), Some("Thread"));
    let landed = d.land(Ok(out));
    assert!(landed.failed.is_none(), "{landed:?}");
    d.with(|d| {
        let marks: Vec<_> = d.state().marks_on(page).cloned().collect();
        let ids: Vec<String> = marks.iter().map(|a| a.id.0.to_string()).collect();
        assert_eq!(ids.join(" "), ui.told(), "the ids it was told");
        let [note, reply] = &marks[..] else {
            panic!("{marks:?}")
        };
        assert_eq!(note.contents, "Tie out to the K-1");
        assert_eq!(
            reply.kind,
            AnnotKind::Reply {
                parent: note.name()
            }
        );
        assert_eq!(reply.contents, "Tied");
        assert_eq!(
            (d.timeline().len(), d.undo_label()),
            (versions + 1, Some("Thread"))
        );
    });
}

/// An envelope a plugin labels itself is invalid: its submit says what
/// its edits are called. Nothing is lowered.
#[test]
fn an_envelope_with_a_label_of_its_own_is_invalid() {
    let p = load("labelled", Limits::default());
    let d = Shared::of(doc(1));
    let ui = Arc::new(FakeUi::default());
    let out = run(&p, "labelled", None, with_ui(&d, &ui)).unwrap();
    let f = failure(&ui.told());
    assert_eq!(f.kind, FailureKind::Invalid, "{f:?}");
    assert!(f.message.contains("label"), "{f:?}");
    assert!(out.tx.is_none() && out.label.is_none(), "{out:?}");
}

/// A plugin that crashes after two submits leaves nothing: no version, and
/// nothing to redo (M10.5 §14.3).
#[test]
fn a_crash_after_two_submits_leaves_no_version() {
    let p = load("two-then-trap", Limits::default());
    let d = Shared::of(doc(1));
    let (at, versions) = d.with(|d| (d.current_version(), d.timeline().len()));
    let result = run(&p, "two-then-trap", None, ctx(&d));
    assert!(matches!(result, Err(PluginError::Trapped(_))), "{result:?}");
    let landed = d.land(result);
    assert!(landed.failed.is_none() && !landed.kept, "{landed:?}");
    d.with(|d| {
        assert_eq!((d.current_version(), d.timeline().len()), (at, versions));
        assert!(!d.can_redo());
        assert!(top_level(d).is_empty());
    });
}

/// One that crashes after a commit takes the commit's version with it
/// (M10.5 §8.4): the document is back at the call's base, with nothing to
/// redo to the version, which History no longer shows.
#[test]
fn a_crash_after_a_commit_takes_its_version_away() {
    let p = load("commit-then-trap", Limits::default());
    let d = Shared::of(doc(1));
    let (at, versions) = d.with(|d| (d.current_version(), d.timeline().len()));
    let ui = Arc::new(FakeUi::default());
    let result = run(&p, "commit-then-trap", None, with_ui(&d, &ui));
    assert!(matches!(result, Err(PluginError::Trapped(_))), "{result:?}");
    let told = ui.told();
    let committed: u64 = told.strip_prefix("committed ").unwrap().parse().unwrap();
    d.with(|d| {
        assert_eq!(
            d.current_version().0,
            committed,
            "it landed while the call ran"
        );
        assert_eq!(top_level(d), ["one"]);
    });
    let landed = d.land(result);
    assert!(landed.failed.is_none() && !landed.kept, "{landed:?}");
    d.with(|d| {
        assert_eq!((d.current_version(), d.timeline().len()), (at, versions));
        assert!(!d.can_redo(), "nothing to redo to it");
        let mut shown =
            (d.timeline().into_iter()).flat_map(|v| [v.id].into_iter().chain(v.branches));
        assert!(shown.all(|v| v.0 != committed), "History shows it");
        assert!(top_level(d).is_empty());
    });
}

/// One that crashes after the user edited since its commit can't take its
/// version away, the user's edit being made on it: a version of its own
/// takes back the plugin's edits, and the user's stay (M10.5 §8.4).
#[test]
fn a_crash_after_the_user_edits_takes_back_only_the_plugin_s_edits() {
    let p = load("commit-wait-trap", Limits::default());
    let d = Shared::of(doc(2));
    let (first, second) = d.with(|d| (d.page(0).unwrap(), d.page(1).unwrap()));
    let versions = d.with(|d| d.timeline().len());
    let ui = FakeUi::editing(&d, move |d| {
        d.commit_ops(vec![fixture::add(second, &square())], "Mine")
            .unwrap();
    });
    let result = run(&p, "commit-wait-trap", None, with_ui(&d, &ui));
    assert!(matches!(result, Err(PluginError::Trapped(_))), "{result:?}");
    let landed = d.land(result);
    assert!(landed.failed.is_none() && !landed.kept, "{landed:?}");
    d.with(|d| {
        assert_eq!(
            d.state().marks_on(first).count(),
            0,
            "the plugin's, taken back"
        );
        let mine: Vec<&str> = (d.state().marks_on(second))
            .map(|a| a.author.as_str())
            .collect();
        assert_eq!(mine, ["Greg"]);
        assert_eq!(
            d.timeline().len(),
            versions + 3,
            "its, the user's, and the revert"
        );
    });
}

/// Five or seven pages, with a tape of the user's on page 3.
fn taped(pages: u32) -> (Shared, PageId) {
    let d = Shared::of(doc(pages));
    let third = d.with(|d| d.page(2).unwrap());
    let rect = Box2 {
        x: 300.0,
        y: 300.0,
        w: 80.0,
        h: 120.0,
    };
    let tape = sheaf_doc::Annot {
        kind: AnnotKind::Tape {
            rect,
            size: 10.0,
            tape: Default::default(),
            look: Default::default(),
        },
        ..square()
    };
    d.with(|d| d.commit_ops(vec![fixture::add(third, &tape)], "Tape"))
        .unwrap();
    (d, third)
}

/// A call's edits land only while what it read holds (M10.5 §8.5), the
/// subtotal case: it counts page 3's tapes, the user changes one while it
/// waits, and the total it then adds on page 5 doesn't land. It's told
/// why, and the document is as the user left it.
#[test]
fn a_total_of_tapes_changed_meanwhile_doesnt_land() {
    let p = load("stale", Limits::default());
    let (d, third) = taped(5);
    let fifth = d.with(|d| d.page(4).unwrap());
    let ui = FakeUi::editing(&d, move |d| {
        let mut tape = d.state().marks_on(third).next().unwrap().clone();
        tape.contents = "Rechecked".into();
        d.commit_ops(vec![fixture::update(&tape)], "Recheck tape")
            .unwrap();
    });
    let out = run(&p, "subtotal", None, with_ui(&d, &ui)).unwrap();
    let (at, versions) = d.with(|d| (d.current_version(), d.timeline().len()));
    let landed = d.land(Ok(out));
    let Some((label, e @ EditError::Conflict { .. })) = &landed.failed else {
        panic!("{landed:?}")
    };
    assert_eq!(label, "Total");
    assert_eq!(e.to_string(), "“Recheck tape” changed the same data");
    d.with(|d| {
        assert_eq!(d.state().marks_on(fifth).count(), 0);
        assert_eq!((d.current_version(), d.timeline().len()), (at, versions));
    });
    assert_eq!(
        sheaf_plugin::follow_ups(&landed),
        [DocEvent::EditRefused(e.to_string())]
    );
}

/// An edit of the user's to a page the call didn't read doesn't stop it:
/// its ops are made again on the document as it is, and both edits land,
/// the total under the id the submit answered with.
#[test]
fn a_total_lands_beside_an_edit_to_a_page_it_didn_t_read() {
    let p = load("fresh", Limits::default());
    let (d, _) = taped(7);
    let (fifth, seventh) = d.with(|d| (d.page(4).unwrap(), d.page(6).unwrap()));
    let ui = FakeUi::editing(&d, move |d| {
        d.commit_ops(vec![fixture::add(seventh, &square())], "Mine")
            .unwrap();
    });
    let out = run(&p, "subtotal", None, with_ui(&d, &ui)).unwrap();
    let landed = d.land(Ok(out));
    assert!(landed.failed.is_none(), "{landed:?}");
    d.with(|d| {
        let total: Vec<_> = d.state().marks_on(fifth).collect();
        let [total] = &total[..] else {
            panic!("{total:?}")
        };
        assert_eq!(total.id.0.to_string(), ui.told(), "the id it was told");
        assert_eq!(total.contents, "1 tapes");
        assert_eq!(d.state().marks_on(seventh).count(), 1);
        assert_eq!(d.undo_label(), Some("Total"));
    });
}

/// What a plugin may not do is refused, and nothing is lowered (M10.5
/// §6.7, §14.3): inserting pages, which only the user does; deleting
/// another tool's comment, and flattening, which need the user's approval;
/// and naming a blob by its id, which a plugin uploads instead.
#[test]
fn what_a_plugin_may_not_do_is_refused() {
    let p = load("refused", Limits::default());
    let d = Shared::of(commented());
    let primary = d.with(|d| d.primary().id().stem());
    for (cmd, args, cap) in [
        ("insert-pages", None, Cap::User),
        ("delete-comment", None, Cap::Approval),
        ("flatten", None, Cap::Approval),
        ("raw-blob", Some(primary.as_str()), Cap::User),
    ] {
        let ui = Arc::new(FakeUi::default());
        let out = run(&p, cmd, args, with_ui(&d, &ui)).unwrap();
        let refused = Failure {
            kind: FailureKind::Refused,
            message: cap.to_string(),
        };
        assert_eq!(failure(&ui.told()), refused, "{cmd}");
        assert!(out.tx.is_none(), "{cmd}: {out:?}");
    }
}

/// A plugin learns who the user is only when they grant it `identity`
/// (M10.5 §6.8). Not granted, it's told authors as roles, `<<name>>` in
/// its own mark stays as it wrote it, and no one's name is in anything it
/// reads; granted, it's told names, and `<<name>>` is filled in.
#[test]
fn a_plugin_learns_the_user_s_name_only_when_granted() {
    let me = Identity {
        person: "Zebedee Quist".into(),
        initials: "ZQ".into(),
        ..Identity::default()
    };
    for (identity, said) in [
        (
            false,
            "other: From review; you: ; plugin test-plugin: Seen by <<name>>",
        ),
        (
            true,
            "Pat Reviewer: From review; Zebedee Quist: ; plugin test-plugin: Seen by Zebedee Quist",
        ),
    ] {
        let opts = LoadOptions {
            grants: Grants {
                identity,
                ..Grants::default()
            },
            ..LoadOptions::default()
        };
        let p = load_with(&format!("authors-{identity}"), opts);
        let mut doc = commented();
        doc.set_identity(me.clone());
        let page = doc.page(0).unwrap();
        // The user's, by whoever the document says they are.
        let mine = sheaf_doc::Annot {
            author: String::new(),
            ..square()
        };
        doc.commit_ops(vec![fixture::add(page, &mine)], "Square")
            .unwrap();
        let d = Shared::of(doc);
        let ui = Arc::new(FakeUi::default());
        let out = run(&p, "authors", None, with_ui(&d, &ui)).unwrap();
        let told = ui.told();
        assert_eq!(told, said);
        if !identity {
            for name in ["Zebedee", "Quist", "ZQ", "Pat"] {
                assert!(!told.contains(name), "{name} in {told}");
            }
        }
        assert!(d.land(Ok(out)).failed.is_none());
        d.with(|d| {
            let note = d.state().marks_on(page).last().unwrap();
            assert_eq!(note.author, "test-plugin");
            assert_eq!(Some(note.contents.as_str()), said.rsplit(": ").next());
        });
    }
}

/// An image a plugin uploads becomes a page stamp of it (M10.5 §14.3): the
/// mark holds the stamp's page, a blob of the document's store, and draws
/// it in the file it's saved to; the handle names it again after the
/// call's commit, the two one undo step.
#[test]
fn an_uploaded_image_becomes_a_page_stamp() {
    let p = load("upload", Limits::default());
    let d = Shared::of(doc(1));
    let (page, versions) = d.with(|d| (d.page(0).unwrap(), d.timeline().len()));
    let out = run(&p, "stamp-upload", None, ctx(&d)).unwrap();
    let landed = d.land(Ok(out));
    assert!(landed.failed.is_none(), "{landed:?}");
    let saved = d.with(|d| {
        let stamps: Vec<_> = d.state().mark_records_on(page).cloned().collect();
        assert_eq!(stamps.len(), 2);
        for m in &stamps {
            let kind = &m.annot.kind;
            assert!(matches!(kind, AnnotKind::PageStamp { .. }), "{kind:?}");
            let Some(FormRef::Blob(form)) = &m.stamp else {
                panic!("{:?}", m.stamp)
            };
            assert!(d.store().get(form.id()).is_some(), "in the store");
        }
        assert_eq!(stamps[0].stamp, stamps[1].stamp, "one upload");
        assert_eq!(
            (d.timeline().len(), d.undo_label()),
            (versions + 1, Some("Stamp"))
        );
        sheaf_doc::save::rewrite(d).unwrap()
    });
    // Each stamp's appearance draws the stamp's page.
    let pdf = lopdf::Document::load_mem(&saved).unwrap();
    let page = *pdf.get_pages().values().next().unwrap();
    let annots = pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap();
    let annots = annots.as_array().unwrap();
    assert_eq!(annots.len(), 2);
    for a in annots {
        let a = pdf.get_dictionary(a.as_reference().unwrap()).unwrap();
        let ap = a.get(b"AP").unwrap().as_dict().unwrap();
        let ap = pdf.get_object(ap.get(b"N").unwrap().as_reference().unwrap());
        let ap = ap.unwrap().as_stream().unwrap();
        let content = (ap.decompressed_content()).unwrap_or_else(|_| ap.content.clone());
        assert!(String::from_utf8_lossy(&content).contains(" Do"));
    }
}

/// A call reads its own edits (M10.5 §14.3), deciding by nothing they
/// wrote: the square it submitted is in the page's annotations it reads
/// next, and neither that list nor the square is a read it decided by.
#[test]
fn a_call_reads_its_own_edits_and_doesn_t_decide_by_them() {
    let p = load("read-back", Limits::default());
    let d = Shared::of(doc(1));
    let page = d.with(|d| d.page(0).unwrap());
    let ui = Arc::new(FakeUi::default());
    let out = run(&p, "read-back", None, with_ui(&d, &ui)).unwrap();
    let told = ui.told();
    let made = (told.strip_prefix("made ")).and_then(|t| t.split(' ').next());
    let made: u64 = made.unwrap().parse().unwrap();
    assert_eq!(told, format!("made {made} shown {made}"));
    let read = decided(&out);
    assert!(read.contains(&Key::Order), "{read:?}");
    for written in [Key::PageAnnots(page), Key::Annot(sheaf_doc::AnnotId(made))] {
        assert!(!read.contains(&written), "{written:?} in {read:?}");
    }
}

/// The fixture installed as plugin `id`, which answers each change it's
/// told of as `answer` says: "mark" adds a square on page 1, "commit"
/// commits it at once, and "insist" adds it again when told it was
/// refused.
fn answering(id: &str, answer: &str) -> Plugin {
    let dir = plugin_dir(&format!("{id}-{answer}"));
    let toml = dir.join("sheaf-plugin.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    let ours = text.replacen("id = \"test-plugin\"", &format!("id = \"{id}\""), 1);
    std::fs::write(&toml, ours).unwrap();
    let manifest = Manifest::load(&dir).unwrap();
    assert_eq!(manifest.id, id);
    let p = Runtime::new(Limits::default())
        .unwrap()
        .load(dir, manifest, LoadOptions::default());
    let told = format!("answer={answer}");
    run(&p, "store", Some(&told), CallContext::default()).unwrap();
    p
}

/// `p` told of `event` on `d`.
fn tell(p: &Plugin, d: &Shared, event: DocEvent) -> Result<CallOutput, PluginError> {
    block_on(p.call(Call::Event(event), ctx(d))).expect("reply")
}

/// Answers to answers stop (M10.5 §14.3): two plugins each mark page 1 on
/// every change they're told of. A answers the user's change, one deeper;
/// B's answer to A's is refused, so there's no third version, and B is
/// told why, which it keeps. The same when A commits its answer before it
/// returns: that version is as deep.
#[test]
fn answers_to_answers_stop() {
    for mode in ["mark", "commit"] {
        let (a, b) = (answering("echo-a", mode), answering("echo-b", mode));
        let d = Shared::of(doc(1));
        let page = d.with(|d| d.page(0).unwrap());
        let users = d.with(|d| d.commit_ops(vec![fixture::add(page, &square())], "Square"));
        let instance = d.with(|d| d.instance());
        let changed = |ch: &ChangeSet| DocEvent::Changed(ChangeInfo::of(ch, instance));
        let versions = d.with(|d| d.timeline().len());
        d.calling("echo-a");
        let out = tell(&a, &d, changed(&users.unwrap().change)).unwrap();
        assert_eq!(out.depth, 1, "{mode}");
        assert!(d.land(Ok(out)).failed.is_none(), "{mode}");
        let made = d.made();
        let [answered] = &made[..] else {
            panic!("{mode}: {made:?}")
        };
        assert_eq!(answered.depth, 1, "{mode}");
        d.with(|d| {
            assert_eq!(d.timeline().len(), versions + 1, "{mode}");
            assert_eq!(d.state().marks_on(page).count(), 2, "{mode}");
        });
        d.calling("echo-b");
        let out = tell(&b, &d, changed(answered)).unwrap();
        let why = "it answers a plugin's answer to a change, and those stop there";
        assert_eq!(out.refused, [why], "{mode}");
        assert!(out.tx.is_none(), "{mode}: {out:?}");
        let landed = d.land(Ok(out));
        assert!(d.made().is_empty(), "{mode}");
        d.with(|d| {
            assert_eq!(d.timeline().len(), versions + 1, "{mode}: no third");
            assert_eq!(d.state().marks_on(page).count(), 2, "{mode}");
        });
        let told = sheaf_plugin::follow_ups(&landed);
        assert_eq!(told, [DocEvent::EditRefused(why.into())], "{mode}");
        for e in told {
            tell(&b, &d, e).unwrap();
        }
        let (from, to) = (answered.from.0, answered.to.0);
        assert_eq!(
            answer(&b, "events", None),
            format!("changed:{from}>{to} annots 1,refused:{why}"),
            "{mode}"
        );
    }
}

/// A plugin can't fight the user's undo (M10.5 §14.3): one that marks page
/// 1 on every change is refused when the change is an undo, and is told
/// why, which it keeps. Told, it tries again, and that's refused too, and
/// not told again. Nothing lands, and the undone edit can still be redone.
#[test]
fn an_answer_to_an_undo_is_refused() {
    let p = answering("fighter", "insist");
    let d = Shared::new(doc(1), "fighter");
    let page = d.with(|d| d.page(0).unwrap());
    d.with(|d| d.commit_ops(vec![fixture::add(page, &square())], "Square"))
        .unwrap();
    let undone = d.with(|d| d.undo()).unwrap().unwrap();
    let (at, versions) = d.with(|d| (d.current_version(), d.timeline().len()));
    let instance = d.with(|d| d.instance());
    let out = tell(&p, &d, DocEvent::Changed(ChangeInfo::of(&undone, instance))).unwrap();
    assert!(out.tx.is_none(), "{out:?}");
    let landed = d.land(Ok(out));
    assert_eq!(landed.refused, ["the user undid this"]);
    for e in sheaf_plugin::follow_ups(&landed) {
        let out = tell(&p, &d, e).unwrap();
        assert!(out.tx.is_none() && out.refused.is_empty(), "{out:?}");
        let landed = d.land(Ok(out));
        assert!(sheaf_plugin::follow_ups(&landed).is_empty(), "{landed:?}");
    }
    d.with(|d| {
        assert_eq!((d.current_version(), d.timeline().len()), (at, versions));
        assert_eq!(d.state().marks_on(page).count(), 0);
        assert_eq!(d.redo_label(), Some("Square"), "still there to redo");
    });
    let (from, to) = (undone.from.0, undone.to.0);
    assert_eq!(
        answer(&p, "events", None),
        format!("changed:{from}>{to} annots 1,refused:the user undid this")
    );
}

/// A document whose commits take the UI thread a while to land.
struct Slow(Shared, Duration);

impl Committer for Slow {
    fn commit(&self, tx: Tx, label: &str, depth: u8) -> Result<Committed, EditError> {
        std::thread::sleep(self.1);
        self.0.commit(tx, label, depth)
    }
}

/// A call waiting while its commit lands isn't charged for the time: the
/// UI thread takes longer than the call's whole time limit.
#[test]
fn waiting_on_a_commit_is_free() {
    let limits = Limits {
        call_timeout: Duration::from_millis(200),
        ..Limits::default()
    };
    let p = load("slow-commit", limits);
    let d = Shared::of(doc(1));
    let page = d.with(|d| d.page(0).unwrap());
    let slow = Slow(d.clone(), Duration::from_millis(500));
    let doc = DocContext {
        committer: Some(Arc::new(slow)),
        ..d.context("return.pdf", Arc::new(FakeText::forms()))
    };
    let call = CallContext {
        doc: Some(doc),
        ..CallContext::default()
    };
    let out = run(&p, "stamp-upload", None, call).unwrap();
    assert!(d.land(Ok(out)).failed.is_none());
    d.with(|d| assert_eq!(d.state().marks_on(page).count(), 2));
}
