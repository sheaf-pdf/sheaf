//! Running `pdf-lint` (`sheaf-pdf/pdf-lint`, 0.2.0+) as a standalone CLI
//! oracle over files sheaf writes: [`read`], [`check`], [`options`],
//! [`written`], [`own_structure`], and [`Allowances`].

mod hold;

pub use hold::written;

use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde::Deserialize;
use sheaf_doc::pdf::{LazyPdf, ObjectId, XrefKind};
use sheaf_doc::source::Limits;

/// Minimum `pdf-lint` version the tests accept (`0.2.0`+, including `0.2.0-rc.N`).
const MIN_MAJOR_MINOR: (u32, u32) = (0, 2);

/// PDF version (`1.0`..=`2.0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
}

impl Version {
    pub const fn new(major: u8, minor: u8) -> Version {
        Version { major, minor }
    }

    pub fn parse(text: &str) -> Option<Version> {
        let (maj, min) = text.trim().split_once('.')?;
        Some(Version {
            major: maj.parse().ok()?,
            minor: min.parse().ok()?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// How sheaf made an object (`Merged < Built`, matching `hold.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Made {
    Merged,
    Built,
}

impl Made {
    fn parse(s: &str) -> Made {
        match s {
            "built" => Made::Built,
            _ => Made::Merged,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Made::Built => "built",
            Made::Merged => "merged",
        }
    }
}

/// Which objects of a file `pdf-lint` holds to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    All(Made),
    Only(BTreeMap<ObjectId, Made>),
}

/// Byte span of a finding in the file or in a decoded stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub stream: Option<ObjectId>,
    pub start: usize,
    pub end: usize,
}

/// Validation rule category reported by `pdf-lint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    Type,
    Missing,
    TooNew,
    Deprecated,
    Value,
    Indirect,
    Direct,
    Special,
    Unreached,
    Unresolved,
    Tree,
    Unlisted,
    Duplicate,
    Reader,
    Packed,
    Balance,
    Marked,
    Operator,
    Operands,
    Resource,
    Unused,
    Content,
    PageTree,
    Viewer,
    Xmp,
    ColorSpace,
    Font,
    Linearization,
    Updated,
}

impl Rule {
    pub fn parse(name: &str) -> Rule {
        match name {
            "type" => Rule::Type,
            "missing" => Rule::Missing,
            "too-new" => Rule::TooNew,
            "deprecated" => Rule::Deprecated,
            "value" => Rule::Value,
            "indirect" => Rule::Indirect,
            "direct" => Rule::Direct,
            "special" => Rule::Special,
            "unreached" => Rule::Unreached,
            "unresolved" => Rule::Unresolved,
            "tree" => Rule::Tree,
            "unlisted" => Rule::Unlisted,
            "duplicate" => Rule::Duplicate,
            "reader" => Rule::Reader,
            "packed" => Rule::Packed,
            "balance" => Rule::Balance,
            "marked" => Rule::Marked,
            "operator" => Rule::Operator,
            "operands" => Rule::Operands,
            "resource" => Rule::Resource,
            "unused" => Rule::Unused,
            "content" => Rule::Content,
            "page-tree" => Rule::PageTree,
            "viewer" => Rule::Viewer,
            "xmp" => Rule::Xmp,
            "color-space" => Rule::ColorSpace,
            "font" => Rule::Font,
            "linearization" => Rule::Linearization,
            "updated" => Rule::Updated,
            other => panic!("unknown pdf-lint rule {other:?}"),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Rule::Type => "type",
            Rule::Missing => "missing",
            Rule::TooNew => "too-new",
            Rule::Deprecated => "deprecated",
            Rule::Value => "value",
            Rule::Indirect => "indirect",
            Rule::Direct => "direct",
            Rule::Special => "special",
            Rule::Unreached => "unreached",
            Rule::Unresolved => "unresolved",
            Rule::Tree => "tree",
            Rule::Unlisted => "unlisted",
            Rule::Duplicate => "duplicate",
            Rule::Reader => "reader",
            Rule::Packed => "packed",
            Rule::Balance => "balance",
            Rule::Marked => "marked",
            Rule::Operator => "operator",
            Rule::Operands => "operands",
            Rule::Resource => "resource",
            Rule::Unused => "unused",
            Rule::Content => "content",
            Rule::PageTree => "page-tree",
            Rule::Viewer => "viewer",
            Rule::Xmp => "xmp",
            Rule::ColorSpace => "color-space",
            Rule::Font => "font",
            Rule::Linearization => "linearization",
            Rule::Updated => "updated",
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One finding from `pdf-lint --format json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub object: Option<ObjectId>,
    pub made: Made,
    pub path: String,
    pub table: String,
    pub key: String,
    pub rule: Rule,
    pub value: String,
    pub at: Option<Span>,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}/{} at {}",
            self.rule, self.table, self.key, self.path
        )?;
        if let Some((n, g)) = self.object {
            write!(f, ", in {n} {g} R")?;
        }
        write!(f, " ({}): {}", self.made.word(), self.value)?;
        if let Some(at) = self.at {
            match at.stream {
                None => write!(f, " [bytes {}..{}]", at.start, at.end)?,
                Some((n, g)) => write!(f, " [bytes {}..{} of {n} {g} R's data]", at.start, at.end)?,
            }
        }
        Ok(())
    }
}

/// Check not made by `pdf-lint`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unchecked {
    pub column: String,
    pub why: String,
    pub table: String,
    pub key: String,
}

/// Unlisted key recorded by `pdf-lint`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unlisted {
    pub table: String,
    pub key: String,
    pub class: String,
}

/// CLI options passed to `pdf-lint`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub version: Option<Version>,
    pub deprecations: bool,
}

/// Parsed JSON report for one PDF file.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub version: Version,
    pub trailer: Option<ObjectId>,
    pub findings: Vec<Finding>,
    pub checked: BTreeMap<String, usize>,
    pub keys: usize,
    pub unchecked: BTreeMap<Unchecked, usize>,
    pub unlisted: BTreeMap<Unlisted, usize>,
    pub deprecated: BTreeMap<(String, String), usize>,
    pub ignored: BTreeMap<(String, String), usize>,
    pub no_table: usize,
    pub ties: usize,
    pub truncated: usize,
    pub two_contexts: usize,
    pub two_tables: usize,
}

impl Report {
    /// Findings in `self` that sheaf answers for: every finding in a `Built`
    /// object, plus findings in `Merged` objects or the trailer not already
    /// present in any of `sources`.
    pub fn beyond<'a>(&'a self, sources: &[&Report]) -> Vec<&'a Finding> {
        let carried: HashSet<_> = (sources.iter())
            .flat_map(|s| &s.findings)
            .map(finding_key)
            .collect();
        let in_trailer = |r: &Report, f: &Finding| {
            f.path.starts_with("trailer") || f.object.is_none() || f.object == r.trailer
        };
        let trailer_too_new = (sources.iter()).any(|s| {
            s.findings
                .iter()
                .any(|f| f.rule == Rule::TooNew && in_trailer(s, f))
        });
        (self.findings.iter())
            .filter(|f| {
                f.made == Made::Built
                    || !(carried.contains(&finding_key(f))
                        || (trailer_too_new && f.rule == Rule::TooNew && in_trailer(self, f)))
            })
            .collect()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let objects = self
            .checked
            .values()
            .sum::<usize>()
            .saturating_sub(self.two_tables);
        write!(
            f,
            "PDF {}, {objects} objects checked, {} keys, {} findings",
            self.version,
            self.keys,
            self.findings.len()
        )?;
        for finding in &self.findings {
            write!(f, "\n- {finding}")?;
        }
        Ok(())
    }
}

/// Normalized signature of a finding for source-carrying comparison, ignoring
/// renumbered indirect references (`12 0 R` -> `R`), offsets, and repeat counts.
fn finding_key(f: &Finding) -> (String, String, Rule, String) {
    let val = match &f.rule {
        Rule::TooNew => String::new(),
        Rule::Reader => f.value.chars().filter(|c| !c.is_ascii_digit()).collect(),
        Rule::PageTree
        | Rule::Viewer
        | Rule::Xmp
        | Rule::ColorSpace
        | Rule::Font
        | Rule::Linearization
        | Rule::Updated
        | Rule::Balance
        | Rule::Marked
        | Rule::Content => strip_digits_and_times(&f.value, false),
        Rule::Operator | Rule::Operands => strip_digits_and_times(&f.value, true),
        Rule::Resource if f.key == "-" => strip_digits_and_times(&f.value, false),
        Rule::Tree => strip_refs(strip_times(&f.value)),
        _ => strip_refs(&f.value),
    };
    match f.rule {
        Rule::Unreached | Rule::Updated => ("-".into(), "-".into(), f.rule, val),
        _ => (f.table.clone(), f.key.clone(), f.rule, val),
    }
}

fn strip_times(s: &str) -> &str {
    if let Some((head, tail)) = s.rsplit_once(", ")
        && let Some(n) = tail.strip_suffix(" times")
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        return head;
    }
    s
}

fn strip_digits_and_times(s: &str, keep_word_digits: bool) -> String {
    let mut in_word = false;
    let out: String = (s.chars())
        .filter(|&c| {
            let keep = !c.is_ascii_digit() || (keep_word_digits && in_word);
            in_word = c.is_alphabetic() || (in_word && c.is_ascii_digit());
            keep
        })
        .collect();
    out.strip_suffix(",  times").unwrap_or(&out).to_string()
}

fn strip_refs(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let (mut i, mut kept) = (0, 0);
    while i < b.len() {
        let boundary = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        if boundary && b[i].is_ascii_digit() {
            let n1 = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
            if b.get(i + n1) == Some(&b' ') {
                let g_start = i + n1 + 1;
                let n2 = b[g_start..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .count();
                let r = g_start + n2;
                if n2 > 0
                    && b.get(r..r + 2) == Some(b" R")
                    && b.get(r + 2).is_none_or(|c| !c.is_ascii_alphanumeric())
                {
                    out.push_str(&s[kept..i]);
                    out.push('R');
                    i = r + 2;
                    kept = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push_str(&s[kept..]);
    out
}

/// Input PDF prepared for `pdf-lint`.
pub struct InputFile<'a> {
    what: String,
    bytes: &'a [u8],
    password: Option<&'a str>,
    version: Version,
    trailer: Option<ObjectId>,
}

/// Prepares `bytes` (written by `what`, with optional `password`) for `pdf-lint`.
pub fn read<'a>(what: &str, bytes: &'a [u8], password: Option<&'a str>) -> InputFile<'a> {
    let (version, trailer) = inspect_pdf(bytes, password);
    InputFile {
        what: what.to_string(),
        bytes,
        password,
        version,
        trailer,
    }
}

pub fn file_version(file: &InputFile<'_>) -> Version {
    file.version
}

fn inspect_pdf(bytes: &[u8], password: Option<&str>) -> (Version, Option<ObjectId>) {
    let parsed_header = bytes
        .windows(5)
        .take(1024)
        .position(|w| w == b"%PDF-")
        .and_then(|at| std::str::from_utf8(bytes.get(at + 5..at + 8)?).ok())
        .and_then(Version::parse)
        .unwrap_or(Version::new(1, 4));
    let Ok(pdf) = LazyPdf::open(Arc::<[u8]>::from(bytes), password, &Limits::DEFAULT.reader) else {
        return (parsed_header, None);
    };
    let header = Version::parse(pdf.version()).unwrap_or(parsed_header);
    let cat_ver = pdf
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Version").ok())
        .and_then(|v| v.as_name().ok())
        .and_then(|n| std::str::from_utf8(n).ok())
        .and_then(Version::parse);
    let version = header.max(cat_ver.unwrap_or(header));
    let f = pdf.xref();
    let trailer = if f.kind == XrefKind::Stream && pdf.base_sound() {
        bytes.get(f.header_offset + f.start..).and_then(header_id)
    } else {
        None
    };
    (version, trailer)
}

fn header_id(bytes: &[u8]) -> Option<ObjectId> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
    let mut words = head.split_ascii_whitespace();
    let n = words.next()?.parse().ok()?;
    let g = words.next()?.parse().ok()?;
    words.next()?.starts_with("obj").then_some((n, g))
}

/// Default `pdf-lint` options for sheaf's Arlington tests.
pub fn options() -> Options {
    Options {
        version: None,
        deprecations: true,
    }
}

/// Runs the `pdf-lint` binary on `file` and parses its JSON report.
pub fn check(file: &InputFile<'_>, hold: &Hold, options: &Options) -> Report {
    if let Hold::Only(map) = hold
        && map.is_empty()
    {
        return Report {
            version: options.version.unwrap_or(file.version),
            trailer: file.trailer,
            ..Report::default()
        };
    }
    let bin = pdf_lint_bin();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let base = dir();
    let pdf_path = base.join(format!("{id}.pdf"));
    std::fs::write(&pdf_path, file.bytes).unwrap();

    let mut hold_path = None;
    let mut pw_path = None;
    let mut cmd = Command::new(bin);
    cmd.arg("--format").arg("json").arg("--jobs").arg("1");
    if options.deprecations {
        cmd.arg("--deprecations");
    }
    if let Some(v) = options.version {
        cmd.arg("--pdf-version").arg(v.to_string());
    }
    match hold {
        Hold::All(Made::Built) => {
            cmd.arg("--built");
        }
        Hold::All(Made::Merged) => {}
        Hold::Only(map) => {
            let mut text = String::new();
            for (&(n, g), &made) in map {
                let _ = writeln!(text, "{n} {g} {}", made.word());
            }
            let hp = base.join(format!("{id}.hold"));
            std::fs::write(&hp, text).unwrap();
            cmd.arg("--hold").arg(&hp);
            hold_path = Some(hp);
        }
    }
    if let Some(pw) = file.password {
        let pp = base.join(format!("{id}.pw"));
        std::fs::write(&pp, pw).unwrap();
        cmd.arg("--password-file").arg(&pp);
        pw_path = Some(pp);
    }
    cmd.arg(&pdf_path);

    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("running {}: {e}", bin.display()));
    let _ = std::fs::remove_file(&pdf_path);
    if let Some(hp) = hold_path {
        let _ = std::fs::remove_file(hp);
    }
    if let Some(pp) = pw_path {
        let _ = std::fs::remove_file(pp);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stdout.lines().next().unwrap_or_else(|| {
        panic!(
            "pdf-lint wrote no JSON for {} (status {}): {stderr}",
            file.what, output.status
        )
    });
    let parsed: JsonLine = serde_json::from_str(line).unwrap_or_else(|e| {
        panic!(
            "parsing pdf-lint JSON for {}: {e}\nline: {line}\nstderr: {stderr}",
            file.what
        )
    });
    if let Some(err) = parsed.error {
        panic!("pdf-lint refused {}: {err}", file.what);
    }
    parsed.into_report(file.trailer, file.version)
}

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

/// Resolves and verifies the `pdf-lint` binary once per test binary.
fn pdf_lint_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let mut candidates = Vec::new();
        if let Some(explicit) = std::env::var_os("SHEAF_PDF_LINT") {
            candidates.push(PathBuf::from(explicit));
        } else {
            let tools = std::env::var_os("SHEAF_TOOLS")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .map(|h| PathBuf::from(h).join(".local/share/sheaf-tools"))
                });
            if let Some(tools) = tools {
                let exe = if cfg!(windows) {
                    "pdf-lint.exe"
                } else {
                    "pdf-lint"
                };
                let installed = tools.join("pdf-lint").join(exe);
                if installed.is_file() {
                    candidates.push(installed);
                }
            }
            candidates.push(PathBuf::from(if cfg!(windows) {
                "pdf-lint.exe"
            } else {
                "pdf-lint"
            }));
        }

        let mut why = String::new();
        for cand in candidates {
            match Command::new(&cand).arg("--version").output() {
                Ok(out) if out.status.success() => {
                    let first = String::from_utf8_lossy(&out.stdout);
                    let line = first.lines().next().unwrap_or("");
                    if let Some(ver) = line.strip_prefix("pdf-lint ")
                        && version_at_least(ver, MIN_MAJOR_MINOR)
                    {
                        return cand;
                    }
                    why = format!("{} reported {line:?}, older than 0.2.0", cand.display());
                }
                Ok(out) => {
                    why = format!("{} exited with {}", cand.display(), out.status);
                }
                Err(e) => {
                    why = format!("{}: {e}", cand.display());
                }
            }
        }
        panic!(
            "the Arlington tests require pdf-lint 0.2.0 or newer ({why}): \
             run `just setup-pdf-lint` or set SHEAF_PDF_LINT"
        );
    })
}

fn version_at_least(ver: &str, min: (u32, u32)) -> bool {
    let core = ver.split('-').next().unwrap_or(ver);
    let mut parts = core.split('.');
    let Some(maj) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
        return false;
    };
    let Some(mn) = parts.next().and_then(|s| s.parse::<u32>().ok()) else {
        return false;
    };
    (maj, mn) >= min
}

fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| crate::common::scratch("pdf-lint"))
}

#[derive(Deserialize)]
struct JsonLine {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    version: String,
    #[serde(default)]
    keys: usize,
    #[serde(default)]
    findings: Vec<JsonFinding>,
    #[serde(default)]
    tables: BTreeMap<String, usize>,
    #[serde(default)]
    not_checked: Vec<JsonNotChecked>,
    #[serde(default)]
    unlisted: Vec<JsonUnlisted>,
    #[serde(default)]
    deprecated: Vec<JsonCount>,
    #[serde(default)]
    ignored: Vec<JsonCount>,
    #[serde(default)]
    no_table: usize,
    #[serde(default)]
    ties: usize,
    #[serde(default)]
    truncated: usize,
    #[serde(default)]
    two_contexts: usize,
    #[serde(default)]
    two_tables: usize,
}

#[derive(Deserialize)]
struct JsonFinding {
    rule: String,
    table: String,
    key: String,
    path: String,
    object: Option<(u32, u16)>,
    made: String,
    value: String,
    at: Option<JsonAt>,
}

#[derive(Deserialize)]
struct JsonAt {
    stream: Option<(u32, u16)>,
    start: usize,
    end: usize,
}

#[derive(Deserialize)]
struct JsonNotChecked {
    column: String,
    why: String,
    table: String,
    key: String,
    count: usize,
}

#[derive(Deserialize)]
struct JsonUnlisted {
    table: String,
    key: String,
    class: String,
    #[serde(default)]
    extension: Option<String>,
    count: usize,
}

#[derive(Deserialize)]
struct JsonCount {
    table: String,
    key: String,
    count: usize,
}

impl JsonLine {
    fn into_report(self, trailer: Option<ObjectId>, fallback_ver: Version) -> Report {
        let version = Version::parse(&self.version).unwrap_or(fallback_ver);
        let findings = self
            .findings
            .into_iter()
            .map(|f| Finding {
                object: f.object,
                made: Made::parse(&f.made),
                path: f.path,
                table: f.table,
                key: f.key,
                rule: Rule::parse(&f.rule),
                value: f.value,
                at: f.at.map(|a| Span {
                    stream: a.stream,
                    start: a.start,
                    end: a.end,
                }),
            })
            .collect();
        let unchecked = self
            .not_checked
            .into_iter()
            .map(|n| {
                (
                    Unchecked {
                        column: n.column,
                        why: n.why,
                        table: n.table,
                        key: n.key,
                    },
                    n.count,
                )
            })
            .collect();
        let unlisted = self
            .unlisted
            .into_iter()
            .map(|u| {
                let class = match u.extension {
                    Some(ext) => format!("extension {ext}"),
                    None => u.class,
                };
                (
                    Unlisted {
                        table: u.table,
                        key: u.key,
                        class,
                    },
                    u.count,
                )
            })
            .collect();
        let deprecated = self
            .deprecated
            .into_iter()
            .map(|c| ((c.table, c.key), c.count))
            .collect();
        let ignored = self
            .ignored
            .into_iter()
            .map(|c| ((c.table, c.key), c.count))
            .collect();
        Report {
            version,
            trailer,
            findings,
            checked: self.tables,
            keys: self.keys,
            unchecked,
            unlisted,
            deprecated,
            ignored,
            no_table: self.no_table,
            ties: self.ties,
            truncated: self.truncated,
            two_contexts: self.two_contexts,
            two_tables: self.two_tables,
        }
    }
}

/// Six-column TSV allowances table (`table`, `key`, `rule`, `file`, `version`, `reason`).
pub struct Allowances {
    rows: Vec<Allowance>,
    used: Vec<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct Allowance {
    pub line: usize,
    pub table: String,
    pub key: String,
    pub rule: Rule,
    pub file: Option<String>,
    pub from: Option<Version>,
    pub reason: String,
}

impl Allowances {
    pub fn read(path: &Path) -> Result<Allowances, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut rows = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let line = idx + 1;
            if raw.starts_with('#') {
                continue;
            }
            let cols: Vec<&str> = raw.split('\t').collect();
            let [table, key, rule, file, ver, reason] = cols[..] else {
                return Err(format!(
                    "{}:{line}: expected 6 tab-separated columns, got {}",
                    path.display(),
                    cols.len()
                ));
            };
            let from =
                if ver == "*" {
                    None
                } else {
                    Some(Version::parse(ver).ok_or_else(|| {
                        format!("{}:{line}: invalid version {ver:?}", path.display())
                    })?)
                };
            rows.push(Allowance {
                line,
                table: table.to_string(),
                key: key.to_string(),
                rule: Rule::parse(rule),
                file: (file != "*").then(|| file.to_string()),
                from,
                reason: reason.to_string(),
            });
        }
        let used = rows.iter().map(|_| AtomicBool::new(false)).collect();
        Ok(Allowances { rows, used })
    }

    pub fn rows(&self) -> impl Iterator<Item = &Allowance> {
        self.rows.iter()
    }

    pub fn allows(
        &self,
        finding: &Finding,
        version: Version,
        file: Option<&str>,
    ) -> Option<&Allowance> {
        let pos = self.rows.iter().position(|a| {
            a.table == finding.table
                && a.key == finding.key
                && a.rule == finding.rule
                && a.file.as_deref().is_none_or(|af| Some(af) == file)
                && a.from.is_none_or(|v| version >= v)
        })?;
        self.used[pos].store(true, Ordering::Relaxed);
        Some(&self.rows[pos])
    }

    pub fn unused(&self) -> Vec<&Allowance> {
        self.rows
            .iter()
            .zip(&self.used)
            .filter_map(|(a, u)| (!u.load(Ordering::Relaxed)).then_some(a))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sheaf_doc::forms::fixture::Form;
    use sheaf_doc::materialize::save_rewritten;
    use sheaf_doc::pdf::{Document as Pdf, LazyPdf, Object, dictionary};
    use sheaf_doc::serialize::fixture::object_streams_of;
    use sheaf_doc::source::Limits;

    use super::{Hold, Made, Options, Report, Rule, check, options, read, written};
    use crate::common::{self, scratch};

    #[test]
    fn what_the_version_deprecates_is_a_finding() {
        let mut pdf = Pdf::with_version("2.0");
        let pages = pdf.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => Vec::<Object>::new(),
            "Count" => 0,
        });
        let catalog = pdf.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages,
            "NeedsRendering" => false,
        });
        pdf.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();

        let hold = Hold::Only([(catalog, Made::Built)].into());
        let file = read("2.0 catalog", &bytes, None);
        let report = check(&file, &hold, &options());
        type Deprecated<'a> = (Option<super::ObjectId>, &'a str, &'a str);
        fn deprecated(r: &Report) -> Vec<Deprecated<'_>> {
            r.findings
                .iter()
                .filter(|f| f.rule == Rule::Deprecated)
                .map(|f| (f.object, f.table.as_str(), f.key.as_str()))
                .collect()
        }
        assert_eq!(
            deprecated(&report),
            [(Some(catalog), "Catalog", "NeedsRendering")],
            "{report}"
        );
        let counted = check(&file, &hold, &Options::default());
        assert_eq!(deprecated(&counted), [], "{counted}");
        let at = ("Catalog".to_string(), "NeedsRendering".to_string());
        assert_eq!(counted.deprecated.get(&at), Some(&1), "{counted}");
    }

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
