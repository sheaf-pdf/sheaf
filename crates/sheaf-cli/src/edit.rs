//! `sheaf edit`: ops read from a file (M10.5 §14.2), applied to a PDF as
//! one version and saved where the flags say. The ops are an Op JSON
//! envelope, `{"v": "1.0", "label": …, "ops": […]}` (`--schema` prints its
//! schema), and name things as a person sees them: pages by position,
//! bookmarks by their titles, annotations by `/NM`, files by the names
//! `--with` gives them. What it made, it names the same way.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Args;
use serde_json::{Value, json};
use sheaf_doc::forms::sigs::Verdict;
use sheaf_doc::materialize::StrategyKind;
use sheaf_edit::Envelope;
use sheaf_ops::Progress;
use sheaf_ops::session::{Decided, Previewed, SaveTo, Session};

use crate::decide::Decisions;
use crate::names::Names;

#[derive(Args)]
#[command(
    about = "Apply edits from an Op JSON file to a PDF",
    long_about = "Apply edits from an Op JSON file to a PDF, as one version, and save it \
                  where -o or --in-place says. The edits name pages by position \
                  ({\"pos\": 3}, 1-based), bookmarks by their titles ({\"path\": [\"Part I\", \
                  \"Ch. 2\"]}), annotations by /NM ({\"nm\": \"…\"}) and files by the names \
                  --with gives them ({\"with\": \"logo\"}); never by id. It prints what the \
                  edits did as JSON: {version, created, lines, strategy}."
)]
pub struct EditArgs {
    /// The PDF to edit.
    #[arg(required_unless_present = "schema")]
    input: Option<PathBuf>,
    /// The edits: an Op JSON envelope, in a file, or `-` for standard input.
    #[arg(long, value_name = "FILE|-", required_unless_present = "schema")]
    edits: Option<PathBuf>,
    /// A file the edits name as {"with": NAME}: a PDF, an image, or a file
    /// to attach. A PDF's password is read from the environment variable
    /// PASSWORD_ENV. Repeats.
    #[arg(long = "with", value_name = "NAME=PATH[:PASSWORD_ENV]", value_parser = With::parse)]
    with: Vec<With>,
    /// The input's password.
    #[arg(long)]
    password: Option<String>,
    /// Where to write the result, written whole unless --append.
    #[arg(short, long, required_unless_present_any = ["in_place", "dry_run", "schema"])]
    output: Option<PathBuf>,
    /// Copy the input to the output and append the edits to it, so a
    /// signed input keeps its signatures; it stops, saying why, when the
    /// edits can't be appended.
    #[arg(long, requires = "output")]
    append: bool,
    /// Save into the input itself, appending when the file allows.
    #[arg(long, conflicts_with = "output")]
    in_place: bool,
    /// Print what the edits would do, writing nothing: their lines, what
    /// they remove or replace, what they'd do to signatures, the pages
    /// they touch and what they'd make.
    #[arg(long, conflicts_with_all = ["output", "in_place", "force"])]
    dry_run: bool,
    /// Replace the output file if it's there.
    #[arg(long)]
    force: bool,
    /// Print the schema of the edits' JSON.
    #[arg(long, exclusive = true)]
    schema: bool,
    #[command(flatten)]
    decisions: Decisions,
}

/// A file the edits name: `NAME=PATH[:PASSWORD_ENV]`.
#[derive(Clone)]
struct With {
    name: String,
    path: PathBuf,
    /// The environment variable holding its password.
    password: Option<String>,
}

impl With {
    /// `NAME=PATH`, or `NAME=PATH:PASSWORD_ENV` when what follows the last
    /// `:` is a variable's name and the whole isn't a file that's there
    /// (`C:\a.pdf` is a path).
    fn parse(arg: &str) -> Result<With, String> {
        let (name, rest) = (arg.split_once('='))
            .filter(|(name, rest)| !name.is_empty() && !rest.is_empty())
            .ok_or_else(|| format!("{arg:?} isn't NAME=PATH[:PASSWORD_ENV]"))?;
        let variable = |v: &str| {
            let mut chars = v.chars();
            (chars.next()).is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        let (path, password) = match rest.rsplit_once(':') {
            Some((path, var)) if !path.is_empty() && variable(var) && !Path::new(rest).exists() => {
                (path, Some(var.to_string()))
            }
            _ => (rest, None),
        };
        Ok(With {
            name: name.to_string(),
            path: path.into(),
            password,
        })
    }
}

pub fn run(args: EditArgs, quiet: bool) -> anyhow::Result<()> {
    if args.schema {
        let schema = serde_json::to_value(schemars::schema_for!(Envelope))?;
        crate::print_json(schema, true);
        return Ok(());
    }
    let (Some(input), Some(from)) = (&args.input, &args.edits) else {
        unreachable!("clap requires them without --schema");
    };
    let passwords = (args.with.iter())
        .map(|w| match &w.password {
            Some(var) => std::env::var(var)
                .map(Some)
                .with_context(|| format!("--with {}: {var} isn't set", w.name)),
            None => Ok(None),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let to = match (&args.output, args.in_place) {
        (_, true) => Some(SaveTo::InPlace),
        (Some(path), false) => {
            if path.exists() && !args.force {
                bail!("{} already exists (--force replaces)", path.display());
            }
            Some(SaveTo::File {
                path: path.clone(),
                append: args.append,
            })
        }
        (None, false) => None,
    };
    let answers = args.decisions.answers();
    // What the edits and the save tell the reader, said once it's done.
    let mut notes: Vec<String> = Vec::new();
    // One op from reading the edits to the save, so Ctrl-C anywhere,
    // even while they're still coming on standard input, cancels it.
    let done = crate::progress::run("editing", quiet, |p| {
        let edits = read_edits(from)?;
        p.check()?;
        let mut session = Session::open(input, args.password.as_deref(), p)?;
        for (w, password) in args.with.iter().zip(&passwords) {
            let blob = (session.intern(&w.path, password.as_deref()))
                .with_context(|| format!("--with {}: reading {}", w.name, w.path.display()))?;
            session.name(&w.name, blob);
        }
        p.check()?;
        let Some(to) = to else {
            let previewed = session.preview(&edits, p)?;
            return Ok(dry_run(&session, &edits, &previewed));
        };
        let label = edits.label.clone();
        let commit = session.edit(edits, p)?;
        p.check()?;
        let written = match &to {
            SaveTo::InPlace => input.clone(),
            SaveTo::File { path, .. } => path.clone(),
        };
        let report = (session.save(to, &answers, p).decided())
            .with_context(|| format!("writing {}", written.display()))?;
        notes.extend(sheaf_doc::notice_text(&commit.notices));
        notes.extend(report.notes.iter().map(ToString::to_string));
        let names = Names::of(session.doc.state());
        let version =
            (commit.version).map(|_| label.unwrap_or_else(|| commit.lines[0].text.clone()));
        let strategy = report.strategy.map(|s| match s {
            StrategyKind::Append => "append",
            StrategyKind::Rewrite => "rewrite",
            StrategyKind::Opened => "opened",
        });
        Ok(json!({
            "version": version,
            "created": names.created(&commit.created),
            "lines": lines(&commit.lines),
            "strategy": strategy,
        }))
    })?;
    if !quiet {
        for note in &notes {
            eprintln!("note: {note}");
        }
    }
    crate::print_json(done, true);
    Ok(())
}

/// The envelope in `path`, or standard input's for `-`.
fn read_edits(path: &Path) -> anyhow::Result<Envelope> {
    let (text, from) = match path.as_os_str() == "-" {
        true => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            (text, "standard input".to_string())
        }
        false => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            (text, path.display().to_string())
        }
    };
    serde_json::from_str(&text).with_context(|| format!("reading the edits in {from}"))
}

/// The lines a commit or a preview says, as history shows them.
fn lines(lines: &[sheaf_doc::describe::Line]) -> Value {
    (lines.iter())
        .map(|l| json!({ "text": l.text, "removes": l.removes }))
        .collect()
}

/// What `--dry-run` prints: what the edits would be called, their lines,
/// their consequences for the file, the pages they'd change or add (by
/// position after them) and remove (by position now), and what they'd
/// make, named as in the state they'd leave.
fn dry_run(session: &Session, edits: &Envelope, previewed: &Previewed) -> Value {
    let preview = &previewed.preview;
    let changes = !preview.effects.keys.is_empty();
    let version = changes.then(|| match &edits.label {
        Some(label) => label.clone(),
        None => preview.lines[0].text.clone(),
    });
    let now = session.doc.state();
    let after = Names::of(&previewed.state);
    let (mut pages, mut removed) = (Vec::new(), Vec::new());
    for &page in preview.effects.pages.iter() {
        match after.page(page) {
            Some(pos) => pages.push(pos),
            None => removed.extend(now.pos_of(page).map(|i| i + 1)),
        }
    }
    pages.sort_unstable();
    removed.sort_unstable();
    json!({
        "version": version,
        "lines": lines(&preview.lines),
        "consequences": consequences(previewed),
        "pages": pages,
        "removed_pages": removed,
        "created": after.created(&preview.created),
    })
}

/// What the edits do to the file, in words: what a save of them removes
/// or replaces, and what they'd do to its signatures appended (written
/// whole, a file loses every one), and those whose signed bytes changed
/// already, which it doesn't ask about.
fn consequences(previewed: &Previewed) -> Vec<String> {
    let mut out: Vec<String> = (previewed.removed.iter())
        .map(|r| match r.kind.is_text() {
            true => format!("replaces {r}"),
            false => format!("removes {r}"),
        })
        .collect();
    let sigs = &previewed.signatures;
    if sigs.is_empty() {
        return out;
    }
    let count = |v: Verdict| sigs.iter().filter(|s| s.verdict == v).count();
    let of = |n: usize| match sigs.len() {
        1 => "the file's signature".to_string(),
        all => format!("{n} of the file's {all} signatures"),
    };
    let (breaks, flags) = (count(Verdict::Invalid), count(Verdict::Flagged));
    let broken = count(Verdict::AlreadyBroken);
    if breaks > 0 {
        out.push(format!("breaks {}, even appended", of(breaks)));
    }
    if flags > 0 {
        out.push(format!("appended, {} would flag the change", of(flags)));
    }
    if broken > 0 {
        out.push(format!(
            "finds {} broken already: the signed bytes changed",
            of(broken)
        ));
    }
    if breaks + flags + broken < sigs.len() {
        let kept = sigs.len() - breaks - flags - broken;
        out.push(format!(
            "keeps {} only appended (--append or --in-place)",
            of(kept)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name, a path and maybe the variable holding its password; a path
    /// with a drive's colon is a path.
    #[test]
    fn a_with_names_a_file_and_maybe_its_password() {
        let w = With::parse("logo=img/logo.png").unwrap();
        assert_eq!(
            (w.name.as_str(), w.path.as_path()),
            ("logo", Path::new("img/logo.png"))
        );
        assert_eq!(w.password, None);
        let w = With::parse("deal=C:\\files\\deal.pdf:DEAL_PW").unwrap();
        assert_eq!(w.path, Path::new("C:\\files\\deal.pdf"));
        assert_eq!(w.password.as_deref(), Some("DEAL_PW"));
        let w = With::parse("deal=C:\\deal.pdf").unwrap();
        assert_eq!(
            (w.path.as_path(), w.password),
            (Path::new("C:\\deal.pdf"), None)
        );
        for bad in ["logo", "=a.png", "logo="] {
            assert!(With::parse(bad).is_err(), "{bad}");
        }
    }
}
