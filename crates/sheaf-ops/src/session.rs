//! A document with no window (M10.5 §14.2): what every subcommand runs on,
//! and what Combine, a scan, Images to PDF and the comment summary make.
//! It opens as the app opens a file ([`sheaf_doc::open::read`]), takes
//! edits as ops committed by the command line, saves as the app saves
//! ([`save_with`]), as the reader's flags say ([`SaveTo`]), and exports
//! through the one writer every way out shares ([`sheaf_doc::export`]).
//!
//! Ops read from a file (`sheaf edit`) name things as a person sees them:
//! [`edit`](Session::edit) and [`preview`](Session::preview) take them so,
//! and refuse a session's ids ([`Tx::ops_from_file`]).
//!
//! A save or an export that stops at a choice the caller didn't make is a
//! [`NeedsDecision`] once it's an op's error ([`Decided`]): the command
//! line exits 3 with the choice as JSON, so a script never has one made
//! for it silently (M10.5 proposal 40).

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use in_progress::{Level, Progress};
use sheaf_doc::effects::{Removed, judge};
use sheaf_doc::export::{Export, ExportError, ExportFrom, Exported};
use sheaf_doc::forms::sigs::{Pending, SigImpact, sig_impact};
use sheaf_doc::materialize::{Ask, AskAnswers, SaveHow, SaveReport, SaveStop, save_with};
use sheaf_doc::model::DocState;
use sheaf_doc::open::ViewFacts;
use sheaf_doc::store::{BlobKind, Src};
use sheaf_doc::{
    Actor, Blob, Commit, CommitOpts, Document, EditError, Grant, Preview, Store, Tx, open,
};
use sheaf_edit::{Envelope, Op};

/// A document open with no window, and the files its ops name.
pub struct Session {
    pub doc: Document,
    /// What a view would show beside it that its state doesn't hold, as
    /// its file opened (none for one made in memory, or handed over open):
    /// what a window that takes it shows (Combine's, M10.5 §14.1).
    pub view: ViewFacts,
    /// What the ops name that the document doesn't hold yet: the files
    /// interned for them ([`intern`](Self::intern)), held while the
    /// session lives.
    held: Vec<Blob>,
    /// The files its ops name as `{"with": NAME}` ([`name`](Self::name)).
    named: Vec<(String, Blob)>,
}

/// A document already open, as a session's.
impl From<Document> for Session {
    fn from(doc: Document) -> Session {
        Session::of(doc)
    }
}

/// Where a session's save writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveTo {
    /// The document's own file, as the plan decides: an Append when one
    /// can follow it (`--in-place`).
    InPlace,
    /// Another file: written whole (`-o`), or, with `append`, a copy of
    /// the document's file with the changes appended (`-o … --append`),
    /// which keeps a signed file's bytes and signatures.
    File { path: PathBuf, append: bool },
}

impl Session {
    /// `path` opened as the app opens it, `p` counting its pages: once
    /// this returns, it takes edits. A save a crash cut short is undone
    /// first and the reader told, through `p`.
    pub fn open(path: &Path, password: Option<&str>, p: &dyn Progress) -> Result<Session> {
        let opened = open::read(path, password, p)?;
        if let Some(torn) = &opened.torn {
            p.message(Level::Warn, &format!("{}: {torn}", path.display()));
        }
        Ok(Session {
            view: opened.view,
            ..Session::of(opened.doc)
        })
    }

    /// A document of no pages, made in memory: what a scan and Images to
    /// PDF insert their pages into. Its first save is a Rewrite to the
    /// path it's given.
    pub fn new_empty() -> Session {
        let store = Store::new();
        let empty = sheaf_doc::memory::empty(&store).expect("a file of no pages reads");
        Session::of_empty(store, empty, "Untitled")
    }

    /// [`new_empty`](Self::new_empty), its `/Info` saying it's `title`,
    /// made by sheaf, now, which its saves keep: what the comment summary
    /// inserts its pages into.
    pub fn new_titled(title: &str) -> Session {
        let store = Store::new();
        let empty = sheaf_doc::memory::titled(&store, title).expect("a file of no pages reads");
        Session::of_empty(store, empty, title)
    }

    fn of_empty(store: Arc<Store>, empty: Src, name: &str) -> Session {
        let facts = open::finish(&empty, &in_progress::NoProgress);
        let mut doc = open::start(
            store,
            empty,
            name.into(),
            None,
            sheaf_doc::history::default_policy(),
        );
        doc.ready(facts).expect("a file of no pages opens");
        Session::of(doc)
    }

    fn of(doc: Document) -> Session {
        Session {
            doc,
            view: ViewFacts::default(),
            held: Vec::new(),
            named: Vec::new(),
        }
    }

    /// The file at `path`, read and interned for the ops to name, held
    /// while the session lives: a PDF, opened with `password`; an image;
    /// or any other file, as an attachment's.
    pub fn intern(&mut self, path: &Path, password: Option<&str>) -> Result<Blob> {
        let store = self.doc.store().clone();
        let blob = match crate::combine::is_pdf(path) {
            true => store.read_file(path, password)?.0.blob().clone(),
            false => {
                let bytes =
                    std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
                let kind = BlobKind::image(&bytes).unwrap_or(BlobKind::Attachment);
                store.intern(bytes.into(), kind)?
            }
        };
        self.hold(blob.clone());
        Ok(blob)
    }

    /// `blob`, made for the ops to name, held while the session lives.
    pub fn hold(&mut self, blob: Blob) {
        self.held.push(blob);
    }

    /// `blob` named `name`, for the ops to name as `{"with": NAME}`; a
    /// name given again names the later blob.
    pub fn name(&mut self, name: &str, blob: Blob) {
        self.named.retain(|(n, _)| n != name);
        self.named.push((name.to_string(), blob));
    }

    /// The command line's transaction on the current version, with the
    /// session's names granted.
    pub fn tx(&self, actor: Actor) -> Tx {
        let mut tx = self.doc.tx(actor);
        for (name, blob) in &self.named {
            tx.grant_named(name.clone(), Grant::Blob(blob.clone().into()));
        }
        tx
    }

    /// `ops` as one version of the command line's, after the current one,
    /// labelled `label`: all of them, or none and why.
    pub fn commit(&mut self, ops: Vec<Op>, label: &str) -> Result<Commit, EditError> {
        self.commit_as(Actor::Cli, ops, label)
    }

    /// [`commit`](Self::commit), made by `actor`: a job's, an OCR run's.
    pub fn commit_as(
        &mut self,
        actor: Actor,
        ops: Vec<Op>,
        label: &str,
    ) -> Result<Commit, EditError> {
        let mut tx = self.tx(actor);
        tx.apply_batch(ops, &in_progress::NoProgress)?;
        self.doc.commit(tx, CommitOpts::labelled(label))
    }

    /// The ops of `edits`, read from a file, as one version of the
    /// command line's, named by the envelope's label (else by its first
    /// line, as history names one): all of them, or none and why. `p`
    /// counts what their lowering does and cancels it.
    pub fn edit(&mut self, edits: Envelope, p: &dyn Progress) -> Result<Commit, EditError> {
        let tx = self.file_tx(edits.ops, p)?;
        let opts = match edits.label {
            Some(label) => CommitOpts::labelled(label),
            None => CommitOpts::default(),
        };
        self.doc.commit(tx, opts)
    }

    /// What [`edit`](Self::edit) of `edits` would do, committing nothing.
    pub fn preview(&self, edits: &Envelope, p: &dyn Progress) -> Result<Previewed, EditError> {
        let tx = self.file_tx(edits.ops.clone(), p)?;
        let preview = tx.preview();
        let path = judge(self.doc.state(), tx.state(), &preview.effects.keys);
        let signatures = self.doc.primary().signatures();
        Ok(Previewed {
            signatures: sig_impact(signatures, Pending::Append, path.sig, &path.fields),
            removed: path.removed,
            state: tx.state().clone(),
            preview,
        })
    }

    /// `ops`, read from a file, applied on the current version.
    fn file_tx(&self, ops: Vec<Op>, p: &dyn Progress) -> Result<Tx, EditError> {
        let mut tx = self.tx(Actor::Cli);
        tx.ops_from_file();
        tx.apply_batch(ops, p)?;
        Ok(tx)
    }

    /// The current version saved where `to` says, the reader's `answers`
    /// to what it asks given: to the document's own file as the plan
    /// decides; to another written whole, as `-o` writes; or to a copy of
    /// the document's file, appended to, as `-o … --append` writes. The
    /// saved file is the document's from then on.
    pub fn save(
        &mut self,
        to: SaveTo,
        answers: &AskAnswers,
        p: &dyn Progress,
    ) -> Result<SaveReport, SaveStop> {
        let (path, how) = match to {
            SaveTo::InPlace => {
                let (path, _) = (self.doc.file()).ok_or_else(|| {
                    SaveStop::Failed("the document has no file to save in place".into())
                })?;
                (path.to_path_buf(), SaveHow::Planned)
            }
            SaveTo::File {
                path,
                append: false,
            } => (path, SaveHow::Rewrite),
            SaveTo::File { path, append: true } => (path, SaveHow::Append),
        };
        save_with(&mut self.doc, &path, how, answers, p)
    }

    /// The current version, as the command line exports it.
    pub fn export_from(&self) -> ExportFrom {
        self.doc.export_from(Actor::Cli)
    }

    /// `e` written ([`sheaf_doc::export::run`]), the reader's `answers` to
    /// what it asks given.
    pub fn export(
        &self,
        e: Export,
        answers: &AskAnswers,
        p: &dyn Progress,
    ) -> Result<Exported, ExportError> {
        sheaf_doc::export::run(e, answers, p)
    }
}

/// What an edit would do ([`Session::preview`]): its lines, effects and
/// what it would make, the state it would leave, and its consequences
/// for the file: what saving it removes or replaces, and what appending
/// it would do to the file's signatures, where it could append at all (a
/// Rewrite breaks every one).
pub struct Previewed {
    pub preview: Preview,
    pub state: DocState,
    /// By kind, judged against the version it's made on (M10.5 §7.2).
    pub removed: Vec<Removed>,
    /// Each of the file's signatures, by §9.3's table.
    pub signatures: Vec<SigImpact>,
}

/// A save or an export stopped at a choice the caller didn't make: what it
/// asks, which the command line prints as JSON before it exits 3.
#[derive(Debug, Clone, PartialEq)]
pub struct NeedsDecision(pub Ask);

impl NeedsDecision {
    /// The choice as JSON: its kind, the message, its counts, and for a
    /// signature ask whether an Append anyway is offered (and what it
    /// would leave readable in the signed revision) and the reason the
    /// save is a Rewrite.
    pub fn json(&self) -> serde_json::Value {
        let message = self.0.to_string();
        match &self.0 {
            Ask::FileChanged { path } => serde_json::json!({
                "kind": "file-changed",
                "message": message,
                "path": path.display().to_string(),
            }),
            Ask::Signatures {
                impact,
                append_anyway,
                reason,
            } => {
                let count = |v: sheaf_doc::forms::sigs::Verdict| {
                    impact.iter().filter(|i| i.verdict == v).count()
                };
                let stays: Option<Vec<String>> = append_anyway.as_ref().map(|offer| {
                    (offer.stays_readable.iter())
                        .map(ToString::to_string)
                        .collect()
                });
                serde_json::json!({
                    "kind": "signatures",
                    "message": message,
                    "signatures": impact.len(),
                    "breaks": count(sheaf_doc::forms::sigs::Verdict::Invalid),
                    "flags": count(sheaf_doc::forms::sigs::Verdict::Flagged),
                    "append_anyway": stays.is_some(),
                    "stays_readable": stays,
                    "reason": reason.as_ref().map(ToString::to_string),
                })
            }
            Ask::XfaHoldsValues { fields } => serde_json::json!({
                "kind": "xfa-holds-values",
                "message": message,
                "fields": fields,
            }),
        }
    }
}

impl fmt::Display for NeedsDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a decision is needed: {}", self.0)
    }
}

impl std::error::Error for NeedsDecision {}

/// A save's or an export's result as an op's: one stopped at an ask is a
/// [`NeedsDecision`], one cancelled is [`Cancelled`](crate::Cancelled)
/// (so [`was_cancelled`](crate::was_cancelled) knows it), and anything
/// else is its error.
pub trait Decided<T> {
    fn decided(self) -> Result<T>;
}

impl<T> Decided<T> for Result<T, SaveStop> {
    fn decided(self) -> Result<T> {
        self.map_err(|stop| match stop {
            SaveStop::Ask(ask) => NeedsDecision(ask).into(),
            SaveStop::Cancelled(c) => c.into(),
            SaveStop::Failed(e) => anyhow::anyhow!(e),
        })
    }
}

impl<T> Decided<T> for Result<T, ExportError> {
    fn decided(self) -> Result<T> {
        self.map_err(|e| match e {
            ExportError::Ask(ask) => NeedsDecision(ask).into(),
            ExportError::Cancelled(c) => c.into(),
            e => e.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use in_progress::NoProgress;
    use sheaf_doc::materialize::StrategyKind;
    use sheaf_edit::{BlobSel, Bring, PageSize, PageSource};

    use super::*;

    /// `pages` of `from` inserted at the end.
    fn insert(from: PageSource) -> Op {
        Op::InsertPages {
            from,
            pages: None,
            before: None,
            bring: Bring::default(),
            locals: Vec::new(),
        }
    }

    /// A document of no pages takes its first pages from an insert, and
    /// its first save writes them whole where it's told; its file is that
    /// one from then on, which the next save appends to in place. Before
    /// it's saved, it has no file to save in place.
    #[test]
    fn a_document_made_in_memory_takes_pages_and_saves() {
        let dir = std::env::temp_dir().join(format!(
            "sheaf-session-{}-{:x}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = Session::new_empty();
        assert_eq!(session.doc.page_count(), 0);
        let none = AskAnswers::default();
        let stop = session
            .save(SaveTo::InPlace, &none, &NoProgress)
            .unwrap_err();
        assert!(matches!(stop, SaveStop::Failed(_)), "{stop:?}");
        let blank = PageSource::Blank {
            size: PageSize {
                width: 612.0,
                height: 792.0,
            },
            count: 2,
        };
        session.commit(vec![insert(blank)], "Blank").unwrap();
        assert_eq!(session.doc.page_count(), 2);
        let path = dir.join("made.pdf");
        let to = SaveTo::File {
            path: path.clone(),
            append: false,
        };
        let report = session.save(to, &none, &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(
            lopdf::Document::load_mem(&saved).unwrap().get_pages().len(),
            2
        );
        // A file named for the ops, inserted by its name.
        let other = dir.join("other.pdf");
        std::fs::write(
            &other,
            sheaf_doc::memory::blank_pages(595.0, 842.0, 1).unwrap(),
        )
        .unwrap();
        let blob = session.intern(&other, None).unwrap();
        session.name("other", blob);
        let named = PageSource::File {
            file: BlobSel::With("other".into()),
            name: "other.pdf".into(),
        };
        let edits = Envelope::new(Some("Insert".into()), vec![insert(named)]);
        let previewed = session.preview(&edits, &NoProgress).unwrap();
        assert_eq!(previewed.preview.lines.len(), 1);
        assert_eq!(previewed.state.page_count(), 3);
        assert!(previewed.removed.is_empty() && previewed.signatures.is_empty());
        assert_eq!(session.doc.page_count(), 2, "a preview commits nothing");
        session.edit(edits, &NoProgress).unwrap();
        let report = session.save(SaveTo::InPlace, &none, &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append));
        let appended = std::fs::read(&path).unwrap();
        assert!(appended.starts_with(&saved));
        let back = Session::open(&path, None, &NoProgress).unwrap();
        assert_eq!(back.doc.page_count(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stop at an ask is a [`NeedsDecision`], as JSON its kind, counts
    /// and what's offered; a cancel is a cancel.
    #[test]
    fn a_stop_at_an_ask_needs_a_decision() {
        let stop: Result<(), SaveStop> = Err(SaveStop::Ask(Ask::XfaHoldsValues { fields: 3 }));
        let e = stop.decided().unwrap_err();
        let needed = e.downcast_ref::<NeedsDecision>().expect("a decision");
        assert_eq!(
            needed.json(),
            serde_json::json!({
                "kind": "xfa-holds-values",
                "message": "the form's XFA part holds values for 3 field(s), which the save removes",
                "fields": 3,
            })
        );
        let stop: Result<(), SaveStop> = Err(SaveStop::Cancelled(in_progress::Cancelled));
        assert!(crate::was_cancelled(&stop.decided().unwrap_err()));
    }
}
