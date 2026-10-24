//! Helpers the forms tests share: fixtures, a crop, saving both ways, and
//! what a saved file holds. A test that saves both ways is a case taking
//! its [`How`], run twice by `sheaf_doc::both_ways!`: by a Rewrite, and by
//! an Append. And what the realization, text and blank-page tests share: a
//! file of pages saying "page i", opened as the app opens it, views over a
//! pool of their own, and an OCR engine that reads nothing. And a page
//! drawn as M5's separators are found, for the codes tests and the gate.

#![allow(dead_code)]
// The tests' scratch files go in the temp directory, which clippy.toml
// bans outside tests; each test that includes this is a crate of its own.
#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::Context;
use image::GrayImage;
use lopdf::xref::XrefType;
use lopdf::{Document as Pdf, LazyPdf, Object, ObjectId, Stream, dictionary};
use sheaf_doc::export::{Export, ExportFrom, Out};
use sheaf_doc::forms::corpus::Corpus;
use sheaf_doc::forms::fixture::{Form, Rect, source};
use sheaf_doc::forms::{FileForm, Finding, Report, Value, check};
pub use sheaf_doc::materialize::{How, StrategyKind};
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{Actor, CommitOpts, Document, FixedCount, PageId, Src, Stale, Store};
use sheaf_edit::{Margins, Rotation};
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_ocr::{EngineInfo, EngineKind, InputKind, OcrEngine, OcrImage, OcrWord};
use sheaf_ops::views::Views;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Draw, Job, PageIndex, Renderer};

/// Half of a row's inside, in pixels.
pub const HALF_A_ROW: usize = 196 * 26 / 2;

/// A new, empty directory for test `name` to write its files to: in the
/// temp directory, or, in the process [`own_dirs`] runs a test in, beside
/// the directories it watches.
pub fn scratch(name: &str) -> PathBuf {
    let base = match std::env::var_os(OWN_DIRS) {
        Some(done) => Path::new(&done).with_file_name("scratch"),
        None => std::env::temp_dir(),
    };
    let dir = base.join(format!("sheaf-forms-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Set in the process [`own_dirs`] runs a test in: the file it writes
/// once the test has run.
const OWN_DIRS: &str = "SHEAF_TEST_OWN_DIRS";

/// The variables that name where a process may put temp and cache files:
/// the temp directory (`TMPDIR` on Unix, `TMP` and `TEMP` on Windows), the
/// cache directory on Linux, and the home directory, under which macOS
/// keeps caches.
const DIRS: [&str; 5] = ["TMPDIR", "TMP", "TEMP", "XDG_CACHE_HOME", "HOME"];

/// `test`, which runs `body`, run in a process of its own (the binary run
/// again for `test` alone) whose temp, cache and home directories
/// ([`DIRS`]) are new and empty, and must still be empty after: sheaf
/// writes no file of a document's but the one it's asked to (M10.5 §3.9),
/// and a directory the binary's tests share can't say what one test
/// wrote. The test's own files go in its [`scratch`] directory, outside
/// them. In that process, `body` itself.
pub fn own_dirs(test: &str, body: impl FnOnce()) {
    if let Some(done) = std::env::var_os(OWN_DIRS) {
        body();
        std::fs::write(done, "").unwrap();
        return;
    }
    let dir = std::env::temp_dir().join(format!("sheaf-own-dirs-{test}-{}", std::process::id()));
    let done = dir.join("done");
    let _ = std::fs::remove_dir_all(&dir);
    let mut run = Command::new(std::env::current_exe().unwrap());
    run.args([test, "--exact", "--nocapture", "--quiet"])
        .env(OWN_DIRS, &done);
    for var in DIRS {
        let own = dir.join(var.to_lowercase());
        std::fs::create_dir_all(&own).unwrap();
        run.env(var, own);
    }
    let out = (run.output()).unwrap_or_else(|e| panic!("running {test} in its own process: {e}"));
    let said = || {
        let [out, err] = [&out.stdout, &out.stderr].map(|b| String::from_utf8_lossy(b));
        format!("{out}{err}")
    };
    assert!(out.status.success(), "{test}: {}\n{}", out.status, said());
    assert!(
        done.exists(),
        "{test} ran nothing in its own process: does `{test} --exact` still name it?"
    );
    for var in DIRS {
        let own = dir.join(var.to_lowercase());
        let written: Vec<PathBuf> = (std::fs::read_dir(&own).unwrap())
            .map(|e| e.unwrap().path())
            .collect();
        assert!(written.is_empty(), "{test} wrote to its {var}: {written:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `path` opened as the app opens it.
pub fn open(path: &Path) -> Document {
    sheaf_doc::open::file(path, None).unwrap().doc
}

pub fn write(dir: &Path, name: &str, form: &mut Form) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, form.bytes()).unwrap();
    path
}

/// Row `i` of a page, 30 pt high, from the top.
pub fn row(i: usize) -> Rect {
    let top = 740.0 - 50.0 * i as f32;
    [100.0, top - 30.0, 300.0, top]
}

/// A text field filled with `value`, whose appearance is a solid blue box.
pub fn blue(form: &mut Form, name: &str, page: usize, i: usize, value: &str) -> ObjectId {
    let id = form.text(name, page, row(i));
    form.dict(id).set("V", Object::string_literal(value));
    let ap = form.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 200.into(), 30.into()],
        },
        format!("0 0 1 rg 0 0 200 30 re f BT /Helv 10 Tf 2 10 Td ({value}) Tj ET").into_bytes(),
    ));
    form.dict(id).set("AP", dictionary! { "N" => ap });
    id
}

/// Two pages: on the first, text fields `a` (blue) and `c` and a check
/// box; on the second, `b` (blue).
pub fn two_page_form() -> Form {
    let mut f = Form::new(2);
    blue(&mut f, "a", 0, 0, "Alpha");
    f.text("c", 0, row(1));
    f.check_box("box", 0, row(2), true);
    blue(&mut f, "b", 1, 0, "Beta");
    f
}

/// [`two_page_form`] with a header Acrobat drew on each page after its
/// content: a form whose `/PieceInfo` says so, drawing "ACROBATHEADER"
/// above the fields.
pub fn acrobat_headed_form() -> Form {
    let mut f = two_page_form();
    let piece = dictionary! { "ADBE_CompoundType" => dictionary! {
        "DocSettings" => dictionary! {}, "Private" => "Header",
    } };
    let helv = f.helv;
    let form = f.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "PieceInfo" => piece,
            "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => helv } },
        },
        b"BT /F1 12 Tf 2 5 Td (ACROBATHEADER) Tj ET".to_vec(),
    ));
    let header = f.pdf.add_object(Stream::new(
        dictionary! {},
        b"/Artifact <</Type /Pagination /Subtype /Header>> BDC q 1 0 0 1 72 760 cm /Xi0 Do Q \
          EMC"
        .to_vec(),
    ));
    for page in f.pages.clone() {
        let dict = f.dict(page);
        let body = dict.get(b"Contents").unwrap().clone();
        dict.set("Contents", vec![body, header.into()]);
        let resources = dict.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
        resources.set("XObject", dictionary! { "Xi0" => form });
    }
    f
}

/// Blue pixels inside row `i` of a 612-pixel-wide render of a page whose
/// shown area's top is at `top` in user space.
pub fn blue_in(r: &Raster, i: usize, top: f32) -> usize {
    let [x0, y0, x1, y1] = row(i).map(|v| v as usize);
    let top = top as usize;
    let mut n = 0;
    for y in (top - y1 + 2)..(top - y0 - 2) {
        for x in (x0 + 2)..(x1 - 2) {
            let p = &r.data[(y * r.width as usize + x) * 4..][..3];
            if p[2] > 200 && p[0] < 60 && p[1] < 60 {
                n += 1;
            }
        }
    }
    n
}

/// Whether `needle` is anywhere in the file: in its bytes, in a string
/// (decoded from hex), or in a stream once decoded.
pub fn holds(bytes: &[u8], needle: &[u8]) -> bool {
    let found = |hay: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    if found(bytes) {
        return true;
    }
    // Strings the file writes in hex, and streams, decoded.
    fn any(o: &Object, found: &dyn Fn(&[u8]) -> bool) -> bool {
        match o {
            Object::String(s, _) => found(s),
            Object::Array(a) => a.iter().any(|o| any(o, found)),
            Object::Dictionary(d) => d.iter().any(|(_, o)| any(o, found)),
            Object::Stream(s) => {
                s.dict.iter().any(|(_, o)| any(o, found))
                    || found(
                        &s.decompressed_content()
                            .unwrap_or_else(|_| s.content.clone()),
                    )
            }
            _ => false,
        }
    }
    let pdf = Pdf::load_mem(bytes).unwrap();
    pdf.objects.values().any(|o| any(o, &found))
}

/// The form in `bytes`, read as a file's is when it opens.
pub fn form_in(bytes: &[u8]) -> FileForm {
    sheaf_doc::forms::read(&source(bytes.to_vec()))
}

/// The text fields' values, by full name.
pub fn values(bytes: &[u8]) -> BTreeMap<String, String> {
    form_in(bytes)
        .fields
        .into_iter()
        .filter_map(|f| match f.value {
            Some(Value::Text(t)) => Some((f.name, t.to_string())),
            _ => None,
        })
        .collect()
}

/// The form check on `bytes`, which must be clean.
pub fn clean(bytes: &[u8], what: &str) -> Report {
    let report = check(bytes, None).unwrap();
    assert!(report.is_clean(), "{what}: {:?}", report.findings);
    report
}

/// `doc` saved to `path`, its own file, `how`, whatever the plan would
/// decide: by a Rewrite, or by an Append. What it was saved as, and the
/// file.
pub fn saved(doc: &mut Document, path: &Path, how: How) -> (StrategyKind, Vec<u8>) {
    let report = how.save(doc, path);
    let strategy = report.strategy.expect("the file was written");
    (strategy, std::fs::read(path).unwrap())
}

/// `doc` saved to `path`, its own file, as the save's plan says
/// (`save_with`'s `SaveHow::Planned`, which a session's in-place save
/// runs), a signature or an XFA part's values given up when it asks. What
/// it was saved as, and the file.
pub fn planned(doc: &mut Document, path: &Path) -> (StrategyKind, Vec<u8>) {
    let answers = sheaf_doc::materialize::AskAnswers {
        break_signatures: true,
        drop_xfa_values: true,
        ..Default::default()
    };
    let how = sheaf_doc::materialize::SaveHow::Planned;
    let report =
        sheaf_doc::materialize::save_with(doc, path, how, &answers, &sheaf_ops::NoProgress);
    let strategy = report.unwrap().strategy.expect("the file was written");
    (strategy, std::fs::read(path).unwrap())
}

/// `doc` written by a Rewrite, then saved to `path`, its file, as the
/// save's plan says, which must be a Rewrite too: what each wrote. For a
/// document something's gone from, which strict removal rewrites (M9 bug
/// 14, M10.5 proposal 9).
pub fn rewritten_and_saved(doc: &mut Document, path: &Path) -> [(StrategyKind, Vec<u8>); 2] {
    let written = (StrategyKind::Rewrite, rewritten(doc));
    let saved = planned(doc, path);
    assert_eq!(saved.0, StrategyKind::Rewrite, "something's gone");
    [written, saved]
}

/// The export `make` makes of `doc`'s current version, the user's, put at
/// `path` (replacing a file there), what it asks answered as print answers
/// it: its bytes.
pub fn exported_to(
    doc: &Document,
    make: impl FnOnce(ExportFrom, Out) -> Export,
    path: &Path,
) -> Vec<u8> {
    let out = Out::File {
        path: path.into(),
        overwrite: true,
    };
    let e = make(doc.export_from(Actor::User), out);
    let answers = sheaf_doc::export::PRINT_ANSWERS;
    sheaf_doc::export::run(e, &answers, &sheaf_ops::NoProgress).unwrap();
    std::fs::read(path).unwrap()
}

/// What `doc` writes by a Rewrite: a save's bytes.
pub fn rewritten(doc: &Document) -> Vec<u8> {
    sheaf_doc::save::rewrite(doc).unwrap()
}

/// What `doc` writes by an Append onto its file, whatever a save would
/// plan: a recovery section's shape too.
pub fn appended(doc: &Document) -> Vec<u8> {
    sheaf_doc::materialize::appended(doc).unwrap()
}

/// Whether a save of `doc` to `path`, its file, drops the form's XFA
/// copy: save policy plans it when the form isn't the file's
/// (`forms::rules::plan`).
pub fn drops_xfa(doc: &Document, path: &Path) -> bool {
    use sheaf_doc::materialize::{SaveInput, SaveTransform, plan};
    let input = SaveInput::detached(&doc.snapshot());
    let plan = plan(&input, &doc.save_target(path));
    (plan.transforms.iter()).any(|t| matches!(t, SaveTransform::DropXfa { .. }))
}

/// `pages` of `doc` cropped by `m`, as Document > Crop Pages crops them:
/// one op, in a transaction made on the version the dialog opened on,
/// validated.
pub fn crop(doc: &mut Document, pages: &[PageId], m: Margins) {
    let snap = doc.snapshot();
    let mut tx = Document::tx_at(&snap, doc.lower_cx(Actor::User));
    tx.apply(fx::crop(pages, m), &sheaf_ops::NoProgress)
        .unwrap();
    let opts = CommitOpts::based(snap.version, Stale::Validate, "Crop");
    doc.commit(tx, opts).unwrap();
}

/// What the section of `whole` after its first `base` bytes, the file it
/// was appended to, writes: the objects read from past them, but its
/// cross-reference stream, each with whether it's a page's dictionary.
pub fn appended_objects(base: usize, whole: &[u8]) -> BTreeMap<ObjectId, bool> {
    let limits = &sheaf_doc::source::Limits::DEFAULT.reader;
    let pdf = LazyPdf::open(Arc::<[u8]>::from(whole), None, limits).unwrap();
    let past = |id| pdf.raw_span(id).is_some_and(|span| span.start >= base);
    (pdf.ids().filter(|&id| past(id)))
        .filter_map(|id| {
            let object = pdf.get(id).ok();
            let named = |name: &[u8]| object.and_then(|o| o.type_name().ok()) == Some(name);
            let page = matches!(object, Some(Object::Dictionary(_))) && named(b"Page");
            (!named(b"XRef")).then_some((id, page))
        })
        .collect()
}

/// What `doc` writes `how`.
pub fn written(doc: &Document, how: How) -> Vec<u8> {
    match how {
        How::Rewritten => rewritten(doc),
        How::Appended => appended(doc),
    }
}

/// Object references in the file's structure that lead nowhere: an
/// `/OBJR` whose `/Obj` is null or an object the file doesn't have. None
/// may, once a page is gone: what was there leaves the tree with it.
pub fn dangling_structure(bytes: &[u8]) -> usize {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let objr = |o: &Object| {
        let o = match o {
            Object::Reference(id) => pdf.get_object(*id).ok(),
            o => Some(o),
        };
        o.and_then(|o| o.as_dict().ok())
            .filter(|d| d.get(b"Type").and_then(|t| t.as_name()).ok() == Some(b"OBJR"))
            .is_some_and(|d| match d.get(b"Obj") {
                Ok(Object::Reference(id)) => pdf.get_object(*id).is_err(),
                _ => true,
            })
    };
    pdf.objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Type").and_then(|t| t.as_name()).ok() == Some(b"StructElem"))
        .map(|d| match d.get(b"K") {
            Ok(Object::Array(kids)) => kids.iter().filter(|k| objr(k)).count(),
            Ok(k) => usize::from(objr(k)),
            Err(_) => 0,
        })
        .sum()
}

/// The catalog's `/AcroForm /Fields`, or None with no `/AcroForm`.
pub fn fields_of(bytes: &[u8]) -> Option<Vec<Object>> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let form = pdf.catalog().unwrap().get(b"AcroForm").ok()?;
    let form = pdf.dereference(form).unwrap().1.as_dict().unwrap();
    let fields = pdf.dereference(form.get(b"Fields").unwrap()).unwrap().1;
    Some(fields.as_array().unwrap().clone())
}

/// A corpus form, or None (and the corpus tests skip) without one.
pub fn corpus_form(path: &str) -> Option<Vec<u8>> {
    let corpus = Corpus::or_skip()?;
    let entry = corpus.entry(path).unwrap().expect("in the manifest");
    Some(std::fs::read(corpus.path(&entry)).unwrap())
}

/// `items`, or those of them whose corpus path (`path`) the environment
/// variable `var` names, comma-separated, when it's set: every one it
/// names, or a failure, as it is when it's set and names none (a leftover
/// export mustn't make a whole run check nothing). A narrowed run says so.
pub fn narrowed<T>(var: &str, items: Vec<T>, path: impl Fn(&T) -> &str) -> Vec<T> {
    let Ok(only) = std::env::var(var) else {
        return items;
    };
    let named: BTreeSet<&str> = (only.split(',').map(str::trim))
        .filter(|p| !p.is_empty())
        .collect();
    assert!(!named.is_empty(), "{var} is set but names no file");
    let items: Vec<T> = (items.into_iter())
        .filter(|item| named.contains(path(item)))
        .collect();
    assert_eq!(
        items.len(),
        named.len(),
        "{var} names a file the run doesn't take: {only}"
    );
    eprintln!("narrowed by {var} to {} files", items.len());
    items
}

/// `bytes` with field `name`'s `/V` set to `value` (its appearance as it
/// was), as a fill sheaf can't make yet (slice 9) would leave it.
pub fn filled(bytes: &[u8], name: &str, value: &str) -> Vec<u8> {
    let field = form_in(bytes)
        .fields
        .into_iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field {name}"));
    // In an update, as Reader saves a fill: the original's bytes, and the
    // usage rights' signature over them, stay sound.
    sheaf_doc::forms::fixture::append_revision(bytes, &[field.object], |pdf| {
        pdf.get_dictionary_mut(field.object)
            .unwrap()
            .set("V", Object::string_literal(value));
    })
}

/// Whether `finding` is what a save told to break signatures and drop an
/// XFA part's values may leave: what save policy decides, and asks about
/// first.
pub fn told(finding: &Finding) -> bool {
    matches!(
        finding,
        Finding::StaleSignature { .. }
            | Finding::StaleSigFlags { .. }
            | Finding::XfaMismatch { .. }
            | Finding::EarlierValues { .. }
    )
}

/// What the form check finds in the file at `path` that it didn't in the
/// file it was (`was`; none to compare when its check failed), as
/// [`Report::added_since`] tells it, but what the save may leave
/// (`leaves`, [`told`] where it was told to): an error, when it finds any.
/// A finding the file had, naming fewer now (widgets on no page the save
/// dropped, a missing font fewer widgets name), isn't one.
pub fn nothing_found(
    was: Option<&Report>,
    path: &Path,
    leaves: fn(&Finding) -> bool,
) -> Result<(), String> {
    let Some(was) = was else {
        return Ok(());
    };
    let bytes = std::fs::read(path).unwrap();
    let now = sheaf_doc::forms::check(&bytes, None).map_err(|e| format!("check: {e:#}"))?;
    let added: Vec<_> = (now.added_since(was).into_iter())
        .filter(|f| !leaves(f))
        .collect();
    match added.is_empty() {
        true => Ok(()),
        false => Err(format!("the form check finds {added:?}")),
    }
}

/// What the form check finds in `bytes`.
pub fn findings(bytes: &[u8]) -> Vec<Finding> {
    check(bytes, None).unwrap().findings
}

/// `bytes` opened as the app opens the file `name`, with `password`.
pub fn opened(bytes: Vec<u8>, name: &str, password: Option<&str>) -> Document {
    let store = Store::new();
    let kind = BlobKind::file(Path::new(name), password);
    let primary = store.open(bytes.into(), kind).unwrap();
    ready(store, primary, name).unwrap()
}

/// `primary`, a file of `store`'s, opened as the app opens it.
pub fn ready(store: Arc<Store>, primary: Src, name: &str) -> anyhow::Result<Document> {
    let policy = Box::new(FixedCount(10));
    let mut d = sheaf_doc::open::start(store, primary.clone(), name.into(), None, policy);
    let facts = sheaf_doc::open::finish(&primary, &sheaf_ops::NoProgress);
    d.ready(facts)?;
    Ok(d)
}

/// A file of `n` Letter pages as lopdf writes it in `kind`, each drawing
/// a few shapes and "page i" in Helvetica, which their parent node gives
/// them.
pub fn file(n: usize, kind: XrefType) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let kids: Vec<Object> = (0..n)
        .map(|i| {
            let content = format!(
                "0 0 1 rg 60 600 200 120 re f 1 0 0 rg {} 200 80 80 re f \
                 BT /F1 28 Tf 72 400 Td (page {i}) Tj ET",
                100 + 40 * i
            );
            let content = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            let page = dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
            pdf.add_object(page).into()
        })
        .collect();
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => n as i64,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    pdf.reference_table.cross_reference_type = kind;
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// A document's views, drawn by a pool of the test's own.
pub fn views() -> Views {
    Views::start().unwrap()
}

/// Reads no words, as fast as it's given pages.
pub struct Blind;

impl OcrEngine for Blind {
    fn input(&self) -> InputKind {
        InputKind::Gray
    }

    fn recognize(&mut self, _: OcrImage, _: u32) -> anyhow::Result<Vec<OcrWord>> {
        Ok(Vec::new())
    }

    fn describe(&self) -> EngineInfo {
        EngineInfo {
            kind: EngineKind::Tesseract,
            name: "blind".into(),
            languages: Vec::new(),
        }
    }
}

/// `engine`'s page `page` drawn as M5's separators are found (proposal
/// 15): its content alone, at `dpi`, in gray. Slice 5's `separate::find`
/// draws the same picture through `Views`.
pub fn drawn(engine: &PdfiumEngine, page: u32, dpi: f32) -> anyhow::Result<GrayImage> {
    let size = engine.page_size(PageIndex(page))?;
    let job = Job::Page {
        width: (size.width * dpi / 72.0).round() as u32,
        rotation: Rotation::R0,
    };
    let draw = Draw {
        annots: false,
        forms: false,
    };
    let raster = engine.render_with(PageIndex(page), job, PixelFormat::Rgba, draw)?;
    raster.to_gray().context("a render with its pixels short")
}
