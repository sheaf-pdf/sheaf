//! The Op JSON examples docs/plugins.md gives a plugin's author, each read
//! as a plugin's envelope and landed as a plugin call's edits are: so an
//! example that no longer reads, names what isn't there, or makes what a
//! plugin may not, fails CI.

use std::path::Path;

use in_progress::NoProgress;
use sheaf_doc::{CommitOpts, Document, FixedCount, Stale, Store};
use sheaf_edit::{Applied, Envelope};

/// What marks an example in docs/plugins.md: the line before its `json`
/// block.
const EXAMPLE: &str = "<!-- plugin-ops example -->";

/// How many examples the docs hold: a lost example fails the count.
const EXAMPLES: usize = 3;

/// The `json` blocks the docs mark as examples; every comment that names
/// `plugin-ops` must be a marker that says so exactly.
fn examples() -> Vec<String> {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins.md");
    let text = std::fs::read_to_string(&docs).unwrap();
    for line in text
        .lines()
        .filter(|l| l.contains("<!--") && l.contains("plugin-ops"))
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

/// The document the examples are written for, as the docs say: four
/// Letter pages, their ids 0 to 3.
fn binder() -> Document {
    let store = Store::new();
    let primary = sheaf_doc::store::blank(&store, 4);
    let d = Document::new(
        store,
        primary,
        "binder.pdf".into(),
        None,
        &[],
        Box::new(FixedCount(10)),
    );
    let ids: Vec<u64> = d.state().order().iter().map(|p| p.0).collect();
    assert_eq!(ids, [0, 1, 2, 3], "the pages the docs name");
    d
}

/// Every example is an envelope a plugin may submit (in this sheaf's
/// version of the edits, with no label of its own), and lands as a
/// plugin's: lowered into its transaction, answered with lines, and
/// committed as a call's end commits it.
#[test]
fn plugin_docs_examples_land() {
    let examples = examples();
    assert_eq!(examples.len(), EXAMPLES, "examples found");
    for (i, example) in examples.iter().enumerate() {
        let n = i + 1;
        let envelope: Envelope =
            serde_json::from_str(example).unwrap_or_else(|e| panic!("example {n}: {e}\n{example}"));
        assert_eq!(envelope.v, sheaf_edit::VERSION, "example {n}");
        assert_eq!(
            envelope.label, None,
            "example {n}: a plugin's submit names its edits"
        );
        let mut d = binder();
        let mut tx = d
            .published()
            .plugin_tx("my-plugin".into(), "0.1.0".into(), false);
        let applied = (tx.apply_said(envelope.ops, &NoProgress))
            .unwrap_or_else(|e| panic!("example {n}: {e}"));
        tx.mark_handed_out();
        let applied = Applied::from(applied);
        assert!(!applied.lines.is_empty(), "example {n}: {applied:?}");
        let base = tx.base();
        let opts = CommitOpts::based(base, Stale::Relower, "Example");
        let commit = d.commit(tx, opts);
        commit.unwrap_or_else(|e| panic!("example {n} doesn't land: {e}"));
    }
}
