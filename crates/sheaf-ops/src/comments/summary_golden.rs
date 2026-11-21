//! What the comment summary writes, held by digest (M18 slice 19): each
//! sheet of a summary of fixed comments, both layouts, a line of
//! `summary_golden.txt`: the case, the xxh3-128 of the sheet's content,
//! and each resource it's bound to as `key/name/digest`, the digest of
//! the object the name binds and all it reaches (a font's is the id its
//! name is made from).
//!
//! The file was written once, by the content writer as it was before
//! content went through pdf-content-streams, and is what the sheets'
//! bytes are held to since: a line that differs is a byte that changed,
//! to find, not a golden to write again. `SHEAF_GOLDEN_UPDATE=1` writes
//! it. Each sheet is read back too: its tokens, each operation written
//! again by [`ContentWriter::operation`], give the same bytes.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use sheaf_doc::content::{ContentWriter, ResCx, Reserved, Tok, tokens};
use sheaf_doc::file::ForeignAnnot;
use sheaf_doc::open::{FileAnnot, FileAnnots, FileBody};
use sheaf_doc::pdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, StreamExt, Value};
use sheaf_doc::source::Limits;
use sheaf_doc::{Actor, Annot, AnnotId, AnnotKind, Box2, Document, FixedCount, Pt, Store};
use sheaf_edit::Rotation;

use super::{Layout, summary_bytes};
use crate::comments::{CommentFilter, Sort};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/comments/summary_golden.txt"
);

fn digest(bytes: &[u8]) -> String {
    format!("{:032x}", sheaf_util::checksum(bytes))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A mark of `kind` by `author` saying `contents`, changed at `modified`.
fn mark(id: u64, kind: AnnotKind, author: &str, contents: &str, modified: &str) -> Annot {
    Annot {
        id: AnnotId(id),
        kind,
        color: [200, 30, 30],
        opacity: 1.0,
        stroke: 1.0,
        author: author.into(),
        contents: contents.into(),
        modified: modified.into(),
        created: modified.into(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// Another tool's comment `name`, in the thread of `parent`.
fn theirs(name: &str, parent: Option<&str>, contents: &str) -> ForeignAnnot {
    ForeignAnnot {
        subtype: "Text".into(),
        author: "Ann".into(),
        contents: contents.into(),
        modified: "D:20261001120000".into(),
        rect: Box2 {
            x: 300.0,
            y: 300.0,
            w: 20.0,
            h: 20.0,
        },
        name: name.into(),
        parent: parent.map(str::to_string),
        grouped: false,
        state: None,
        created: String::new(),
        slot: 0,
        hidden: false,
        color: Some([0, 0, 255]),
        mark: None,
        stamp: None,
    }
}

/// A document of three blank pages with fixed comments: on the first a
/// note whose text has a character no face has, one only ZapfDingbats
/// has, and what a literal string escapes, a tickmark, and a highlight
/// with no text; on the third a square whose text wraps, another tool's
/// comment and a reply to it.
fn commented() -> Document {
    let at = |x: f32, y: f32| Box2 {
        x,
        y,
        w: 120.0,
        h: 30.0,
    };
    let first = vec![
        mark(
            1,
            AnnotKind::Note {
                pos: Pt { x: 100.0, y: 500.0 },
            },
            "Greg",
            "Why? (see §2\\3) – naïve 漢 ✓ done",
            "D:20261014090000",
        ),
        mark(
            2,
            AnnotKind::Stamp {
                rect: Box2 {
                    x: 320.0,
                    y: 530.0,
                    w: 24.0,
                    h: 24.0,
                },
                glyph: "✓".into(),
            },
            "Lee",
            "",
            "D:20261014100000",
        ),
        mark(
            3,
            AnnotKind::Highlight {
                rects: vec![at(72.0, 700.0)],
            },
            "Greg",
            "",
            "D:20261014110000",
        ),
    ];
    let third = vec![mark(
        4,
        AnnotKind::Square {
            rect: at(72.0, 400.0),
        },
        "",
        "A long comment, so that it wraps: the quick brown fox jumps over the lazy dog, and \
         then the lazy dog, having been jumped over, gets up and walks the whole way home.",
        "D:20261015120000",
    )];
    let store = Store::new();
    let primary = sheaf_doc::store::blank(&store, 3);
    let mut d = sheaf_doc::open::start(
        store,
        primary,
        "reviewed.pdf".into(),
        None,
        Box::new(FixedCount(10)),
    );
    let mut annots = FileAnnots::default();
    for (index, list) in HashMap::from([(0, first), (2, third)]) {
        let marks = list.into_iter().map(|a| FileAnnot {
            object: None,
            slot: 0,
            body: FileBody::Mark {
                annot: Box::new(a),
                held: None,
            },
        });
        annots.pages.entry(index).or_default().extend(marks);
    }
    let others = [
        theirs("theirs-1", None, "Checked (twice)."),
        theirs("theirs-2", Some("theirs-1"), "Agreed."),
    ];
    for (i, f) in (10_000..).zip(others) {
        annots.pages.entry(2).or_default().push(FileAnnot {
            object: None,
            slot: f.slot,
            body: FileBody::Foreign {
                id: AnnotId(i),
                summary: Arc::new(f),
            },
        });
    }
    let facts = sheaf_doc::open::Facts {
        annots: Arc::new(annots),
        ..Default::default()
    };
    d.ready(facts).expect("the document is ready");
    d
}

/// `content` read back as pdf-content-streams' tokens and written again an
/// operation at a time: the same bytes.
fn read_back(case: &str, content: &[u8]) {
    let mut w = ContentWriter::new();
    let mut operands: Vec<&[u8]> = Vec::new();
    let lim = Limits::DEFAULT.content.family();
    for token in tokens(content, &ResCx::none(), &lim) {
        let (at, tok) = token.unwrap_or_else(|e| panic!("{case}: {e}:\n{}", text(content)));
        match tok {
            Tok::Operand(operand) => operands.push(operand),
            Tok::Op(op) => w.operation(std::mem::take(&mut operands), op),
            Tok::InlineImage(_) => w.operation(std::mem::take(&mut operands), &content[at]),
        }
    }
    assert!(operands.is_empty(), "{case}: operands no operator takes");
    let (again, needs) = w.finish(&Reserved::default());
    assert!(needs.0.is_empty(), "{case}");
    assert!(
        again == content,
        "{case}: read back and written again, it differs:\n--- written\n{}--- again\n{}",
        text(content),
        text(&again)
    );
}

/// `object` of `pdf` and all it reaches, in a form its numbers don't
/// change: a reference is what it names (or how far back, where it
/// names something it's in), a dictionary's entries in key order, a
/// stream decoded and without its `/Length` and filters. `/Parent` isn't
/// followed.
fn deep(pdf: &Pdf, object: &Object, out: &mut Vec<u8>, within: &mut Vec<ObjectId>) {
    let run = |out: &mut Vec<u8>, bytes: &[u8]| {
        out.extend((bytes.len() as u64).to_le_bytes());
        out.extend(bytes);
    };
    let entries = |out: &mut Vec<u8>, dict: &Dictionary, within: &mut Vec<ObjectId>| {
        let skipped: [&[u8]; 4] = [b"Parent", b"Length", b"Filter", b"DecodeParms"];
        let mut entries: Vec<_> = (dict.iter())
            .filter(|(key, _)| !skipped.iter().any(|s| s[..] == key[..]))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        out.extend((entries.len() as u64).to_le_bytes());
        for (key, value) in entries {
            run(out, key);
            deep(pdf, value, out, within);
        }
    };
    match object.value() {
        Value::Null => out.push(b'n'),
        Value::Bool(b) => out.extend([b'b', u8::from(*b)]),
        Value::Integer(i) => {
            out.push(b'i');
            out.extend(i.to_le_bytes());
        }
        Value::Real(r) => {
            out.push(b'r');
            out.extend(r.to_bits().to_le_bytes());
        }
        Value::Name(name) => {
            out.push(b'/');
            run(out, name.as_bytes());
        }
        Value::String(s, _) => {
            out.push(b'(');
            run(out, s.as_bytes().unwrap_or(b""));
        }
        Value::Array(items) => {
            out.push(b'[');
            out.extend((items.len() as u64).to_le_bytes());
            for item in items {
                deep(pdf, item, out, within);
            }
        }
        Value::Dict(d) => {
            out.push(b'<');
            entries(out, d, within);
        }
        Value::Stream(s) => {
            out.push(b's');
            entries(out, &s.dict, within);
            run(out, &decoded(s));
        }
        Value::Ref(id) => match within.iter().rposition(|at| at == id) {
            Some(at) => {
                out.push(b'^');
                out.extend(((within.len() - at) as u64).to_le_bytes());
            }
            None => match pdf.get_object(*id) {
                Ok(named) => {
                    within.push(*id);
                    deep(pdf, named, out, within);
                    within.pop();
                }
                Err(_) => out.push(b'?'),
            },
        },
    }
}

/// A stream's data, decoded.
fn decoded(s: &Stream) -> Vec<u8> {
    s.decompressed_content()
        .unwrap_or_else(|_| s.raw_bytes().to_vec())
}

/// `object`, a reference followed.
fn direct<'a>(pdf: &'a Pdf, object: &'a Object) -> &'a Object {
    match object.as_reference() {
        Some(id) => pdf.get_object(id).unwrap_or(object),
        None => object,
    }
}

/// A line for each sheet of the summary `bytes`, and what it wrote.
fn sheets(case: &str, bytes: &[u8]) -> Vec<(String, String)> {
    let pdf = Pdf::load_mem(bytes).unwrap_or_else(|e| panic!("{case}: {e}"));
    let mut lines = Vec::new();
    for (i, id) in pdf.get_pages().into_values().enumerate() {
        let case = format!("{case}:sheet{i}");
        let page = pdf.get_dictionary(id).expect("a sheet");
        let content = page.get(b"Contents").map(|c| direct(&pdf, c));
        let content = content.and_then(Object::as_stream).expect("its content");
        let content = decoded(content);
        read_back(&case, &content);
        let mut line = format!("{case}\t{}", digest(&content));
        let resources = page.get(b"Resources").map(|r| direct(&pdf, r));
        let resources = resources.and_then(Object::as_dict);
        let mut categories: Vec<_> = (resources.into_iter().flat_map(Dictionary::iter))
            .filter_map(|(key, c)| Some((text(key), direct(&pdf, c).as_dict()?)))
            .collect();
        categories.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, category) in categories {
            let mut entries: Vec<_> = category.iter().map(|(n, o)| (text(n), o)).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, object) in entries {
                let mut bytes = Vec::new();
                deep(&pdf, object, &mut bytes, &mut Vec::new());
                let _ = write!(line, "\t{key}/{name}/{}", digest(&bytes));
            }
        }
        lines.push((line, text(&content)));
    }
    lines
}

/// Every sheet of the summaries, in the golden's order: the comments
/// alone, by page and by author with a filter said, and each page beside
/// its comments.
fn cases() -> Vec<(String, String)> {
    let doc = commented();
    let from = doc.export_from(Actor::User);
    let by_author = CommentFilter {
        sort: Sort::Author,
        authors: ["Greg".to_string(), "Ann".to_string()].into(),
        ..CommentFilter::default()
    };
    let made = [
        ("comments", CommentFilter::default(), Layout::CommentsOnly),
        ("comments-by-author", by_author, Layout::CommentsOnly),
        ("pages", CommentFilter::default(), Layout::Pages),
    ];
    let mut lines = Vec::new();
    for (case, cf, layout) in made {
        let title = "Review of “reviewed.pdf” (draft)";
        let bytes = summary_bytes(&from, &cf, "", layout, title).expect("a summary");
        let again = summary_bytes(&from, &cf, "", layout, title).expect("a summary");
        assert!(bytes == again, "{case}: the same summary, twice");
        let made = sheets(&format!("summary:{case}"), &bytes);
        assert!(!made.is_empty(), "{case}: a sheet");
        lines.extend(made);
    }
    lines
}

/// What the summary writes is what the golden holds, line for line.
#[test]
fn what_the_summary_writes_is_what_the_golden_holds() {
    let cases = cases();
    // `SHEAF_GOLDEN_SHOW=<part of a case's name>` prints what those sheets
    // hold (with `--nocapture`).
    if let Some(show) = std::env::var_os("SHEAF_GOLDEN_SHOW") {
        let show = show.to_string_lossy().into_owned();
        for (line, shown) in cases.iter().filter(|(line, _)| line.contains(&show)) {
            eprintln!("{line}\n{shown}");
        }
    }
    let got: String = cases.iter().map(|(line, _)| format!("{line}\n")).collect();
    if std::env::var_os("SHEAF_GOLDEN_UPDATE").is_some_and(|v| v == "1") {
        std::fs::write(GOLDEN, got).expect("the golden written");
        return;
    }
    let want = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("{GOLDEN}: {e}; SHEAF_GOLDEN_UPDATE=1 writes it"));
    let want: Vec<&str> = want.lines().collect();
    let mut differs = String::new();
    for (i, (line, shown)) in cases.iter().enumerate() {
        let held = want.get(i).copied().unwrap_or("(no line)");
        if held != line {
            let _ = writeln!(
                differs,
                "line {}:\n  held {held}\n  got  {line}\n{shown}",
                i + 1
            );
        }
    }
    for held in want.iter().skip(cases.len()) {
        let _ = writeln!(differs, "held, and not written: {held}");
    }
    assert!(
        differs.is_empty(),
        "what's written differs from {GOLDEN}: a byte changed, to find (the golden isn't \
         written again):\n{differs}"
    );
}
