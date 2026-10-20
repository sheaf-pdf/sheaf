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
//! qpdf is `$SHEAF_QPDF`, else `qpdf` on the PATH; pdf.js is `node` on the
//! PATH with `$SHEAF_PDFJS`, pdfjs-dist's `legacy/build/pdf.mjs`. A test
//! whose tools are missing says so and passes. Run with
//! `cargo test -p sheaf-ops --test interop -- --ignored --nocapture`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Dictionary, Object, dictionary};
use sheaf_doc::forms::fixture::{Form, outline_tree};
use sheaf_doc::materialize::How;
use sheaf_doc::serialize::fixture::{
    PREFIX, USER, attachment_only, edited, encrypted, form, object_streams_encrypted,
    rc4_aes_128_and_aes_256, written,
};
use sheaf_doc::tx::fixture::{delete, delete_named, move_pages, patch_bookmark, rotate};
use sheaf_doc::{PageId, Snapshot};
use sheaf_edit::{BookmarkPatch, Turn};

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
        let snap = doc.snapshot();
        let ids: Vec<PageId> = snap.state.order().iter().copied().collect();
        let exported = |export: fn(&Snapshot, &Path) -> anyhow::Result<()>, how: &str| {
            let path = dir.join(format!("{file}-{how}.pdf"));
            export(&snap, &path).unwrap();
            std::fs::read(&path).unwrap()
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
                sheaf_doc::save::extract_bytes(&snap, &ids[..1]).unwrap(),
            ),
            (
                "Print".into(),
                sheaf_doc::save::print_pdf(&snap, &ids, true).unwrap(),
            ),
            (
                "PrintClean".into(),
                sheaf_doc::save::print_pdf(&snap, &ids, false).unwrap(),
            ),
            (
                "Clean".into(),
                exported(sheaf_doc::save::export_clean_copy, "clean-copy"),
            ),
            (
                "Flattened".into(),
                exported(
                    |snap, path| sheaf_doc::save::export_flattened(snap, path).map(drop),
                    "flattened-copy",
                ),
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
    use sheaf_doc::materialize::{AskAnswers, Why, save};
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
        save(&mut doc, &path, &answers, &sheaf_ops::NoProgress).unwrap();
        doc.undo().unwrap();
        let report = save(&mut doc, &path, &answers, &sheaf_ops::NoProgress).unwrap();
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
