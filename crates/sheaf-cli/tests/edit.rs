//! `sheaf edit` end to end, through the binary: the examples
//! docs/document-model.md gives, real signed files appended to and asked
//! about, and the command line writing no file but its outputs.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::{binder, edits, ok, run, scratch, sheaf};
use lopdf::{Document as Pdf, Object};
use serde_json::{Value, json};
use sheaf_doc::effects::SigClass;
use sheaf_doc::forms::corpus::{Corpus, Entry};
use sheaf_doc::forms::sigs::{Pending, sig_impact};

/// What marks an example in docs/document-model.md: the line before its
/// `json` block.
const EXAMPLE: &str = "<!-- sheaf-edit example -->";

/// How many examples the docs hold: a lost example fails the count.
const EXAMPLES: usize = 5;

/// The `json` blocks the docs mark as `sheaf edit` examples; every comment
/// that names `sheaf-edit` must be a marker that says so exactly.
fn examples() -> Vec<String> {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/document-model.md");
    let text = std::fs::read_to_string(&docs).unwrap();
    for line in text
        .lines()
        .filter(|l| l.contains("<!--") && l.contains("sheaf-edit"))
    {
        assert_eq!(line.trim(), EXAMPLE, "a mistyped marker");
    }
    let mut lines = text.lines();
    let mut out = Vec::new();
    while let Some(line) = lines.next() {
        if line.trim() != EXAMPLE {
            continue;
        }
        assert_eq!(
            lines.next().map(str::trim),
            Some("```json"),
            "{EXAMPLE} before a json block"
        );
        let block: Vec<&str> = lines.by_ref().take_while(|l| l.trim() != "```").collect();
        out.push(block.join("\n"));
    }
    out
}

/// Every example in the docs runs on the binder they're written for, with
/// `--with appendix=… --with logo=…`: as a dry run, and saved. So an
/// example that no longer reads, or names what isn't there, fails CI.
#[test]
fn docs_json_examples_run() {
    let dir = scratch("docs-examples");
    let input = binder(&dir);
    let examples = examples();
    assert_eq!(examples.len(), EXAMPLES, "examples found");
    let with = [
        format!("appendix={}", dir.join("appendix.pdf").display()),
        format!("logo={}", dir.join("logo.png").display()),
    ];
    for (i, example) in examples.iter().enumerate() {
        let parsed: Value = serde_json::from_str(example)
            .unwrap_or_else(|e| panic!("example {}: {e}\n{example}", i + 1));
        let file = edits(&dir, &format!("example-{i}.json"), &parsed);
        let out = dir.join(format!("example-{i}.pdf"));
        let mut args: Vec<String> = ["edit", path(&input), "--edits", path(&file)]
            .map(String::from)
            .into();
        for w in &with {
            args.extend(["--with".to_string(), w.clone()]);
        }
        let mut dry = args.clone();
        dry.push("--dry-run".into());
        let said: Value = serde_json::from_str(&ok(&sheaf().args(&dry).output().unwrap())).unwrap();
        assert!(!said["lines"].as_array().unwrap().is_empty(), "{said}");
        assert!(!out.exists(), "a dry run writes nothing");
        args.extend(["-o".to_string(), path(&out).to_string()]);
        let done: Value =
            serde_json::from_str(&ok(&sheaf().args(&args).output().unwrap())).unwrap();
        assert_eq!(done["lines"], said["lines"], "example {}", i + 1);
        Pdf::load(&out).unwrap();
        ok(&run(&["inspect", path(&out), "--json"]));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// `--schema` prints the schema the Op JSON version this build writes
/// has, as sheaf-edit commits it.
#[test]
fn the_schema_is_the_committed_one() {
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../sheaf-edit/schema/ops-{}.json",
        sheaf_edit::VERSION
    ));
    let committed: Value =
        serde_json::from_str(&std::fs::read_to_string(committed).unwrap()).unwrap();
    let printed: Value = serde_json::from_str(&ok(&run(&["edit", "--schema"]))).unwrap();
    assert_eq!(printed, committed);
}

/// `--edits -` reads the edits from standard input, and saved in place,
/// they're appended to the file.
#[test]
fn edits_come_from_standard_input() {
    use std::io::Write;
    use std::process::Stdio;
    let dir = scratch("stdin");
    let input = binder(&dir);
    let before = std::fs::read(&input).unwrap();
    let mut child = (sheaf().args(["edit", path(&input), "--edits", "-", "--in-place"]))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let edits = json!({"v": "1.0", "label": "Turn", "ops": [
        {"op": "rotate-pages", "pages": {"positions": "1-2"}, "by": 180},
    ]});
    (child.stdin.take().unwrap())
        .write_all(edits.to_string().as_bytes())
        .unwrap();
    let done: Value = serde_json::from_str(&ok(&child.wait_with_output().unwrap())).unwrap();
    assert_eq!(
        (&done["version"], &done["strategy"]),
        (&json!("Turn"), &json!("append"))
    );
    let after = std::fs::read(&input).unwrap();
    assert!(after.len() > before.len() && after.starts_with(&before));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Signed files from the corpus's `signatures/`, each named: approval
/// signatures of both subfilters, one file signed twice, and two
/// certifications that allow comments (DocMDP P = 3). Each is held to its
/// manifest row: signed, every signature intact and valid, and not
/// encrypted.
const SIGNED: [&str; 6] = [
    "signatures/safedocs/0000020.pdf",
    "signatures/safedocs/0000615.pdf",
    "signatures/safedocs/0000581.pdf",
    "signatures/pyhanko/pdf/minimal-two-fields-signed-twice.pdf",
    "signatures/safedocs/0319699.pdf",
    "signatures/safedocs/0045169.pdf",
];

/// The corpus's rows for [`SIGNED`], each checked as its comment says.
fn signed(corpus: &Corpus) -> Vec<Entry> {
    let rows = corpus.signatures().unwrap();
    let mut subfilters = BTreeSet::new();
    let picked: Vec<Entry> = (SIGNED.iter())
        .map(|path| {
            let row = (rows.iter().find(|r| r.path == *path))
                .unwrap_or_else(|| panic!("{path} isn't in signatures/manifest.tsv"))
                .clone();
            let signatures: u32 = row.column("signatures").parse().unwrap();
            assert!(signatures > 0, "{path}");
            assert_eq!(row.column("intact"), row.column("signatures"), "{path}");
            assert_eq!(row.column("valid"), row.column("signatures"), "{path}");
            assert_eq!(row.column("encryption"), "", "{path}");
            for sub in row.column("subfilters").split(',') {
                subfilters.insert(sub.split(':').next().unwrap().to_string());
            }
            row
        })
        .collect();
    assert_eq!(
        subfilters,
        BTreeSet::from(["ETSI.CAdES.detached".into(), "adbe.pkcs7.detached".into()])
    );
    picked
}

/// Each signature dictionary's `/ByteRange` in `bytes`, sorted.
fn byte_ranges(bytes: &[u8]) -> Vec<Vec<i64>> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let mut out: Vec<Vec<i64>> = (pdf.objects.values())
        .filter_map(|o| o.as_dict().ok()?.get(b"ByteRange").ok()?.as_array().ok())
        .map(|a| a.iter().map(|n| n.as_i64().unwrap()).collect())
        .collect();
    out.sort();
    out
}

/// A note on page 1.
fn note() -> Value {
    json!({"v": "1.0", "label": "Note", "ops": [
        {"op": "add-mark", "page": {"pos": 1},
         "mark": {"kind": {"type": "note", "pos": {"x": 100, "y": 100}}, "contents": "Seen",
                  "style": {"color": [220, 40, 40], "opacity": 1, "stroke": 1}}},
    ]})
}

/// A mark added to a real signed file and saved `-o out.pdf --append`
/// keeps its signatures: the file is the output's first bytes, each
/// signature's `/ByteRange` is the same and covers the same bytes, the
/// output's signatures read as the file's, and §9.3's table says an
/// Append of a comment keeps each. Written whole, the same edit stops and
/// asks (3), with the ask as JSON, and writes nothing; `--signatures break`
/// answers it.
#[test]
fn a_signed_file_appended_keeps_its_signatures() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("signed-corpus");
    let file = edits(&dir, "note.json", &note());
    for row in signed(&corpus) {
        let input = corpus.path(&row);
        let name = Path::new(&row.path).file_stem().unwrap().to_string_lossy();
        let original = std::fs::read(&input).unwrap();
        let edit = |out: &PathBuf, more: &[&str]| {
            let mut args = vec![
                "edit",
                path(&input),
                "--edits",
                path(&file),
                "-o",
                path(out),
            ];
            args.extend(more);
            run(&args)
        };
        let appended = dir.join(format!("{name}-appended.pdf"));
        let done: Value = serde_json::from_str(&ok(&edit(&appended, &["--append"]))).unwrap();
        assert_eq!(done["strategy"], "append", "{}", row.path);
        let bytes = std::fs::read(&appended).unwrap();
        assert!(
            bytes.len() > original.len() && bytes.starts_with(&original),
            "{}",
            row.path
        );
        let ranges = byte_ranges(&original);
        assert_eq!(
            ranges.len(),
            row.column("signatures").parse::<usize>().unwrap(),
            "{}",
            row.path
        );
        assert_eq!(byte_ranges(&bytes), ranges, "{}", row.path);
        for r in &ranges {
            for (start, len) in [(r[0], r[1]), (r[2], r[3])] {
                let span = start as usize..(start + len) as usize;
                assert_eq!(bytes[span.clone()], original[span], "{}", row.path);
            }
        }
        let before = sheaf_doc::open::file(&input, None).unwrap().doc;
        let after = sheaf_doc::open::file(&appended, None).unwrap().doc;
        let sigs = before.primary().signatures();
        assert_eq!(after.primary().signatures(), sigs, "{}", row.path);
        let kept = sig_impact(sigs, Pending::Append, SigClass::ANNOT, &BTreeSet::new());
        assert!(
            kept.iter().all(|s| !s.verdict.asks()),
            "{}: {kept:?}",
            row.path
        );
        // Written whole: an ask, and nothing written.
        let whole = dir.join(format!("{name}-whole.pdf"));
        let asked = edit(&whole, &[]);
        assert_eq!(asked.status.code(), Some(3), "{}", row.path);
        let ask: Value = serde_json::from_slice(&asked.stdout).unwrap();
        assert_eq!(ask["kind"], "signatures", "{}", row.path);
        assert_eq!(ask["signatures"], sigs.signed.len(), "{}", row.path);
        assert!(!whole.exists(), "{}", row.path);
        let done: Value =
            serde_json::from_str(&ok(&edit(&whole, &["--signatures", "break"]))).unwrap();
        assert_eq!(done["strategy"], "rewrite", "{}", row.path);
        assert!(!std::fs::read(&whole).unwrap().starts_with(&original));
        assert_eq!(
            std::fs::read(&input).unwrap(),
            original,
            "the corpus file is as it was"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The corpus's files whose signatures were broken when they came (B5):
/// pyHanko finds none of their signatures intact, and their digests don't
/// match the bytes they sign.
const BROKEN: [&str; 2] = [
    "signatures/safedocs/0008830.pdf",
    "signatures/safedocs/0010094.pdf",
];

/// A signature broken already isn't asked about (B5): a mark on each of
/// [`BROKEN`] written whole is a Rewrite, not a stop to ask (3); and a
/// file `--signatures break` wrote, its signatures broken, isn't asked
/// about on its next edit written whole.
#[test]
fn a_signature_broken_already_isn_t_asked_about() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("broken-corpus");
    let file = edits(&dir, "note.json", &note());
    let whole = |input: &Path, out: &Path, more: &[&str]| {
        let mut args = vec!["edit", path(input), "--edits", path(&file), "-o", path(out)];
        args.extend(more);
        run(&args)
    };
    let rows = corpus.signatures().unwrap();
    for name in BROKEN {
        let row = (rows.iter().find(|r| r.path == name)).unwrap();
        assert_eq!(row.column("intact"), "0", "{name}");
        assert_eq!(row.column("encryption"), "", "{name}");
        let out = dir.join(Path::new(name).file_name().unwrap());
        let done: Value = serde_json::from_str(&ok(&whole(&corpus.path(row), &out, &[]))).unwrap();
        assert_eq!(done["strategy"], "rewrite", "{name}");
    }
    let row = &signed(&corpus)[0];
    let broken = dir.join("broken.pdf");
    let done: Value = serde_json::from_str(&ok(&whole(
        &corpus.path(row),
        &broken,
        &["--signatures", "break"],
    )))
    .unwrap();
    assert_eq!(done["strategy"], "rewrite", "{}", row.path);
    let again = dir.join("again.pdf");
    let done: Value = serde_json::from_str(&ok(&whole(&broken, &again, &[]))).unwrap();
    assert_eq!(done["strategy"], "rewrite", "{}", row.path);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What names where a process may put temp and cache files: the temp
/// folder (`TMPDIR` on Unix, `TMP` and `TEMP` on Windows), the cache
/// folder on Linux, and the home folder, under which macOS keeps caches.
const DIRS: [&str; 5] = ["TMPDIR", "TMP", "TEMP", "XDG_CACHE_HOME", "HOME"];

/// The command line writes no file but its outputs (M10.5 §3.9): an edit
/// that inserts blank pages and an image, adds a mark and flattens, then
/// export, split, combine, scan and images to PDF, each run with its temp,
/// cache and home folders new and empty, leave them empty.
#[test]
fn the_command_line_writes_nothing_but_its_outputs() {
    let dir = scratch("own-dirs");
    let input = binder(&dir);
    let logo = dir.join("logo.png");
    let watched = dir.join("watched");
    let file = edits(
        &dir,
        "made.json",
        &json!({"v": "1.0", "ops": [
            {"op": "insert-pages", "from": {"type": "blank",
             "size": {"width": 612, "height": 792}, "count": 2}},
            {"op": "insert-pages", "before": {"pos": 1},
             "from": {"type": "images", "images": [{"with": "logo"}], "fit": "original"}},
            {"op": "add-mark", "page": {"pos": 2},
             "mark": {"kind": {"type": "note", "pos": {"x": 100, "y": 100}}, "contents": "Seen",
                      "style": {"color": [220, 40, 40], "opacity": 1, "stroke": 1}}},
            {"op": "flatten-annotations", "pages": "all", "which": "all"},
        ]}),
    );
    let out = |name: &str| dir.join(name).to_str().unwrap().to_string();
    let edited = out("edited.pdf");
    let with = format!("logo={}", logo.display());
    let runs: Vec<Vec<String>> = [
        vec![
            "edit",
            path(&input),
            "--edits",
            path(&file),
            "--with",
            &with,
            "-o",
            &edited,
        ],
        vec![
            "export",
            &edited,
            "-f",
            "png",
            "--pages",
            "1-2",
            "-o",
            &out("pngs"),
        ],
        vec!["export", &edited, "-f", "text", "-o", &out("edited.txt")],
        vec!["split", &edited, "--every", "3", "-o", &out("parts")],
        vec!["combine", &edited, path(&logo), "-o", &out("combined.pdf")],
        vec!["scan", path(&logo), "-o", &out("scanned.pdf")],
        vec!["images-to-pdf", path(&logo), "-o", &out("images.pdf")],
    ]
    .into_iter()
    .map(|args| args.into_iter().map(String::from).collect())
    .collect();
    std::fs::create_dir_all(out("pngs")).unwrap();
    std::fs::create_dir_all(out("parts")).unwrap();
    for args in runs {
        let _ = std::fs::remove_dir_all(&watched);
        let mut cmd = sheaf();
        cmd.args(&args);
        for var in DIRS {
            let own = watched.join(var.to_lowercase());
            std::fs::create_dir_all(&own).unwrap();
            cmd.env(var, own);
        }
        ok(&cmd.output().unwrap());
        for var in DIRS {
            let own = watched.join(var.to_lowercase());
            let written: Vec<PathBuf> = (std::fs::read_dir(&own).unwrap())
                .map(|e| e.unwrap().path())
                .collect();
            assert!(
                written.is_empty(),
                "{args:?} wrote to its {var}: {written:?}"
            );
        }
    }
    // Flattened, the comments are drawn, and gone as comments.
    let pdf = Pdf::load(dir.join("edited.pdf")).unwrap();
    assert_eq!(pdf.get_pages().len(), 7);
    let comment = |o: &Object| {
        let subtype = o.as_dict().ok().and_then(|d| d.get(b"Subtype").ok());
        matches!(subtype, Some(Object::Name(n)) if n == b"Text" || n == b"Highlight")
    };
    assert_eq!(pdf.objects.values().filter(|o| comment(o)).count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
