//! Greg's corpus of real files (M10 proposal 21): where it is, and the
//! manifests of the categories the tests read, a row per file:
//! `forms/manifest.tsv` for the forms under `forms/`,
//! `scans/manifest.tsv` for the scanned documents under `scans/`, and
//! `signatures/manifest.tsv` for the signed files under `signatures/`.
//! Each of the corpus's categories has a manifest of its own, in its
//! folder.
//!
//! Tests look in `$SHEAF_CORPUS`, then in `../pdf-corpus` beside the
//! workspace, and skip, passing with a message, when neither is there
//! (CI has no corpus). The corpus's files aren't published:
//! [fetch-pdf-corpus] rebuilds it, byte for byte, from where each was.
//! A folder that is there without `forms/manifest.tsv` fails them
//! instead: a corpus laid out before the manifests moved into their
//! categories, or a wrong `$SHEAF_CORPUS`, would otherwise pass every
//! corpus test by running none. Nothing is assumed about what it holds.
//! Files marked `restricted` are read in place: no copy, excerpt or
//! rendering of them is committed.
//!
//! [fetch-pdf-corpus]: https://github.com/sheaf-pdf/fetch-pdf-corpus

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};

pub struct Corpus {
    pub root: PathBuf,
}

/// The forms manifest, relative to the corpus root.
const FORMS: &str = "forms/manifest.tsv";

/// The scans manifest, relative to the corpus root.
const SCANS: &str = "scans/manifest.tsv";

/// The signed files' manifest, relative to the corpus root.
const SIGNATURES: &str = "signatures/manifest.tsv";

/// One row of a manifest (`forms/`, `scans/` or `signatures/
/// manifest.tsv`). Columns it lacks are left empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    /// Relative to the corpus root (`forms/irs/f1040.pdf`).
    pub path: String,
    pub form: String,
    pub title: String,
    pub pages: u32,
    /// A form's `acroform`, `hybrid`, `xfa-only`, or `unparsed` (pypdf
    /// couldn't read it); a scan's `scan` (half its pages or more covered
    /// by an image), `scan-ocr` (with invisible text: another program's
    /// recognized text), `scan-text` (with visible text), or `text` or
    /// `no-text` (only looked like a scan).
    pub kind: String,
    /// Terminal AcroForm fields.
    pub fields: u32,
    /// Fields by type: `text`, `check`, `radio`, `choice`, `push`, `sig`.
    pub field_types: BTreeMap<String, u32>,
    /// Fields with a script, by event: `calculate`, `format`,
    /// `keystroke`, `validate`.
    pub field_scripts: BTreeMap<String, u32>,
    pub source: String,
    /// `public-domain`, `restricted`, `unstated` or `unknown`.
    pub license: String,
    pub sha256: String,
    /// The columns the fields above don't name, by their header's name:
    /// the signatures manifest's `signatures`, `subfilters`, `certified`
    /// (DocMDP's `/P`, empty when it certifies nothing), `intact` and
    /// `valid` (how many signatures pyHanko found so), among them.
    pub extra: BTreeMap<String, String>,
}

impl Entry {
    pub fn restricted(&self) -> bool {
        self.license == "restricted"
    }

    /// Column `name` of [`extra`](Self::extra), empty when it has none.
    pub fn column(&self, name: &str) -> &str {
        self.extra.get(name).map_or("", String::as_str)
    }
}

impl Corpus {
    /// `$SHEAF_CORPUS`, else `../pdf-corpus` beside the workspace: the
    /// first that's there. Panics when it has no `forms/manifest.tsv`.
    pub fn find() -> Option<Corpus> {
        let beside = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../pdf-corpus");
        let env = std::env::var_os("SHEAF_CORPUS").map(PathBuf::from);
        Corpus::first_of(env.into_iter().chain([beside])).unwrap_or_else(|e| panic!("{e:#}"))
    }

    /// [`Corpus::find`], or a line saying the test skips.
    pub fn or_skip() -> Option<Corpus> {
        Corpus::find().or_else(|| {
            eprintln!(
                "skipped: no corpus (fetch-pdf-corpus fetch --out ../pdf-corpus, or set \
                 SHEAF_CORPUS; README.md's Building says how)"
            );
            None
        })
    }

    /// The first of `roots` that's a folder, if any, and an error when
    /// it has no `forms/manifest.tsv`.
    pub(crate) fn first_of(roots: impl IntoIterator<Item = PathBuf>) -> Result<Option<Corpus>> {
        let Some(root) = roots.into_iter().find(|root| root.is_dir()) else {
            return Ok(None);
        };
        if !root.join(FORMS).is_file() {
            anyhow::bail!(
                "{} has no {FORMS}: not a corpus, or one from before the manifests moved \
                 into their categories (fetch it again with fetch-pdf-corpus, or fix \
                 SHEAF_CORPUS)",
                root.display()
            );
        }
        Ok(Some(Corpus { root }))
    }

    /// `forms/manifest.tsv`'s rows: every file under `forms/`.
    pub fn forms(&self) -> Result<Vec<Entry>> {
        self.rows(FORMS)
    }

    /// `scans/manifest.tsv`'s rows: every file under `scans/`. An error
    /// when the corpus has none, as one from before its scans came does.
    pub fn scans(&self) -> Result<Vec<Entry>> {
        self.rows(SCANS)
    }

    /// `signatures/manifest.tsv`'s rows: every file under `signatures/`.
    /// An error when the corpus has none, as one from before its signed
    /// files came does.
    pub fn signatures(&self) -> Result<Vec<Entry>> {
        self.rows(SIGNATURES)
    }

    /// The rows of the manifest at `manifest`, relative to the root.
    fn rows(&self, manifest: &str) -> Result<Vec<Entry>> {
        let path = self.root.join(manifest);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        parse_manifest(&text).with_context(|| path.display().to_string())
    }

    /// The row for `path` (`forms/irs/f1040es.pdf`), if `forms/` has it.
    pub fn entry(&self, path: &str) -> Result<Option<Entry>> {
        Ok(self.forms()?.into_iter().find(|e| e.path == path))
    }

    /// Where an entry's file is.
    pub fn path(&self, entry: &Entry) -> PathBuf {
        self.root.join(&entry.path)
    }
}

/// `read` of each of `entries`, in their order, on a thread per core (16
/// at most).
pub fn each_entry<T: Send>(entries: &[Entry], read: impl Fn(&Entry) -> T + Sync) -> Vec<T> {
    let outcomes = Mutex::new(BTreeMap::new());
    let queue = Mutex::new(entries.iter().enumerate());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(16));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some((i, entry)) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let outcome = read(entry);
                    outcomes.lock().unwrap().insert(i, outcome);
                }
            });
        }
    });
    outcomes.into_inner().unwrap().into_values().collect()
}

/// The columns [`Entry`]'s fields hold.
const NAMED: [&str; 11] = [
    "path",
    "form",
    "title",
    "pages",
    "kind",
    "fields",
    "field_types",
    "field_scripts",
    "source",
    "license",
    "sha256",
];

/// A manifest's rows, by its header's column names.
pub fn parse_manifest(text: &str) -> Result<Vec<Entry>> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<&str> = lines.next().context("no header")?.split('\t').collect();
    if !header.contains(&"path") {
        anyhow::bail!("no path column");
    }
    let mut out = Vec::new();
    for (n, line) in lines.enumerate() {
        let cells: BTreeMap<&str, &str> = header.iter().copied().zip(line.split('\t')).collect();
        let get = |key: &str| cells.get(key).copied().unwrap_or("").trim().to_string();
        let number = |key: &str| -> Result<u32> {
            let v = get(key);
            if v.is_empty() || v == "-" {
                return Ok(0);
            }
            v.parse()
                .with_context(|| format!("row {}: {key} {v:?}", n + 2))
        };
        out.push(Entry {
            path: get("path"),
            form: get("form"),
            title: get("title"),
            pages: number("pages")?,
            kind: get("kind"),
            fields: number("fields")?,
            field_types: counts(&get("field_types"))
                .with_context(|| format!("row {}: field_types", n + 2))?,
            field_scripts: counts(&get("field_scripts"))
                .with_context(|| format!("row {}: field_scripts", n + 2))?,
            source: get("source"),
            license: get("license"),
            sha256: get("sha256"),
            extra: (cells.iter())
                .filter(|(key, _)| !NAMED.contains(key))
                .map(|(key, value)| (key.to_string(), value.trim().to_string()))
                .collect(),
        });
    }
    Ok(out)
}

/// `check:82,push:10,text:66` as counts; `-` or nothing as none.
fn counts(cell: &str) -> Result<BTreeMap<String, u32>> {
    let mut out = BTreeMap::new();
    if cell.is_empty() || cell == "-" {
        return Ok(out);
    }
    for item in cell.split(',') {
        let (key, n) = item.split_once(':').context(item.to_string())?;
        out.insert(key.trim().to_string(), n.trim().parse()?);
    }
    Ok(out)
}
