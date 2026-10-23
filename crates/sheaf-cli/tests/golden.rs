//! The command line's goldens (M10.5 §14.2, proposal 40): `sheaf edit`
//! addressing by position, title path, `/NM` and `--with`, its dry run,
//! `sheaf inspect --json` and `--schema`, and every exit status: 0 done,
//! 1 failed, 2 usage, 3 a decision needed (the ask as JSON), and 130
//! cancelled. Each `tests/golden/<case>.txt` holds a run's arguments, the
//! edits it read (its keys sorted, as the command line prints JSON), its
//! exit status, its standard output and its standard error, the last two
//! normalized: the scratch folder is `$DIR`, with `/`
//! after it, the binary is `sheaf-cli`, and a mark's name, random, is
//! `sheaf:#`. A run that differs fails; `SHEAF_GOLDEN_UPDATE=1` writes
//! what it got instead.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use common::{binder, scratch, sheaf};
use serde_json::{Value, json};

/// One run of the binary.
struct Case {
    name: &'static str,
    /// Its arguments: `$DIR` is the scratch folder, `$EDITS` the file
    /// `edits` is written to.
    args: Vec<&'static str>,
    edits: Option<Value>,
    /// Only the first lines of its standard output, for a long one.
    head: Option<usize>,
}

impl Case {
    /// `sheaf-cli edit $DIR/in.pdf --edits $EDITS` and `more`.
    fn edit(name: &'static str, edits: Value, more: &[&'static str]) -> Case {
        Case::edit_of(name, "$DIR/in.pdf", edits, more)
    }

    /// `sheaf-cli edit <input> --edits $EDITS` and `more`.
    fn edit_of(
        name: &'static str,
        input: &'static str,
        edits: Value,
        more: &[&'static str],
    ) -> Case {
        let mut args = vec!["edit", input, "--edits", "$EDITS"];
        args.extend(more);
        Case {
            name,
            args,
            edits: Some(edits),
            head: None,
        }
    }

    /// `sheaf-cli args…`, reading no edits.
    fn run(name: &'static str, args: &[&'static str]) -> Case {
        Case {
            name,
            args: args.to_vec(),
            edits: None,
            head: None,
        }
    }

    fn args(&self, dir: &Path) -> Vec<String> {
        let edits = dir.join(format!("{}.json", self.name));
        (self.args.iter())
            .map(|a| {
                a.replace("$EDITS", &edits.display().to_string())
                    .replace("$DIR", &dir.display().to_string())
            })
            .collect()
    }
}

/// A style for a mark: red, opaque, a point wide.
fn red() -> Value {
    json!({"color": [220, 40, 40], "opacity": 1, "stroke": 1})
}

fn cases() -> Vec<Case> {
    let v = "1.0";
    vec![
        // Positions are 1-based, against the document as each op finds it:
        // after page 2 goes, position 2 is what was page 3.
        Case::edit(
            "edit-positions",
            json!({"v": v, "label": "Tidy the binder", "ops": [
                {"op": "delete-pages", "pages": {"positions": "2"}},
                {"op": "rotate-pages", "pages": {"ids": [{"pos": 2}]}, "by": 90},
                {"op": "add-bookmark", "at": "last", "title": "The end", "target": {"pos": 3},
                 "local": "$1"},
            ]}),
            &["-o", "$DIR/positions.pdf"],
        ),
        // Bookmarks by their titles from the top.
        Case::edit(
            "edit-title-path",
            json!({"v": v, "ops": [
                {"op": "patch-bookmarks", "bookmarks": [{"path": ["Part I", "Ch. 1"]}],
                 "patch": {"title": "Chapter 1"}},
                {"op": "add-bookmark", "parent": {"path": ["Part II"]}, "at": "first",
                 "title": "Schedule B", "target": {"pos": 4}},
                {"op": "move-pages", "pages": {"bookmark": {"path": ["Part II"]}},
                 "before": {"pos": 1}},
            ]}),
            &["-o", "$DIR/title-path.pdf"],
        ),
        // Annotations by /NM: a reply to another tool's note, and its
        // highlight deleted.
        Case::edit(
            "edit-nm",
            json!({"v": v, "label": "Answer Ann", "ops": [
                {"op": "add-mark", "page": {"pos": 1}, "reply_to": {"nm": "n1"},
                 "mark": {"kind": {"type": "reply"}, "contents": "Tied", "style": red()}},
                {"op": "delete-annotations", "annots": [{"nm": "h1", "page": {"pos": 3}}]},
            ]}),
            &["-o", "$DIR/nm.pdf"],
        ),
        // Files by the names --with gives them: a PDF's pages, an image as
        // a page, and the image as a background; with a Bates footer on
        // some pages and a note, for inspect to show.
        Case::edit(
            "edit-with",
            json!({"v": v, "label": "Add the appendix", "ops": [
                {"op": "insert-pages", "before": {"pos": 1}, "locals": ["$1"],
                 "from": {"type": "file", "file": {"with": "appendix"}, "name": "appendix.pdf"}},
                {"op": "insert-pages",
                 "from": {"type": "images", "images": [{"with": "logo"}], "fit": "original"}},
                {"op": "set-watermark", "z": "under", "scope": "all",
                 "spec": {"content": {"type": "image", "image": {"with": "logo"},
                                      "scale": 0.5, "opacity": 0.3}}},
                {"op": "set-header-footer", "scope": {"pages": {"positions": "2-5"}},
                 "spec": {"texts": ["", "", "", "", "<<bates>>", ""], "size": 9,
                          "color": [0, 0, 0],
                          "margins": {"top": 36, "bottom": 36, "left": 36, "right": 36},
                          "first_number": 1,
                          "bates": {"prefix": "ACME-", "suffix": "", "digits": 4, "start": 1}}},
                {"op": "add-mark", "page": {"pos": 1},
                 "mark": {"kind": {"type": "note", "pos": {"x": 300, "y": 400}},
                          "contents": "See the appendix", "style": red()}},
            ]}),
            &[
                "--with",
                "appendix=$DIR/appendix.pdf",
                "--with",
                "logo=$DIR/logo.png",
                "-o",
                "$DIR/with.pdf",
            ],
        ),
        // A dry run writes nothing, and says what the edits would do.
        Case::edit(
            "edit-dry-run",
            json!({"v": v, "ops": [
                {"op": "delete-pages", "pages": {"positions": "2"}},
                {"op": "delete-annotations", "annots": [{"nm": "n1"}]},
                {"op": "add-mark", "page": {"pos": 2}, "local": "$1",
                 "mark": {"kind": {"type": "square", "rect": {"x": 72, "y": 72, "w": 100, "h": 50}},
                          "style": red()}},
            ]}),
            &["--dry-run"],
        ),
        // An op that can't be done: the edit fails whole (1).
        Case::edit(
            "edit-fails",
            json!({"v": v, "ops": [
                {"op": "rotate-pages", "pages": {"ids": [{"pos": 1}]}, "by": 90},
                {"op": "delete-pages", "pages": {"positions": "9"}},
            ]}),
            &["-o", "$DIR/fails.pdf"],
        ),
        // A session's id means nothing in a file (1, Invalid).
        Case::edit(
            "edit-id",
            json!({"v": v, "ops": [
                {"op": "rotate-pages", "pages": {"ids": [17]}, "by": 90},
            ]}),
            &["-o", "$DIR/id.pdf"],
        ),
        // No target, and no dry run (2).
        Case::edit("edit-usage", json!({"v": v, "ops": []}), &[]),
        // A mark on a signed file, written whole, breaks the signature: it
        // stops and asks (3), and writes nothing.
        Case::edit_of(
            "edit-signature-asks",
            "$DIR/signed.pdf",
            note_on_page_1(),
            &["-o", "$DIR/signed-out.pdf"],
        ),
        // A dry run says so.
        Case::edit_of(
            "edit-signature-dry-run",
            "$DIR/signed.pdf",
            note_on_page_1(),
            &["--dry-run"],
        ),
        // Appended, it keeps it.
        Case::edit_of(
            "edit-signature-appended",
            "$DIR/signed.pdf",
            note_on_page_1(),
            &["-o", "$DIR/signed-out.pdf", "--append"],
        ),
        // --force only replaces an output that's there: it answers no ask.
        Case::edit_of(
            "edit-force-answers-nothing",
            "$DIR/signed.pdf",
            note_on_page_1(),
            &["-o", "$DIR/signed-out.pdf", "--force"],
        ),
        // Without --force an output that's there stays (1); with it, it's
        // replaced.
        Case::edit(
            "edit-force-refused",
            rotate_first(),
            &["-o", "$DIR/taken.pdf"],
        ),
        Case::edit(
            "edit-force-replaces",
            rotate_first(),
            &["-o", "$DIR/taken.pdf", "--force"],
        ),
        Case::run("inspect-json", &["inspect", "$DIR/with.pdf", "--json"]),
        // A form's summary: the signed field, its widget, the signature.
        Case::run("inspect-form", &["inspect", "$DIR/signed.pdf", "--json"]),
        Case {
            head: Some(12),
            ..Case::run("edit-schema", &["edit", "--schema"])
        },
    ]
}

/// A note on page 1.
fn note_on_page_1() -> Value {
    json!({"v": "1.0", "label": "Note", "ops": [
        {"op": "add-mark", "page": {"pos": 1},
         "mark": {"kind": {"type": "note", "pos": {"x": 300, "y": 300}}, "contents": "Seen",
                  "style": red()}},
    ]})
}

/// Page 1 turned a quarter.
fn rotate_first() -> Value {
    json!({"v": "1.0", "ops": [
        {"op": "rotate-pages", "pages": {"ids": [{"pos": 1}]}, "by": 90},
    ]})
}

/// `case`'s run, as its golden holds it.
fn render(dir: &Path, case: &Case, args: &[String], run: &Output) -> String {
    let shown: Vec<String> = (args.iter())
        .map(|a| normalize(dir, a))
        .map(|a| match a.contains(' ') {
            true => format!("'{a}'"),
            false => a,
        })
        .collect();
    let mut out = format!("$ sheaf-cli {}\n", shown.join(" "));
    if let Some(edits) = &case.edits {
        let edits = serde_json::to_string_pretty(&sorted(edits.clone())).unwrap();
        out += &format!("--- edits\n{edits}\n");
    }
    let code = (run.status.code()).map_or_else(|| format!("{}", run.status), |c| c.to_string());
    out += &format!("--- exit {code}\n");
    let mut stdout = normalize(dir, &String::from_utf8_lossy(&run.stdout));
    if let Some(n) = case.head {
        stdout = stdout.lines().take(n).map(|l| format!("{l}\n")).collect();
        stdout += "…\n";
    }
    out += &format!("--- stdout\n{stdout}");
    let stderr = normalize(dir, &String::from_utf8_lossy(&run.stderr));
    out + &format!("--- stderr\n{stderr}")
}

/// `value` with each object's keys in order, as the command line prints
/// JSON: the same whatever the build turns on of serde_json.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// `text` with the scratch folder `$DIR` and a `/` after it, the binary
/// `sheaf-cli`, and each mark's random name `sheaf:#`.
fn normalize(dir: &Path, text: &str) -> String {
    let text = text
        .replace(&dir.display().to_string(), "$DIR")
        .replace("$DIR\\", "$DIR/")
        .replace("sheaf-cli.exe", "sheaf-cli");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("sheaf:") {
        let (before, after) = rest.split_at(at + "sheaf:".len());
        out += before;
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 {
            out.push('#');
        }
        rest = &after[digits..];
    }
    out + rest
}

/// `got`, held to the golden `name`, or written as it when updating.
fn held(name: &str, got: &str) -> Result<(), String> {
    let path = goldens().join(format!("{name}.txt"));
    if std::env::var_os("SHEAF_GOLDEN_UPDATE").is_some_and(|v| v == "1") {
        std::fs::write(&path, got).unwrap();
        return Ok(());
    }
    let want = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "{name}: {} ({e}); SHEAF_GOLDEN_UPDATE=1 writes it",
            path.display()
        )
    })?;
    match want == got {
        true => Ok(()),
        false => Err(format!(
            "{name} differs from {} (SHEAF_GOLDEN_UPDATE=1 rewrites it):\n--- want\n{want}--- got\n{got}",
            path.display()
        )),
    }
}

fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// A one-page form, signed.
fn signed(path: &Path) {
    use sheaf_doc::forms::fixture::{Form, row};
    let mut form = Form::new(1);
    let sig = form.signature("sig", 0, row(2));
    form.sign(sig);
    std::fs::write(path, form.bytes()).unwrap();
}

#[test]
fn the_command_line_is_as_its_goldens_say() {
    let dir = scratch("golden");
    let input = binder(&dir);
    signed(&dir.join("signed.pdf"));
    let taken = dir.join("taken.pdf");
    std::fs::write(&taken, "taken").unwrap();
    let before = std::fs::read(&input).unwrap();
    let mut failed = Vec::new();
    for case in cases() {
        if let Some(edits) = &case.edits {
            common::edits(&dir, &format!("{}.json", case.name), edits);
        }
        let args = case.args(&dir);
        let run = sheaf().args(&args).env_remove("RUST_LOG").output().unwrap();
        if let Err(e) = held(case.name, &render(&dir, &case, &args, &run)) {
            failed.push(e);
        }
        match case.name {
            "edit-force-refused" => assert_eq!(std::fs::read(&taken).unwrap(), b"taken"),
            "edit-force-replaces" => {
                let pdf = lopdf::Document::load(&taken).unwrap();
                assert_eq!(pdf.get_pages().len(), 4);
            }
            _ => {}
        }
    }
    // Every edit was of a copy: the binder is as it was.
    assert_eq!(std::fs::read(&input).unwrap(), before);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Ctrl-C stops an edit cleanly (130), writing nothing. The handler that
/// turns it into a cancel is installed as the op's progress starts
/// (progress.rs), so the signal goes once the op's first line is on
/// standard error: before it, the signal would kill the process. The
/// edits, which the op reads from standard input, go once the handler has
/// said it cancelled, so the op sees the cancel however loaded the
/// machine is.
#[cfg(unix)]
#[test]
fn ctrl_c_cancels_an_edit() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;
    let dir = scratch("golden-cancel");
    binder(&dir);
    let case = Case {
        name: "edit-cancelled",
        args: vec![
            "edit",
            "$DIR/in.pdf",
            "--edits",
            "-",
            "-o",
            "$DIR/cancelled.pdf",
        ],
        edits: Some(rotate_first()),
        head: None,
    };
    let args = case.args(&dir);
    let mut child = (sheaf().args(&args).env_remove("RUST_LOG"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut said = String::new();
    stderr.read_line(&mut said).unwrap();
    assert_eq!(said, "editing…\n");
    let kill = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    while !said.contains("stopping…") {
        assert!(stderr.read_line(&mut said).unwrap() > 0, "{said}");
    }
    let edits = case.edits.as_ref().unwrap().to_string();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(edits.as_bytes()).unwrap();
    drop(stdin);
    stderr.read_to_string(&mut said).unwrap();
    let status = child.wait().unwrap();
    let mut stdout = Vec::new();
    (child.stdout.take().unwrap())
        .read_to_end(&mut stdout)
        .unwrap();
    let run = Output {
        status,
        stdout,
        stderr: said.into_bytes(),
    };
    held(case.name, &render(&dir, &case, &args, &run)).unwrap();
    assert!(!dir.join("cancelled.pdf").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
