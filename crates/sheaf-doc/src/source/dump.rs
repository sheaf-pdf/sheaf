//! What sheaf reads of the common structures of every file the corpus's
//! tests read ([`structures_dump`]): a line a file, a column a structure,
//! counts and checksums only. It asserts nothing: it is for setting a
//! change to what reads a file's page tree, its name and number trees,
//! its text strings or its XMP claims beside what was there (M18 slice
//! 21).
//!
//! A file is read as [`open::file`](crate::open::file) reads it, its bytes
//! in a store of its own (a corpus file is never written to, so no save a
//! crash cut short is undone), and each structure by the reader
//! [`open::finish`](crate::open::finish) reads it with. The columns:
//!
//! - **pages:** the page table ([`Source::pages`]), each page's object and
//!   repeat, its boxes, rotation and user unit, resources, contents,
//!   annotation entries, struct parents and parents;
//! - **labels:** `/PageLabels`' ranges ([`label_ranges`]);
//! - **attachments:** every field of each ([`attachments_in`]);
//! - **dests:** the whole `/Names /Dests` tree as [`name_tree`] gives it,
//!   each key and its value as written;
//! - **outline:** each bookmark's title, destination and the rest
//!   ([`outline`]);
//! - **layers:** each group or heading ([`layers_in`]);
//! - **portfolio:** a `/Collection`'s fields and the file shown first
//!   ([`collection`]);
//! - **fields:** each terminal field's full name, tooltip and its value's
//!   length, never the value ([`Source::form`]);
//! - **scripts:** the document scripts as a combine copies them from a
//!   file ([`document_scripts`]), each name and its value as written;
//! - **claims:** what its XMP says it conforms to
//!   ([`Source::conformance`]): `PDF/A-…`, `PDF/UA-…`, `-` for none, or
//!   `error`.
//!
//! Each column is `name=count:checksum`, or `name=-` where the file has
//! no such structure (labels, a portfolio). A file that doesn't open is
//! the one line `unopened:` and why.
//!
//! [`label_ranges`]: crate::labels::label_ranges
//! [`attachments_in`]: crate::attachments::attachments_in
//! [`name_tree`]: crate::attachments::name_tree
//! [`outline`]: crate::open::outline
//! [`layers_in`]: crate::layers::layers_in
//! [`collection`]: crate::portfolio::collection
//! [`document_scripts`]: crate::materialize::emit::form::document_scripts

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::Path;
use std::time::Instant;

use crate::content::corpus::{each_path, written};
use crate::forms::Value as FieldValue;
use crate::forms::corpus::Corpus;
use crate::geom::Rect;
use crate::pdf::{Object, ObjectId};
use crate::store::{BlobKind, Src, Store};

/// Where [`structures_dump`] writes: a path. Unset, it reads nothing.
/// `just structures-dump <file>` sets it.
const DUMP: &str = "SHEAF_STRUCTURES_DUMP";

/// A corpus path whose records [`structures_dump`] prints in full to
/// stderr, reading that file alone: to set two runs beside each other.
const DUMP_ONLY: &str = "SHEAF_STRUCTURES_DUMP_ONLY";

/// The records of one column: how many, and the checksum of every value
/// written; where the records are printed in full, each value too, with
/// its name, texts as they are.
struct Column {
    name: &'static str,
    count: usize,
    sum: sheaf_util::Checksum,
    full: Option<String>,
}

impl Column {
    fn new(name: &'static str, full: bool) -> Column {
        Column {
            name,
            count: 0,
            sum: sheaf_util::Checksum::new(),
            full: full.then(String::new),
        }
    }

    /// A record of kind `what` begins.
    fn begin(&mut self, what: &str) {
        self.count += 1;
        self.sum.write(what.as_bytes());
        self.sum.write(&[0]);
        if let Some(full) = &mut self.full {
            full.push_str(what);
        }
    }

    fn shown(&mut self, name: &str, shown: impl std::fmt::Display) {
        if let Some(full) = &mut self.full {
            full.push_str(&format!(" {name}={shown}"));
        }
    }

    fn number(&mut self, name: &str, v: u64) {
        self.sum.write(&v.to_le_bytes());
        self.shown(name, v);
    }

    fn signed(&mut self, name: &str, v: i64) {
        self.sum.write(&v.to_le_bytes());
        self.shown(name, v);
    }

    fn flag(&mut self, name: &str, v: bool) {
        self.sum.write(&[u8::from(v)]);
        self.shown(name, v);
    }

    /// Bytes, their length first.
    fn bytes(&mut self, name: &str, v: &[u8]) {
        self.sum.write(&(v.len() as u64).to_le_bytes());
        self.sum.write(v);
        if self.full.is_some() {
            let hex: String = v.iter().map(|b| format!("{b:02x}")).collect();
            self.shown(name, format!("<{hex}>"));
        }
    }

    /// A text, its length first.
    fn text(&mut self, name: &str, v: &str) {
        self.sum.write(&(v.len() as u64).to_le_bytes());
        self.sum.write(v.as_bytes());
        self.shown(name, format!("{v:?}"));
    }

    /// What may not be there: whether it is, then it.
    fn maybe<T>(&mut self, name: &str, v: Option<T>, write: impl FnOnce(&mut Column, &str, T)) {
        self.sum.write(&[u8::from(v.is_some())]);
        match v {
            Some(v) => write(self, name, v),
            None => self.shown(name, "-"),
        }
    }

    /// One of sheaf's own small types, which this slice doesn't change
    /// (a style, a view, a reference to an attachment), by its `Debug`.
    fn debug(&mut self, name: &str, v: &impl Debug) {
        let text = format!("{v:?}");
        self.sum.write(&(text.len() as u64).to_le_bytes());
        self.sum.write(text.as_bytes());
        self.shown(name, text);
    }

    fn id(&mut self, name: &str, id: ObjectId) {
        self.sum.write(&id.number.to_le_bytes());
        self.sum.write(&id.generation.to_le_bytes());
        self.shown(name, format!("{} {} R", id.number, id.generation));
    }

    /// Objects, how many first.
    fn ids(&mut self, name: &str, ids: impl IntoIterator<Item = ObjectId>) {
        let ids: Vec<ObjectId> = ids.into_iter().collect();
        self.sum.write(&(ids.len() as u64).to_le_bytes());
        for id in &ids {
            self.sum.write(&id.number.to_le_bytes());
            self.sum.write(&id.generation.to_le_bytes());
        }
        if self.full.is_some() {
            let ids: Vec<String> = (ids.iter())
                .map(|id| format!("{} {} R", id.number, id.generation))
                .collect();
            self.shown(name, format!("[{}]", ids.join(", ")));
        }
    }

    /// Numbers by their bits.
    fn bits(&mut self, name: &str, vs: &[f32]) {
        for v in vs {
            self.sum.write(&v.to_bits().to_le_bytes());
        }
        if self.full.is_some() {
            let vs: Vec<String> = vs.iter().map(|v| format!("{v}")).collect();
            self.shown(name, vs.join(","));
        }
    }

    fn rect(&mut self, name: &str, r: Rect) {
        self.bits(name, &[r.x0, r.y0, r.x1, r.y1]);
    }

    /// An object as PDF writes it: 0 where it isn't there, 2 where it
    /// can't be written.
    fn object(&mut self, name: &str, v: Option<&Object>) {
        let written = v.map(|v| {
            let mut out = Vec::new();
            crate::pdf::write::write(&mut out, v).map(|()| out)
        });
        let said: u64 = match &written {
            None => 0,
            Some(Ok(_)) => 1,
            Some(Err(_)) => 2,
        };
        self.sum.write(&said.to_le_bytes());
        match written {
            None => self.shown(name, "-"),
            Some(Ok(bytes)) => {
                self.sum.write(&(bytes.len() as u64).to_le_bytes());
                self.sum.write(&bytes);
                self.shown(name, String::from_utf8_lossy(&bytes).escape_debug());
            }
            Some(Err(_)) => self.shown(name, "unwritten"),
        }
    }

    /// The record ends.
    fn end(&mut self) {
        if let Some(full) = &mut self.full {
            full.push('\n');
        }
    }

    /// The column as the line has it, `name=count:checksum`; the records
    /// in full go to stderr under `at`.
    fn finish(self, at: &str) -> String {
        if let Some(full) = self.full {
            eprint!("== {at}: {}\n{full}", self.name);
        }
        format!("{}={}:{:032x}", self.name, self.count, self.sum.finish())
    }
}

/// The page table: a record a page.
fn pages(src: &Src, full: bool) -> Column {
    let mut c = Column::new("pages", full);
    for page in src.pages() {
        c.begin("page");
        c.id("id", page.id);
        c.number("repeat", u64::from(page.repeat));
        let geom = &page.geom;
        c.rect("media", geom.media);
        c.rect("crop", geom.crop);
        for (name, r) in [
            ("trim", geom.trim),
            ("bleed", geom.bleed),
            ("art", geom.art),
        ] {
            c.maybe(name, r, Column::rect);
        }
        c.signed("rotate", i64::from(geom.rotate.degrees()));
        c.bits("user_unit", &[geom.user_unit]);
        c.object("resources", page.resources.as_deref());
        c.ids("contents", page.contents.iter().copied());
        c.number("annots", page.annots.len() as u64);
        for entry in page.annots.iter() {
            c.object("annot", Some(entry));
        }
        c.maybe("struct_parents", page.struct_parents, Column::signed);
        c.ids("parents", page.parents());
        c.end();
    }
    c
}

/// `/PageLabels`: a record a range, or none.
fn labels(src: &Src, full: bool) -> Option<Column> {
    let ranges = crate::labels::label_ranges(src.pdf())?;
    let mut c = Column::new("labels", full);
    for range in &ranges {
        c.begin("range");
        c.number("start", u64::from(range.start));
        c.debug("style", &range.style);
        c.text("prefix", &range.prefix);
        c.number("first", u64::from(range.first));
        c.end();
    }
    Some(c)
}

fn attachments(src: &Src, full: bool) -> Column {
    let mut c = Column::new("attachments", full);
    for a in crate::attachments::attachments_in(src.pdf(), src.page_ids()) {
        c.begin("attachment");
        c.debug("at", &a.at);
        c.text("name", &a.name);
        c.text("description", &a.description);
        c.maybe("size", a.size, Column::number);
        c.text("modified", &a.modified);
        c.text("created", &a.created);
        c.text("folder", &a.folder);
        c.number("fields", a.fields.len() as u64);
        for (key, value) in &a.fields {
            c.text("key", key);
            c.text("value", value);
        }
        c.end();
    }
    c
}

/// The whole `/Names /Dests` tree.
fn dests(src: &Src, full: bool) -> Column {
    let mut c = Column::new("dests", full);
    for (key, value) in crate::attachments::name_tree(src.pdf(), b"Dests") {
        c.begin("dest");
        c.text("key", &key);
        c.object("value", Some(value));
        c.end();
    }
    c
}

fn outline(src: &Src, full: bool) -> Column {
    let mut c = Column::new("outline", full);
    for item in crate::open::outline(src) {
        c.begin("item");
        c.text("title", &item.title);
        c.maybe("page", item.page, |c, name, page| {
            c.number(name, u64::from(page))
        });
        c.number("depth", u64::from(item.depth));
        c.debug("view", &item.view);
        c.debug("color", &item.color);
        c.debug("style", &item.style);
        c.flag("open", item.open);
        c.maybe("object", item.object, Column::id);
        c.flag("kept", item.kept);
        c.end();
    }
    c
}

fn layers(src: &Src, full: bool) -> Column {
    let mut c = Column::new("layers", full);
    for layer in crate::layers::layers_in(src.pdf()) {
        c.begin("layer");
        c.debug("id", &layer.id);
        c.text("name", &layer.name);
        c.flag("on", layer.on);
        c.flag("locked", layer.locked);
        c.number("depth", u64::from(layer.depth));
        c.end();
    }
    c
}

/// A portfolio's `/Collection`: a record a field, then the file shown
/// first; none where the file isn't one.
fn portfolio(src: &Src, full: bool) -> Option<Column> {
    let collection = crate::portfolio::collection(src.pdf())?;
    let mut c = Column::new("portfolio", full);
    for field in &collection.fields {
        c.begin("field");
        c.text("key", &field.key);
        c.text("name", &field.name);
        c.debug("kind", &field.kind);
        c.signed("order", field.order);
        c.flag("visible", field.visible);
        c.end();
    }
    c.begin("initial");
    c.maybe("initial", collection.initial.as_deref(), Column::text);
    c.end();
    Some(c)
}

/// The form's terminal fields: each full name, tooltip and its value's
/// length in bytes, never the value.
fn fields(src: &Src, full: bool) -> Column {
    let mut c = Column::new("fields", full);
    for field in &src.form().fields {
        c.begin("field");
        c.text("name", &field.name);
        c.maybe("tooltip", field.tooltip.as_deref(), Column::text);
        match &field.value {
            None => c.number("value", 0),
            Some(FieldValue::Text(text)) => {
                c.number("value", 1);
                c.number("text", text.len() as u64);
            }
            Some(FieldValue::State(state)) => {
                c.number("value", 2);
                c.number("state", state.len() as u64);
            }
            Some(FieldValue::Items(items)) => {
                c.number("value", 3);
                c.number("items", items.len() as u64);
                for item in items.iter() {
                    c.number("item", item.len() as u64);
                }
            }
            Some(FieldValue::Signed) => c.number("value", 4),
        }
        c.end();
    }
    c
}

/// The document scripts, as a combine copies them from a file.
fn scripts(src: &Src, full: bool) -> Column {
    let mut c = Column::new("scripts", full);
    for (name, script) in crate::materialize::emit::form::document_scripts(src) {
        c.begin("script");
        c.bytes("name", &name);
        c.object("value", Some(&script));
        c.end();
    }
    c
}

/// What its XMP claims: each claim by its name, `+` between, `-` for
/// none, `error` where it can't be read.
fn claims(src: &Src) -> String {
    let Ok(claims) = src.conformance() else {
        return "claims=error".into();
    };
    let mut said = Vec::new();
    if let Some(pdfa) = claims.pdfa {
        said.push(pdfa.to_string());
    }
    if let Some(ua) = claims.pdfua {
        said.push(ua.to_string());
    }
    match said.is_empty() {
        true => "claims=-".into(),
        false => format!("claims={}", said.join("+")),
    }
}

/// The dump's line for the file at `path` of the corpus.
fn dump_file(corpus: &Corpus, path: &str, full: bool) -> Vec<String> {
    let none = |why: String| vec![format!("{path}\tunopened: {why}")];
    let bytes = match std::fs::read(corpus.root.join(path)) {
        Ok(bytes) => bytes,
        Err(e) => return none(e.to_string()),
    };
    let kind = BlobKind::file(Path::new(path), None);
    let src = match Store::new().open(bytes.into(), kind) {
        Ok(src) => src,
        Err(e) => return none(format!("{e:#}").replace(['\n', '\t'], " ")),
    };
    let absent = |name: &str| format!("{name}=-");
    let columns = [
        pages(&src, full).finish(path),
        labels(&src, full).map_or_else(|| absent("labels"), |c| c.finish(path)),
        attachments(&src, full).finish(path),
        dests(&src, full).finish(path),
        outline(&src, full).finish(path),
        layers(&src, full).finish(path),
        portfolio(&src, full).map_or_else(|| absent("portfolio"), |c| c.finish(path)),
        fields(&src, full).finish(path),
        scripts(&src, full).finish(path),
        claims(&src),
    ];
    vec![format!("{path}\t{}", columns.join("\t"))]
}

/// Every file the dump reads, each once, in the order of their paths:
/// every file of the forms, scans, signatures and conformance manifests,
/// and the pool's sample (`bench/pool-sample.txt`).
fn picks(corpus: &Corpus) -> Vec<String> {
    let list = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/pool-sample.txt");
    let pool = std::fs::read_to_string(list).expect("bench/pool-sample.txt");
    let pool = (pool.lines().map(str::trim))
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    let manifests = [
        corpus.forms(),
        corpus.scans(),
        corpus.signatures(),
        corpus.conformance(),
    ];
    let listed = (manifests.into_iter())
        .flat_map(|rows| rows.expect("the corpus's manifests"))
        .map(|e| e.path);
    let picks: BTreeSet<String> = listed.chain(pool).collect();
    picks.into_iter().collect()
}

/// What sheaf reads of the common structures of every file the corpus's
/// tests read, to set beside itself after a change to what reads them: a
/// line a file ([`dump_file`]), the files in the order of their paths,
/// written to the path in [`DUMP`]. Two runs over the same code write the
/// same file. With [`DUMP_ONLY`] it reads the one file named, and prints
/// each record in full to stderr.
#[test]
#[ignore = "a dump of the corpus's common structures, written to $SHEAF_STRUCTURES_DUMP"]
fn structures_dump() {
    let to = std::env::var_os(DUMP);
    let only = std::env::var(DUMP_ONLY).ok();
    if to.is_none() && only.is_none() {
        eprintln!("skipped: neither {DUMP} nor {DUMP_ONLY} is set");
        return;
    }
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let started = Instant::now();
    let full = only.is_some();
    let paths = picks(&corpus);
    eprintln!("{} files", paths.len());
    let lines = each_path(
        paths,
        (only.as_deref(), DUMP_ONLY),
        |path| dump_file(&corpus, path, full),
        |path, said| format!("{path}\tpanicked: {said}"),
    );
    let mut said: BTreeMap<String, usize> = BTreeMap::new();
    for line in &lines {
        let mut fields = line.split('\t').skip(1);
        let first = fields.next().unwrap_or_default();
        let key = match first.split_once(':') {
            Some((what, _)) if what == "unopened" || what == "panicked" => what.to_string(),
            _ => "opened".to_string(),
        };
        *said.entry(key).or_default() += 1;
        for field in line.split('\t').skip(1) {
            if let Some((name, count)) = field.split_once('=')
                && let Some((count, _)) = count.split_once(':')
                && count != "0"
            {
                *said.entry(format!("with {name}")).or_default() += 1;
            }
        }
    }
    eprintln!(
        "{} lines, {said:?}, in {:.0} s",
        lines.len(),
        started.elapsed().as_secs_f64()
    );
    written(&lines, full, to);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::fixture::Form;

    /// The dump's line for a form of two pages, read from its bytes.
    #[test]
    fn a_file_s_line_has_a_column_a_structure() {
        let mut f = Form::new(2);
        let name = f.text("name", 0, [10.0, 700.0, 210.0, 720.0]);
        f.fill(name, "Trenton");
        let bytes = f.bytes();
        let store = Store::new();
        let src = (store.open(bytes.into(), BlobKind::file(Path::new("f.pdf"), None))).unwrap();
        let line = [
            pages(&src, false).finish("f.pdf"),
            fields(&src, false).finish("f.pdf"),
            scripts(&src, false).finish("f.pdf"),
            claims(&src),
        ];
        assert!(line[0].starts_with("pages=2:"), "{line:?}");
        assert!(line[1].starts_with("fields=1:"), "{line:?}");
        assert!(line[2].starts_with("scripts=0:"), "{line:?}");
        assert_eq!(line[3], "claims=-");
        assert!(labels(&src, false).is_none());
        assert!(portfolio(&src, false).is_none());

        // Two readings of the same file say the same, and the value's
        // length is all a field's record holds of it.
        let again = fields(&src, true).finish("f.pdf");
        assert_eq!(line[1], again);
        let mut c = Column::new("fields", true);
        c.begin("field");
        c.text("name", "name");
        c.maybe("tooltip", None::<&str>, Column::text);
        c.number("value", 1);
        c.number("text", "Trenton".len() as u64);
        c.end();
        assert_eq!(c.finish("f.pdf"), again);
    }
}
