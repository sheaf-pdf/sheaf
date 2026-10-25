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
//! qpdf is `$SHEAF_QPDF`, else `qpdf` on the PATH; pdf.js is `node` on the
//! PATH with `$SHEAF_PDFJS`, pdfjs-dist's `legacy/build/pdf.mjs`. A test
//! whose tools are missing says so and passes. Run with
//! `cargo test -p sheaf-ops --test interop -- --ignored --nocapture`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use lopdf::{Dictionary, Object, dictionary};
use sheaf_doc::export::{Export, ExportFrom, Out};
use sheaf_doc::forms::fixture::{Form, outline_tree};
use sheaf_doc::materialize::{How, StrategyKind};
use sheaf_doc::serialize::fixture::{
    PREFIX, USER, attachment_only, edited, encrypted, form, object_streams_encrypted,
    rc4_aes_128_and_aes_256, written,
};
use sheaf_doc::tx::fixture::{
    add, clear_ocr, delete, delete_named, flatten, header, move_pages, ocr, patch_bookmark,
    remove_header, remove_watermark, rotate, set_header, set_watermark, watermark,
};
use sheaf_doc::{Annot, AnnotId, AnnotKind, Box2, DestView, OcrWord, PageId};
use sheaf_edit::{
    BatesSpec, BookmarkPatch, FlattenSet, GridPos, Margins, Op, PageRef, Rotation, Sibling, Turn, Z,
};

mod common;
use common::scratch;

/// qpdf, or None when there's none, saying so.
fn qpdf() -> Option<PathBuf> {
    let qpdf = std::env::var_os("SHEAF_QPDF").map_or_else(|| PathBuf::from("qpdf"), PathBuf::from);
    let found = Command::new(&qpdf)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if !found {
        println!("skipped qpdf: none at {} (set SHEAF_QPDF)", qpdf.display());
    }
    found.then_some(qpdf)
}

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
/// bytes before its header; encrypted with RC4-128, AES-128, AES-256 and,
/// for its attachment only, `/EFF`; and encrypted with AES-256 in an
/// object stream. Each with its name and the password it opens with.
fn bases_of(plain: &lopdf::Document) -> Vec<(String, Vec<u8>, Option<&'static str>)> {
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
    let mut states = rc4_aes_128_and_aes_256(&plain).to_vec();
    states.push(("/EFF", attachment_only(&plain)));
    for (name, state) in &states {
        let [table, stream] = encrypted(&plain, state, |_| {});
        bases.push((format!("{name}, table"), table, Some(USER)));
        bases.push((format!("{name}, stream"), stream, Some(USER)));
    }
    let aes_256 = &states[2].1;
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
        let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
        let exported = |export: fn(ExportFrom, Out) -> Export, how: &str| {
            let path = dir.join(format!("{file}-{how}.pdf"));
            common::exported_to(&doc, export, &path)
        };
        let saved = (format!("{how:?}"), how.write(&doc));
        if how == How::Appended {
            outputs_checked(
                &qpdf,
                &dir,
                &file,
                &name,
                password,
                [saved],
                &mut checked,
                &mut failed,
            );
            continue;
        }
        let outputs = [
            saved,
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
        ];
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

/// What pdf.js reads of a file: its pages, and each one's text and how
/// many annotations it has.
const PDFJS_PAGES: &str = r#"
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [lib, file] = process.argv.slice(2);
const pdfjs = await import(pathToFileURL(lib).href);
const data = new Uint8Array(readFileSync(file));
const doc = await pdfjs.getDocument({ data, verbosity: 0 }).promise;
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
    let id = Object::string_literal("0123456789abcdef");
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

/// A file of three pages whose first has another tool's link and two of
/// its comments, `/NM` `theirs` and `kept`, and whose outline has a
/// bookmark that opens a URI, a structure element its `/SE`, and one to
/// the third page.
fn records_base() -> Vec<u8> {
    fn text(s: &str) -> Object {
        Object::string_literal(s)
    }
    let mut f = Form::new(3);
    let rect = |y: i64| Object::from(vec![72.into(), y.into(), 144.into(), (y + 24).into()]);
    let mut uri = |url: &str| {
        f.pdf
            .add_object(dictionary! { "S" => "URI", "URI" => text(url) })
    };
    let (site_url, link_url) = (uri("https://example.com/"), uri("https://example.org/"));
    let section = f
        .pdf
        .add_object(dictionary! { "Type" => "StructElem", "S" => "Sect" });
    let site = dictionary! { "Title" => text("Site"), "A" => site_url, "SE" => section };
    let third = dictionary! {
        "Title" => text("Third"),
        "Dest" => vec![f.pages[2].into(), "Fit".into()],
    };
    outline_tree(&mut f, vec![(0, site), (0, third)]);
    let annot = |subtype: &str, y, more: Dictionary| {
        let mut d = dictionary! { "Type" => "Annot", "Subtype" => subtype, "Rect" => rect(y) };
        d.extend(&more);
        d
    };
    let link = annot("Link", 72, dictionary! { "A" => link_url });
    let comment = |name: &str, said: &str, y| {
        let about =
            dictionary! { "NM" => text(name), "Contents" => text(said), "T" => text("Ann") };
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
    let pdf = lopdf::Document::load_mem(bytes).unwrap();
    let first = pdf.get_pages()[&1];
    let mut out: Vec<(String, [f32; 4])> = (pdf.get_page_annotations(first).unwrap().iter())
        .map(|a| {
            let subtype = a.get(b"Subtype").unwrap().as_name().unwrap();
            let rect = a.get(b"Rect").unwrap().as_array().unwrap();
            let n = |i: usize| rect[i].as_float().unwrap() as f32;
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
        "DocSettings" => dictionary! {}, "Private" => "Watermark",
    } };
    let form = f.pdf.add_object(lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "PieceInfo" => piece,
            "BBox" => vec![0.into(), 0.into(), 300.into(), 60.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => f.helv } },
        },
        b"0 0 1 rg BT /F1 48 Tf 4 12 Td (ACROBAT) Tj ET".to_vec(),
    ));
    let theirs = f.pdf.add_object(lopdf::Stream::new(
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
    let header = pdf.add_object(lopdf::Stream::new(dictionary! {}, header.to_vec()));
    let dict = pdf.get_dictionary_mut(page).unwrap();
    let mut contents = dict.get(b"Contents").unwrap().as_array().unwrap().clone();
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
    let clean = lopdf::Document::load_mem(&clean).unwrap();
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
