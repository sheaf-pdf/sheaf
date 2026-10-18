//! What a save will do, decided before a byte is written (M10.5 §9.2):
//! its strategy and why, what it asks the reader first, and the form
//! transforms save policy applies (§11.8). And why a save stops, before
//! writing or at the write.
//!
//! Slice 9 rewrites every save (the always-Rewrite rule, until slice 10
//! brings Append back), so its plans are Rewrites ([`plan`]), and only the
//! reasons and asks it gives are here.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use super::record::SaveInput;
use crate::serialize::durable::FileStamp;

/// What a save does, and what it asks first.
#[derive(Debug, Clone, PartialEq)]
pub struct SavePlan {
    pub strategy: Strategy,
    /// Choices for the reader, asked before anything is written.
    pub asks: Vec<Ask>,
    /// What save policy does to the form as it's written.
    pub transforms: Vec<SaveTransform>,
    /// What the reader should be told about the saved file.
    pub notes: Vec<Note>,
}

/// How a save writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Strategy {
    /// Nothing changed since the saved file: nothing is written, and the
    /// saved record is the version's.
    Unchanged,
    /// A section appended to a file, which keeps every byte before it.
    Append { onto: Onto },
    /// A whole new file, for these reasons.
    Rewrite { why: Vec<Why> },
}

/// How a save that wrote is noted: what a [`SaveRecord`] and the History
/// panel's [`SaveNote`] say.
///
/// [`SaveRecord`]: super::SaveRecord
/// [`SaveNote`]: super::SaveNote
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyKind {
    /// No save wrote it: the file as it was opened.
    Opened,
    Append,
    Rewrite,
}

/// What an Append writes onto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Onto {
    /// The saved file, in place.
    Saved,
    /// A copy of the saved file at the new path: Save As of a document
    /// with a signature.
    CopyOf,
}

/// Why a save is a Rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// It writes another file than the saved one.
    OtherTarget,
    /// Nothing says what the saved file holds: the document was made in
    /// memory, or recovered without a record.
    NoSaveRecord,
    /// Every save is a Rewrite, until slice 10 brings Append back.
    AlwaysRewrite,
    /// The header and footer is drawn by a Rewrite, as it was before
    /// generators (until slice 17).
    LegacyHeaderFooter,
}

/// A choice a save asks the reader to make (M10.5 §9.2).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Ask {
    /// The file at `path` isn't the one sheaf read: Save As…, Overwrite or
    /// Cancel.
    #[error("{} changed on disk since sheaf read it", .path.display())]
    FileChanged { path: Arc<Path> },
    /// A Rewrite breaks the file's `signatures` signed fields (a
    /// certification is one): Rewrite or Cancel. Slice 10 counts each
    /// signature's impact, and offers an Append anyway.
    #[error("a full save would break the file's {signatures} signature(s)")]
    Signatures {
        signatures: usize,
        /// None until slice 10.
        append_anyway: Option<AppendAnyway>,
        /// What made the save a Rewrite, when that's why it asks.
        reason: Option<Why>,
    },
    /// The save drops the form's XFA part, whose datasets hold values for
    /// `fields` fields that only Acrobat and Reader show.
    #[error("the form's XFA part holds values for {fields} field(s), which the save removes")]
    XfaHoldsValues { fields: usize },
}

/// An Append offered to a signed document in place of a Rewrite, which
/// keeps what it removes readable in the earlier revision. Slice 10 offers
/// it, with what stays readable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendAnyway {}

/// Why a save stopped (M10.5 §11.1): nothing was written.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SaveStop {
    /// The reader has a choice to make first.
    #[error(transparent)]
    Ask(Ask),
    #[error("{0}")]
    Failed(String),
    /// The reader cancelled it.
    #[error("the save was cancelled")]
    Cancelled(#[from] in_progress::Cancelled),
}

/// The reader's answers to a plan's asks, for the save made with them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AskAnswers {
    /// Replace a file another program changed ([`Ask::FileChanged`]).
    pub overwrite: bool,
    /// Rewrite a signed file ([`Ask::Signatures`]).
    pub break_signatures: bool,
    /// Drop XFA that holds values ([`Ask::XfaHoldsValues`]).
    pub drop_xfa_values: bool,
}

/// Where a save writes, and how ([`Document::save_target`] says for a
/// save).
///
/// [`Document::save_target`]: crate::Document::save_target
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveTarget {
    pub path: Arc<Path>,
    /// The path names the document's file: the saved one, or a restored
    /// document's base.
    pub same_file: bool,
    /// Saves keep the encryption and their numbers; exports choose.
    pub profile: Profile,
    /// The document's file as sheaf last read or wrote it, when the path
    /// names it: the save asks before it replaces another program's file,
    /// and checks again just before the rename (M10.5 proposal 33).
    pub stamp: Option<FileStamp>,
}

/// How an output is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Profile {
    pub encryption: Encryption,
    pub numbering: Numbering,
}

/// Whether an output is encrypted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Encryption {
    /// As the primary is: saves, Save As and recovery, and the exports
    /// that keep it.
    #[default]
    Keep,
    /// Not at all: print and page images.
    None,
}

/// How an output's objects are numbered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Numbering {
    /// Every surviving primary object keeps its number, and keys the
    /// saved record numbered keep theirs: saves.
    #[default]
    Stable,
    /// From 1, with identical streams written once: exports.
    Dense,
}

/// What save policy does to the form as it's written (M10.5 §11.8), never
/// an edit: undo across the save restores nothing in the file, and the
/// next save applies the policy again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTransform {
    /// The XFA part goes, its tooltips copied onto the fields' `/TU` when
    /// `tooltips_to_tu`.
    DropXfa { tooltips_to_tu: bool },
    /// Reader's usage rights go: their signature covers the original
    /// bytes.
    DropUr3,
    /// `/SigFlags` follows the signatures the target holds.
    FixSigFlags,
}

/// What a save of `input` to `target` does, decided before a byte is
/// written (M10.5 §9.2). Every save is a Rewrite until slice 10, with its
/// reasons: another file than the saved one, no record of the saved file,
/// the legacy header and footer. It asks first when the file isn't the one
/// sheaf read or wrote last (by the target's stamp), when the
/// Rewrite breaks the primary's signatures (a certification counts as
/// one), unless it writes onto the file an earlier Rewrite of the document
/// wrote, which has none left to break, and when save policy drops an XFA
/// part whose datasets hold values.
/// Its transforms are save policy's ([`crate::forms::rules::plan`]).
pub fn plan(input: &SaveInput, target: &SaveTarget) -> SavePlan {
    let saved = input.saved.as_ref().map(|s| &*s.record);
    let st = &input.target;
    let mut why = Vec::new();
    if !target.same_file {
        why.push(Why::OtherTarget);
    }
    if saved.is_none() {
        why.push(Why::NoSaveRecord);
    }
    if st.header_footer().is_some() {
        why.push(Why::LegacyHeaderFooter);
    }
    if why.is_empty() {
        why.push(Why::AlwaysRewrite);
    }
    let strategy = Strategy::Rewrite { why };
    let mut asks = Vec::new();
    if target.same_file
        && let Some(stamp) = &target.stamp
        && !stamp.holds(&target.path).unwrap_or(false)
    {
        asks.push(Ask::FileChanged {
            path: target.path.clone(),
        });
    }
    let mut transforms = Vec::new();
    let mut notes = Vec::new();
    // A file a Rewrite of the document wrote holds none of its signatures
    // as they were signed: a save onto it breaks nothing more.
    let broken = target.same_file && saved.is_some_and(|s| s.strategy == StrategyKind::Rewrite);
    if let Some(info) = input.sources.get(&input.primary) {
        let base = info.src.form();
        let signatures = signatures_broken(&info.src);
        if signatures > 0 && !broken {
            asks.push(Ask::Signatures {
                signatures,
                append_anyway: None,
                reason: None,
            });
        }
        transforms = crate::forms::rules::plan(base, st, input.primary, saved, &strategy);
        for t in &transforms {
            match t {
                SaveTransform::DropXfa { .. } => {
                    notes.push(Note(crate::forms::rules::XFA_REMOVED.into()));
                    let fields = st.form().xfa.values;
                    if fields > 0 {
                        asks.push(Ask::XfaHoldsValues { fields });
                    }
                }
                SaveTransform::DropUr3 => {
                    notes.push(Note(crate::forms::rules::RIGHTS_REMOVED.into()));
                }
                SaveTransform::FixSigFlags => {}
            }
        }
    }
    SavePlan {
        strategy,
        asks,
        transforms,
        notes,
    }
}

/// What a Rewrite of `src` breaks: its signed fields, a certification
/// counting as one.
pub(crate) fn signatures_broken(src: &crate::Source) -> usize {
    let facts = &src.form().facts;
    facts.signed.max(usize::from(facts.certified))
}

/// The first of `plan`'s asks `answers` doesn't answer: what the reader
/// is asked before the save writes. None when they're all answered.
pub fn unanswered(plan: &SavePlan, answers: &AskAnswers) -> Option<Ask> {
    (plan.asks.iter())
        .find(|ask| match ask {
            Ask::FileChanged { .. } => !answers.overwrite,
            Ask::Signatures { .. } => !answers.break_signatures,
            Ask::XfaHoldsValues { .. } => !answers.drop_xfa_values,
        })
        .cloned()
}

/// Something the reader should be told about a saved file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note(pub String);

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
