//! Exports (M10.5 §12): every way out of a document besides a save, one
//! writer. An [`Export`] is a version of the document, the transforms that
//! make what's written of it, how it's written ([`Profile`]) and where
//! ([`Out`]); [`run`] writes it:
//! 1. its gates ([`preflight`]): never the document's own file, never an
//!    existing one unless told to replace it, and each transform one its
//!    actor may make;
//! 2. the transforms, lowered on an [`ExportTx`] over the version, as the
//!    binder lowers ops, with their cascades: nothing becomes an edit;
//! 3. the plan of a Rewrite of what they leave, onto the primary's objects,
//!    with save policy's form rules ([`materialize::plan()`]), whose asks are
//!    the export's: before signatures in what's exported stop validating,
//!    and before an XFA part that holds values is dropped;
//! 4. written as the plan says ([`materialize`]), checked as every file is
//!    (§11.9), and put at its path durably, or handed back as bytes.
//!
//! A failure anywhere writes nothing. The presets are the exports the app
//! and the command line make: [`Export::extract`], [`Export::print`],
//! [`Export::flattened`], [`Export::clean_copy`] and the two of Remove
//! Blanks ([`Export::rejects`]).
//!
//! [`materialize`]: crate::materialize::materialize

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use in_progress::Progress;
use sheaf_edit::{
    FlattenSet, MarkupKeep, Op, OutlinePolicy, PageNumbering, PageRef, PageSel, Sibling, Transform,
};

use crate::materialize::{
    self, Ask, AskAnswers, Encryption, Note, Numbering, Private, Profile, SaveInput, SavePlan,
    SaveStop, SaveTarget, Unheld,
};
use crate::model::{DocState, Document, PageId, Snapshot};
use crate::serialize::durable;
use crate::tx::export::transform_name;
use crate::tx::{Actor, Cap, EditError, ExportTx, LowerCx, actor_exports, transform_cap};

/// What an export is made from: a version of a document, and what its
/// transforms lower with (the document's store, its id allocator, who the
/// user is, the actor). Send: an export runs off the thread that made it.
#[derive(Clone)]
pub struct ExportFrom {
    pub snap: Snapshot,
    pub cx: Arc<LowerCx>,
}

impl Document {
    /// The current version, as `actor` exports it.
    pub fn export_from(&self, actor: Actor) -> ExportFrom {
        ExportFrom {
            snap: self.snapshot(),
            cx: self.lower_cx(actor),
        }
    }
}

/// An export: what it's made from, what's done to it, how it's written
/// and where.
#[derive(Clone)]
pub struct Export {
    pub from: ExportFrom,
    pub transforms: Vec<Transform>,
    pub profile: Profile,
    pub out: Out,
}

/// Where an export goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// A file at `path`, put there durably; one that's there already only
    /// when `overwrite`.
    File { path: PathBuf, overwrite: bool },
    /// Its bytes, handed back: a printer's, a page image's.
    Bytes,
}

/// What stops an export before it starts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Gate {
    /// It would replace the document's own file.
    #[error("an export can't replace the open file; choose another name")]
    OwnFile { path: PathBuf },
    /// A file is there already, and the export wasn't told to replace it.
    #[error("{} already exists", .path.display())]
    Exists { path: PathBuf },
    /// Its actor may not make one of its transforms.
    #[error("{transform}: {cap}")]
    Refused { transform: &'static str, cap: Cap },
}

/// Why an export wrote nothing.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// One of its gates ([`preflight`]).
    #[error(transparent)]
    Refused(Gate),
    /// The reader has a choice to make first: run it again with the answer.
    #[error(transparent)]
    Ask(Ask),
    /// A transform can't be lowered.
    #[error(transparent)]
    Edit(EditError),
    #[error(transparent)]
    Write(#[from] durable::WriteError),
    #[error("{0}")]
    Failed(String),
    #[error("the export was cancelled")]
    Cancelled(#[from] in_progress::Cancelled),
}

impl From<EditError> for ExportError {
    fn from(e: EditError) -> ExportError {
        match e {
            EditError::Cancelled => ExportError::Cancelled(in_progress::Cancelled),
            e => ExportError::Edit(e),
        }
    }
}

impl From<SaveStop> for ExportError {
    fn from(stop: SaveStop) -> ExportError {
        match stop {
            SaveStop::Ask(ask) => ExportError::Ask(ask),
            SaveStop::Failed(why) => ExportError::Failed(why),
            SaveStop::Cancelled(c) => ExportError::Cancelled(c),
        }
    }
}

/// What an export made.
#[derive(Debug, Clone, PartialEq)]
pub struct Exported {
    /// How many pages it has.
    pub pages: usize,
    /// How many annotations its flattens drew into their pages.
    pub flattened: usize,
    /// What the reader should be told of it.
    pub notes: Vec<Note>,
    /// Its bytes, for [`Out::Bytes`]: None once they're in its file.
    pub bytes: Option<Vec<u8>>,
}

/// How exports are written: encrypted as the primary is (M10.5 proposal
/// 10), numbered from 1 (§11.6).
const EXPORT: Profile = Profile {
    encryption: Encryption::Keep,
    numbering: Numbering::Dense,
    private: Private::Keep,
    unheld: Unheld::Keep,
};

/// What a print file's asks are answered: it's for a printer or a page
/// image, never a file someone keeps, so what it drops (an XFA part, the
/// signatures it can't keep) is dropped, and nothing asks.
pub const PRINT_ANSWERS: AskAnswers = AskAnswers {
    overwrite: false,
    break_signatures: true,
    append_anyway: false,
    stays_readable: false,
    found: false,
    drop_xfa_values: true,
};

/// `pages`, kept as `outline`, `labels` and `numbering` say.
fn keep(
    pages: &[PageId],
    outline: OutlinePolicy,
    labels: bool,
    numbering: PageNumbering,
) -> Transform {
    Transform::KeepPages {
        pages: PageSel::ids(pages.iter().copied()),
        outline,
        labels,
        numbering,
    }
}

/// Every page's annotations that `which` says flattened.
fn flatten(which: FlattenSet) -> Transform {
    Transform::Op(Op::FlattenAnnotations {
        pages: PageSel::All,
        which,
    })
}

impl Export {
    /// The whole document as a save writes it, to `out`: no transform, its
    /// encryption and object numbers kept.
    pub fn whole(from: ExportFrom, out: Out) -> Export {
        Export {
            from,
            transforms: Vec::new(),
            profile: Profile::default(),
            out,
        }
    }

    /// Acrobat's Extract Pages, and a split's part: `pages`, kept in
    /// document order, with their markup, the bookmarks that lead to them
    /// (a bookmark over them a heading) and their page labels, a header's
    /// page tokens numbering them as the file does: pages 5 and 6 of 10
    /// say "Page 1 of 2" and "Page 2 of 2".
    pub fn extract(from: ExportFrom, pages: &[PageId], out: Out) -> Export {
        let keep = keep(pages, OutlinePolicy::Headings, true, PageNumbering::AsKept);
        Export {
            from,
            transforms: vec![keep],
            profile: EXPORT,
            out,
        }
    }

    /// The file sent to a printer: `pages` in document order, as they look
    /// now, in memory, a header's page tokens numbering them as the
    /// document does, as print preview shows them: pages 5 and 6 of 10
    /// say "Page 5 of 10" and "Page 6 of 10" (Greg, 2026-10-22), with the
    /// document's labels, which the file doesn't keep. With `markup`,
    /// every printed comment and mark is drawn into its page, and so is the
    /// form's widgets' look, so any printing system shows it; without, the
    /// markup is left out, the form printing as it is. Either way sheaf's
    /// private data is left out, as a clean copy leaves it (§16 row 27),
    /// but what of it can't be taken out is a note, not a failure
    /// ([`Private::Scrub`]): what sheaf drew is plain content, so the file
    /// opened in sheaf again says what it printed, its header numbered as
    /// the document numbered it, not against the file's own pages.
    /// Unencrypted: the bytes go to a printer or a page image, never to a
    /// file someone keeps (§12.3). Its asks are answered
    /// ([`PRINT_ANSWERS`]).
    pub fn print(from: ExportFrom, pages: &[PageId], markup: bool) -> Export {
        let keep = keep(pages, OutlinePolicy::Drop, false, PageNumbering::AsDocument);
        let markup = match markup {
            true => flatten(FlattenSet::Printable),
            false => Transform::RemoveMarkup {
                keep: MarkupKeep::LINKS_AND_WIDGETS,
            },
        };
        Export {
            from,
            transforms: vec![keep, markup, Transform::RemovePrivateData],
            profile: Profile {
                encryption: Encryption::None,
                private: Private::Scrub,
                ..EXPORT
            },
            out: Out::Bytes,
        }
    }

    /// Export Flattened: every annotation shown drawn into its page and
    /// taken off, the form's widgets too, so it looks the same in any
    /// viewer and nothing on it can be edited. Links stay.
    pub fn flattened(from: ExportFrom, out: Out) -> Export {
        Export {
            from,
            transforms: vec![flatten(FlattenSet::All)],
            profile: EXPORT,
            out,
        }
    }

    /// PDFlyer's Export Clean Copy: the document as it stands, every
    /// comment and mark taken off, sheaf's and anyone else's, and none of
    /// sheaf's private data (§15). Links, the form and the bookmarks stay.
    pub fn clean_copy(from: ExportFrom, out: Out) -> Export {
        Export {
            from,
            transforms: vec![
                Transform::RemoveMarkup {
                    keep: MarkupKeep::LINKS_AND_WIDGETS,
                },
                Transform::RemovePrivateData,
            ],
            profile: Profile {
                private: Private::Strip,
                ..EXPORT
            },
            out,
        }
    }

    /// Remove Blanks' second file: the pages it took out, each with a
    /// bookmark of its own saying what it is (`titled`, by page), in
    /// place of the document's, numbered as the file has them.
    pub fn rejects(from: ExportFrom, titled: &[(PageId, String)], out: Out) -> Export {
        let pages: Vec<PageId> = titled.iter().map(|(page, _)| *page).collect();
        let keep = keep(&pages, OutlinePolicy::Drop, true, PageNumbering::AsKept);
        let mut transforms = vec![keep];
        transforms.extend(titled.iter().map(|(page, title)| {
            Transform::Op(Op::AddBookmark {
                parent: None,
                at: Sibling::Last,
                title: title.clone(),
                target: Some(PageRef::Id(*page)),
                view: None,
                color: None,
                style: Default::default(),
                local: None,
            })
        }));
        Export {
            from,
            transforms,
            profile: EXPORT,
            out,
        }
    }
}

/// What stops `e` before it starts (§12.1), in order; none when nothing
/// does. An export never replaces the document's own file, nor a file
/// that's there unless it's told to, and each of its transforms must be one
/// its actor may make: an op's as its version resolves it (one that
/// resolves only after the transforms before it is checked as it's
/// lowered).
pub fn preflight(e: &Export) -> Vec<Gate> {
    let mut gates = Vec::new();
    if let Out::File { path, overwrite } = &e.out {
        if let Err(gate) = not_its_file(&e.from.snap, path) {
            gates.push(gate);
        } else if !overwrite && path.exists() {
            gates.push(Gate::Exists { path: path.clone() });
        }
    }
    let cx = &e.from.cx;
    let base = Document::tx_at(&e.from.snap, cx.clone());
    for t in &e.transforms {
        let refused = match (t, transform_cap(t)) {
            (_, Some(cap)) => (!actor_exports(&cx.actor, cap)).then_some(cap),
            (Transform::Op(op), None) => match base.check(op) {
                Err(EditError::Refused(cap)) => Some(cap),
                _ => None,
            },
            _ => None,
        };
        if let Some(cap) = refused {
            let transform = transform_name(t);
            gates.push(Gate::Refused { transform, cap });
        }
    }
    gates
}

/// [`Gate::OwnFile`] when `path` is the file `snap`'s document was opened
/// from: what every export refuses as its target (§12.1), those written
/// by writers of their own too (a comment summary, CSV, text, XFDF).
pub fn not_its_file(snap: &Snapshot, path: &Path) -> Result<(), Gate> {
    match (snap.file.as_deref()).is_some_and(|file| durable::same_file(file, path)) {
        true => Err(Gate::OwnFile { path: path.into() }),
        false => Ok(()),
    }
}

/// `e`, written (§12): its gates, its transforms lowered, a Rewrite of
/// what they leave planned and asked about with `answers`, written,
/// checked, and put in its file or handed back. `p` counts each part's
/// pages and says when to stop. A failure anywhere writes nothing.
pub fn run(e: Export, answers: &AskAnswers, p: &dyn Progress) -> Result<Exported, ExportError> {
    let Planned {
        input,
        plan,
        target,
        out,
        pages,
        flattened,
    } = planned(e, answers, p)?;
    p.check()?;
    let written = materialize::materialize(&input, &plan, target, p)?;
    let bytes = match out {
        Out::File { path, overwrite } => {
            if !overwrite && path.exists() {
                return Err(ExportError::Refused(Gate::Exists { path }));
            }
            durable::write(&path, &written.bytes, None)?;
            None
        }
        Out::Bytes => Some(written.bytes),
    };
    Ok(Exported {
        pages,
        flattened,
        notes: written.notes,
        bytes,
    })
}

/// What [`run`] would stop `e` at before it writes anything: its gates, a
/// transform that can't be lowered, and what it asks that `answers` don't
/// answer. Nothing is written. What several exports that write as one
/// (a split's parts, Remove Blanks' two files) check first, so that none
/// is written when a later one would ask.
pub fn check(e: &Export, answers: &AskAnswers, p: &dyn Progress) -> Result<(), ExportError> {
    planned(e.clone(), answers, p).map(drop)
}

/// An export planned: what it writes, as its plan says, where.
struct Planned {
    input: SaveInput,
    plan: SavePlan,
    target: SaveTarget,
    out: Out,
    pages: usize,
    flattened: usize,
}

/// `e` up to its write: its gates, its transforms lowered, and a Rewrite
/// of what they leave planned, with nothing it asks left unanswered by
/// `answers`.
fn planned(e: Export, answers: &AskAnswers, p: &dyn Progress) -> Result<Planned, ExportError> {
    if let Some(gate) = preflight(&e).into_iter().next() {
        return Err(ExportError::Refused(gate));
    }
    let Export {
        from,
        transforms,
        mut profile,
        out,
    } = e;
    let mut xtx = ExportTx::new(&from.snap, from.cx);
    for t in transforms {
        xtx.apply(t, p)?;
    }
    // A transform that takes it out holds the output to the profile's
    // check, the post-condition's failing closed unless it says otherwise.
    if profile.private == Private::Keep {
        profile.private = xtx.private();
    }
    if xtx.unheld() != Unheld::Keep {
        profile.unheld = xtx.unheld();
    }
    let pages = xtx.state().page_count();
    let flattened = xtx.flattened();
    let input = SaveInput {
        target: xtx.state().clone(),
        ..SaveInput::detached(&from.snap)
    };
    drop(xtx);
    let path = match &out {
        Out::File { path, .. } => path.as_path(),
        // Bytes only: no file.
        Out::Bytes => Path::new(""),
    };
    let target = SaveTarget {
        path: path.into(),
        // Never the document's file (the gates).
        same_file: false,
        profile,
        // Only a save asks whether its file changed.
        stamp: None,
    };
    let mut plan = materialize::plan(&input, &target);
    exported_asks(&mut plan, &input.target);
    let plan = plan.answered(&input, &target, answers);
    if let Some(ask) = materialize::unanswered(&plan, answers) {
        return Err(ExportError::Ask(ask));
    }
    Ok(Planned {
        input,
        plan,
        target,
        out,
        pages,
        flattened,
    })
}

/// The print file of `pages` of `from`, as [`Export::print`] makes it, its
/// asks answered ([`PRINT_ANSWERS`]): what a printer, a page image and the
/// print preview's Save as PDF are made from.
pub fn printed(
    from: ExportFrom,
    pages: &[PageId],
    markup: bool,
    p: &dyn Progress,
) -> Result<Vec<u8>, ExportError> {
    let exported = run(Export::print(from, pages, markup), &PRINT_ANSWERS, p)?;
    Ok(exported.bytes.expect("a print file is bytes"))
}

/// What print preview realizes of page `page` of `from`'s version (§16 row
/// 28): with `markup`, the version with the page's printed annotations
/// drawn into it as the print file draws them ([`Export::print`]'s
/// flatten); without, the version as it is, which draws as the print file
/// does under print's inputs, markup left out
/// ([`ViewInputs::print`](crate::realize::ViewInputs::print)).
pub fn print_preview(
    from: &ExportFrom,
    page: PageId,
    markup: bool,
) -> Result<DocState, ExportError> {
    if !markup {
        return Ok(from.snap.state.clone());
    }
    let mut xtx = ExportTx::new(&from.snap, from.cx.clone());
    let flatten = Op::FlattenAnnotations {
        pages: PageSel::ids([page]),
        which: FlattenSet::Printable,
    };
    xtx.apply(Transform::Op(flatten), &in_progress::NoProgress)?;
    Ok(xtx.state().clone())
}

/// `plan`'s asks as an export's of `exported`: the signatures it asks
/// about only those still in what's exported (a page left out takes its
/// signature fields with it, which the export can't break), and a
/// certification, which covers the whole file.
fn exported_asks(plan: &mut SavePlan, exported: &DocState) {
    for ask in &mut plan.asks {
        if let Ask::Signatures { impact, .. } = ask {
            impact
                .retain(|i| i.signature.is_empty() || exported.field_named(&i.signature).is_some());
        }
    }
    plan.asks.retain(|ask| match ask {
        Ask::Signatures { impact, .. } => impact.iter().any(|i| i.verdict.asks()),
        _ => true,
    });
}

#[cfg(any(test, feature = "fixtures"))]
impl Document {
    /// For tests: the bytes of the export `make` makes of the current
    /// version, the user's, to [`Out::Bytes`], what it asks answered as
    /// print answers it ([`PRINT_ANSWERS`]).
    pub fn export_bytes(&self, make: impl FnOnce(ExportFrom, Out) -> Export) -> Vec<u8> {
        let e = make(self.export_from(Actor::User), Out::Bytes);
        let exported = run(e, &PRINT_ANSWERS, &in_progress::NoProgress).unwrap();
        exported.bytes.expect("an export to bytes is its bytes")
    }
}
