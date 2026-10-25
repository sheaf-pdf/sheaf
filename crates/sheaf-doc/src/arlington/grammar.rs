//! The model's tables, read from its TSV files (upstream's README.md, "TSV
//! Data Overview"; INTERNAL_GRAMMAR.md, "TSV file rules" and "TSV Data
//! Fields"). The Type cell is split into its types here, and a cell with
//! an item per type (IndirectReference, PossibleValues, SpecialCase, Link)
//! into those items; the version predicates around a type, a value or a
//! link are taken off and kept beside it. What a predicate means is the
//! checker's to say.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};

use crate::materialize::emit::PdfVersion;

/// Every table's header: upstream's 12 columns, in their order.
pub const HEADER: [&str; 12] = [
    "Key",
    "Type",
    "SinceVersion",
    "DeprecatedIn",
    "Required",
    "IndirectReference",
    "Inheritable",
    "DefaultValue",
    "PossibleValues",
    "SpecialCase",
    "Link",
    "Note",
];

/// The model: its tables, by name.
pub struct Grammar {
    tables: BTreeMap<String, Table>,
}

/// One object's table: `Catalog`, `AnnotSquare`, `ArrayOfAnnots`.
#[derive(Debug)]
pub struct Table {
    /// Its file's name less `.tsv`, which links name it by.
    pub name: String,
    pub rows: Vec<Row>,
    /// The first of its repeating rows (`0*`, `1*`: INTERNAL_GRAMMAR.md,
    /// "Column 1 - Key"), which is also the first array element they
    /// stand for; `rows.len()` when it has none.
    repeats: usize,
}

/// Where in an object a value is: under a dictionary's key, or an array's
/// element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At<'a> {
    Key(&'a [u8]),
    Index(usize),
}

/// One row: a key of a dictionary or an element of an array. The fields
/// are upstream's columns, in their order.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The line of its table's file it's on, the header being line 1.
    pub line: usize,
    /// A key; an array element's index (`0`); `n*`, an element of each
    /// repeating set; or `*`, any other key or element.
    pub key: String,
    /// The types its value may be, each maybe wrapped.
    pub r#type: Vec<Wrapped<ArlType>>,
    /// A version (`1.5`), or an `fn:Extension` or `fn:Eval` predicate.
    pub since_version: String,
    /// A version, or empty when it isn't deprecated.
    pub deprecated_in: String,
    /// `TRUE`, `FALSE`, or an `fn:IsRequired` predicate.
    pub required: String,
    /// Per type: `TRUE` (it must be an indirect reference), `FALSE`, or an
    /// `fn:MustBeDirect` or `fn:MustBeIndirect` predicate. A cell with one
    /// value, unbracketed, says it for every type.
    pub indirect_reference: Vec<String>,
    pub inheritable: bool,
    /// As the cell has it: the checker reads no defaults.
    pub default_value: String,
    /// Per type, the values it may take, each maybe wrapped; none when
    /// any may be.
    pub possible_values: Vec<Vec<Wrapped<String>>>,
    /// Per type, the predicate its value meets; empty when there's none.
    pub special_case: Vec<String>,
    /// Per type, the tables its value may be, each maybe wrapped; none
    /// for a simple type.
    pub link: Vec<Vec<Wrapped<String>>>,
    pub note: String,
}

/// Arlington's types (INTERNAL_GRAMMAR.md, "Column 2 - Type"): PDF's
/// objects, refined as the specification refines them (a date is a
/// string, a rectangle an array of four numbers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArlType {
    Array,
    Bitmask,
    Boolean,
    Date,
    Dictionary,
    Integer,
    Matrix,
    Name,
    NameTree,
    Null,
    Number,
    NumberTree,
    Rectangle,
    Stream,
    String,
    StringAscii,
    StringByte,
    StringText,
}

/// A cell's item with the version predicates around it taken off,
/// outermost first: `fn:IsPDFVersion(1.7,fn:Extension(ADBE_Extn3,X))` is
/// `X` in `IsPDFVersion` of `1.7` and `Extension` of `ADBE_Extn3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapped<T> {
    pub wraps: Vec<Wrap>,
    pub item: T,
}

/// A version predicate around an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrap {
    pub func: WrapFn,
    /// Its first argument: a version (`1.5`, which [`version`] reads), or
    /// for `Extension` the extension's name.
    pub arg: String,
}

/// The predicates that wrap an item: those INTERNAL_GRAMMAR.md's "Column
/// 11 - Link" allows around a link, which wrap types and values too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WrapFn {
    SinceVersion,
    BeforeVersion,
    IsPDFVersion,
    Deprecated,
    Extension,
}

impl Grammar {
    /// The tables in `dir`, a `.tsv` file each. An error names the file
    /// and line of a row that doesn't read, or that links to a table `dir`
    /// doesn't have.
    pub fn load(dir: &Path) -> Result<Grammar> {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "tsv") {
                paths.push(path);
            }
        }
        paths.sort();
        let mut tables = BTreeMap::new();
        for path in &paths {
            let name = path.file_stem().and_then(|s| s.to_str());
            let name = name.with_context(|| format!("{}: not a UTF-8 name", path.display()))?;
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let table = Table::parse(name, &text).with_context(|| path.display().to_string())?;
            tables.insert(name.to_string(), table);
        }
        if tables.is_empty() {
            bail!("{} has no tables", dir.display());
        }
        for table in tables.values() {
            for row in &table.rows {
                if let Some(link) = row
                    .link
                    .iter()
                    .flatten()
                    .find(|l| !tables.contains_key(&l.item))
                {
                    bail!(
                        "{}: line {}: links to {}, which has no table",
                        dir.join(format!("{}.tsv", table.name)).display(),
                        row.line,
                        link.item
                    );
                }
            }
        }
        Ok(Grammar { tables })
    }

    /// The table named `name`.
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    /// Every table, by name.
    pub fn tables(&self) -> impl Iterator<Item = &Table> {
        self.tables.values()
    }
}

/// The vendored model ([`DIR`](super::DIR)), read once. Panics, naming
/// the file and line, when it doesn't read.
pub fn grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(|| {
        Grammar::load(Path::new(super::DIR))
            .unwrap_or_else(|e| panic!("the Arlington model doesn't read: {e:#}"))
    })
}

impl Table {
    /// The table in `text`, a TSV file's whole: upstream's header, then a
    /// row a line, each line ended.
    fn parse(name: &str, text: &str) -> Result<Table> {
        let body = text
            .strip_suffix('\n')
            .context("its last line isn't ended")?;
        let mut lines = body.split('\n');
        let header = lines.next().unwrap_or_default();
        if header.ends_with('\r') {
            bail!("line 1: CRLF line ends (.gitattributes keeps the model's bytes as they are)");
        }
        if header.split('\t').ne(HEADER) {
            bail!("line 1: not upstream's header");
        }
        let mut rows = Vec::new();
        let mut keys = HashSet::new();
        for (i, text) in lines.enumerate() {
            let line = i + 2;
            let row = Row::parse(line, text).with_context(|| format!("line {line}"))?;
            if !keys.insert(row.key.clone()) {
                bail!("line {line}: a second row for {}", row.key);
            }
            rows.push(row);
        }
        if rows.is_empty() {
            bail!("no rows");
        }
        let repeats = rows
            .iter()
            .position(|r| repeating(&r.key))
            .unwrap_or(rows.len());
        for (i, row) in rows.iter().enumerate().skip(repeats) {
            if row.key != format!("{i}*") {
                bail!(
                    "line {}: repeating rows come last, numbered from the element they start at",
                    row.line
                );
            }
        }
        Ok(Table {
            name: name.to_string(),
            rows,
            repeats,
        })
    }

    /// The row for the value at `at`: the row with its key or index, else,
    /// for an array's element, the repeating row it falls on, else the
    /// wildcard `*`.
    pub fn row(&self, at: At) -> Option<&Row> {
        let named = |key: &[u8]| self.rows.iter().find(|r| r.key.as_bytes() == key);
        match at {
            At::Key(key) => named(key),
            At::Index(i) => named(i.to_string().as_bytes()).or_else(|| self.repeated(i)),
        }
        .or_else(|| named(b"*"))
    }

    /// The repeating rows, a set the array's elements after the other
    /// rows go through again and again; none when the table has none.
    pub fn repeating(&self) -> &[Row] {
        &self.rows[self.repeats..]
    }

    /// The repeating row element `i` falls on, if the table has them and
    /// they start at or before it.
    fn repeated(&self, i: usize) -> Option<&Row> {
        let set = self.rows.len() - self.repeats;
        (set > 0 && i >= self.repeats).then(|| &self.rows[self.repeats + (i - self.repeats) % set])
    }
}

/// Whether `key` is a repeating row's: a digit, then `*`.
fn repeating(key: &str) -> bool {
    key.strip_suffix('*')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

impl Row {
    /// The row on line `line`, from its text.
    fn parse(line: usize, text: &str) -> Result<Row> {
        let fields: Vec<&str> = text.split('\t').collect();
        let [
            key,
            ty,
            since,
            deprecated,
            required,
            indirect,
            inheritable,
            default,
            values,
            special,
            link,
            note,
        ] = fields[..]
        else {
            bail!("{} fields, not 12", fields.len());
        };
        for (column, cell) in [
            ("Key", key),
            ("Type", ty),
            ("SinceVersion", since),
            ("Required", required),
            ("IndirectReference", indirect),
        ] {
            if cell.is_empty() {
                bail!("no {column}");
            }
        }
        let r#type = split(ty, ';')?
            .into_iter()
            .map(|t| {
                let (wraps, name) = unwrap(t)?;
                let item = ArlType::named(name)
                    .with_context(|| format!("Type {name:?} isn't one of Arlington's"))?;
                Ok(Wrapped { wraps, item })
            })
            .collect::<Result<Vec<_>>>()?;
        let n = r#type.len();
        let inheritable = match inheritable {
            "TRUE" => true,
            "FALSE" => false,
            other => bail!("Inheritable is {other:?}, not TRUE or FALSE"),
        };
        let indirect_reference = if indirect.starts_with('[') {
            groups(indirect, n, "IndirectReference")?
                .into_iter()
                .map(str::to_string)
                .collect()
        } else {
            vec![indirect.to_string(); n]
        };
        let possible_values = groups(values, n, "PossibleValues")?;
        let special_case = groups(special, n, "SpecialCase")?;
        let link = groups(link, n, "Link")?;
        Ok(Row {
            line,
            key: key.to_string(),
            r#type,
            since_version: since.to_string(),
            deprecated_in: deprecated.to_string(),
            required: required.to_string(),
            indirect_reference,
            inheritable,
            default_value: default.to_string(),
            possible_values: possible_values
                .into_iter()
                .map(items)
                .collect::<Result<_>>()?,
            special_case: special_case.into_iter().map(str::to_string).collect(),
            link: link.into_iter().map(items).collect::<Result<_>>()?,
            note: note.to_string(),
        })
    }
}

impl ArlType {
    /// All 18, in the model's order (its names', alphabetical).
    pub const ALL: [ArlType; 18] = [
        ArlType::Array,
        ArlType::Bitmask,
        ArlType::Boolean,
        ArlType::Date,
        ArlType::Dictionary,
        ArlType::Integer,
        ArlType::Matrix,
        ArlType::Name,
        ArlType::NameTree,
        ArlType::Null,
        ArlType::Number,
        ArlType::NumberTree,
        ArlType::Rectangle,
        ArlType::Stream,
        ArlType::String,
        ArlType::StringAscii,
        ArlType::StringByte,
        ArlType::StringText,
    ];

    /// Its name in the model: `name-tree`, `string-text`.
    pub fn name(self) -> &'static str {
        match self {
            ArlType::Array => "array",
            ArlType::Bitmask => "bitmask",
            ArlType::Boolean => "boolean",
            ArlType::Date => "date",
            ArlType::Dictionary => "dictionary",
            ArlType::Integer => "integer",
            ArlType::Matrix => "matrix",
            ArlType::Name => "name",
            ArlType::NameTree => "name-tree",
            ArlType::Null => "null",
            ArlType::Number => "number",
            ArlType::NumberTree => "number-tree",
            ArlType::Rectangle => "rectangle",
            ArlType::Stream => "stream",
            ArlType::String => "string",
            ArlType::StringAscii => "string-ascii",
            ArlType::StringByte => "string-byte",
            ArlType::StringText => "string-text",
        }
    }

    /// The type the model names `name`.
    pub fn named(name: &str) -> Option<ArlType> {
        ArlType::ALL.into_iter().find(|t| t.name() == name)
    }
}

impl WrapFn {
    pub const ALL: [WrapFn; 5] = [
        WrapFn::SinceVersion,
        WrapFn::BeforeVersion,
        WrapFn::IsPDFVersion,
        WrapFn::Deprecated,
        WrapFn::Extension,
    ];

    /// Its name, less `fn:`.
    pub fn name(self) -> &'static str {
        match self {
            WrapFn::SinceVersion => "SinceVersion",
            WrapFn::BeforeVersion => "BeforeVersion",
            WrapFn::IsPDFVersion => "IsPDFVersion",
            WrapFn::Deprecated => "Deprecated",
            WrapFn::Extension => "Extension",
        }
    }
}

/// A cell that holds an item per type, `[a,b];[];[c]`, as its `n` items
/// with their brackets off (INTERNAL_GRAMMAR.md, "TSV Data Fields"); `n`
/// empty ones for an empty cell.
fn groups<'a>(cell: &'a str, n: usize, column: &str) -> Result<Vec<&'a str>> {
    if cell.is_empty() {
        return Ok(vec![""; n]);
    }
    let groups = split(cell, ';')?
        .into_iter()
        .map(|g| {
            let inner = g.strip_prefix('[').and_then(|g| g.strip_suffix(']'));
            inner
                .filter(|inner| balanced(inner))
                .with_context(|| format!("{column}: {g:?} isn't one bracketed item"))
        })
        .collect::<Result<Vec<_>>>()?;
    if groups.len() != n {
        bail!("{column} has {} items for {n} types", groups.len());
    }
    Ok(groups)
}

/// A type's list of values or links, `a,fn:SinceVersion(1.5,b)`, each
/// unwrapped; none for an empty one.
fn items(group: &str) -> Result<Vec<Wrapped<String>>> {
    if group.is_empty() {
        return Ok(Vec::new());
    }
    split(group, ',')?
        .into_iter()
        .map(|item| {
            let (wraps, item) = unwrap(item)?;
            if item.is_empty() {
                bail!("{group:?} has an empty item");
            }
            let item = item.to_string();
            Ok(Wrapped { wraps, item })
        })
        .collect()
}

/// `item` with the version predicates around it taken off, outermost
/// first; an error for one that doesn't read, a version that isn't one
/// among them.
fn unwrap(mut item: &str) -> Result<(Vec<Wrap>, &str)> {
    let mut wraps = Vec::new();
    while let Some((wrap, inner)) = wrapper(item) {
        wraps.push(wrap);
        item = inner;
    }
    let name = item.strip_prefix("fn:").and_then(|f| f.split_once('('));
    if let Some((name, _)) = name
        && WrapFn::ALL.into_iter().any(|f| f.name() == name)
    {
        bail!("{item:?}: a version predicate that doesn't read");
    }
    Ok((wraps, item))
}

/// `fn:F(arg,inner)`, when `F` is a [`WrapFn`], `arg` its version (for
/// `Extension`, its name) and the call the whole of `item`, as `F` of
/// `arg`, and `inner`.
fn wrapper(item: &str) -> Option<(Wrap, &str)> {
    let (name, args) = item.strip_prefix("fn:")?.split_once('(')?;
    let func = WrapFn::ALL.into_iter().find(|f| f.name() == name)?;
    let (arg, inner) = args.strip_suffix(')')?.split_once(',')?;
    if arg.contains(['(', ')']) || !balanced(inner) {
        return None;
    }
    if func != WrapFn::Extension && version(arg).is_none() {
        return None;
    }
    let arg = arg.to_string();
    Some((Wrap { func, arg }, inner))
}

/// A version as the model writes one: `1.0` to `1.7`, or `2.0`
/// (INTERNAL_GRAMMAR.md, "Parameters to predicates").
pub fn version(text: &str) -> Option<PdfVersion> {
    match text.as_bytes() {
        [b'1', b'.', minor @ b'0'..=b'7'] => Some(PdfVersion::new(1, minor - b'0')),
        b"2.0" => Some(PdfVersion::new(2, 0)),
        _ => None,
    }
}

/// `s` split at each `sep` outside brackets, parentheses and single-quoted
/// strings; an error when those don't pair up.
pub(super) fn split(s: &str, sep: char) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut open = Vec::new();
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            _ if quoted => {}
            '[' => open.push(']'),
            '(' => open.push(')'),
            ']' | ')' => {
                if open.pop() != Some(c) {
                    bail!("{s:?}: a {c} out of place");
                }
            }
            _ if c == sep && open.is_empty() => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if quoted || !open.is_empty() {
        bail!("{s:?}: a bracket, parenthesis or quote left open");
    }
    parts.push(&s[start..]);
    Ok(parts)
}

/// Whether the brackets, parentheses and quotes in `s` pair up (a cell
/// holds no tab, so it splits nowhere).
fn balanced(s: &str) -> bool {
    split(s, '\t').is_ok()
}
