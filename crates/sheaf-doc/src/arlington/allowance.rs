//! Findings held for a backlog entry: a checked-in TSV file, a `#` header
//! saying what it holds, then a row for each finding allowed, its table,
//! key, rule, the file it's in (a corpus path, or `*` for any), the PDF
//! version from which on it's allowed (`2.0`, or `*` for any), the backlog
//! entry (`B12`, `F3`) and why. A finding sheaf is answerable for that no
//! row allows fails its test, and a row that allows nothing fails the test
//! that owns it: each is known, and none is skipped in silence.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail, ensure};

use super::check::{Finding, Rule};
use super::grammar::grammar;
use crate::materialize::emit::PdfVersion;

/// The findings a file allows, each as many times as it comes.
#[derive(Debug)]
pub struct Allowances {
    list: Vec<Allowance>,
    /// Whether each of `list` has allowed a finding.
    used: Vec<AtomicBool>,
}

/// A row of an allowances file: the findings of its table, key and rule,
/// in one file or any, of a version and later or any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowance {
    /// The line of its file it's on.
    pub line: usize,
    /// A table of the model's, or `-`, as an unreached object's finding
    /// has it.
    pub table: String,
    pub key: String,
    pub rule: Rule,
    /// The corpus file whose findings alone it allows (its path, as
    /// `forms/manifest.tsv` has it); none for any file's.
    pub file: Option<String>,
    /// The version of the files whose findings alone it allows, and the
    /// later ones', as the checked file's version ([`file_version`]) is
    /// told; none for any version's.
    ///
    /// [`file_version`]: super::file_version
    pub from: Option<PdfVersion>,
    /// The backlog entry it's held for: `B` or `F` and its number.
    pub entry: String,
    pub reason: String,
}

impl Allowances {
    /// The allowances in the file `path`. An error names the line that
    /// doesn't read: a row without its seven fields, a table the model
    /// hasn't, a rule there isn't, no file, a version that isn't one, an
    /// entry that isn't a backlog entry's number, no reason, or a second
    /// row for a table, key, rule, file and version.
    pub fn read(path: &Path) -> Result<Allowances> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Allowances::parse(&text).with_context(|| path.display().to_string())
    }

    fn parse(text: &str) -> Result<Allowances> {
        let header = (text.lines())
            .take_while(|line| line.starts_with('#'))
            .count();
        ensure!(header > 0, "line 1: no # header");
        let mut list: Vec<Allowance> = Vec::new();
        for (i, text) in text.lines().enumerate().skip(header) {
            let line = i + 1;
            ensure!(
                !text.starts_with('#'),
                "line {line}: a # line among the rows"
            );
            let allowance = Allowance::parse(line, text).with_context(|| format!("line {line}"))?;
            if let Some(first) = list.iter().find(|a| a.covers(&allowance)) {
                bail!(
                    "line {line}: {}/{} {} in {} from {} is on line {} already",
                    allowance.table,
                    allowance.key,
                    allowance.rule,
                    allowance.file.as_deref().unwrap_or("*"),
                    allowance.from.map_or("*".to_string(), |v| v.to_string()),
                    first.line
                );
            }
            list.push(allowance);
        }
        let used = list.iter().map(|_| AtomicBool::new(false)).collect();
        Ok(Allowances { list, used })
    }

    /// The allowance for `finding`, in a file of `version`, the corpus
    /// file `file` (none for a file a test made), if there's one; it's
    /// then used.
    pub fn allows(
        &self,
        finding: &Finding,
        version: PdfVersion,
        file: Option<&str>,
    ) -> Option<&Allowance> {
        let at = (self.list.iter()).position(|a| {
            a.table == finding.table
                && a.key == finding.key
                && a.rule == finding.rule
                && a.file
                    .as_ref()
                    .is_none_or(|only| Some(only.as_str()) == file)
                && a.from.is_none_or(|from| version >= from)
        })?;
        self.used[at].store(true, Ordering::Relaxed);
        Some(&self.list[at])
    }

    /// Every allowance, in the file's order.
    pub fn rows(&self) -> impl Iterator<Item = &Allowance> {
        self.list.iter()
    }

    /// The allowances that haven't allowed a finding.
    pub fn unused(&self) -> Vec<&Allowance> {
        (self.list.iter().zip(&self.used))
            .filter(|(_, used)| !used.load(Ordering::Relaxed))
            .map(|(a, _)| a)
            .collect()
    }
}

impl Allowance {
    fn parse(line: usize, text: &str) -> Result<Allowance> {
        let fields: Vec<&str> = text.split('\t').collect();
        let [table, key, rule, file, from, entry, reason] = fields[..] else {
            bail!(
                "{} fields, not 7: table, key, rule, file, version, entry, reason",
                fields.len()
            );
        };
        ensure!(
            table == "-" || grammar().table(table).is_some(),
            "{table:?} isn't a table of the model's"
        );
        ensure!(!key.is_empty(), "no key");
        let rule = Rule::named(rule).with_context(|| format!("{rule:?} isn't a rule"))?;
        ensure!(!file.is_empty(), "no file (`*` for any)");
        let from = match from {
            "*" => None,
            from => Some(
                PdfVersion::parse(from.as_bytes())
                    .with_context(|| format!("{from:?} isn't a version (`*` for any)"))?,
            ),
        };
        let number = entry.strip_prefix(['B', 'F']).unwrap_or_default();
        ensure!(
            !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()),
            "the entry {entry:?} isn't a backlog entry (B or F and its number)"
        );
        ensure!(!reason.trim().is_empty(), "no reason");
        Ok(Allowance {
            line,
            table: table.to_string(),
            key: key.to_string(),
            rule,
            file: (file != "*").then(|| file.to_string()),
            from,
            entry: entry.to_string(),
            reason: reason.to_string(),
        })
    }

    /// Whether it allows what `other` does.
    fn covers(&self, other: &Allowance) -> bool {
        let own = (&self.table, &self.key, self.rule, &self.file, self.from);
        own == (
            &other.table,
            &other.key,
            other.rule,
            &other.file,
            other.from,
        )
    }
}
