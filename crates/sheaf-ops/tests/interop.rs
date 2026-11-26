//! What other readers make of the files sheaf saves (M10.5 slice 7),
//! ignored, as they need tools CI doesn't have: `qpdf --check` passes on
//! every save and, from slice 9, every export (extracted pages, print with
//! and without markup, Clean Copy, Export Flattened) of plain, encrypted,
//! prefixed and cross-reference-stream files, and a file with bytes
//! before its header appends to what the
//! same file without them does, in qpdf and pdf.js. From slice 8, the old
//! writers write from records: a bookmark that opens a URI, another
//! tool's link and comments come through a reorder by both strategies,
//! and a comment sheaf deleted doesn't.
//!
//! From slice 10 every save's Append half is an Append again, and three
//! Appends in a row, each after the last, pass `qpdf --check` too, as does
//! a delete undone after its save, whose planned Append is a Rewrite for
//! the objects the first save pruned.
//!
//! From slice 16 a page with Acrobat's watermark, given a watermark and a
//! background of sheaf's that replace it, written either way, then opened
//! again and its watermark moved, then both removed, passes `qpdf
//! --check`, and pdf.js loads it and reads each of sheaf's text once,
//! inside its `/Artifact`, and nothing of Acrobat's.
//!
//! From slice 17 so do two pages with Acrobat's header, given a header and
//! footer of sheaf's with page and Bates numbers, which replaces it,
//! written either way; then their pages traded after a save, an Append
//! that numbers them again, each keeping its Bates number; then opened
//! again and the header removed. pdf.js reads each slot once, inside an
//! `/Artifact` of its own, numbered as the file has its pages.
//!
//! From slice 18 so does a page given recognized text and a header,
//! written either way, then opened again and its text cleared, a Rewrite:
//! pdf.js reads the page's own text, then the recognized words, inside
//! their block's `/ShOCR`, then the header; and, cleared, no recognized
//! word.
//!
//! From slice 19 so does a tagged page holding a filled field and a mark,
//! written either way, then flattened, a Rewrite: pdf.js reads the page's
//! own text alone, then, flattened, each annotation's text after it, inside
//! an `/Artifact` of its own.
//!
//! From object streams' slice 1 (B3), a file qpdf linearized is
//! linearized to sheaf and to `qpdf --check-linearization`, and neither
//! once sheaf appends to it or rewrites it.
//!
//! From object streams' slice 2 a Rewrite and every export of a file whose
//! cross-references are a stream, or of a hybrid-reference file, pack
//! what they write into object streams, and a table's don't. Each packed
//! output, of every base, RC4-40's among them, passes `qpdf --check` with
//! its password, and so does what `qpdf --decrypt` makes of an encrypted
//! one; pdf.js reads with the password the pages, text and annotations it
//! reads of the same output of the document's table base, and an
//! encrypted one not at all without it.
//!
//! From M10 slice 6 a text field's appearance sheaf makes, in each case
//! the pixel checks (`tests/fields.rs`) hold to PDFium's own, passes `qpdf
//! --check`, and poppler's `pdftoppm` draws it with its ink where it draws
//! PDFium's.
//!
//! From M10 slice 7 a text field's value and a note, each drawn in its
//! font and, for what that lacks, the fallback font sheaf bundles, written
//! either way, pass `qpdf --check`; PDFium, pdftoppm and pdf.js each draw
//! the file pixel for pixel as they draw it with the whole program in
//! place of its subset (so each reads the subset's `/CIDToGIDMap` stream
//! right), their ink in the same boxes, and pdf.js reads the value and the
//! note back from their appearances. The map's size after Flate is held
//! under a bound.
//!
//! From M10 slice 8 a check box's and a radio button's appearances sheaf
//! makes, off and on, a combo box's, a list box's and a push button's, in
//! each case the pixel checks (`tests/buttons.rs`) hold to PDFium's own,
//! pass `qpdf --check`; pdftoppm draws each where it draws PDFium's, a
//! mark loosely (poppler draws ZapfDingbats in a face of its own) and an
//! `Off` with no frame not at all; and pdf.js reads each and warns of
//! nothing, but that it doesn't support a push button with no action,
//! which each push button here is.
//!
//! From M10 slice 9 a form of each kind of field a value is set in, each
//! filled through `SetFieldValues` and written either way, passes `qpdf
//! --check`; qpdf and pdf.js read each field's value, pdf.js the text
//! field's text from its appearance too, warning of nothing; and pdftoppm
//! draws each widget's ink where PDFium draws it, and
//! nothing else of the page otherwise than the form as built.
//!
//! qpdf is `$SHEAF_QPDF`, else `qpdf` on the PATH; pdf.js is `node` on the
//! PATH with `$SHEAF_PDFJS`, pdfjs-dist's `legacy/build/pdf.mjs`, which
//! draws through the canvas pdfjs-dist installs beside it
//! (`@napi-rs/canvas`); pdftoppm is `$SHEAF_PDFTOPPM`, else `pdftoppm` on
//! the PATH. A test whose tools are missing says so and passes. Run with
//! `cargo test -p sheaf-ops --test interop -- --ignored --nocapture`.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use sheaf_doc::export::{Export, ExportFrom, Out};
use sheaf_doc::forms::fixture::{Form, hybrid_reference, outline_tree};
use sheaf_doc::materialize::{How, StrategyKind};
use sheaf_doc::pdf::{Dictionary, LazyPdf, Object, ReadLimits, XrefKind, dictionary, literal};
use sheaf_doc::serialize::fixture::{
    PREFIX, USER, attachment_only, edited, encrypted, form, object_streams_encrypted,
    object_streams_of, rc4_40, rc4_aes_128_and_aes_256, written,
};
use sheaf_doc::tx::fixture::{
    add, clear_ocr, delete, delete_named, flatten, header, move_pages, ocr, patch_bookmark,
    remove_header, remove_watermark, rotate, set_header, set_watermark, watermark,
};
use sheaf_doc::{Annot, AnnotId, AnnotKind, Box2, DestView, Document, OcrWord, PageId};
use sheaf_edit::{
    BatesSpec, BookmarkPatch, FlattenSet, GridPos, Margins, Op, PageRef, Rotation, Sibling, Turn, Z,
};

mod common;
use common::fields;
use common::{note, open, planned, qpdf, qpdf_linearized, saved, scratch};
use sheaf_image::raster::{PixelFormat, Raster};

/// pdf.js's `pdf.mjs`, to run with node, or None when there's no node or
/// no pdf.js, saying so.
fn pdfjs() -> Option<PathBuf> {
    let node = Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    let pdfjs = std::env::var_os("SHEAF_PDFJS")
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    match (node, pdfjs) {
        (true, Some(pdfjs)) => Some(pdfjs),
        (false, _) => {
            println!("skipped pdf.js: no node on the PATH");
            None
        }
        (true, None) => {
            println!("skipped pdf.js: set SHEAF_PDFJS to pdfjs-dist's legacy/build/pdf.mjs");
            None
        }
    }
}

/// What a tool printed, or why it failed.
fn run(command: &mut Command) -> Result<String, String> {
    let out = command.output().map_err(|e| e.to_string())?;
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(said)
    } else {
        Err(format!(
            "{}: {said}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// What `program` prints, run with `args` and then each of `files`.
fn printed(program: &Path, args: &[&OsStr], files: &[PathBuf; 2]) -> [String; 2] {
    files
        .clone()
        .map(|file| run(Command::new(program).args(args).arg(file)).unwrap())
}

/// qpdf with `password`, when the file has one.
fn qpdf_with(qpdf: &Path, password: Option<&str>) -> Command {
    let mut command = Command::new(qpdf);
    if let Some(password) = password {
        command.arg(format!("--password={password}"));
    }
    command
}

/// Every file to save: [`form`] as each of [`bases_of`].
fn bases() -> Vec<(String, Vec<u8>, Option<&'static str>)> {
    bases_of(&form())
}

/// `plain` unencrypted, with a table, a stream, and object streams; with
/// bytes before its header; encrypted with RC4-40, RC4-128, AES-128,
/// AES-256 and, for its attachment only, `/EFF`; and encrypted with
/// AES-256 in an object stream. Each with its name and the password it
/// opens with.
fn bases_of(plain: &sheaf_doc::pdf::Document) -> Vec<(String, Vec<u8>, Option<&'static str>)> {
    let plain = plain.clone();
    let [table, stream] = written(&plain);
    let mut packed = Vec::new();
    plain.clone().save_modern(&mut packed).unwrap();
    let mut bases = vec![
        ("plain, table".to_string(), table.clone(), None),
        ("plain, stream".to_string(), stream.clone(), None),
        ("plain, object streams".to_string(), packed, None),
        (
            "prefixed, table".to_string(),
            [&PREFIX, &table[..]].concat(),
            None,
        ),
        (
            "prefixed, stream".to_string(),
            [&PREFIX, &stream[..]].concat(),
            None,
        ),
    ];
    let mut states = vec![rc4_40(&plain)];
    states.extend(rc4_aes_128_and_aes_256(&plain));
    states.push(("/EFF", attachment_only(&plain)));
    for (name, state) in &states {
        let [table, stream] = encrypted(&plain, state, |_| {});
        bases.push((format!("{name}, table"), table, Some(USER)));
        bases.push((format!("{name}, stream"), stream, Some(USER)));
    }
    let aes_256 = &states[3].1;
    bases.push((
        "AES-256, object streams".to_string(),
        object_streams_encrypted(&plain, aes_256),
        Some(USER),
    ));
    bases
}

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf"]
    qpdf_checks_every_output,
    qpdf_checks_every_output_appended: qpdf_checks
);

/// Every way out of every base, a save `how` and the exports, passes
/// `qpdf --check`.
fn qpdf_checks(how: How) {
    let Some(qpdf) = qpdf() else {
        return;
    };
    let dir = scratch(&format!("interop-check-{how:?}"));
    let mut checked = 0;
    let mut failed = Vec::new();
    for (name, bytes, password) in bases() {
        let file = name.replace([',', ' ', '/'], "");
        let base = dir.join(format!("{file}.pdf"));
        std::fs::write(&base, &bytes).unwrap();
        let doc = edited(&base, password);
        let outputs = match how {
            How::Appended => vec![(format!("{how:?}"), how.write(&doc))],
            How::Rewritten => ways_out(&doc, &dir, &file).to_vec(),
        };
        outputs_checked(
            &qpdf,
            &dir,
            &file,
            &name,
            password,
            outputs,
            &mut checked,
            &mut failed,
        );
    }
    println!("qpdf --check: {checked} outputs, {} fail", failed.len());
    assert!(failed.is_empty(), "{failed:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What `doc`, edited from the base whose file is `file`, writes: a
/// Rewrite and each export (its first page extracted, print with and
/// without markup, Clean Copy and Export Flattened, the last two put in
/// `dir`), each by what it is.
fn ways_out(doc: &Document, dir: &Path, file: &str) -> [(String, Vec<u8>); 6] {
    let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
    let exported = |export: fn(ExportFrom, Out) -> Export, how: &str| {
        let path = dir.join(format!("{file}-{how}.pdf"));
        common::exported_to(doc, export, &path)
    };
    [
        (format!("{:?}", How::Rewritten), How::Rewritten.write(doc)),
        (
            "Extract".into(),
            doc.export_bytes(|f, out| Export::extract(f, &ids[..1], out)),
        ),
        (
            "Print".into(),
            doc.export_bytes(|f, _| Export::print(f, &ids, true)),
        ),
        (
            "PrintClean".into(),
            doc.export_bytes(|f, _| Export::print(f, &ids, false)),
        ),
        ("Clean".into(), exported(Export::clean_copy, "clean-copy")),
        (
            "Flattened".into(),
            exported(Export::flattened, "flattened-copy"),
        ),
    ]
}

/// `outputs` of base `name` (its file `file`), each by what it is, checked
/// by `qpdf --check` with `password`: counted in `checked`, each failure
/// in `failed`.
#[allow(clippy::too_many_arguments)]
fn outputs_checked(
    qpdf: &Path,
    dir: &Path,
    file: &str,
    name: &str,
    password: Option<&str>,
    outputs: impl IntoIterator<Item = (String, Vec<u8>)>,
    checked: &mut usize,
    failed: &mut Vec<String>,
) {
    for (what, out) in outputs {
        let path = dir.join(format!("{file}-{what}.pdf"));
        std::fs::write(&path, &out).unwrap();
        *checked += 1;
        if let Err(e) = run(qpdf_with(qpdf, password).arg("--check").arg(&path)) {
            failed.push(format!("{name}, {what}: {e}"));
        }
    }
}

/// What pdf.js reads of a file, opened with the password given after it
/// if any: its pages, and each one's text and how many annotations it
/// has. An encrypted file given no password, or the wrong one, fails.
const PDFJS_PAGES: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [lib, file, password] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const data = new Uint8Array(readFileSync(file));
const doc = await pdfjs.getDocument({ data, password, verbosity: 0 }).promise;
console.log(`pages ${doc.numPages}`);
for (let n = 1; n <= doc.numPages; n++) {
  const page = await doc.getPage(n);
  const text = await page.getTextContent();
  const annotations = await page.getAnnotations();
  const said = text.items.map((item) => item.str).join(" ");
  console.log(`page ${n}: ${JSON.stringify(said)}, ${annotations.length} annotations`);
}
"#;

/// Needs qpdf, or node and pdf.js, too.
#[test]
fn a_prefixed_append_opens_identically_in_pdfjs_and_qpdf() {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch("interop-prefixed");
    let script = dir.join("pages.mjs");
    std::fs::write(&script, PDFJS_PAGES).unwrap();
    let [table, stream] = written(&form());
    for (file, kind) in [(table, "table"), (stream, "stream")] {
        let appended = |name: &str, bytes: &[u8]| {
            let base = dir.join(format!("{name}-{kind}.pdf"));
            std::fs::write(&base, bytes).unwrap();
            let out = How::Appended.write(&edited(&base, None));
            let path = dir.join(format!("{name}-{kind}-appended.pdf"));
            std::fs::write(&path, out).unwrap();
            path
        };
        let prefixed = [&PREFIX, &file[..]].concat();
        let files = [appended("file", &file), appended("prefixed", &prefixed)];
        if let Some(qpdf) = &qpdf {
            let [want, got] = printed(qpdf, &["--show-npages".as_ref()], &files);
            assert_eq!(want.trim(), "1", "{kind}");
            assert_eq!(got, want, "{kind}: qpdf --show-npages");
            let pages = ["--json".as_ref(), "--json-key=pages".as_ref()];
            let [want, got] = printed(qpdf, &pages, &files);
            assert_eq!(got, want, "{kind}: qpdf --json pages");
            println!("qpdf, {kind}: the same pages");
        }
        if let Some(pdfjs) = &pdfjs {
            let script = [script.as_os_str(), pdfjs.as_os_str()];
            let [want, got] = printed(Path::new("node"), &script, &files);
            assert!(want.starts_with("pages 1\n"), "{kind}: {want}");
            assert!(want.contains("Page 1"), "{kind}: {want}");
            assert_eq!(got, want, "{kind}: pdf.js");
            println!("pdf.js, {kind}: {want}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What pdf.js reads of `path` with [`PDFJS_PAGES`], given `password`, or
/// why it couldn't.
fn pdfjs_read(
    pdfjs: &Path,
    script: &Path,
    path: &Path,
    password: Option<&str>,
) -> Result<String, String> {
    run(Command::new("node")
        .arg(script)
        .arg(pdfjs)
        .arg(path)
        .args(password))
}

/// Every base of [`form`]'s ([`bases`]) and of a hybrid-reference file's
/// ([`hybrid_reference`], and its table writing), turned and noted,
/// then rewritten and exported ([`ways_out`]): each output of a base whose
/// cross-references are a stream, or a hybrid's, holds object streams,
/// and of a table's none. Each packed one passes `qpdf --check` with its
/// base's password, and an encrypted one's `qpdf --decrypt` passes it
/// too. pdf.js reads each packed one, with the password, as it reads the
/// same output of the document's table base, unpacked, and an encrypted
/// one not at all without it.
#[test]
#[ignore = "needs qpdf, or node and pdf.js"]
fn packed_outputs_read_alike_in_qpdf_and_pdfjs() {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch("interop-packed");
    let script = dir.join("pages.mjs");
    std::fs::write(&script, PDFJS_PAGES).unwrap();
    let hybrid = hybrid_reference();
    let [hybrid_table, _] = written(&sheaf_doc::pdf::Document::load_mem(&hybrid).unwrap());
    // Each document's bases, its unencrypted table first.
    let documents = [
        bases(),
        vec![
            ("hybrid, table".to_string(), hybrid_table, None),
            ("hybrid".to_string(), hybrid, None),
        ],
    ];
    let open = |bytes: &[u8], password| {
        LazyPdf::open(Arc::<[u8]>::from(bytes), password, &ReadLimits::DEFAULT).unwrap()
    };
    let mut failed = Vec::new();
    let (mut packed, mut checked, mut decrypted, mut read, mut refused) = (0, 0, 0, 0, 0);
    for bases in documents {
        let mut want: Vec<Option<String>> = Vec::new();
        for (name, bytes, password) in bases {
            let file = name.replace([',', ' ', '/'], "");
            let base = dir.join(format!("{file}.pdf"));
            std::fs::write(&base, &bytes).unwrap();
            let source = open(&bytes, password);
            let packs = source.xref().kind == XrefKind::Stream || source.xref().hybrid;
            let outputs = ways_out(&edited(&base, password), &dir, &file);
            for (i, (what, out)) in outputs.into_iter().enumerate() {
                let name = format!("{name}, {what}");
                let path = dir.join(format!("{file}-{what}.pdf"));
                std::fs::write(&path, &out).unwrap();
                let pdf = open(&out, password);
                assert_eq!(!object_streams_of(&pdf).is_empty(), packs, "{name}");
                let reads = (pdfjs.as_ref()).map(|p| pdfjs_read(p, &script, &path, password));
                // The first base's outputs, unpacked, are read as the
                // others' should be.
                if want.len() == i {
                    assert!(!packs && password.is_none(), "{name}: a table first");
                    want.push(reads.map(Result::unwrap));
                    continue;
                }
                if !packs {
                    continue;
                }
                packed += 1;
                if let Some(qpdf) = &qpdf {
                    checked += 1;
                    if let Err(e) = run(qpdf_with(qpdf, password).arg("--check").arg(&path)) {
                        failed.push(format!("{name}: qpdf --check: {e}"));
                    }
                    if pdf.is_encrypted() {
                        decrypted += 1;
                        let plain = dir.join(format!("{file}-{what}-decrypted.pdf"));
                        let mut decrypt = qpdf_with(qpdf, password);
                        decrypt.arg("--decrypt").arg(&path).arg(&plain);
                        let check = || run(Command::new(qpdf).arg("--check").arg(&plain));
                        if let Err(e) = run(&mut decrypt).and_then(|_| check()) {
                            failed.push(format!("{name}: qpdf --decrypt, then --check: {e}"));
                        }
                    }
                }
                let (Some(pdfjs), Some(reads)) = (&pdfjs, reads) else {
                    continue;
                };
                read += 1;
                match reads {
                    Ok(got) if Some(&got) == want[i].as_ref() => {}
                    got => failed.push(format!("{name}: pdf.js read {got:?}, not {:?}", want[i])),
                }
                if pdf.is_encrypted() {
                    refused += 1;
                    if let Ok(got) = pdfjs_read(pdfjs, &script, &path, None) {
                        failed.push(format!("{name}: pdf.js read {got:?} with no password"));
                    }
                }
            }
        }
    }
    println!(
        "{packed} packed outputs: qpdf --check {checked}, --decrypt then --check {decrypted}; \
         pdf.js read {read} alike, {refused} encrypted refused without the password; {} fail",
        failed.len()
    );
    assert!(failed.is_empty(), "{failed:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Three Appends in a row onto each base, each after the last (a page
/// turned, a note put on it, the pages moved): the file passes `qpdf
/// --check` after each, and has its pages.
#[test]
#[ignore = "needs qpdf"]
fn three_appends_pass_qpdf_check() {
    let Some(qpdf) = qpdf() else {
        return;
    };
    let dir = scratch("interop-three-appends");
    let mut failed = Vec::new();
    for (name, bytes, password) in bases() {
        let path = dir.join(format!("{}.pdf", name.replace([',', ' ', '/'], "")));
        std::fs::write(&path, &bytes).unwrap();
        let mut doc = edited(&path, password);
        for i in 0..3 {
            if i > 0 {
                let first = doc.page(0).unwrap();
                let turn = rotate(&[first], Turn::Quarter);
                doc.commit_ops(vec![turn], "turn").unwrap();
            }
            sheaf_doc::materialize::save_appended(&mut doc, &path).unwrap();
            if let Err(e) = run(qpdf_with(&qpdf, password).arg("--check").arg(&path)) {
                failed.push(format!("{name}, Append {}: {e}", i + 1));
            }
        }
        let pages = run(qpdf_with(&qpdf, password).arg("--show-npages").arg(&path)).unwrap();
        assert_eq!(pages.trim(), doc.page_count().to_string(), "{name}");
    }
    assert!(failed.is_empty(), "{failed:#?}");
    println!(
        "qpdf --check: three Appends onto each of {} bases",
        bases().len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Of each base made of a form of three pages, the second with a field,
/// that page deleted and saved (a Rewrite, which prunes its objects), and
/// the delete undone and saved again as the plan says: the Append it
/// plans refers to objects the file hasn't got, so the save is a Rewrite
/// instead, which passes `qpdf --check` and has every page.
#[test]
#[ignore = "needs qpdf"]
fn pages_deleted_saved_and_undone_pass_qpdf_check() {
    use sheaf_doc::materialize::{AskAnswers, SaveHow, Why, save_with};
    let Some(qpdf) = qpdf() else {
        return;
    };
    let dir = scratch("interop-undone-delete");
    let mut f = Form::new(3);
    let field = f.text("t", 1, sheaf_doc::forms::fixture::row(0));
    f.fill(field, "value");
    let id = literal("0123456789abcdef");
    f.pdf.trailer.set("ID", vec![id.clone(), id]);
    let plain = f.pdf;
    let mut failed = Vec::new();
    let answers = AskAnswers::default();
    for (name, bytes, password) in bases_of(&plain) {
        let path = dir.join(format!("{}.pdf", name.replace([',', ' ', '/'], "")));
        std::fs::write(&path, &bytes).unwrap();
        let mut doc = sheaf_doc::open::file(&path, password).unwrap().doc;
        let gone = doc.page(1).unwrap();
        doc.commit_ops(vec![delete(&[gone])], "delete").unwrap();
        save_with(
            &mut doc,
            &path,
            SaveHow::Planned,
            &answers,
            &sheaf_ops::NoProgress,
        )
        .unwrap();
        doc.undo().unwrap();
        let report = save_with(
            &mut doc,
            &path,
            SaveHow::Planned,
            &answers,
            &sheaf_ops::NoProgress,
        )
        .unwrap();
        assert_eq!(report.why, [Why::BaseObjectsMissing], "{name}");
        if let Err(e) = run(qpdf_with(&qpdf, password).arg("--check").arg(&path)) {
            failed.push(format!("{name}: {e}"));
        }
        let pages = run(qpdf_with(&qpdf, password).arg("--show-npages").arg(&path)).unwrap();
        assert_eq!(pages.trim(), "3", "{name}");
    }
    assert!(failed.is_empty(), "{failed:#?}");
    println!(
        "qpdf --check: pages deleted, saved and undone on each of {} bases",
        bases_of(&plain).len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file sheaf wrote, linearized by qpdf: sheaf's cross-reference facts
/// read its linearization as holding, and qpdf calls it linearized. Given
/// a note and saved as planned, an Append after its bytes, and rewritten
/// instead, it is linearized to neither (B3).
#[test]
#[ignore = "needs qpdf"]
fn a_linearized_file_appended_to_is_linearized_no_more() {
    let Some(qpdf) = qpdf() else {
        return;
    };
    let dir = scratch("interop-linearized");
    let [plain, _] = written(&form());
    let base = dir.join("base.pdf");
    std::fs::write(&base, plain).unwrap();
    std::fs::write(&base, How::Rewritten.write(&edited(&base, None))).unwrap();
    let linearized = dir.join("linearized.pdf");
    run(Command::new(&qpdf)
        .arg("--linearize")
        .arg(&base)
        .arg(&linearized))
    .unwrap();
    // Whether sheaf and qpdf call the file at `path` linearized, each.
    let read = |path: &Path| {
        let sheaf = open(path).primary().xref().linearized;
        (sheaf, qpdf_linearized(&qpdf, path).unwrap())
    };
    assert_eq!(read(&linearized), (true, true));
    let was = std::fs::read(&linearized).unwrap();
    // A copy of the linearized file, opened and given a note.
    let noted = |name: &str| {
        let path = dir.join(name);
        std::fs::write(&path, &was).unwrap();
        let mut doc = open(&path);
        let mark = add(doc.page(0).unwrap(), &note("after"));
        doc.commit_ops(vec![mark], "note").unwrap();
        (path, doc)
    };
    let (path, mut doc) = noted("appended.pdf");
    let (strategy, bytes) = planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    assert!(bytes.starts_with(&was), "an Append keeps the file's bytes");
    assert!(!doc.saved_record().unwrap().xref.linearized);
    assert_eq!(read(&path), (false, false), "appended");
    let (path, mut doc) = noted("rewritten.pdf");
    let (strategy, _) = saved(&mut doc, &path, How::Rewritten);
    assert_eq!(strategy, StrategyKind::Rewrite);
    assert!(!doc.saved_record().unwrap().xref.linearized);
    assert_eq!(read(&path), (false, false), "rewritten");
    println!("qpdf --check-linearization: linearized, then neither appended to nor rewritten");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file of three pages whose first has another tool's link and two of
/// its comments, `/NM` `theirs` and `kept`, and whose outline has a
/// bookmark that opens a URI, a structure element its `/SE`, and one to
/// the third page.
fn records_base() -> Vec<u8> {
    let mut f = Form::new(3);
    let rect = |y: i64| Object::from(vec![72.into(), y.into(), 144.into(), (y + 24).into()]);
    let mut uri = |url: &str| {
        f.pdf
            .add_object(dictionary! { "S" => Object::name("URI"), "URI" => literal(url) })
    };
    let (site_url, link_url) = (uri("https://example.com/"), uri("https://example.org/"));
    let section = f.pdf.add_object(
        dictionary! { "Type" => Object::name("StructElem"), "S" => Object::name("Sect") },
    );
    let site = dictionary! { "Title" => literal("Site"), "A" => site_url, "SE" => section };
    let third = dictionary! {
        "Title" => literal("Third"),
        "Dest" => vec![f.pages[2].into(), Object::name("Fit")],
    };
    outline_tree(&mut f, vec![(0, site), (0, third)]);
    let annot = |subtype: &str, y, more: Dictionary| {
        let mut d = dictionary! {
            "Type" => Object::name("Annot"),
            "Subtype" => Object::name(subtype),
            "Rect" => rect(y),
        };
        for (k, v) in more.iter() {
            d.set(k.to_vec(), v.clone());
        }
        d
    };
    let link = annot("Link", 72, dictionary! { "A" => link_url });
    let comment = |name: &str, said: &str, y| {
        let about = dictionary! {
            "NM" => literal(name),
            "Contents" => literal(said),
            "T" => literal("Ann"),
        };
        annot("Text", y, about)
    };
    let annots = [
        link,
        comment("theirs", "Deleted by sheaf", 144),
        comment("kept", "Kept by sheaf", 216),
    ]
    .map(|a| f.pdf.add_object(a));
    f.set_annots(0, &annots);
    f.bytes()
}

/// What pdf.js reads of a file's records: each bookmark's title and the
/// URI it opens or the page it goes to, and each page's annotations, a
/// link by its URI and a comment by what it says.
const PDFJS_RECORDS: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [lib, file] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const data = new Uint8Array(readFileSync(file));
const doc = await pdfjs.getDocument({ data, verbosity: 0 }).promise;
for (const item of (await doc.getOutline()) ?? []) {
  const to = Array.isArray(item.dest)
    ? `page ${(await doc.getPageIndex(item.dest[0])) + 1}`
    : `<${item.url ?? item.unsafeUrl}>`;
  console.log(`bookmark ${JSON.stringify(item.title)} ${to}`);
}
for (let n = 1; n <= doc.numPages; n++) {
  const page = await doc.getPage(n);
  const said = (await page.getAnnotations()).map((a) =>
    a.subtype === "Link"
      ? `Link <${a.url ?? a.unsafeUrl}>`
      : `${a.subtype} ${JSON.stringify(a.contentsObj?.str ?? a.contents)}`,
  );
  console.log(`page ${n}: ${said.join("; ")}`);
}
"#;

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, or node and pdf.js"]
    bookmarks_links_and_comments_come_through_both_strategies,
    bookmarks_links_and_comments_come_through_both_strategies_appended: records_come_through
);

/// What an Append writes after a reorder and a bookmark renamed, and a
/// Rewrite once a comment is deleted too, passes `qpdf --check`, and pdf.js
/// reads the bookmarks, links and comments the document has.
fn records_come_through(how: How) {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch(&format!("interop-records-{how:?}"));
    let script = dir.join("records.mjs");
    std::fs::write(&script, PDFJS_RECORDS).unwrap();
    let base = dir.join("records.pdf");
    std::fs::write(&base, records_base()).unwrap();
    let mut doc = sheaf_doc::open::file(&base, None).unwrap().doc;
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    let third = (doc.state().bookmarks_preorder().into_iter())
        .find(|(_, _, b)| &*b.title == "Third")
        .map(|(_, id, _)| id)
        .unwrap();
    // The third page first, and its bookmark renamed, so an Append writes
    // the outline anew.
    let renamed = BookmarkPatch {
        title: Some("3".into()),
        ..Default::default()
    };
    let ops = vec![
        move_pages(&[pages[2]], Some(pages[0])),
        patch_bookmark(third, renamed),
    ];
    doc.commit_ops(ops, "Reorder").unwrap();
    // A deleted comment makes the next save a Rewrite.
    if how == How::Rewritten {
        let delete = delete_named(pages[0], &["theirs"]);
        doc.commit_ops(vec![delete], "Delete").unwrap();
    }
    let strategy = format!("{how:?}");
    let path = dir.join(format!("records-{strategy}.pdf"));
    std::fs::write(&path, how.write(&doc)).unwrap();
    let outputs = [(strategy, path)];
    for (strategy, path) in &outputs {
        if let Some(qpdf) = &qpdf {
            run(Command::new(qpdf).arg("--check").arg(path))
                .unwrap_or_else(|e| panic!("qpdf --check, {strategy}: {e}"));
        }
        if let Some(pdfjs) = &pdfjs {
            let said = run(Command::new("node").arg(&script).arg(pdfjs).arg(path)).unwrap();
            let theirs = match how {
                How::Appended => "Text \"Deleted by sheaf\"; ",
                How::Rewritten => "",
            };
            let want = format!(
                "bookmark \"Site\" <https://example.com/>\n\
                 bookmark \"3\" page 1\n\
                 page 1: \n\
                 page 2: Link <https://example.org/>; {theirs}Text \"Kept by sheaf\"\n\
                 page 3: \n"
            );
            assert_eq!(said, want, "pdf.js, {strategy}");
            println!("pdf.js, {strategy}:\n{said}");
        }
    }
    if qpdf.is_some() {
        println!("qpdf --check: {} outputs, 0 fail", outputs.len());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What pdf.js reads of a cropped page's records: the page's view (the
/// box it shows), each annotation's subtype and rect, each bookmark's
/// `/XYZ` left and top, and each text item's string, transform, width and
/// height.
const PDFJS_CROPPED: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [lib, file] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const data = new Uint8Array(readFileSync(file));
const doc = await pdfjs.getDocument({ data, verbosity: 0 }).promise;
const page = await doc.getPage(1);
console.log(`view ${page.view.join(" ")}`);
for (const a of await page.getAnnotations()) {
  console.log(`annot ${a.subtype} ${a.rect.join(" ")}`);
}
for (const item of (await doc.getOutline()) ?? []) {
  const [, kind, left, top] = Array.isArray(item.dest) ? item.dest : [];
  console.log(`bookmark ${kind?.name} ${left} ${top}`);
}
for (const item of (await page.getTextContent()).items) {
  if (item.str.trim()) {
    const at = [...item.transform, item.width, item.height].join(" ");
    console.log(`text ${JSON.stringify(item.str)} ${at}`);
  }
}
"#;

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, or node and pdf.js"]
    a_cropped_page_s_records_land_in_pdfjs,
    a_cropped_page_s_records_land_in_pdfjs_appended: cropped_records_land
);

/// A page with a mark of sheaf's, a widget with a value, recognized text
/// and a bookmark to a point on it, saved and opened again, then cropped
/// by margins none alike and written `how`: the file passes `qpdf
/// --check`, and pdf.js shows the crop, and reads each record where it's
/// stored in PageSpace, which the crop didn't move: the annotations'
/// rects as the file had them before, the widget's where the form put it
/// and the mark's around its box; the bookmark's `/XYZ`; and each
/// recognized word's text within its word's box, to a point.
fn cropped_records_land(how: How) {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch(&format!("interop-cropped-{how:?}"));
    let script = dir.join("cropped.mjs");
    std::fs::write(&script, PDFJS_CROPPED).unwrap();
    let mut f = Form::new(1);
    common::blue(&mut f, "a", 0, 0, "Alpha");
    let base = common::write(&dir, "cropped.pdf", &mut f);
    let mut doc = sheaf_doc::open::file(&base, None).unwrap().doc;
    let page = doc.page(0).unwrap();
    let square = Box2 {
        x: 325.0,
        y: 325.0,
        w: 50.0,
        h: 50.0,
    };
    let mark = Annot {
        id: AnnotId(0),
        kind: AnnotKind::Square { rect: square },
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: String::new(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    // Two words, as recognized: boxes in the page's view.
    let words = [
        ("sheaf", 300.0, 392.0, 100.0),
        ("interop", 150.0, 560.0, 140.0),
    ];
    let words = words.map(|(text, x, y, w)| OcrWord {
        text: text.into(),
        rect: Box2 { x, y, w, h: 24.0 },
        confidence: 1.0,
    });
    let layer = sheaf_doc::text_layer::from_engine(words.to_vec(), &doc.geom(page), "test");
    let view = DestView::Xyz {
        left: Some(300.0),
        top: Some(400.0),
        zoom: None,
    };
    let bookmark = Op::AddBookmark {
        parent: None,
        at: Sibling::First,
        title: "Square".into(),
        target: Some(PageRef::Id(page)),
        view: Some(view.clone()),
        color: None,
        style: Default::default(),
        local: None,
    };
    let words = layer.words.clone();
    let mut ops = vec![add(page, &mark), bookmark];
    ops.extend(ocr([(page, Arc::new(layer))]));
    doc.commit_ops(ops, "records").unwrap();
    How::Rewritten.save(&mut doc, &base);
    let annots = rects_in(&std::fs::read(&base).unwrap());
    let mut doc = sheaf_doc::open::file(&base, None).unwrap().doc;
    let page = doc.page(0).unwrap();
    let uneven = Margins {
        top: 50.0,
        bottom: 20.0,
        left: 72.0,
        right: 30.0,
    };
    common::crop(&mut doc, &[page], uneven);
    let path = dir.join(format!("cropped-{how:?}.pdf"));
    let bytes = how.write(&doc);
    std::fs::write(&path, &bytes).unwrap();
    // As stored: the annotations' rects the crop didn't move.
    assert_eq!(rects_in(&bytes), annots, "{how:?}");
    if let Some(qpdf) = &qpdf {
        run(Command::new(qpdf).arg("--check").arg(&path))
            .unwrap_or_else(|e| panic!("qpdf --check, {how:?}: {e}"));
        println!("qpdf --check, {how:?}: passes");
    }
    if let Some(pdfjs) = &pdfjs {
        let said = run(Command::new("node").arg(&script).arg(pdfjs).arg(&path)).unwrap();
        println!("pdf.js, {how:?}:\n{said}");
        let stored = Stored {
            crop: doc.geom(page).crop,
            annots,
            square,
            view,
            words,
        };
        read_as_stored(&said, &stored, how);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What [`cropped_records_land`]'s cropped page stores, in PageSpace.
struct Stored {
    crop: sheaf_doc::geom::Rect,
    /// The annotations' rects, each by subtype, as the file had them
    /// before the crop.
    annots: Vec<(String, [f32; 4])>,
    /// The mark's box.
    square: Box2,
    /// The bookmark's view.
    view: DestView,
    /// The recognized words.
    words: Vec<OcrWord>,
}

/// The first page's annotations' `/Rect`s in `bytes`, each by subtype.
fn rects_in(bytes: &[u8]) -> Vec<(String, [f32; 4])> {
    let pdf = sheaf_doc::pdf::Document::load_mem(bytes).unwrap();
    let first = pdf.get_pages()[&1];
    let mut out: Vec<(String, [f32; 4])> = (pdf.get_page_annotations(first).unwrap().iter())
        .map(|a| {
            let subtype = a.get(b"Subtype").unwrap().as_name().unwrap();
            let rect = a.get(b"Rect").unwrap().as_array().unwrap();
            let n = |i: usize| rect[i].as_number().unwrap() as f32;
            let r = [n(0), n(1), n(2), n(3)];
            // As pdf.js normalizes it.
            let r = [
                r[0].min(r[2]),
                r[1].min(r[3]),
                r[0].max(r[2]),
                r[1].max(r[3]),
            ];
            (String::from_utf8_lossy(subtype).into_owned(), r)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// [`PDFJS_CROPPED`]'s lines `said`, checked against what's `stored`: the
/// view is the crop; the annotations' rects are as they were, the widget's
/// where the form put it and the mark's around its box; the bookmark's
/// `/XYZ` is its view's; and each recognized word is read within its box.
fn read_as_stored(said: &str, stored: &Stored, how: How) {
    let numbers = |rest: &str| -> Vec<f32> {
        rest.split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect()
    };
    let crop = stored.crop;
    let shown = numbers(said.lines().find_map(|l| l.strip_prefix("view ")).unwrap());
    assert_eq!(
        shown,
        [crop.x0, crop.y0, crop.x1, crop.y1],
        "{how:?}: the view"
    );
    let mut annots: Vec<(String, [f32; 4])> = (said.lines())
        .filter_map(|l| l.strip_prefix("annot "))
        .map(|l| {
            let (subtype, rest) = l.split_once(' ').unwrap();
            let r = numbers(rest);
            (subtype.to_string(), [r[0], r[1], r[2], r[3]])
        })
        .collect();
    annots.sort_by(|a, b| a.0.cmp(&b.0));
    let near = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1e-3);
    let before = &stored.annots;
    let alike = annots.len() == before.len()
        && (annots.iter().zip(before)).all(|(a, b)| a.0 == b.0 && near(&a.1, &b.1));
    assert!(alike, "{how:?}: {annots:?}, stored {before:?}");
    let rect = |subtype: &str| annots.iter().find(|a| a.0 == subtype).unwrap().1;
    assert!(
        near(&rect("Widget"), &common::row(0)),
        "{how:?}: the widget"
    );
    let [x0, y0, x1, y1] = rect("Square");
    let square = stored.square;
    let around = x0 <= square.x && y0 <= square.y && x1 >= square.right() && y1 >= square.bottom();
    assert!(
        around,
        "{how:?}: the mark at {:?}, its box {square:?}",
        rect("Square")
    );
    let DestView::Xyz { left, top, .. } = stored.view else {
        unreachable!("an /XYZ")
    };
    let mark = format!("bookmark XYZ {} {}", left.unwrap(), top.unwrap());
    assert!(said.lines().any(|l| l == mark), "{how:?}: no {mark:?}");
    for word in &stored.words {
        let quoted = format!("text {:?} ", word.text);
        let line = said.lines().find(|l| l.starts_with(&quoted));
        let at = numbers(&line.unwrap_or_else(|| panic!("{how:?}: no {quoted:?}"))[quoted.len()..]);
        let (x, y, w, h) = (at[4], at[5], at[6], at[7]);
        let b = word.rect;
        let within = x >= b.x - 1.0
            && x + w <= b.right() + 1.0
            && y >= b.y - 1.0
            && y + h <= b.bottom() + 1.0;
        assert!(
            within,
            "{how:?}: {:?} read at {at:?}, stored {b:?}",
            word.text
        );
    }
}

/// What pdf.js reads of a page's text, the first's unless a third
/// argument numbers another: where each marked content begins, by its tag,
/// and ends, and each text item's string between; then how many operators
/// the page draws with, its fonts, graphics states and images loaded.
const PDFJS_MARKED: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [lib, file, number = "1"] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const data = new Uint8Array(readFileSync(file));
const doc = await pdfjs.getDocument({ data, verbosity: 0 }).promise;
const page = await doc.getPage(Number(number));
const content = await page.getTextContent({ includeMarkedContent: true });
for (const item of content.items) {
  if (item.type === "beginMarkedContent" || item.type === "beginMarkedContentProps") {
    console.log(`begin ${item.tag}`);
  } else if (item.type === "endMarkedContent") {
    console.log("end");
  } else if (item.str?.trim()) {
    console.log(`text ${JSON.stringify(item.str)}`);
  }
}
console.log(`operators ${(await page.getOperatorList()).fnArray.length}`);
"#;

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, or node and pdf.js"]
    a_watermark_s_blocks_read_in_pdfjs,
    a_watermark_s_blocks_read_in_pdfjs_appended: watermark_blocks_read
);

/// A one-page form whose page draws, after its own content and in a
/// stream of its own, the watermark Acrobat would add: `/Fm0`, a form whose
/// `/PieceInfo` says so, drawing "ACROBAT".
fn with_acrobat_s_watermark() -> Form {
    let mut f = Form::new(1);
    let piece = dictionary! { "ADBE_CompoundType" => dictionary! {
        "DocSettings" => dictionary! {}, "Private" => Object::name("Watermark"),
    } };
    let form = f.pdf.add_object(sheaf_doc::pdf::Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"), "PieceInfo" => piece,
            "BBox" => vec![0.into(), 0.into(), 300.into(), 60.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => f.helv } },
        },
        b"0 0 1 rg BT /F1 48 Tf 4 12 Td (ACROBAT) Tj ET".to_vec(),
    ));
    let theirs = f.pdf.add_object(sheaf_doc::pdf::Stream::new(
        dictionary! {},
        b"/Artifact <</Type /Pagination /Subtype /Watermark>> BDC q \
          1 0 0 1 150 350 cm /Fm0 Do Q EMC"
            .to_vec(),
    ));
    let page = f.pages[0];
    let own = f.dict(page).get(b"Contents").unwrap().clone();
    f.dict(page).set("Contents", vec![own, theirs.into()]);
    let resources = f.dict(page).get_mut(b"Resources").unwrap();
    let xobjects = dictionary! { "Fm0" => form };
    resources.as_dict_mut().unwrap().set("XObject", xobjects);
    f
}

/// A page with Acrobat's watermark given a watermark over it, which
/// replaces Acrobat's, and a background under it, written `how`; then
/// saved `how`, opened again with both found, and its watermark moved,
/// written `how` again; then both removed and written `how`: each file
/// passes `qpdf --check`, and pdf.js loads the page and reads the
/// background's text, the page's own, then the watermark's, each of
/// sheaf's once and inside its `/Artifact`, and nothing of Acrobat's.
fn watermark_blocks_read(how: How) {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch(&format!("interop-watermark-{how:?}"));
    let script = dir.join("marked.mjs");
    std::fs::write(&script, PDFJS_MARKED).unwrap();
    let base = common::write(&dir, "watermark.pdf", &mut with_acrobat_s_watermark());
    let acrobat = std::fs::read(&base).unwrap();
    let mut doc = sheaf_doc::open::file(&base, None).unwrap().doc;
    let ops = vec![
        set_watermark(watermark("DRAFT"), Z::Over, None),
        set_watermark(watermark("COPY"), Z::Under, None),
    ];
    doc.commit_ops(ops, "Watermark").unwrap();
    let set = how.write(&doc);
    how.save(&mut doc, &base);
    let mut doc = sheaf_doc::open::file(&base, None).unwrap().doc;
    let mut moved = watermark("DRAFT");
    moved.position = GridPos::Top;
    doc.commit_ops(vec![set_watermark(moved, Z::Over, None)], "Move")
        .unwrap();
    let moved = how.write(&doc);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    let remove = remove_watermark(None, &pages);
    doc.commit_ops(vec![remove], "Remove").unwrap();
    let ours = "begin Artifact\ntext \"COPY\"\nend\n\
                text \"Page 1\"\n\
                begin Artifact\ntext \"DRAFT\"\nend";
    let outputs = [
        (
            "acrobat",
            acrobat,
            "text \"Page 1\"\nbegin Artifact\ntext \"ACROBAT\"\nend",
        ),
        ("set", set, ours),
        ("moved", moved, ours),
        ("removed", how.write(&doc), "text \"Page 1\""),
    ];
    for (what, bytes, want) in outputs {
        let path = dir.join(format!("watermark-{what}-{how:?}.pdf"));
        std::fs::write(&path, &bytes).unwrap();
        if let Some(qpdf) = &qpdf {
            run(Command::new(qpdf).arg("--check").arg(&path))
                .unwrap_or_else(|e| panic!("qpdf --check, {what}, {how:?}: {e}"));
            println!("qpdf --check, {what}, {how:?}: passes");
        }
        if let Some(pdfjs) = &pdfjs {
            let read = marked_read(pdfjs, &script, &path, 1, &format!("{what}, {how:?}"));
            assert_eq!(read, want, "pdf.js, {what}, {how:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What pdf.js reads of page `number` of the file at `path`, by
/// [`PDFJS_MARKED`] at `script`: its lines but the last, which must say
/// the page loads some operators. `what` names the file to print.
fn marked_read(pdfjs: &Path, script: &Path, path: &Path, number: usize, what: &str) -> String {
    let mut node = Command::new("node");
    node.arg(script)
        .arg(pdfjs)
        .arg(path)
        .arg(number.to_string());
    let said = run(&mut node).unwrap();
    println!("pdf.js, {what}, page {number}:\n{said}");
    let (read, operators) = said.trim_end().rsplit_once('\n').unwrap();
    let operators: usize = operators
        .strip_prefix("operators ")
        .unwrap()
        .parse()
        .unwrap();
    assert!(operators > 0, "pdf.js, {what}, page {number}: {said}");
    read.to_string()
}

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, or node and pdf.js"]
    a_header_s_blocks_read_in_pdfjs,
    a_header_s_blocks_read_in_pdfjs_appended: header_blocks_read
);

/// Two pages with Acrobat's header given a header and footer of sheaf's,
/// which replaces it: "CONFIDENTIAL" at the header's left, "Page <<1>> of
/// <<N>>" at the footer's center and Bates numbers at its right, written
/// `how`; then saved `how` and its pages traded, saved as planned, an
/// Append that draws the page numbers again (§3.5's live redraw), each page
/// keeping its Bates number; then opened again and the header and footer
/// removed, written `how`: each file passes `qpdf --check`, and pdf.js
/// loads each page and reads its own text, then each of sheaf's slots
/// once, inside an `/Artifact` of its own, the page's place in the
/// document as it's written and the Bates number it was given, and
/// nothing of Acrobat's.
fn header_blocks_read(how: How) {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch(&format!("interop-header-{how:?}"));
    let script = dir.join("marked.mjs");
    std::fs::write(&script, PDFJS_MARKED).unwrap();
    let base = common::write(&dir, "header.pdf", &mut common::acrobat_headed_form());
    let acrobat = std::fs::read(&base).unwrap();
    let mut doc = common::open(&base);
    let mut spec = header("CONFIDENTIAL");
    spec.texts[4] = "Page <<1>> of <<N>>".into();
    spec.texts[5] = "<<bates>>".into();
    spec.bates = Some(BatesSpec {
        prefix: "SHF".into(),
        suffix: String::new(),
        digits: 4,
        start: 1,
    });
    doc.commit_ops(vec![set_header(spec, None)], "Header & footer")
        .unwrap();
    let set = how.write(&doc);
    how.save(&mut doc, &base);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    doc.commit_ops(vec![move_pages(&[pages[1]], Some(pages[0]))], "Move")
        .unwrap();
    let (strategy, traded) = common::planned(&mut doc, &base);
    assert_eq!(strategy, StrategyKind::Append, "{how:?}: the live redraw");
    let mut doc = common::open(&base);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    doc.commit_ops(vec![remove_header(&pages)], "Remove")
        .unwrap();
    // Page `body` of the file as made, the `at`th now: its own text, then
    // sheaf's slots.
    let ours = |body: usize, at: usize| {
        format!(
            "text \"Page {body}\"\n\
             begin Artifact\ntext \"CONFIDENTIAL\"\nend\n\
             begin Artifact\ntext \"Page {at} of 2\"\nend\n\
             begin Artifact\ntext \"SHF000{body}\"\nend"
        )
    };
    let theirs =
        |body: usize| format!("text \"Page {body}\"\nbegin Artifact\ntext \"ACROBATHEADER\"\nend");
    let outputs = [
        ("acrobat", acrobat, [theirs(1), theirs(2)]),
        ("set", set, [ours(1, 1), ours(2, 2)]),
        ("traded", traded, [ours(2, 1), ours(1, 2)]),
        (
            "removed",
            how.write(&doc),
            ["text \"Page 2\"".into(), "text \"Page 1\"".into()],
        ),
    ];
    for (what, bytes, want) in outputs {
        let path = dir.join(format!("header-{what}-{how:?}.pdf"));
        std::fs::write(&path, &bytes).unwrap();
        if let Some(qpdf) = &qpdf {
            run(Command::new(qpdf).arg("--check").arg(&path))
                .unwrap_or_else(|e| panic!("qpdf --check, {what}, {how:?}: {e}"));
            println!("qpdf --check, {what}, {how:?}: passes");
        }
        if let Some(pdfjs) = &pdfjs {
            for (number, want) in (1..).zip(want) {
                let read = marked_read(pdfjs, &script, &path, number, &format!("{what}, {how:?}"));
                assert_eq!(read, want, "pdf.js, {what}, {how:?}, page {number}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, or node and pdf.js"]
    an_ocr_block_reads_in_pdfjs,
    an_ocr_block_reads_in_pdfjs_appended: ocr_block_reads
);

/// A page saying "Page 1" given recognized text, two words on a line, and
/// a header over it, written `how`; then saved `how`, opened again and its
/// text cleared, saved as planned, a Rewrite: each file passes `qpdf
/// --check`, and pdf.js reads the page's own text, then the recognized
/// words, inside the block's `/ShOCR`, then the header, inside its
/// `/Artifact`; and, cleared, no recognized word.
fn ocr_block_reads(how: How) {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch(&format!("interop-ocr-{how:?}"));
    let script = dir.join("marked.mjs");
    std::fs::write(&script, PDFJS_MARKED).unwrap();
    let base = common::write(&dir, "ocr.pdf", &mut Form::new(1));
    let mut doc = common::open(&base);
    let page = doc.page(0).unwrap();
    let words = [("Recognized", 72.0), ("words", 200.0)].map(|(text, x)| OcrWord {
        text: text.into(),
        rect: Box2 {
            x,
            y: 300.0,
            w: 110.0,
            h: 18.0,
        },
        confidence: 1.0,
    });
    let layer = sheaf_doc::text_layer::from_engine(words.to_vec(), &doc.geom(page), "test");
    let mut ops = ocr([(page, Arc::new(layer))]);
    ops.push(set_header(header("CONFIDENTIAL"), None));
    doc.commit_ops(ops, "Recognize text").unwrap();
    let set = how.write(&doc);
    how.save(&mut doc, &base);
    let mut doc = common::open(&base);
    let page = doc.page(0).unwrap();
    doc.commit_ops(vec![clear_ocr(&[page])], "Clear").unwrap();
    let (strategy, cleared) = common::planned(&mut doc, &base);
    assert_eq!(strategy, StrategyKind::Rewrite, "{how:?}: cleared");
    let header = "begin Artifact\ntext \"CONFIDENTIAL\"\nend";
    let outputs = [
        (
            "set",
            set,
            format!(
                "text \"Page 1\"\nbegin ShOCR\ntext \"Recognized\"\ntext \"words\"\nend\n{header}"
            ),
        ),
        ("cleared", cleared, format!("text \"Page 1\"\n{header}")),
    ];
    for (what, bytes, want) in outputs {
        let path = dir.join(format!("ocr-{what}-{how:?}.pdf"));
        std::fs::write(&path, &bytes).unwrap();
        if let Some(qpdf) = &qpdf {
            run(Command::new(qpdf).arg("--check").arg(&path))
                .unwrap_or_else(|e| panic!("qpdf --check, {what}, {how:?}: {e}"));
            println!("qpdf --check, {what}, {how:?}: passes");
        }
        if let Some(pdfjs) = &pdfjs {
            let read = marked_read(pdfjs, &script, &path, 1, &format!("{what}, {how:?}"));
            assert_eq!(read, want, "pdf.js, {what}, {how:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, or node and pdf.js"]
    a_flattened_page_reads_in_pdfjs,
    a_flattened_page_reads_in_pdfjs_appended: flattened_reads
);

/// A tagged page holding a filled text field and a mark of sheaf's saying
/// "Look", written `how`, then flattened and saved as planned, a Rewrite:
/// each file passes `qpdf --check`; pdf.js reads the page's own text alone
/// while they're annotations, and once flattened, each's text after it,
/// inside an `/Artifact` of its own, the field's in the text field's own
/// marked content, in the order the page listed them.
fn flattened_reads(how: How) {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch(&format!("interop-flatten-{how:?}"));
    let script = dir.join("marked.mjs");
    std::fs::write(&script, PDFJS_MARKED).unwrap();
    let mut form = Form::new(1);
    let field = form.text("Name", 0, [100.0, 600.0, 300.0, 620.0]);
    form.fill(field, "Ada");
    form.tag();
    let base = common::write(&dir, "flatten.pdf", &mut form);
    let mut doc = common::open(&base);
    let page = doc.page(0).unwrap();
    let look = Annot {
        id: AnnotId(0),
        kind: AnnotKind::FreeText {
            rect: Box2 {
                x: 72.0,
                y: 300.0,
                w: 200.0,
                h: 40.0,
            },
            size: 12.0,
        },
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: String::new(),
        contents: "Look".into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    doc.commit_ops(vec![add(page, &look)], "Look").unwrap();
    let marked = how.write(&doc);
    how.save(&mut doc, &base);
    let page = doc.page(0).unwrap();
    doc.commit_ops(vec![flatten(&[page], FlattenSet::All)], "Flatten")
        .unwrap();
    let (strategy, flattened) = common::planned(&mut doc, &base);
    assert_eq!(strategy, StrategyKind::Rewrite, "{how:?}: what went goes");
    let outputs = [
        ("marked", marked, "text \"Page 1\"".to_string()),
        (
            "flattened",
            flattened,
            "text \"Page 1\"\n\
             begin Artifact\nbegin Tx\ntext \"Ada\"\nend\nend\n\
             begin Artifact\ntext \"Look\"\nend"
                .to_string(),
        ),
    ];
    for (what, bytes, want) in outputs {
        let path = dir.join(format!("flatten-{what}-{how:?}.pdf"));
        std::fs::write(&path, &bytes).unwrap();
        if let Some(qpdf) = &qpdf {
            run(Command::new(qpdf).arg("--check").arg(&path))
                .unwrap_or_else(|e| panic!("qpdf --check, {what}, {how:?}: {e}"));
            println!("qpdf --check, {what}, {how:?}: passes");
        }
        if let Some(pdfjs) = &pdfjs {
            let read = marked_read(pdfjs, &script, &path, 1, &format!("{what}, {how:?}"));
            assert_eq!(read, want, "pdf.js, {what}, {how:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each export of an edited form (its page turned, a note on it) passes
/// `qpdf --check`, and pdf.js reads its page and the annotations it keeps:
/// an extract keeps the note and the field, a clean copy and a print
/// without markup the field alone, and a flattened copy and a print with
/// markup none, the field's value drawn into the page.
#[test]
#[ignore = "needs qpdf, or node and pdf.js"]
fn the_exports_read_in_pdfjs() {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch("interop-exports");
    let script = dir.join("pages.mjs");
    std::fs::write(&script, PDFJS_PAGES).unwrap();
    let [base, _] = written(&form());
    let path = dir.join("form.pdf");
    std::fs::write(&path, base).unwrap();
    let doc = edited(&path, None);
    let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
    let value = sheaf_doc::serialize::fixture::VALUE;
    let outputs: [(&str, Vec<u8>, usize); 5] = [
        (
            "extract",
            doc.export_bytes(|f, out| Export::extract(f, &ids, out)),
            2,
        ),
        (
            "print",
            doc.export_bytes(|f, _| Export::print(f, &ids, true)),
            0,
        ),
        (
            "print-clean",
            doc.export_bytes(|f, _| Export::print(f, &ids, false)),
            1,
        ),
        ("clean", doc.export_bytes(Export::clean_copy), 1),
        ("flattened", doc.export_bytes(Export::flattened), 0),
    ];
    for (what, bytes, annotations) in outputs {
        let out = dir.join(format!("{what}.pdf"));
        std::fs::write(&out, bytes).unwrap();
        if let Some(qpdf) = &qpdf {
            run(Command::new(qpdf).arg("--check").arg(&out))
                .unwrap_or_else(|e| panic!("qpdf --check, {what}: {e}"));
            println!("qpdf --check, {what}: passes");
        }
        if let Some(pdfjs) = &pdfjs {
            let read = run(Command::new("node").arg(&script).arg(pdfjs).arg(&out)).unwrap();
            println!("pdf.js, {what}: {read}");
            assert!(read.starts_with("pages 1\n"), "{what}: {read}");
            assert!(read.contains("Page 1"), "{what}: {read}");
            let kept = format!(", {annotations} annotations");
            assert!(read.contains(&kept), "{what}: {read}");
            if annotations == 0 {
                assert!(read.contains(value), "{what}: {read}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page sheaf wrote recognized text on, a block of its own
/// (`text_layer::fixture::block_page`), with a header another file's
/// generator drew merged into the page's own content, its list marked as
/// the generator's, given a watermark of the document's: its clean copy,
/// which holds none of sheaf's private data, passes `qpdf --check`, and
/// pdf.js reads the page as it read the file, then the watermark, inside
/// an `/Artifact` of its own.
#[test]
#[ignore = "needs qpdf, or node and pdf.js"]
fn a_clean_copy_reads_in_pdfjs() {
    let (qpdf, pdfjs) = (qpdf(), pdfjs());
    if qpdf.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch("interop-clean");
    let script = dir.join("marked.mjs");
    std::fs::write(&script, PDFJS_MARKED).unwrap();
    let mut pdf = sheaf_doc::text_layer::fixture::block_page();
    let page = *pdf.get_pages().get(&1).unwrap();
    let header: &[u8] = b"/Artifact <</Type/Pagination/Subtype/Header/SHEF_Gen (g1)>> BDC\n\
        q\nBT\n0 0 0 rg\n/F1 10 Tf\n1 0 0 1 72 748.8 Tm\n(Draft) Tj\nET\nQ\nEMC\n";
    let header = pdf.add_object(sheaf_doc::pdf::Stream::new(dictionary! {}, header.to_vec()));
    let dict = pdf.get_dictionary_mut(page).unwrap();
    let mut contents = dict.get(b"Contents").unwrap().as_array().unwrap().to_vec();
    // After the page's own content, inside the streams that isolate it.
    contents.insert(2, header.into());
    dict.set("Contents", contents);
    let base = dir.join("merged.pdf");
    pdf.save(&base).unwrap();
    let mut doc = common::open(&base);
    doc.commit_ops(
        vec![set_watermark(watermark("MINE"), Z::Over, None)],
        "Watermark",
    )
    .unwrap();
    let path = dir.join("clean.pdf");
    let clean = common::exported_to(&doc, Export::clean_copy, &path);
    let clean = sheaf_doc::pdf::Document::load_mem(&clean).unwrap();
    assert_eq!(sheaf_doc::private::found(&clean), []);
    for (what, path) in [("merged", &base), ("clean", &path)] {
        if let Some(qpdf) = &qpdf {
            run(Command::new(qpdf).arg("--check").arg(path))
                .unwrap_or_else(|e| panic!("qpdf --check, {what}: {e}"));
            println!("qpdf --check, {what}: passes");
        }
    }
    if let Some(pdfjs) = &pdfjs {
        let merged = marked_read(pdfjs, &script, &base, 1, "merged");
        assert!(
            merged.contains("Saved") && merged.contains("Draft"),
            "{merged}"
        );
        let read = marked_read(pdfjs, &script, &path, 1, "clean");
        let want = format!("{merged}\nbegin Artifact\ntext \"MINE\"\nend");
        assert_eq!(read, want, "pdf.js, clean");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// pdftoppm (poppler), `$SHEAF_PDFTOPPM` or `pdftoppm` on the PATH, or
/// None when there's none, saying so.
fn pdftoppm() -> Option<PathBuf> {
    let program =
        std::env::var_os("SHEAF_PDFTOPPM").map_or_else(|| "pdftoppm".into(), PathBuf::from);
    let found = Command::new(&program)
        .arg("-v")
        .output()
        .is_ok_and(|out| out.status.success());
    if !found {
        println!(
            "skipped pdftoppm: none at {} (set SHEAF_PDFTOPPM)",
            program.display()
        );
    }
    found.then_some(program)
}

/// Page 1 of the file at `path` as pdftoppm draws it, at the pixel checks'
/// scale (144 dpi is two pixels a point), RGBA.
fn poppler_drew(pdftoppm: &Path, path: &Path) -> Raster {
    let prefix = path.with_extension("poppler");
    let dpi = (72 * fields::SCALE).to_string();
    run(Command::new(pdftoppm)
        .args(["-r", &dpi, "-f", "1", "-l", "1", "-singlefile"])
        .arg(path)
        .arg(&prefix))
    .unwrap_or_else(|e| panic!("pdftoppm, {}: {e}", path.display()));
    ppm(&std::fs::read(prefix.with_extension("poppler.ppm")).unwrap())
}

/// A binary PPM (`P6`, 8 bits a sample) as an RGBA raster, opaque.
fn ppm(bytes: &[u8]) -> Raster {
    // P6, its width, its height and 255, each after one white-space byte.
    let mut words = bytes.splitn(5, |b| b.is_ascii_whitespace());
    let mut word = || {
        std::str::from_utf8(words.next().unwrap())
            .unwrap()
            .to_string()
    };
    assert_eq!(word(), "P6");
    let [width, height]: [u32; 2] = [word(), word()].map(|w| w.parse().unwrap());
    assert_eq!(word(), "255");
    let rgb = words.next().unwrap();
    assert_eq!(rgb.len(), (width * height * 3) as usize);
    let data = rgb
        .chunks(3)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    Raster {
        width,
        height,
        format: PixelFormat::Rgba,
        data,
    }
}

/// From M10 slice 6 a text field's appearance sheaf makes, for each pair
/// the pixel checks (`tests/fields.rs`) hold PDFium's own to, passes `qpdf
/// --check`, and pdftoppm draws it where it draws PDFium's: its ink inside
/// the widget's box, in the box PDFium's ink is in, and nothing outside.
#[test]
#[ignore = "needs qpdf and pdftoppm"]
fn text_field_appearances_pass_qpdf_and_poppler_draws_them_where_pdfium_s() {
    let (Some(qpdf), Some(pdftoppm)) = (qpdf(), pdftoppm()) else {
        return;
    };
    let dir = scratch("fields-interop");
    let mut failed = Vec::new();
    let pairs = fields::held_pairs();
    for (ours_case, theirs_case) in &pairs {
        let (name, px) = (ours_case.file_name(), ours_case.pixels());
        let saved = fields::ours(ours_case).saved;
        let copy = fields::theirs(theirs_case);
        let [ours, pdfium] = [("sheaf", &saved), ("pdfium", &copy)].map(|(what, bytes)| {
            let path = dir.join(format!("{name}.{what}.pdf"));
            std::fs::write(&path, bytes).unwrap();
            path
        });
        if let Err(e) = run(Command::new(&qpdf).arg("--check").arg(&ours)) {
            failed.push(format!("{name}: qpdf --check: {e}"));
        }
        let [drawn, theirs] = [&ours, &pdfium].map(|p| poppler_drew(&pdftoppm, p));
        let [inked, pdfium_inked] = [&drawn, &theirs].map(|r| fields::ink_box(r, px));
        let outside = fields::ink(&drawn, px, false);
        let differ = fields::differing_in(&drawn, &theirs, px);
        println!("{name}\tink {inked:?}, PDFium's {pdfium_inked:?}\t{differ} px differ");
        if inked.is_none() || inked != pdfium_inked || outside != 0 {
            failed.push(format!(
                "{name}: ink {inked:?} ({outside} px outside), PDFium's {pdfium_inked:?}"
            ));
        }
    }
    println!(
        "qpdf --check and pdftoppm: {} outputs, {} fail",
        pairs.len(),
        failed.len()
    );
    assert!(failed.is_empty(), "{failed:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What pdf.js makes of each file given after pdf.js's `pdf.mjs`: `file`
/// and its path, then each warning pdf.js gives while it reads and draws
/// it (a line `Warning: …` of its own; pdf.js says them through
/// `console.warn`, which node writes to stderr, so the script says them
/// on stdout instead, under the file they're of), the first page drawn at
/// two pixels a point with its annotations' appearances, and each
/// annotation's subtype, field type and the text pdf.js reads of its
/// appearance, as JSON.
const PDFJS_WARNED: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

console.warn = (...said) => console.log(said.join(" "));
const [lib, ...files] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const standardFontDataUrl = new URL("../../standard_fonts/", pathToFileURL(lib)).pathname;
for (const file of files) {
  console.log(`file ${file}`);
  const data = new Uint8Array(readFileSync(file));
  const verbosity = pdfjs.VerbosityLevel.WARNINGS;
  const task = pdfjs.getDocument({ data, standardFontDataUrl, verbosity });
  const doc = await task.promise;
  const page = await doc.getPage(1);
  const viewport = page.getViewport({ scale: 2 });
  const { canvas, context } = doc.canvasFactory.create(viewport.width, viewport.height);
  await page.render({ canvas, canvasContext: context, viewport }).promise;
  for (const a of await page.getAnnotations()) {
    const text = (a.textContent ?? []).join("\n");
    console.log(`annotation ${JSON.stringify([a.subtype, a.fieldType ?? null, text])}`);
  }
  await task.destroy();
}
"#;

/// What pdf.js 6.4.299 warns of a push button that has no action (`/A` or
/// `/AA`) and no tooltip: it draws the button's appearance and gives it
/// nothing to do. The pixel checks' push buttons have none.
const PDFJS_NO_ACTION: &str = "Warning: Push buttons without action dictionaries are not supported";

/// The warnings among `lines`, what [`PDFJS_WARNED`] printed under one
/// file: how many are the one a push button with no action gets
/// ([`PDFJS_NO_ACTION`]), counted only for a file that `has_push`, and
/// every other line that says `Warning`.
fn warned<'a>(lines: &[&'a str], has_push: bool) -> (usize, Vec<&'a str>) {
    let warnings = lines.iter().filter(|l| l.contains("Warning"));
    let (allowed, other): (Vec<&str>, Vec<&str>) =
        warnings.partition(|&&l| has_push && l == PDFJS_NO_ACTION);
    (allowed.len(), other)
}

/// A warning is any line that says so; only a push button's file may have
/// the one of no action, and that line exactly.
#[test]
fn pdfjs_warnings_are_told_from_the_one_a_push_button_gets() {
    let lines = [
        "annotation [\"Widget\",\"Btn\",\"\"]",
        PDFJS_NO_ACTION,
        "Warning: Indexing all PDF objects",
        PDFJS_NO_ACTION,
    ];
    assert_eq!(
        warned(&lines, true),
        (2, vec!["Warning: Indexing all PDF objects"])
    );
    let (allowed, other) = warned(&lines, false);
    assert_eq!((allowed, other.len()), (0, 3));
    assert_eq!(warned(&lines[..1], true), (0, vec![]));
    let longer = format!("{PDFJS_NO_ACTION} here");
    assert_eq!(warned(&[&longer], true), (0, vec![&*longer]));
}

/// From M10 slice 8 a check box's and a radio button's appearances sheaf
/// makes, in each case the pixel checks (`tests/buttons.rs`) hold to
/// PDFium's own (each frame off, framed and not, and each mark on), a combo
/// box's, a list box's and a push button's, each saved: every file passes
/// `qpdf --check`; pdftoppm draws each where it draws PDFium's, its ink in
/// the box PDFium's is in and none outside it (but where it draws the page
/// pixel for pixel as PDFium's: a round frame's ring meeting the box's
/// side, which its anti-aliasing takes a pixel past for both), a mark (the
/// pixels its on state draws otherwise than its `Off`) within the pixel
/// checks' tolerance of where PDFium's reaches and [`INK_APART`] more on
/// each edge, as poppler draws ZapfDingbats in a face of its own, and
/// nothing at all for an `Off` with no frame; and pdf.js reads every file,
/// draws its page and lists its widget, and warns of nothing ([`warned`])
/// but, of each push button, that it supports none without an action
/// ([`PDFJS_NO_ACTION`]): these have none, and it says so of each, which
/// also shows its warnings are heard.
#[test]
#[ignore = "needs qpdf, pdftoppm, or node and pdf.js"]
fn button_and_choice_appearances_pass_qpdf_and_other_readers_draw_them() {
    use common::buttons::{self, Shows};

    let (qpdf, pdftoppm, pdfjs) = (qpdf(), pdftoppm(), pdfjs());
    if qpdf.is_none() && pdftoppm.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch("buttons-interop");
    let cases: Vec<buttons::Case> = [
        buttons::toggle_frames(),
        buttons::marks(),
        buttons::choices(),
        buttons::pushes(),
    ]
    .into_iter()
    .flatten()
    .map(|(case, _, _)| case)
    .collect();
    let mut failed = Vec::new();
    let mut files = Vec::new();
    let mut pushes = std::collections::HashSet::new();
    let (mut bare, mut marked, mut boxed) = (0, 0, 0);
    for case in &cases {
        let (name, px) = (case.file_name(), case.pixels());
        let saved = buttons::ours(case).saved;
        let copy = buttons::theirs(case);
        let [ours, pdfium] = [("sheaf", &saved), ("pdfium", &copy)].map(|(what, bytes)| {
            let path = dir.join(format!("{name}.{what}.pdf"));
            std::fs::write(&path, bytes).unwrap();
            path
        });
        files.push(ours.clone());
        if matches!(case.shows, Shows::Push { .. }) {
            pushes.insert(ours.display().to_string());
        }
        if let Some(qpdf) = &qpdf
            && let Err(e) = run(Command::new(qpdf).arg("--check").arg(&ours))
        {
            failed.push(format!("{name}: qpdf --check: {e}"));
        }
        let Some(pdftoppm) = &pdftoppm else {
            continue;
        };
        let drew = |path: &Path| poppler_drew(pdftoppm, path);
        let [drawn, theirs] = [&ours, &pdfium].map(|p| drew(p));
        let outside = fields::ink(&drawn, px, false);
        let on = matches!(case.shows, Shows::Toggle { on: true, .. });
        if !case.paints() {
            bare += 1;
            let inside = fields::ink(&drawn, px, true);
            println!("{name}\tpdftoppm inks {inside} px in the box, {outside} outside");
            if inside + outside != 0 {
                failed.push(format!(
                    "{name}: pdftoppm inks {inside} px, {outside} outside"
                ));
            }
        } else if on {
            marked += 1;
            // Each side's mark: its on state drawn against its `Off`.
            let mark = |path: &Path, drawn: &Raster| {
                let off = path.with_extension("off.pdf");
                std::fs::write(&off, buttons::showing(&std::fs::read(path).unwrap(), "Off"))
                    .unwrap();
                buttons::differing_box(drawn, &drew(&off), px)
            };
            let [ours_mark, pdfium_mark] =
                [(&ours, &drawn), (&pdfium, &theirs)].map(|(p, r)| mark(p, r));
            let apart = (ours_mark.zip(pdfium_mark))
                .map(|(a, b)| buttons::mark_apart(case, a, b, INK_APART as f32));
            println!("{name}\tpdftoppm's mark {ours_mark:?}, PDFium's {pdfium_mark:?}: {apart:?}");
            let near = apart.is_some_and(|a| a.iter().all(|(d, allowed)| d <= allowed));
            if !near || outside != 0 {
                failed.push(format!(
                    "{name}: mark {ours_mark:?} ({outside} px outside), PDFium's {pdfium_mark:?}"
                ));
            }
        } else {
            boxed += 1;
            let [inked, pdfium_inked] = [&drawn, &theirs].map(|r| fields::ink_box(r, px));
            // Ink outside the box is held where pdftoppm draws the page as
            // it draws PDFium's, pixel for pixel: a round frame's ring
            // meeting the box's side, which its anti-aliasing takes a pixel
            // past, as it does PDFium's.
            let whole = [0, 0, drawn.width as usize, drawn.height as usize];
            let alike = fields::differing_in(&drawn, &theirs, whole) == 0;
            let outside = if alike { 0 } else { outside };
            println!("{name}\tink {inked:?}, PDFium's {pdfium_inked:?}; the page alike: {alike}");
            if inked.is_none() || inked != pdfium_inked || outside != 0 {
                failed.push(format!(
                    "{name}: ink {inked:?} ({outside} px outside), PDFium's {pdfium_inked:?}"
                ));
            }
        }
    }
    if let Some(pdfjs) = &pdfjs {
        let script = dir.join("warned.mjs");
        std::fs::write(&script, PDFJS_WARNED).unwrap();
        let said = run(Command::new("node").arg(&script).arg(pdfjs).args(&files))
            .unwrap_or_else(|e| panic!("pdf.js: {e}"));
        let (mut read, mut no_action) = (0, 0);
        for block in said.split("file ").skip(1) {
            let (path, lines) = block.split_once('\n').unwrap();
            let lines: Vec<&str> = lines.lines().collect();
            let widgets = (lines.iter())
                .filter(|l| l.starts_with("annotation [\"Widget\""))
                .count();
            read += 1;
            let push = pushes.contains(path);
            let (allowed, other) = warned(&lines, push);
            no_action += allowed;
            // A push button's one warning is said, so warnings are heard.
            if !other.is_empty() || widgets != 1 || (push && allowed == 0) {
                failed.push(format!(
                    "pdf.js, {path}: {widgets} widgets, {allowed} warnings of no action, \
                     warned {other:?}"
                ));
            }
        }
        println!(
            "pdf.js read {read} files, {no_action} warnings of a push button with no action; \
             its annotations:\n{said}"
        );
        assert_eq!(read, files.len(), "pdf.js read every file");
    }
    println!(
        "qpdf --check and pdftoppm: {} outputs ({bare} Off with no frame, {marked} marks, \
         {boxed} by their ink's box), {} fail",
        cases.len(),
        failed.len()
    );
    assert!(failed.is_empty(), "{failed:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What pdf.js draws of a file's first page, at two pixels a point and
/// with its annotations' appearances (pdf.js's default), as a binary PPM to
/// the path given third; then each annotation's subtype and the text pdf.js
/// reads of its appearance, its lines joined by `\n`. The standard 14 fonts
/// a file doesn't embed are drawn in pdf.js's own faces, beside it.
const PDFJS_DRAWN: &str = r#"
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [lib, file, out] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const standardFontDataUrl = new URL("../../standard_fonts/", pathToFileURL(lib)).pathname;
const data = new Uint8Array(readFileSync(file));
const doc = await pdfjs.getDocument({ data, standardFontDataUrl, verbosity: 0 }).promise;
const page = await doc.getPage(1);
const viewport = page.getViewport({ scale: 2 });
const { width, height } = viewport;
const { canvas, context } = doc.canvasFactory.create(width, height);
await page.render({ canvas, canvasContext: context, viewport }).promise;
const rgba = context.getImageData(0, 0, width, height).data;
const rgb = Buffer.alloc(width * height * 3);
for (let i = 0, j = 0; i < rgba.length; i += 4, j += 3) {
  rgb[j] = rgba[i];
  rgb[j + 1] = rgba[i + 1];
  rgb[j + 2] = rgba[i + 2];
}
writeFileSync(out, Buffer.concat([Buffer.from(`P6\n${width} ${height}\n255\n`), rgb]));
for (const annotation of await page.getAnnotations()) {
  const text = (annotation.textContent ?? []).join("\n");
  console.log(`${annotation.subtype} ${JSON.stringify(text)}`);
}
"#;

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, pdftoppm, or node and pdf.js"]
    text_in_two_fonts_draws_alike_in_other_readers,
    text_in_two_fonts_draws_alike_in_other_readers_appended: two_fonts_drawn
);

/// What a note in [`two_fonts_drawn`] says: three cities' names, each with
/// letters Helvetica's WinAnsi lacks, in Latin, Greek and Cyrillic.
const CITIES: &str = "\u{141}\u{f3}d\u{17a}, \u{391}\u{3b8}\u{3ae}\u{3bd}\u{3b1}, \
                      \u{41c}\u{43e}\u{441}\u{43a}\u{432}\u{430}";

/// How many bytes, after Flate, the `/CIDToGIDMap` of [`two_fonts_drawn`]'s
/// subset may take: 80 as measured, for its 14 glyphs, the last of them
/// glyph 795 of the program, so a map of 1,592 bytes before Flate (two a
/// CID; at most 7,768, for the program's 3,884 glyphs). Unfiltered, it
/// would be past this.
const MAP_AFTER_FLATE: usize = 256;

/// How many pixels a side of what one reader inks in an annotation's box
/// may lie from where PDFium inks it: anti-aliasing, and the faces poppler
/// draws the standard 14 in.
const INK_APART: usize = 2;

/// From M10 slice 7 a text field's value and a note, each drawn in its
/// font and the fallback font sheaf bundles for what Helvetica lacks,
/// written `how`: the file holds one subset of the fallback, passes `qpdf
/// --check`, and PDFium (appearances only), pdftoppm and pdf.js each draw
/// it pixel for pixel as they draw the same file with the whole program in
/// place of the subset ([`with_whole_fallback`]), and not as they draw its
/// twin, the subset's map taken away ([`with_identity_map`]), so each reads
/// the subset's `/CIDToGIDMap` stream right; their ink is in the same
/// boxes, within [`INK_APART`], and pdf.js reads the value and the note
/// back from their appearances. The map's size after Flate is held under
/// [`MAP_AFTER_FLATE`].
///
/// [`with_whole_fallback`]: sheaf_doc::content::fixture::with_whole_fallback
/// [`with_identity_map`]: sheaf_doc::content::fixture::with_identity_map
fn two_fonts_drawn(how: How) {
    use sheaf_doc::content::fixture::{
        FALLBACK_TEXT, fallback_subsets, subsets_hold, with_identity_map, with_whole_fallback,
    };
    use sheaf_doc::pdf::{Document as Pdf, StreamExt};
    use sheaf_render::PageIndex;
    use sheaf_render::pdfium::PdfiumEngine;

    let (qpdf, pdftoppm, pdfjs) = (qpdf(), pdftoppm(), pdfjs());
    let dir = scratch(&format!("interop-two-fonts-{how:?}"));
    let mut form = Form::new(1);
    form.text("Name", 0, [72.0, 600.0, 400.0, 624.0]);
    let base = common::write(&dir, "two-fonts.pdf", &mut form);
    let mut doc = common::open(&base);
    sheaf_doc::tx::fixture::fill(&mut doc, "Name", FALLBACK_TEXT);
    let page = doc.page(0).unwrap();
    let mut cities = note(CITIES);
    cities.kind = AnnotKind::FreeText {
        rect: Box2 {
            x: 72.0,
            y: 300.0,
            w: 300.0,
            h: 40.0,
        },
        size: 14.0,
    };
    let c = doc.commit_ops(vec![add(page, &cities)], "Note").unwrap();
    assert!(c.notices.is_empty(), "{how:?}: {:?}", c.notices);
    let bytes = how.write(&doc);
    let saved = dir.join(format!("two-fonts-{how:?}.pdf"));
    std::fs::write(&saved, &bytes).unwrap();

    // One subset, every code shown in it a glyph of its cut with a text.
    let subsets = fallback_subsets(&bytes);
    let [(font, glyphs)] = subsets.as_slice() else {
        panic!("{how:?}: {} subsets, not one", subsets.len());
    };
    let shown = subsets_hold(&bytes);
    assert!(shown > 0, "{how:?}: nothing shown in the subset");

    // Its map, as the save wrote it: Flate, and small.
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let resolved = |object: &Object| pdf.dereference(object).unwrap().1.clone();
    let font = pdf.get_dictionary(*font).unwrap();
    let descendants = resolved(font.get(b"DescendantFonts").unwrap());
    let descendant = resolved(&descendants.as_array().unwrap()[0]);
    let map = resolved(descendant.as_dict().unwrap().get(b"CIDToGIDMap").unwrap());
    let map = map.as_stream().expect("a subset's map is a stream");
    let filter = map.dict.get(b"Filter").and_then(Object::as_name);
    assert_eq!(
        filter,
        Some(&b"FlateDecode"[..]),
        "{how:?}: its map's filter"
    );
    let (stored, decoded) = (
        map.raw_bytes().len(),
        map.decompressed_content().unwrap().len(),
    );
    let last = glyphs.iter().max().unwrap();
    println!(
        "{how:?}: one subset of {} glyphs, the last {last}, {shown} codes shown in it; its \
         /CIDToGIDMap {decoded} bytes, {stored} after Flate",
        glyphs.len()
    );
    assert_eq!(
        decoded,
        2 * (usize::from(*last) + 1),
        "{how:?}: two bytes a CID"
    );
    assert!(
        stored < MAP_AFTER_FLATE,
        "{how:?}: {stored} bytes after Flate"
    );

    if let Some(qpdf) = &qpdf {
        run(Command::new(qpdf).arg("--check").arg(&saved))
            .unwrap_or_else(|e| panic!("qpdf --check, {how:?}: {e}"));
        println!("qpdf --check, {how:?}: passes");
    }

    // The same file with the whole program for its subset, and its twin,
    // the subset's map taken away: neither has a map stream.
    let [whole, twin] = [
        ("whole", with_whole_fallback(&bytes)),
        ("twin", with_identity_map(&bytes)),
    ]
    .map(|(what, made)| {
        assert_eq!(subsets_hold(&made), 0, "{how:?}, {what}: no map");
        let path = dir.join(format!("two-fonts-{how:?}-{what}.pdf"));
        std::fs::write(&path, made).unwrap();
        path
    });

    // Each annotation's box, by its subtype, in pixels from the top left.
    let mut boxes = Vec::new();
    let annots = pdf
        .get_page_annotations(pdf.page_iter().next().unwrap())
        .unwrap();
    for annot in annots {
        let subtype = annot.get(b"Subtype").and_then(Object::as_name).unwrap();
        let rect = resolved(annot.get(b"Rect").unwrap());
        let [x0, y0, x1, y1]: [f64; 4] = (rect.as_array().unwrap().iter())
            .map(|n| n.as_number().unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let px = |v: f64| (2.0 * v) as usize;
        let at = [px(x0), px(792.0 - y1), px(x1) + 1, px(792.0 - y0) + 1];
        boxes.push((String::from_utf8_lossy(subtype).into_owned(), at));
    }
    let names: Vec<&str> = boxes.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(names, ["Widget", "FreeText"], "{how:?}");

    // Each reader's drawing of the file, of it whole and of its twin: the
    // first two the same pixels, the twin's not.
    let pdfium = |path: &Path| {
        let bytes = std::fs::read(path).unwrap();
        let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
        fields::flat_at(&engine, PageIndex(0), 2 * 612).unwrap()
    };
    let pdfjs_drew = |pdfjs: &Path, script: &Path, path: &Path| {
        let out = path.with_extension("pdfjs.ppm");
        let said = run(Command::new("node")
            .arg(script)
            .arg(pdfjs)
            .arg(path)
            .arg(&out))
        .unwrap_or_else(|e| panic!("pdf.js, {}: {e}", path.display()));
        (ppm(&std::fs::read(&out).unwrap()), said)
    };
    let files = [&saved, &whole, &twin];
    let mut drawn = vec![("PDFium", files.map(|p| pdfium(p)))];
    if let Some(pdftoppm) = &pdftoppm {
        drawn.push(("pdftoppm", files.map(|p| poppler_drew(pdftoppm, p))));
    }
    if let Some(pdfjs) = &pdfjs {
        let script = dir.join("drawn.mjs");
        std::fs::write(&script, PDFJS_DRAWN).unwrap();
        let [(ours, said), (whole, _), (twin, _)] = files.map(|p| pdfjs_drew(pdfjs, &script, p));
        println!("pdf.js, {how:?}, its annotations:\n{said}");
        let read = said.lines().map(|line| line.split_once(' ').unwrap());
        // By subtype: pdf.js lists widgets after other annotations.
        let read: BTreeMap<&str, String> = read
            .map(|(subtype, text)| (subtype, serde_json::from_str(text).unwrap()))
            .collect();
        let want = [("Widget", FALLBACK_TEXT), ("FreeText", CITIES)];
        let want: BTreeMap<&str, String> = want.iter().map(|&(s, t)| (s, t.into())).collect();
        assert_eq!(read, want, "{how:?}: what pdf.js reads of the appearances");
        drawn.push(("pdf.js", [ours, whole, twin]));
    }
    let mut inks: Vec<(&str, Vec<[usize; 4]>)> = Vec::new();
    for (reader, [ours, whole, twin]) in &drawn {
        let page = [0, 0, ours.width as usize, ours.height as usize];
        let [apart, twin_apart] =
            [whole, twin].map(|other| fields::differing_in(ours, other, page));
        let inked: Vec<[usize; 4]> = (boxes.iter())
            .map(|(subtype, at)| {
                (fields::ink_box(ours, *at))
                    .unwrap_or_else(|| panic!("{how:?}, {reader}: no ink in the {subtype}"))
            })
            .collect();
        println!(
            "{how:?}, {reader}: {apart} px apart from the whole program's, {twin_apart} from the \
             twin's; ink {inked:?}"
        );
        assert_eq!(
            apart, 0,
            "{how:?}, {reader}: the subset draws as the whole program"
        );
        assert!(
            twin_apart > 0,
            "{how:?}, {reader}: the twin draws as the subset"
        );
        inks.push((reader, inked));
    }
    let (_, pdfium_ink) = &inks[0];
    for (reader, inked) in &inks[1..] {
        for ((subtype, _), (ours, theirs)) in boxes.iter().zip(inked.iter().zip(pdfium_ink)) {
            let near = (ours.iter().zip(theirs)).all(|(a, b)| a.abs_diff(*b) <= INK_APART);
            assert!(
                near,
                "{how:?}, {reader}: the {subtype} inked at {ours:?}, PDFium {theirs:?}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    #[ignore = "needs qpdf, pdftoppm, or node and pdf.js"]
    a_form_filled_of_each_kind_reads_in_other_readers,
    a_form_filled_of_each_kind_reads_in_other_readers_appended: each_kind_filled
);

/// A field of each kind a value is set in, on one page of a form built in
/// code: a text field `name`; a check box `agree`, off; a radio group
/// `pick` of two buttons, neither on; a combo box `state` (CA, NY); and a
/// list box `kind` (W-2, 1099). Each widget's box, by its field's name.
fn of_each_kind() -> (Form, Vec<(&'static str, [f32; 4])>) {
    use sheaf_doc::forms::fixture::row;
    let mut form = Form::new(1);
    let [x0, y0, _, y1] = row(2);
    let buttons = [[x0, y0, x0 + 20.0, y1], [x0 + 40.0, y0, x0 + 60.0, y1]];
    form.text("name", 0, row(0));
    form.check_box("agree", 0, row(1), false);
    form.radio("pick", 0, &buttons, None);
    form.choice("state", 0, row(3), &["CA", "NY"], true);
    let mut list = row(5);
    list[1] -= 20.0;
    form.choice("kind", 0, list, &["W-2", "1099"], false);
    let boxes = vec![
        ("name", row(0)),
        ("agree", row(1)),
        ("pick", buttons[0]),
        ("pick", buttons[1]),
        ("state", row(3)),
        ("kind", list),
    ];
    (form, boxes)
}

/// What pdf.js reads of each widget on a file's first page, for each file
/// given after pdf.js's `pdf.mjs`: `file` and its path, then each warning
/// it gives (a line `Warning: …`, said on stdout as [`PDFJS_WARNED`] says
/// them), and a line `field` with the widget's field name, field value,
/// button value (a radio button's on state) and the text it reads of its
/// appearance, as JSON. The page is drawn first, so the appearances are
/// read as drawn.
const PDFJS_FIELDS: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

console.warn = (...said) => console.log(said.join(" "));
const [lib, ...files] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const standardFontDataUrl = new URL("../../standard_fonts/", pathToFileURL(lib)).pathname;
for (const file of files) {
  console.log(`file ${file}`);
  const data = new Uint8Array(readFileSync(file));
  const verbosity = pdfjs.VerbosityLevel.WARNINGS;
  const task = pdfjs.getDocument({ data, standardFontDataUrl, verbosity });
  const doc = await task.promise;
  const page = await doc.getPage(1);
  const viewport = page.getViewport({ scale: 2 });
  const { canvas, context } = doc.canvasFactory.create(viewport.width, viewport.height);
  await page.render({ canvas, canvasContext: context, viewport }).promise;
  for (const a of await page.getAnnotations()) {
    const text = (a.textContent ?? []).join("\n");
    const said = [a.fieldName ?? null, a.fieldValue ?? null, a.buttonValue ?? null, text];
    console.log(`field ${JSON.stringify(said)}`);
  }
  await task.destroy();
}
"#;

/// From M10 slice 9 a form of each kind of field a value is set in
/// ([`of_each_kind`]), each filled through `SetFieldValues` (a text, the
/// check box on, the radio group's second button pressed, an option of
/// each choice) and written `how`: qpdf passes `--check` on it and reads
/// each field's value (`--json`'s form); pdf.js reads every widget's field
/// value, and each radio button's on state, the text field's text from its
/// appearance (it reads no other kind's), and warns of nothing; and
/// pdftoppm draws the page with
/// each widget's ink in the box PDFium's ink is in, within [`INK_APART`]
/// (none for the radio button that's off, as PDFium draws none), and
/// nothing else of the page otherwise than it draws the form as built.
fn each_kind_filled(how: How) {
    use sheaf_doc::tx::fixture as fx;
    use sheaf_render::PageIndex;
    use sheaf_render::pdfium::PdfiumEngine;

    let (qpdf, pdftoppm, pdfjs) = (qpdf(), pdftoppm(), pdfjs());
    if qpdf.is_none() && pdftoppm.is_none() && pdfjs.is_none() {
        return;
    }
    let dir = scratch(&format!("interop-each-kind-{how:?}"));
    let (mut form, boxes) = of_each_kind();
    let base = common::write(&dir, "kinds.pdf", &mut form);
    let mut doc = common::open(&base);
    fx::fill(&mut doc, "name", "Camden Lee");
    fx::set_buttons(&mut doc, "agree", "Yes");
    let st = doc.state();
    let pick = st.field_named("pick").unwrap();
    let second = st.field(pick).unwrap().term.as_ref().unwrap().widgets[1];
    fx::press(&mut doc, second);
    fx::choose(&mut doc, "state", &["NY"]);
    fx::choose(&mut doc, "kind", &["1099"]);
    let saved = dir.join(format!("kinds-{how:?}.pdf"));
    std::fs::write(&saved, how.write(&doc)).unwrap();
    // What each reader should read: the field's value by its name.
    let want = [
        ("name", "Camden Lee"),
        ("agree", "Yes"),
        ("pick", "1"),
        ("state", "NY"),
        ("kind", "1099"),
    ];

    if let Some(qpdf) = &qpdf {
        run(Command::new(qpdf).arg("--check").arg(&saved))
            .unwrap_or_else(|e| panic!("qpdf --check, {how:?}: {e}"));
        let said = run(Command::new(qpdf)
            .args(["--json", "--json-key=acroform"])
            .arg(&saved))
        .unwrap_or_else(|e| panic!("qpdf --json, {how:?}: {e}"));
        let json: serde_json::Value = serde_json::from_str(&said).unwrap();
        // qpdf's JSON says a text string `u:…` and a name `/…`.
        let read: Vec<(String, String)> = (json["acroform"]["fields"].as_array().unwrap().iter())
            .map(|f| {
                let value = f["value"].as_str().unwrap_or_default();
                let value =
                    (value.strip_prefix("u:").or_else(|| value.strip_prefix('/'))).unwrap_or(value);
                (f["fullname"].as_str().unwrap().to_string(), value.into())
            })
            .collect();
        println!("qpdf, {how:?}: passes --check, reads {read:?}");
        let mut wanted: Vec<(String, String)> = (want.iter())
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        // A widget a line: the radio group's two.
        wanted.insert(3, ("pick".into(), "1".into()));
        assert_eq!(read, wanted, "qpdf, {how:?}");
    }

    if let Some(pdfjs) = &pdfjs {
        let script = dir.join("fields.mjs");
        std::fs::write(&script, PDFJS_FIELDS).unwrap();
        let said = run(Command::new("node").arg(&script).arg(pdfjs).arg(&saved))
            .unwrap_or_else(|e| panic!("pdf.js, {how:?}: {e}"));
        println!("pdf.js, {how:?}:\n{said}");
        let lines: Vec<&str> = said.lines().collect();
        let (_, warnings) = warned(&lines, false);
        assert!(warnings.is_empty(), "pdf.js, {how:?}: {warnings:?}");
        let fields: Vec<serde_json::Value> = (lines.iter())
            .filter_map(|l| l.strip_prefix("field "))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        // pdf.js gives a choice's value as the array of its texts.
        let value = |f: &serde_json::Value| match &f[1] {
            serde_json::Value::Array(items) => (items.iter())
                .map(|i| i.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
                .join(","),
            v => v.as_str().unwrap_or_default().to_string(),
        };
        let read: Vec<(String, String)> = (fields.iter())
            .map(|f| (f[0].as_str().unwrap().to_string(), value(f)))
            .collect();
        let mut wanted: Vec<(String, String)> = (want.iter())
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        wanted.insert(3, ("pick".into(), "1".into()));
        assert_eq!(read, wanted, "pdf.js, {how:?}");
        let buttons: Vec<&str> = (fields.iter())
            .filter(|f| f[0] == "pick")
            .map(|f| f[2].as_str().unwrap())
            .collect();
        assert_eq!(buttons, ["0", "1"], "pdf.js, {how:?}: the radio buttons");
        // The text a widget's appearance shows, which pdf.js reads of a
        // text field's alone.
        let texts: Vec<(&str, &str)> = (fields.iter())
            .map(|f| (f[0].as_str().unwrap(), f[3].as_str().unwrap()))
            .filter(|(_, text)| !text.is_empty())
            .collect();
        let shown = [("name", "Camden Lee")];
        assert_eq!(texts, shown, "pdf.js, {how:?}: what the appearances read");
    }

    if let Some(pdftoppm) = &pdftoppm {
        let [drawn, was] = [&saved, &base].map(|p| poppler_drew(pdftoppm, p));
        let engine = PdfiumEngine::open_bytes(std::fs::read(&saved).unwrap().into(), None).unwrap();
        let pdfium = fields::flat_at(&engine, PageIndex(0), drawn.width).unwrap();
        let mut inside = 0;
        for (name, [x0, y0, x1, y1]) in &boxes {
            let px = |v: f32| (2.0 * v) as usize;
            let at = [px(*x0), px(792.0 - y1), px(*x1) + 1, px(792.0 - y0) + 1];
            let [ours, theirs] = [&drawn, &pdfium].map(|r| fields::ink_box(r, at));
            println!("pdftoppm, {how:?}: {name} inked at {ours:?}, PDFium at {theirs:?}");
            let near = match (ours, theirs) {
                (Some(a), Some(b)) => (a.iter().zip(b)).all(|(a, b)| a.abs_diff(b) <= INK_APART),
                (a, b) => a == b,
            };
            assert!(
                near,
                "pdftoppm, {how:?}: {name} at {ours:?}, PDFium {theirs:?}"
            );
            inside += fields::differing_in(&drawn, &was, at);
        }
        let page = [0, 0, drawn.width as usize, drawn.height as usize];
        let outside = fields::differing_in(&drawn, &was, page) - inside;
        assert_eq!(outside, 0, "pdftoppm, {how:?}: drawn outside the widgets");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
