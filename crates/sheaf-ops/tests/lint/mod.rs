//! What sheaf's Arlington tests need on top of pdf-lint: a file sheaf
//! wrote read as pdf-lint reads one, qpdf decoding what's compressed or
//! encrypted in it ([`read`]); the objects a save wrote, its hold
//! ([`written`]); the object streams a writer packs held to the model's
//! `ObjectStream` table, which pdf-lint leaves out ([`check`]); and the
//! reader's findings in the structure the writer wrote held as its own
//! ([`own_structure`]).

mod hold;

pub use hold::written;

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use pdf_lint::bytes::{Decode, Written};
use pdf_lint::predicate::{self, Cx, Fields, Gate, Reference, Since, Structure, Trees, Truth};
use pdf_lint::{
    ArlType, At, Class, Dict, Document, Entry, Finding, Hold, Made, Object, ObjectId, Options,
    Qpdf, Report, Row, Rule, Source, Span, Table, Unchecked, Unlisted, UnlistedIn, Value, Version,
    Wrapped,
};

/// `bytes`, a file `what` wrote, read as pdf-lint reads one, with
/// `password`: from its bytes, qpdf decoding what isn't in plain view
/// ([`Decoder`]).
pub fn read(what: &str, bytes: &[u8], password: Option<&str>) -> Document {
    let decoder = Decoder {
        what,
        bytes,
        password,
        qpdf: OnceCell::new(),
    };
    pdf_lint::bytes::read_with(bytes, &decoder)
}

/// qpdf, as a file `what` wrote is read with, as pdf-lint's command line
/// reads one: run only when the reader asks for something, the file's
/// bytes then written to a file of their own for it, which goes when the
/// decoder does. A file all in plain view runs no qpdf and writes no
/// file. A file that needs qpdf when there's none that decodes fails its
/// test, saying why, and never passes for want of what qpdf would decode.
struct Decoder<'a> {
    what: &'a str,
    bytes: &'a [u8],
    password: Option<&'a str>,
    /// The file the bytes were written to, and qpdf for it, from the
    /// first ask.
    qpdf: OnceCell<(PathBuf, Qpdf)>,
}

impl Decoder<'_> {
    /// qpdf for the file, written on the first ask.
    fn qpdf(&self) -> &Qpdf {
        let (_, qpdf) = self.qpdf.get_or_init(|| {
            if let Err(why) = ready() {
                panic!(
                    "{} needs qpdf to decode what's compressed or encrypted in it, and {why}: \
                     install qpdf {} or later, or name one in PDF_LINT_QPDF",
                    self.what,
                    pdf_lint::qpdf::OLDEST
                );
            }
            static FILES: AtomicUsize = AtomicUsize::new(0);
            let n = FILES.fetch_add(1, Ordering::Relaxed);
            let path = dir().join(format!("{n}.pdf"));
            std::fs::write(&path, self.bytes).unwrap();
            let qpdf = Qpdf::new(&path);
            let qpdf = match self.password {
                Some(password) => qpdf.password(password),
                None => qpdf,
            };
            (path, qpdf)
        });
        qpdf
    }
}

impl Drop for Decoder<'_> {
    fn drop(&mut self) {
        if let Some((path, _)) = self.qpdf.get() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Decode for Decoder<'_> {
    fn decode(&self, ids: &[ObjectId]) -> BTreeMap<ObjectId, Result<Vec<u8>, String>> {
        self.qpdf().decode(ids)
    }

    fn plaintext(&self, ids: &[ObjectId]) -> BTreeMap<ObjectId, Result<Object, String>> {
        self.qpdf().plaintext(ids)
    }

    fn decode_written(&self, stream: Written) -> Result<Vec<u8>, String> {
        self.qpdf().decode_written(stream)
    }
}

/// [`Qpdf::ready`] for the qpdf the tests decode with, `PDF_LINT_QPDF`'s
/// or the one on the `PATH`: asked once, when a file first needs it.
fn ready() -> &'static Result<String, String> {
    static READY: OnceLock<Result<String, String>> = OnceLock::new();
    READY.get_or_init(|| Qpdf::new(PathBuf::new()).ready())
}

/// The directory the files qpdf reads are written to: one for the test
/// binary's run, as [`scratch`](crate::common::scratch) empties its
/// directory each time it's asked for it.
fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| crate::common::scratch("pdf-lint"))
}

/// What sheaf's tests check with: pdf-lint's defaults, but for what the
/// version checked at deprecates, a finding, as sheaf's own checker had
/// it, where pdf-lint by default only counts it. As by default, a field
/// merged with its widget is checked as both tables (not `one_table`), a
/// key the model doesn't list is a finding in a built object only
/// (`unlisted`), and no key is exempt: sheaf's own keys are second-class
/// names (`SHEF_`, F24), which are never findings.
pub fn options() -> Options {
    Options {
        deprecations: true,
        ..Options::default()
    }
}

/// `doc`'s objects that `hold` holds, held to the model with `options`,
/// as [`pdf_lint::check`] holds them, and the object streams it holds as
/// built too ([`object_streams`]).
pub fn check(doc: &Document, hold: &Hold, options: &Options) -> Report {
    let mut report = pdf_lint::check(doc, hold, options);
    object_streams(doc, hold, options, &mut report);
    report
}

/// `report`'s reader findings in the file's own structure (its header,
/// cross-reference sections and `%%EOF`: those of no object, or of the
/// trailer) that are in what the writer wrote, from byte `from` on, held
/// as built. pdf-lint holds them as merged, as it holds the trailer (its
/// check.rs's `made`), so a like fault in a file read would carry one
/// the writer made. One the reader can't place is the writer's in a file
/// written whole (`from` 0), and is left merged in an update's.
pub fn own_structure(report: &mut Report, from: usize) {
    let trailer = report.trailer;
    for finding in &mut report.findings {
        let structure = finding.object.is_none() || finding.object == trailer;
        let written = match finding.at {
            Some(Span {
                stream: None,
                start,
                ..
            }) => start >= from,
            _ => from == 0,
        };
        if finding.rule == Rule::Reader && structure && written {
            finding.made = Made::Built;
        }
    }
}

/// How many links the walk follows from an object stream: what it
/// reaches, through references that may cycle, is bounded.
const DEPTH: usize = 8;

/// The object streams of `doc` that `hold` holds as built, held to the
/// model's `ObjectStream` table ([`Walk`]), what's found and counted
/// added to `report`. pdf-lint 0.1.0 leaves object streams out of its
/// walk, as it does a file's other structures (its check.rs's `left_out`),
/// where sheaf's own checker walked each one sheaf packs as
/// `ObjectStream`; this walks them until pdf-lint does. An object the walk
/// goes on to by reference isn't unreached.
fn object_streams(doc: &Document, hold: &Hold, options: &Options, report: &mut Report) {
    let table = (pdf_lint::grammar().table("ObjectStream")).expect("the model has the table");
    let version = options
        .version
        .unwrap_or_else(|| pdf_lint::file_version(doc));
    let extensions = report.extensions.clone();
    let mut walk = Walk {
        doc,
        hold,
        version,
        extensions: &extensions,
        options,
        report: &mut *report,
        reached: BTreeSet::new(),
    };
    for (&id, object) in &doc.objects {
        if let Ok(object) = object
            && walk.built(id)
            && is_object_stream(object)
        {
            walk.hold(table, id, object, 0);
        }
    }
    let reached = std::mem::take(&mut walk.reached);
    (report.findings)
        .retain(|f| f.rule != Rule::Unreached || !f.object.is_some_and(|id| reached.contains(&id)));
}

/// The object streams a writer built, held to the model as pdf-lint holds
/// a dictionary it reaches, each cell as pdf-lint's predicates read it:
/// each key its table lists, from the version it came in and not one that
/// deprecates it, of a type its row allows, an indirect reference or
/// direct as the row asks, one of its PossibleValues and meeting its
/// SpecialCase; each key the table doesn't list unlisted, as
/// [`Options::unlisted`] says, but a second- or third-class name or an
/// exempt one; each key the table requires there; and none written twice.
/// What a value links to is held to its table the same way ([`Walk::linked`]),
/// to [`DEPTH`] links. What's checked is counted as pdf-lint counts it.
struct Walk<'a> {
    doc: &'a Document,
    hold: &'a Hold,
    version: Version,
    /// The extensions the file is checked under.
    extensions: &'a BTreeSet<String>,
    options: &'a Options,
    report: &'a mut Report,
    /// The objects the walk went on to by reference, each once.
    reached: BTreeSet<ObjectId>,
}

/// Where a value is: at `at` in `container`, which is in the object `id`,
/// held to `table`, `depth` links from an object stream.
#[derive(Clone, Copy)]
struct Place<'a> {
    table: &'a Table,
    id: ObjectId,
    container: &'a Object,
    at: At<'a>,
    depth: usize,
}

impl<'a> Walk<'a> {
    /// `container`, a dictionary or a stream in the object `id`, `depth`
    /// links from an object stream, held to `table`.
    fn hold(&mut self, table: &'a Table, id: ObjectId, container: &'a Object, depth: usize) {
        let dict = match &container.value {
            Value::Dict(dict) => dict,
            Value::Stream(stream) => &stream.dict,
            _ => return,
        };
        *self.report.checked.entry(table.name.clone()).or_default() += 1;
        for entry in dict.last_entries() {
            let at = At::Key(&entry.key);
            let written = entry_at(entry);
            match table.row(at) {
                Some(row) => {
                    let place = Place {
                        table,
                        id,
                        container,
                        at,
                        depth,
                    };
                    let filters = self.filters(dict, at);
                    self.value(place, row, &entry.value, written, &filters);
                }
                None => self.unlisted(table, id, &entry.key, &entry.value, written),
            }
        }
        for twice in dict.duplicates() {
            let value = format!("{} (its key written again later)", shown(&twice.value));
            let key = String::from_utf8_lossy(&twice.key);
            self.find(table, id, &key, Rule::Duplicate, value, entry_at(twice));
        }
        for row in table.rows.iter().filter(|row| !row.key.ends_with('*')) {
            let at = At::Key(row.key.as_bytes());
            let came = matches!(self.since(row), Since::In(since) if since <= self.version);
            let there = (dict.get(row.key.as_bytes())).is_some_and(|v| self.present(v));
            if !came || there {
                continue;
            }
            match self.with_cx(container, at, None, |cx| {
                predicate::required(&row.required, cx)
            }) {
                Truth::True => {
                    let absent = "absent".to_string();
                    self.find(table, id, &row.key, Rule::Missing, absent, container.at);
                }
                Truth::False => {}
                Truth::Unknown(why) => self.unchecked("Required", why, table, &row.key),
            }
        }
    }

    /// `raw`, written at `written` at `place`, held to `row` of its table;
    /// `filters`, those it's the decode parameters of ([`Walk::filters`]).
    fn value(
        &mut self,
        place: Place<'a>,
        row: &Row,
        raw: &'a Object,
        written: Option<Span>,
        filters: &[Option<&'a [u8]>],
    ) {
        let Place {
            table,
            id,
            container,
            at,
            ..
        } = place;
        let key = key_of(at);
        let value = match raw.value {
            Value::Ref(to) => match self.doc.get(to) {
                Ok(value) => value,
                Err(_) => {
                    let value = format!("{} {} R", to.0, to.1);
                    return self.find(table, id, &key, Rule::Unresolved, value, written);
                }
            },
            _ => raw,
        };
        // A dictionary's null entry is no entry (ISO 32000-2, 7.3.7), unless
        // its row allows null.
        let null = (row.r#type.iter()).any(|t| t.item == ArlType::Null);
        if matches!((at, &value.value), (At::Key(_), Value::Null)) && !null {
            return;
        }
        self.report.keys += 1;
        match self.since(row) {
            Since::In(since) if since > self.version => {
                let value = format!("{} (since {since})", shown(value));
                self.find(table, id, &key, Rule::TooNew, value, written);
            }
            Since::In(_) | Since::Extension(_) => {}
            Since::Unknown(why) => self.unchecked("SinceVersion", why, table, &key),
        }
        if let Some(from) = pdf_lint::grammar::version(&row.deprecated_in)
            && from <= self.version
        {
            let value = format!("{} (deprecated in {from})", shown(value));
            self.deprecated(table, id, &key, value, written);
        }
        let gate = |wraps| predicate::gate(wraps, self.version, self.extensions);
        let typed =
            (row.r#type.iter()).position(|t| is(value, t.item) && gate(&t.wraps) != Gate::Excluded);
        let Some(i) = typed else {
            return self.find(table, id, &key, Rule::Type, shown(value), written);
        };
        match self.with_cx(container, at, None, |cx| {
            predicate::reference(&row.indirect_reference[i], cx)
        }) {
            Reference::Indirect if !matches!(raw.value, Value::Ref(_)) => {
                self.find(table, id, &key, Rule::Indirect, shown(value), written);
            }
            Reference::Direct if matches!(raw.value, Value::Ref(_)) => {
                self.find(table, id, &key, Rule::Direct, shown(raw), written);
            }
            Reference::Unknown(why) => self.unchecked("IndirectReference", why, table, &key),
            _ => {}
        }
        self.values(place, (row, i), value, written);
        let special = &row.special_case[i];
        if !special.is_empty() {
            match self.with_cx(container, at, Some(table), |cx| {
                predicate::holds_fully(special, cx)
            }) {
                Truth::True => {}
                Truth::False => {
                    let value = format!("{} ({special})", shown(value));
                    self.find(table, id, &key, Rule::Special, value, written);
                }
                Truth::Unknown(why) => self.unchecked("SpecialCase", why, table, &key),
            }
        }
        self.linked(place, (row, i), (raw, value), filters);
    }

    /// What `value`, `raw` resolved, at `place`, of `row`'s `i`th type,
    /// links to, held to it: an array's elements to the rows of its one
    /// linked table, a dictionary or a stream to its linked table, or, of
    /// several, to the one for the filter it's the decode parameters of
    /// (`filters`: `FilterFlateDecode` for `/FlateDecode`). A reference is
    /// followed, once, to an object held as built, but an object stream,
    /// which is held on its own; one to an object held as merged, or not
    /// held, isn't sheaf's to walk. A table that can't be told, and what
    /// lies past [`DEPTH`], are counted as not checked.
    fn linked(
        &mut self,
        place: Place<'a>,
        (row, i): (&Row, usize),
        (raw, value): (&'a Object, &'a Object),
        filters: &[Option<&'a [u8]>],
    ) {
        let links = self.links(&row.link[i]);
        let (table, key) = (place.table, key_of(place.at));
        if links.is_empty()
            || !matches!(
                value.value,
                Value::Array(_) | Value::Dict(_) | Value::Stream(_)
            )
        {
            return;
        }
        if place.depth == DEPTH {
            let why = format!("more than {DEPTH} links from an object stream");
            return self.unchecked("Link", why, table, &key);
        }
        let id = match raw.value {
            Value::Ref(to) if !self.built(to) || is_object_stream(value) => return,
            Value::Ref(to) if !self.reached.insert(to) => return,
            Value::Ref(to) => to,
            _ => place.id,
        };
        let depth = place.depth + 1;
        match &value.value {
            Value::Array(elements) => {
                let &[linked] = &links[..] else {
                    let why = "an array linked to several tables".to_string();
                    return self.unchecked("Link", why, table, &key);
                };
                *self.report.checked.entry(linked.name.clone()).or_default() += 1;
                for (n, element) in elements.iter().enumerate() {
                    let at = At::Index(n);
                    let filters = filters.get(n).map_or(&[][..], std::slice::from_ref);
                    match linked.row(at) {
                        Some(row) => {
                            let place = Place {
                                table: linked,
                                id,
                                container: value,
                                at,
                                depth,
                            };
                            self.value(place, row, element, element.at, filters);
                        }
                        None => {
                            let n = n.to_string();
                            self.unlisted(linked, id, n.as_bytes(), element, element.at);
                        }
                    }
                }
            }
            _ => {
                let filter = match filters {
                    [Some(filter)] => {
                        let name = format!("Filter{}", String::from_utf8_lossy(filter));
                        links.iter().find(|t| t.name == name)
                    }
                    _ => None,
                };
                match (&links[..], filter) {
                    ([linked], _) | (_, Some(linked)) => self.hold(linked, id, value, depth),
                    _ => {
                        let why = "which of its linked tables can't be told".to_string();
                        self.unchecked("Link", why, table, &key);
                    }
                }
            }
        }
    }

    /// Whether `value`, at `place`, of `row`'s `i`th type, is one of the
    /// row's PossibleValues for the type that the version lets in: a
    /// constant, or a predicate, as pdf-lint reads them. One that can't be
    /// told makes them all unknown, met or not.
    fn values(
        &mut self,
        place: Place,
        (row, i): (&Row, usize),
        value: &Object,
        written: Option<Span>,
    ) {
        let Place {
            table,
            id,
            container,
            at,
            ..
        } = place;
        let let_in = |item: &&Wrapped<String>| {
            predicate::gate(&item.wraps, self.version, self.extensions) != Gate::Excluded
        };
        let listed: Vec<&str> = (row.possible_values[i].iter())
            .filter(let_in)
            .map(|item| item.item.as_str())
            .collect();
        if listed.is_empty() {
            return;
        }
        let (mut met, mut unknown) = (false, None);
        for item in &listed {
            let truth = if item.starts_with("fn:") {
                self.with_cx(container, at, Some(table), |cx| {
                    predicate::holds_fully(item, cx)
                })
            } else if is_value(item, value) {
                Truth::True
            } else {
                Truth::False
            };
            match truth {
                Truth::True => met = true,
                Truth::False => {}
                Truth::Unknown(why) => {
                    unknown.get_or_insert(why);
                }
            }
        }
        let key = key_of(at);
        match (unknown, met) {
            (Some(why), _) => self.unchecked("PossibleValues", why, table, &key),
            (None, true) => {}
            (None, false) => {
                let value = format!("{} (not one of {})", shown(value), listed.join(", "));
                self.find(table, id, &key, Rule::Value, value, written);
            }
        }
    }

    /// The filters that a value at `at` in `dict` is the decode parameters
    /// of, in order (ISO 32000-2, 7.3.8.2): `/Filter`'s for `/DecodeParms`,
    /// `/FFilter`'s for `/FDecodeParms`, none for another key. One that
    /// isn't a name is none.
    fn filters(&self, dict: &'a Dict, at: At) -> Vec<Option<&'a [u8]>> {
        let key: &[u8] = match at {
            At::Key(b"DecodeParms") => b"Filter",
            At::Key(b"FDecodeParms") => b"FFilter",
            _ => return Vec::new(),
        };
        let doc = self.doc;
        let resolved = |o: &'a Object| match o.value {
            Value::Ref(to) => doc.get(to).ok(),
            _ => Some(o),
        };
        match dict.get(key).and_then(resolved).map(|f| &f.value) {
            Some(Value::Name(name)) => vec![Some(name.as_slice())],
            Some(Value::Array(names)) => (names.iter())
                .map(|name| resolved(name).and_then(Object::as_name))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The tables of `links` the version lets in, or all of them when it
    /// lets in none, as pdf-lint takes them.
    fn links(&self, links: &[Wrapped<String>]) -> Vec<&'static Table> {
        let table = |link: &Wrapped<String>| {
            (pdf_lint::grammar().table(&link.item)).expect("the model has the table")
        };
        let let_in = |link: &&Wrapped<String>| {
            predicate::gate(&link.wraps, self.version, self.extensions) != Gate::Excluded
        };
        let allowed: Vec<&Table> = links.iter().filter(let_in).map(table).collect();
        match allowed.is_empty() {
            true => links.iter().map(table).collect(),
            false => allowed,
        }
    }

    /// Whether the hold holds the object `id` as built.
    fn built(&self, id: ObjectId) -> bool {
        match self.hold {
            Hold::All(made) => *made == Made::Built,
            Hold::Only(objects) => objects.get(&id) == Some(&Made::Built),
        }
    }

    /// `key`, `value` written at `written` in the object `id`, which
    /// `table` doesn't list: counted, and a finding as
    /// [`Options::unlisted`] says of a built object, unless it's a second-
    /// or third-class name or exempt.
    fn unlisted(
        &mut self,
        table: &Table,
        id: ObjectId,
        key: &[u8],
        value: &Object,
        written: Option<Span>,
    ) {
        let class = Class::of(key);
        let exempt = (self.options.exempt.iter()).any(|e| match e.prefix {
            true => key.starts_with(&e.name),
            false => key == e.name.as_slice(),
        });
        let key = String::from_utf8_lossy(key).into_owned();
        if self.options.unlisted != UnlistedIn::Never && class == Class::Other && !exempt {
            self.find(table, id, &key, Rule::Unlisted, shown(value), written);
        }
        let unlisted = Unlisted {
            table: table.name.clone(),
            key,
            class,
        };
        *self.report.unlisted.entry(unlisted).or_default() += 1;
    }

    /// What `row`'s SinceVersion says, under the file's extensions.
    fn since(&self, row: &Row) -> Since {
        predicate::since(&row.since_version, self.extensions)
    }

    /// Whether an entry's value, `value`, is there: not null, nor a
    /// reference to null. A reference to an object the file hasn't is
    /// there, and unresolved.
    fn present(&self, value: &Object) -> bool {
        let resolved = match value.value {
            Value::Ref(to) => self.doc.get(to).ok(),
            _ => Some(value),
        };
        resolved.is_none_or(|o| !o.is_null())
    }

    /// `f` of the predicates' view of `at` in `container`, with the table
    /// whose defaults stand in for a key it lacks, for a SpecialCase or
    /// PossibleValues.
    fn with_cx<R>(
        &self,
        container: &Object,
        at: At,
        defaults: Option<&Table>,
        f: impl FnOnce(&Cx) -> R,
    ) -> R {
        let doc = self.doc;
        let resolve = |id: ObjectId| doc.get(id).ok();
        let hex = |at: Span| Source::hex(doc, at);
        let (trees, structure, fields) =
            (Trees::default(), Structure::default(), Fields::default());
        f(&Cx {
            container,
            at,
            parents: &[],
            trailer: doc.trailer.dict(),
            resolve: &resolve,
            version: self.version,
            defaults,
            size: doc.size,
            hex: &hex,
            trees: &trees,
            structure: &structure,
            fields: &fields,
            extensions: self.extensions,
        })
    }

    fn find(
        &mut self,
        table: &Table,
        id: ObjectId,
        key: &str,
        rule: Rule,
        value: String,
        at: Option<Span>,
    ) {
        self.report.findings.push(Finding {
            object: Some(id),
            made: Made::Built,
            path: "-".to_string(),
            table: table.name.clone(),
            key: key.to_string(),
            rule,
            value,
            at,
        });
    }

    /// What the version deprecates at `key` of `table`, in the object
    /// `id`: counted, and with [`Options::deprecations`] a finding.
    fn deprecated(
        &mut self,
        table: &Table,
        id: ObjectId,
        key: &str,
        value: String,
        at: Option<Span>,
    ) {
        let deprecated = (table.name.clone(), key.to_string());
        *self.report.deprecated.entry(deprecated).or_default() += 1;
        if self.options.deprecations {
            self.find(table, id, key, Rule::Deprecated, value, at);
        }
    }

    fn unchecked(&mut self, column: &'static str, why: String, table: &Table, key: &str) {
        let unchecked = Unchecked {
            column,
            why,
            table: table.name.clone(),
            key: key.to_string(),
        };
        *self.report.unchecked.entry(unchecked).or_default() += 1;
    }
}

/// Whether `object` is an object stream.
fn is_object_stream(object: &Object) -> bool {
    match &object.value {
        Value::Stream(stream) => {
            stream.dict.get(b"Type").and_then(Object::as_name) == Some(b"ObjStm")
        }
        _ => false,
    }
}

/// The key `at` is, or the element's index, as a finding names it.
fn key_of(at: At) -> String {
    match at {
        At::Key(key) => String::from_utf8_lossy(key).into_owned(),
        At::Index(i) => i.to_string(),
    }
}

/// Where an entry's key and value were written, the two together.
fn entry_at(entry: &Entry) -> Option<Span> {
    match (entry.key_at, entry.value.at) {
        (Some(key), Some(value)) => Some(Span {
            end: value.end,
            ..key
        }),
        (key, value) => value.or(key),
    }
}

/// Whether `o` is of the model's type `ty`, as PDF's objects go (ISO
/// 32000-2, 7.3): what refines one (a date's text, a rectangle's numbers)
/// isn't told, as `ObjectStream`'s rows ask none of it.
fn is(o: &Object, ty: ArlType) -> bool {
    use ArlType as T;
    matches!(
        (&o.value, ty),
        (Value::Integer(_), T::Integer | T::Bitmask | T::Number)
            | (Value::Real(_), T::Number)
            | (Value::Bool(_), T::Boolean)
            | (Value::Name(_), T::Name)
            | (Value::Null, T::Null)
            | (Value::Stream(_), T::Stream)
            | (
                Value::String(_),
                T::String | T::StringAscii | T::StringByte | T::StringText | T::Date
            )
            | (Value::Array(_), T::Array | T::Rectangle | T::Matrix)
            | (Value::Dict(_), T::Dictionary | T::NameTree | T::NumberTree)
    )
}

/// Whether `value` is the constant `item` of a PossibleValues cell: a
/// name, or a number.
fn is_value(item: &str, value: &Object) -> bool {
    match &value.value {
        Value::Name(name) => item == "*" || name.as_slice() == item.as_bytes(),
        Value::Integer(_) | Value::Real(_) => item.parse::<f64>().ok() == value.as_number(),
        _ => false,
    }
}

/// `o` as a finding shows it: a value as written, an array's elements, a
/// dictionary and a stream by what they are.
fn shown(o: &Object) -> String {
    match &o.value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Real(r) => r.to_string(),
        Value::Name(name) => format!("/{}", String::from_utf8_lossy(name)),
        Value::String(s) => format!("({})", s.text().unwrap_or("ciphertext".to_string())),
        Value::Ref((n, g)) => format!("{n} {g} R"),
        Value::Array(elements) => {
            let shown: Vec<String> = (elements.iter())
                .map(|e| match e.value {
                    Value::Array(_) => "[…]".to_string(),
                    _ => shown(e),
                })
                .collect();
            format!("[{}]", shown.join(" "))
        }
        Value::Dict(_) => "<< … >>".to_string(),
        Value::Stream(_) => "stream".to_string(),
    }
}

#[cfg(test)]
mod tests {
    //! The object streams sheaf packs, held to `ObjectStream` beside
    //! pdf-lint's walk ([`check`]).

    use std::sync::Arc;

    use lopdf::LazyPdf;
    use pdf_lint::{Document, Hold, Made, Object, ObjectId, Options, Report, Rule, Version, dict};
    use sheaf_doc::forms::fixture::Form;
    use sheaf_doc::materialize::save_rewritten;
    use sheaf_doc::serialize::fixture::object_streams_of;
    use sheaf_doc::source::Limits;

    use super::{check, options, read, written};
    use crate::common::{self, scratch};

    /// Object streams with a fault of each kind the table makes: a key it
    /// doesn't list, a required one missing, one of a type it doesn't
    /// allow, a value it doesn't allow or that fails its SpecialCase, one
    /// direct that must be indirect, a reference to nothing, a key written
    /// twice; and what they link to: a filter in an array that isn't one,
    /// directly and through a reference, and decode parameters, a
    /// dictionary and an array's, held to the table of the filter they
    /// pair with. Each is found, in the object it's in, held as built, and
    /// counted, and the array referenced isn't unreached; held as merged,
    /// as a source's, none is checked.
    #[test]
    fn an_object_streams_faults_are_found() {
        let mut doc = file(Version::new(1, 7));
        let extended = dict! { "Type" => "ObjStm", "N" => 0, "First" => 0, "Length" => 0 };
        let container = dict! {
            "Type" => "ObjStm",
            "First" => -1,
            "Length" => 10,
            "Filter" => vec![Object::from("Bogus")],
            "Extends" => Object::stream(extended),
            "Stray" => 1,
        };
        doc.insert(3, Object::stream(container));
        let container = dict! {
            "Type" => "ObjStm",
            "N" => "x",
            "First" => 0,
            "First" => 0,
            "DL" => -1,
            "Length" => Object::reference(99),
            "Filter" => "FlateDecode",
            "DecodeParms" => dict! { "Predictor" => 3 },
        };
        doc.insert(4, Object::stream(container));
        let container = dict! {
            "Type" => "ObjStm",
            "N" => 0,
            "First" => 0,
            "Length" => 0,
            "Filter" => Object::reference(6),
            "DecodeParms" => vec![dict! { "Predictor" => 3 }.into(), Object::null()],
        };
        doc.insert(5, Object::stream(container));
        doc.insert(6, vec![Object::from("FlateDecode"), Object::from("Bogus")]);
        let hold = Hold::Only((3..=6).map(|n| ((n, 0), Made::Built)).collect());
        let report = check(&doc, &hold, &options());
        let at = |n| Some((n, 0));
        assert_eq!(
            found(&report),
            [
                (at(3), "ArrayOfCompressionFilterNames", "0", Rule::Value),
                (at(3), "ObjectStream", "Extends", Rule::Indirect),
                (at(3), "ObjectStream", "First", Rule::Value),
                (at(3), "ObjectStream", "N", Rule::Missing),
                (at(3), "ObjectStream", "Stray", Rule::Unlisted),
                (at(4), "FilterFlateDecode", "Predictor", Rule::Value),
                (at(4), "ObjectStream", "DL", Rule::Value),
                (at(4), "ObjectStream", "DL", Rule::Special),
                (at(4), "ObjectStream", "First", Rule::Duplicate),
                (at(4), "ObjectStream", "Length", Rule::Unresolved),
                (at(4), "ObjectStream", "N", Rule::Type),
                (at(5), "FilterFlateDecode", "Predictor", Rule::Value),
                (at(6), "ArrayOfCompressionFilterNames", "1", Rule::Value),
            ],
            "{report}"
        );
        // The three, and the one that the first extends.
        assert_eq!(report.checked.get("ObjectStream"), Some(&4), "{report}");
        let filters = report.checked.get("FilterFlateDecode");
        assert_eq!(filters, Some(&2), "{report}");
        let merged = check(&doc, &Hold::All(Made::Merged), &options());
        assert!(!merged.checked.contains_key("ObjectStream"), "{merged}");
    }

    /// An object stream in a file of a version before object streams came
    /// in (1.5): each of its keys is too new.
    #[test]
    fn an_object_stream_before_1_5_is_too_new() {
        let mut doc = file(Version::new(1, 4));
        let container = dict! { "Type" => "ObjStm", "N" => 0, "First" => 0, "Length" => 0 };
        doc.insert(3, Object::stream(container));
        let report = check(
            &doc,
            &Hold::Only([((3, 0), Made::Built)].into()),
            &options(),
        );
        let at = Some((3, 0));
        assert_eq!(
            found(&report),
            [
                (at, "ObjectStream", "First", Rule::TooNew),
                (at, "ObjectStream", "Length", Rule::TooNew),
                (at, "ObjectStream", "N", Rule::TooNew),
                (at, "ObjectStream", "Type", Rule::TooNew),
            ],
            "{report}"
        );
    }

    /// What the version checked at deprecates is a finding with sheaf's
    /// options, where pdf-lint's defaults only count it: a catalog's
    /// `/NeedsRendering` in a PDF 2.0 file.
    #[test]
    fn what_the_version_deprecates_is_a_finding() {
        let mut doc = file(Version::new(2, 0));
        let catalog = dict! {
            "Type" => "Catalog", "Pages" => Object::reference(2), "NeedsRendering" => false,
        };
        doc.insert(1, catalog);
        let hold = Hold::Only([((1, 0), Made::Built)].into());
        let report = check(&doc, &hold, &options());
        let catalog = (Some((1, 0)), "Catalog", "NeedsRendering", Rule::Deprecated);
        assert_eq!(deprecated(&report), [catalog], "{report}");
        let counted = check(&doc, &hold, &Options::default());
        assert_eq!(deprecated(&counted), [], "{counted}");
        let at = ("Catalog".to_string(), "NeedsRendering".to_string());
        assert_eq!(counted.deprecated.get(&at), Some(&1), "{counted}");
    }

    /// What `report` found that the version deprecates, as [`found`].
    fn deprecated(report: &Report) -> Vec<(Option<ObjectId>, &str, &str, Rule)> {
        let found = found(report).into_iter();
        found.filter(|f| f.3 == Rule::Deprecated).collect()
    }

    /// A file of `version` with a catalog (1) and an empty page tree (2),
    /// what's tested added to it.
    fn file(version: Version) -> Document {
        let mut doc = Document::new(
            version,
            dict! { "Root" => Object::reference(1), "Size" => 7 },
        );
        doc.insert(
            1,
            dict! { "Type" => "Catalog", "Pages" => Object::reference(2) },
        );
        doc.insert(
            2,
            dict! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0 },
        );
        doc
    }

    /// What `report` found: each finding's object, table, key and rule,
    /// sorted.
    fn found(report: &Report) -> Vec<(Option<ObjectId>, &str, &str, Rule)> {
        let mut found: Vec<_> = (report.findings.iter())
            .map(|f| (f.object, f.table.as_str(), f.key.as_str(), f.rule))
            .collect();
        found.sort();
        found
    }

    /// A Rewrite of a file whose cross-references are a stream packs its
    /// objects into object streams: each is held as built, checked as
    /// `ObjectStream` and found faultless.
    #[test]
    fn the_object_streams_a_rewrite_packs_hold() {
        let dir = scratch("lint-packed");
        let path = common::write(&dir, "packed.pdf", &mut Form::new(2));
        let mut doc = common::open(&path);
        save_rewritten(&mut doc, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let pdf = LazyPdf::open(Arc::<[u8]>::from(&bytes[..]), None, &Limits::DEFAULT.reader);
        let pdf = pdf.unwrap();
        let streams = object_streams_of(&pdf);
        assert!(!streams.is_empty(), "a stream file's Rewrite packs");
        let record = doc.saved_record().unwrap();
        let hold = written(&record.map, doc.state(), doc.store(), &pdf, None);
        let report = check(&read("a Rewrite", &bytes, None), &hold, &options());
        let checked = report.checked.get("ObjectStream");
        assert_eq!(checked, Some(&streams.len()), "{report}");
        let found: Vec<_> = (report.findings.iter())
            .filter(|f| f.table == "ObjectStream")
            .collect();
        assert!(found.is_empty(), "{report}");
    }
}
