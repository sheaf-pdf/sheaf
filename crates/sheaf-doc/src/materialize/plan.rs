//! What a save will do, decided before a byte is written (M10.5 §9.2):
//! its strategy and why, what it asks the reader first, and the form
//! transforms save policy applies (§11.8). And why a save stops, before
//! writing or at the write, and what the reader's answers make of a plan
//! ([`SavePlan::answered`]).
//!
//! [`plan`] judges the save's path against the saved version, by value
//! ([`judge`]): a path that changed nothing is an Unchanged plan, which
//! writes nothing; one an update can carry is an Append, onto the saved
//! file, or onto a copy of it for Save As of a signed document; anything
//! else is a Rewrite, for its reasons, strict removal among them. A
//! document restored from its file's recovery sidecar has the sidecar's
//! sections pending (§11.10): an Append writes them first, and what their
//! flags say they did counts with the path. A save that breaks or flags
//! the file's signatures while they hold asks first, by what it does to
//! each (§9.3), offering an Append anyway where one could be written.
//! [`write_file`](super::write_file) checks again as it writes: an Append
//! it finds it can't write, as its section is made, is a Rewrite instead,
//! or a signed document asks first. A recovery sidecar's section is
//! planned apart (`sidecar`): an Append whatever would be decided.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use super::append::Refused;
use super::emit::content;
use super::key::ObjKey;
use super::record::{SaveInput, SaveRecord, TailFlags};
use crate::effects::{AnnotOps, Flags, PathEffects, Removed, RemovedKind, SigClass, judge};
use crate::forms::rules;
use crate::forms::sigs::{Pending, SigImpact, sig_impact};
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
    /// A whole new file, for these reasons: none when its caller chose it
    /// ([`forced_rewrite`]).
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
    /// It writes another file than the saved one, and not as a copy of it:
    /// the document has no signature that holds, or the saved file isn't
    /// the one sheaf read or wrote last.
    OtherTarget,
    /// Nothing says what the saved file holds: the document was made in
    /// memory, or recovered without a record.
    NoSaveRecord,
    /// The saved file can't take an update: its cross-reference sections
    /// had to be rebuilt to read it, or its page tree can't hold the pages.
    BaseUnsound,
    /// What the save removes, which an Append would leave readable in the
    /// file's earlier revision (strict removal, M10.5 proposal 9).
    Removes(Vec<Removed>),
    /// Text the save replaces or clears, likewise.
    ReplacesText(Vec<Removed>),
    /// The Append refers to objects the saved file hasn't got (§9.1's
    /// `present`): a Rewrite pruned them, a form instance's objects in it
    /// are another instance's now (until slice 11), a primary object's
    /// place in it is another record's of that object now, or what it has
    /// names a page that's another page now.
    BaseObjectsMissing,
    /// A save transform an Append can't carry (§11.8): dropping an XFA part
    /// whose datasets hold values the save replaced, which would stay in
    /// the earlier revision.
    TransformNeedsRewrite(SaveTransform),
    /// The removal check (§11.9) found text the Append would leave
    /// readable in the file: in `key`'s object, as the file has it.
    RemovalCheck { key: ObjKey },
    /// Another program changed the file, and the reader chose to overwrite
    /// it: an update can't follow bytes sheaf didn't write.
    FileChanged,
    /// The reader chose a Rewrite over an Append the file's signatures
    /// don't allow.
    Chosen,
    /// The document was restored from its file's recovery sidecar, whose
    /// sections, which the save writes too, removed or replaced text the
    /// file has (§11.10): an Append would leave it readable.
    RecoveredRemovals,
    /// The header and footer is drawn by a Rewrite, as it was before
    /// generators (until slice 17).
    LegacyHeaderFooter,
}

impl Why {
    /// Whether a signed document is offered an Append anyway when the save
    /// is a Rewrite for it (§9.2): for what the Append would leave readable
    /// in the earlier revision, never for a file it can't follow.
    fn allows_anyway(&self) -> bool {
        matches!(
            self,
            Why::Removes(_)
                | Why::ReplacesText(_)
                | Why::TransformNeedsRewrite(_)
                | Why::RemovalCheck { .. }
                | Why::RecoveredRemovals
        )
    }
}

impl fmt::Display for Why {
    /// Why, for the reader (§7.3).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Why::OtherTarget => f.write_str("it's saved as another file"),
            Why::NoSaveRecord => f.write_str("nothing says what the file holds"),
            Why::BaseUnsound => f.write_str("the file can't take an update"),
            Why::Removes(removed) if removed.is_empty() => {
                f.write_str("content was removed since the last save")
            }
            Why::Removes(removed) => write!(f, "{} removed since the last save", listed(removed)),
            Why::ReplacesText(replaced) if replaced.is_empty() => {
                f.write_str("text changed since the last save")
            }
            Why::ReplacesText(replaced) => {
                write!(f, "{} changed since the last save", listed(replaced))
            }
            Why::BaseObjectsMissing => f.write_str("the file no longer holds all the changes need"),
            Why::TransformNeedsRewrite(_) => {
                f.write_str("the form's XFA part holds values the save replaced")
            }
            Why::RemovalCheck { .. } => f.write_str("removed text would stay readable in the file"),
            Why::FileChanged => f.write_str("another program changed the file"),
            Why::Chosen => f.write_str("you chose to rewrite it"),
            Why::RecoveredRemovals => {
                f.write_str("the recovered changes removed text the file holds")
            }
            Why::LegacyHeaderFooter => {
                f.write_str("the header and footer is drawn on the whole file")
            }
        }
    }
}

/// `items` as a list: "1 page, 2 comments and 1 bookmark".
fn listed(items: &[Removed]) -> String {
    let words: Vec<String> = items.iter().map(Removed::to_string).collect();
    match words.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// A choice a save asks the reader to make (M10.5 §9.2).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Ask {
    /// The file at `path` isn't the one sheaf read: Save As…, Overwrite or
    /// Cancel.
    #[error("{} changed on disk since sheaf read it", .path.display())]
    FileChanged { path: Arc<Path> },
    /// The save breaks or flags signatures that hold, as `impact` says of
    /// each (a certification is one): Rewrite or Cancel, or Append anyway
    /// when that's offered. `reason` is what made the save a Rewrite: None
    /// when it's an Append the signatures don't allow.
    #[error(
        "the save would break or flag {} of the file's {} signature(s)",
        asking(.impact),
        .impact.len()
    )]
    Signatures {
        impact: Vec<SigImpact>,
        append_anyway: Option<AppendAnyway>,
        reason: Option<Why>,
    },
    /// The save drops the form's XFA part, whose datasets hold values for
    /// `fields` fields that only Acrobat and Reader show.
    #[error("the form's XFA part holds values for {fields} field(s), which the save removes")]
    XfaHoldsValues { fields: usize },
}

/// How many of `impact`'s signatures the save asks about.
fn asking(impact: &[SigImpact]) -> usize {
    impact.iter().filter(|i| i.verdict.asks()).count()
}

/// An Append offered to a signed document in place of a Rewrite, or of
/// nothing (an Append the signatures don't allow): its signed revision
/// stays as it was, and what the save removes stays readable in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendAnyway {
    /// What stays readable in the earlier revision: what the save removes
    /// or replaces, and an XFA part it drops.
    pub stays_readable: Vec<Removed>,
}

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
    /// Rewrite a signed file ([`Ask::Signatures`]): one an Append its
    /// signatures don't allow would have written too ([`Why::Chosen`]).
    pub break_signatures: bool,
    /// Append to a signed file anyway, where [`Ask::Signatures`] offers it
    /// ([`take`](Self::take)).
    pub append_anyway: bool,
    /// And what the offer said stays readable in the earlier revision
    /// may, when it said anything: the removal check reports what it
    /// finds of that rather than stopping the Append. An offer that said
    /// nothing stays readable is answered without it, and the removal
    /// check, which finds what the save's path didn't say, asks again.
    pub stays_readable: bool,
    /// And what the removal check found, when the offer said that stays
    /// readable too ([`RemovedKind::Found`]): it reports whatever it
    /// finds. Without it, what it finds of a derived object that the
    /// save's path didn't say removes text is a misclassification, which
    /// stops the Append and asks again, saying so (§11.9).
    pub found: bool,
    /// Drop XFA that holds values ([`Ask::XfaHoldsValues`]).
    pub drop_xfa_values: bool,
}

impl AskAnswers {
    /// The Append anyway `offer` offers, taken: what it says stays
    /// readable, when it says anything, may, and what the removal check
    /// found, when it says so.
    pub fn take(&mut self, offer: &AppendAnyway) {
        self.append_anyway = true;
        self.stays_readable |= !offer.stays_readable.is_empty();
        self.found |= finds(offer);
    }

    /// Whether these answers take the Append anyway `offer` offers.
    fn takes(&self, offer: &AppendAnyway) -> bool {
        self.append_anyway
            && (self.stays_readable || offer.stays_readable.is_empty())
            && (self.found || !finds(offer))
    }
}

/// Whether `offer` says text the removal check found stays readable.
fn finds(offer: &AppendAnyway) -> bool {
    (offer.stays_readable.iter()).any(|r| r.kind == RemovedKind::Found)
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

impl SavePlan {
    /// The plan of a save of `input` to `target` as the reader's `answers`
    /// leave it (§9.2):
    /// - an Append or an Unchanged plan answered Overwrite, after another
    ///   program changed the file, is a Rewrite of it ([`Why::FileChanged`]);
    /// - a Rewrite whose signatures' ask offers an Append anyway, taken
    ///   ([`AskAnswers::take`]), is that Append, onto the saved file or a
    ///   copy of it at another path: save policy's transforms in its
    ///   section, the removal check reporting what it leaves readable;
    /// - an Append the signatures don't allow, answered Rewrite, is one
    ///   ([`Why::Chosen`]).
    ///
    /// Each with what it does to the form and asks. Any other plan is as it
    /// was.
    pub fn answered(
        &self,
        input: &SaveInput,
        target: &SaveTarget,
        answers: &AskAnswers,
    ) -> SavePlan {
        let appends = matches!(self.strategy, Strategy::Append { .. } | Strategy::Unchanged);
        if appends && answers.overwrite {
            return rewritten(input, vec![Why::FileChanged], Vec::new(), None);
        }
        let offered = (self.asks.iter()).find_map(|ask| match ask {
            Ask::Signatures {
                append_anyway: Some(offer),
                ..
            } => Some(offer),
            _ => None,
        });
        match self.strategy {
            Strategy::Rewrite { .. } if offered.is_some_and(|o| answers.takes(o)) => {
                appended(input, target, SigClass::empty())
            }
            Strategy::Append { .. }
                if offered.is_some() && answers.break_signatures && !answers.append_anyway =>
            {
                rewritten(input, vec![Why::Chosen], Vec::new(), None)
            }
            _ => self.clone(),
        }
    }
}

/// What a save of `input` to `target` does, decided before a byte is
/// written (M10.5 §9.2), its path judged against the saved version by
/// value:
/// - **Unchanged** when it's the saved file, still as sheaf read or wrote
///   it, nothing on the path changed (an undo back by another route, a
///   save just after one) and nothing is pending (a restored document's
///   sidecar sections, §11.10), whatever else would rewrite it.
/// - **An Append** onto the saved file when nothing stops one, after a
///   restored document's pending sections; onto a copy of it
///   (`Onto::CopyOf`) for Save As of a document whose signatures (or
///   usage rights) hold, while the saved file is as sheaf left it.
/// - **A Rewrite** otherwise, for every reason found: another file
///   ([`Why::OtherTarget`]), no record (or one of a whole document a
///   recovery sidecar restored), the file changed on disk (which asks), a
///   file that can't take an update, the legacy header and footer, pending
///   sections that removed or replaced text, what the path removes or
///   replaces (strict removal), and a save transform an Append can't
///   carry.
///
/// It asks before it breaks or flags the primary's signatures, while they
/// hold, by what it does to each ([`sig_impact`]): an Append by the
/// classes of what it changes, a transform or a later `/Version` written
/// in the catalog or the form counting as anything else, and the fields
/// it touches, the pending sections' too. The ask offers an Append anyway
/// for an Append they don't allow, and for a Rewrite one could replace
/// (each reason one an Append would leave readable). It asks before
/// dropping an XFA part whose datasets hold values. Its transforms are
/// save policy's ([`rules::plan`]).
pub fn plan(input: &SaveInput, target: &SaveTarget) -> SavePlan {
    let changed = target.same_file
        && (target.stamp.as_ref()).is_some_and(|stamp| !stamp.holds(&target.path).unwrap_or(false));
    let mut asks = Vec::new();
    if changed {
        asks.push(Ask::FileChanged {
            path: target.path.clone(),
        });
    }
    let header = input.target.header_footer().is_some();
    let mut why = Vec::new();
    let Some(saved) = (input.saved.as_ref()).filter(|s| s.record.knows_file()) else {
        if !target.same_file {
            why.push(Why::OtherTarget);
        }
        why.push(Why::NoSaveRecord);
        if changed {
            why.push(Why::FileChanged);
        }
        if header {
            why.push(Why::LegacyHeaderFooter);
        }
        return rewritten(input, why, asks, None);
    };
    let record = &saved.record;
    let onto = match target.same_file {
        true => Some(Onto::Saved),
        false => copies(input, record).then_some(Onto::CopyOf),
    };
    if onto.is_none() {
        why.push(Why::OtherTarget);
    }
    if changed {
        why.push(Why::FileChanged);
    }
    let path = judge(&saved.state, &input.target, &input.path_keys);
    let pending = record.pending_tail.as_ref().map(|tail| &tail.flags);
    if why.is_empty() && onto == Some(Onto::Saved) && path.changed.is_empty() && pending.is_none() {
        return SavePlan {
            strategy: Strategy::Unchanged,
            asks,
            transforms: Vec::new(),
            notes: Vec::new(),
        };
    }
    if !record.takes_updates() {
        why.push(Why::BaseUnsound);
    }
    if header {
        why.push(Why::LegacyHeaderFooter);
    }
    if pending.is_some_and(|tail| tail.removes) {
        why.push(Why::RecoveredRemovals);
    }
    // Recognized text or a header removed is replaced text, which says
    // what went.
    let (text, gone): (Vec<Removed>, Vec<Removed>) =
        path.removed.iter().partition(|r| r.kind.is_text());
    let replaces = path.flags.contains(Flags::REPLACES_TEXT);
    if path.flags.contains(Flags::REMOVES) && !(gone.is_empty() && replaces) {
        why.push(Why::Removes(gone));
    }
    if replaces {
        why.push(Why::ReplacesText(text));
    }
    let append = Appending::of(input, record, &path);
    why.extend(append.needs_rewrite.map(Why::TransformNeedsRewrite));
    match onto {
        Some(onto) if why.is_empty() => append.plan(input, onto, &path, asks),
        _ => {
            let stays = onto.map(|_| append.stays_readable(&path));
            rewritten(input, why, asks, stays)
        }
    }
}

/// Whether a save of `input` to another path than `record`'s file is an
/// Append onto a copy of that file (proposal 32): the primary has
/// signatures or usage rights, they hold, and the file is still as sheaf
/// read or wrote it, which the copy checks again.
fn copies(input: &SaveInput, record: &SaveRecord) -> bool {
    let signed =
        (input.sources.get(&input.primary)).is_some_and(|info| !info.src.signatures().is_empty());
    signed && record.signatures_hold && record.stamp.holds(&record.path).unwrap_or(false)
}

/// What an Append of a save's path does to the form, and to signatures.
struct Appending {
    transforms: Vec<SaveTransform>,
    /// Its classes of change (§9.3): the path's and the record's pending
    /// tail's, and anything else when a transform or a later `/Version`
    /// writes the catalog or the form.
    class: SigClass,
    /// The full names of the fields it touches, the pending tail's too.
    fields: BTreeSet<String>,
    /// A transform it can't carry (§11.8).
    needs_rewrite: Option<SaveTransform>,
    /// The pending tail it writes first removed or replaced text the file
    /// has (§11.10).
    recovered: bool,
}

impl Appending {
    /// An Append of `path`, the path of `input` from the saved file of
    /// `record`, after the record's pending tail, whose flags say what it
    /// does. Save policy keeps usage rights when they allow what it
    /// changes ([`rules::plan`]); dropping an XFA part whose datasets hold
    /// values rides it only when the path replaced none of the fields'.
    fn of(input: &SaveInput, record: &SaveRecord, path: &PathEffects) -> Appending {
        let tail = record.pending_tail.as_ref().map(|tail| &tail.flags);
        let mut class = path.sig | tail.map_or(SigClass::empty(), |t| t.sig);
        let mut fields = path.fields.clone();
        fields.extend(tail.into_iter().flat_map(|t| t.fields.iter().cloned()));
        if content::needs_after(&input.target, record.pdf_version) {
            class |= SigClass::OTHER;
        }
        let strategy = Strategy::Append { onto: Onto::Saved };
        let transforms = match input.sources.get(&input.primary) {
            Some(info) => rules::plan(
                &info.src,
                &input.target,
                input.primary,
                Some(record),
                &strategy,
                class,
                path.annots,
            ),
            None => Vec::new(),
        };
        if !transforms.is_empty() {
            class |= SigClass::OTHER;
        }
        let held = input.target.form().xfa.values > 0;
        let replaced = (path.removed.iter()).any(|r| r.kind == RemovedKind::FieldValues);
        let needs_rewrite = (transforms.iter())
            .find(|t| matches!(t, SaveTransform::DropXfa { .. }) && held && replaced)
            .copied();
        Appending {
            transforms,
            class,
            fields,
            needs_rewrite,
            recovered: tail.is_some_and(|t| t.removes),
        }
    }

    /// What stays readable in the file's earlier revision after it: what
    /// `path` removed or replaced, an XFA part it drops, and what the
    /// pending tail removed.
    fn stays_readable(&self, path: &PathEffects) -> Vec<Removed> {
        let mut out = path.removed.clone();
        if (self.transforms.iter()).any(|t| matches!(t, SaveTransform::DropXfa { .. })) {
            out.push(Removed {
                kind: RemovedKind::XfaDatasets,
                count: 1,
            });
        }
        if self.recovered {
            out.push(Removed {
                kind: RemovedKind::Recovered,
                count: 1,
            });
        }
        out
    }

    /// Its plan, onto `onto`, of `input`, whose path is `path`, `asks`
    /// asked first: then before it breaks or flags the primary's
    /// signatures, while they hold, offering it anyway; and what its
    /// transforms ask.
    fn plan(
        self,
        input: &SaveInput,
        onto: Onto,
        path: &PathEffects,
        mut asks: Vec<Ask>,
    ) -> SavePlan {
        let anyway = AppendAnyway {
            stays_readable: self.stays_readable(path),
        };
        let pending = Pending::Append;
        asks.extend(signatures_ask(
            input,
            pending,
            self.class,
            &self.fields,
            Some(anyway),
            None,
        ));
        let notes = transform_asks(input, &self.transforms, &mut asks);
        SavePlan {
            strategy: Strategy::Append { onto },
            asks,
            transforms: self.transforms,
            notes,
        }
    }
}

/// The Append of `input` to `target` a signed document's reader takes
/// anyway, its classes of change `class` beside its own: onto the saved
/// file, or a copy of it at another path, with what it asks.
fn appended(input: &SaveInput, target: &SaveTarget, class: SigClass) -> SavePlan {
    let Some(saved) = (input.saved.as_ref()).filter(|s| s.record.knows_file()) else {
        return rewritten(input, vec![Why::NoSaveRecord], Vec::new(), None);
    };
    let onto = match target.same_file {
        true => Onto::Saved,
        false => Onto::CopyOf,
    };
    let path = judge(&saved.state, &input.target, &input.path_keys);
    let mut append = Appending::of(input, &saved.record, &path);
    append.class |= class;
    append.plan(input, onto, &path, Vec::new())
}

/// A Rewrite of `input` for `why`, `asks` asked first: then before it
/// breaks the primary's signatures, while they hold, for the first reason,
/// offering an Append anyway that would leave `stays_readable` readable
/// when one could be written (Some) and every reason allows it; and what
/// save policy's transforms ask.
fn rewritten(
    input: &SaveInput,
    why: Vec<Why>,
    mut asks: Vec<Ask>,
    stays_readable: Option<Vec<Removed>>,
) -> SavePlan {
    let anyway = (stays_readable.filter(|_| why.iter().all(Why::allows_anyway)))
        .map(|stays_readable| AppendAnyway { stays_readable });
    let reason = why.first().cloned();
    let none = BTreeSet::new();
    let pending = Pending::Rewrite;
    asks.extend(signatures_ask(
        input,
        pending,
        SigClass::OTHER,
        &none,
        anyway,
        reason,
    ));
    let strategy = Strategy::Rewrite { why };
    let saved = input.saved.as_ref().map(|s| &*s.record);
    let transforms = match input.sources.get(&input.primary) {
        Some(info) => rules::plan(
            &info.src,
            &input.target,
            input.primary,
            saved,
            &strategy,
            SigClass::all(),
            AnnotOps::all(),
        ),
        None => Vec::new(),
    };
    let notes = transform_asks(input, &transforms, &mut asks);
    SavePlan {
        strategy,
        asks,
        transforms,
        notes,
    }
}

/// The ask before a save of `pending` breaks or flags the primary's
/// signatures, while they hold (a document with no record counts its
/// own as holding): its changes of `class`, touching the fields `fields`
/// names, judged against each ([`sig_impact`]). None when each allows it.
fn signatures_ask(
    input: &SaveInput,
    pending: Pending,
    class: SigClass,
    fields: &BTreeSet<String>,
    append_anyway: Option<AppendAnyway>,
    reason: Option<Why>,
) -> Option<Ask> {
    let holds = (input.saved.as_ref()).is_none_or(|s| s.record.signatures_hold);
    let info = input.sources.get(&input.primary).filter(|_| holds)?;
    let impact = sig_impact(info.src.signatures(), pending, class, fields);
    (asking(&impact) > 0).then_some(Ask::Signatures {
        impact,
        append_anyway,
        reason,
    })
}

/// The Rewrite a save planned as an Append is instead, for what its
/// section's `refused` (§9.2): the file `target` names written whole, with
/// what a Rewrite does to the form and asks, and a note of why. It asks
/// before it breaks the primary's signatures while they hold, offering an
/// Append anyway for a removal the check found, which would leave it
/// readable beside what the path says it does, never for an unsound file
/// or missing objects. A section that writes what the signatures don't
/// allow, though the plan found the save within them, is instead the
/// Append they don't allow, which asks ([`Refused::Unplanned`]).
pub(crate) fn fallback(input: &SaveInput, target: &SaveTarget, refused: &Refused) -> SavePlan {
    let Some(why) = refused.why() else {
        return appended(input, target, SigClass::OTHER);
    };
    let note = match why {
        Why::BaseUnsound => "Saved the whole file: the file it replaces can't take an update",
        Why::BaseObjectsMissing => {
            "Saved the whole file: the file it replaces no longer holds all the changes need"
        }
        _ => "Saved the whole file, so the text the changes removed doesn't stay in it",
    };
    let stays = match (&why, &input.saved) {
        (Why::RemovalCheck { .. }, Some(saved)) => {
            let path = judge(&saved.state, &input.target, &input.path_keys);
            let mut stays = Appending::of(input, &saved.record, &path).stays_readable(&path);
            stays.push(Removed {
                kind: RemovedKind::Found,
                count: 1,
            });
            stays
        }
        _ => Vec::new(),
    };
    let mut plan = rewritten(input, vec![why], Vec::new(), Some(stays));
    plan.notes.push(Note(note.into()));
    plan
}

/// A Rewrite of `input` to `target`, whatever [`plan`] would decide: its
/// plan when that's a Rewrite, or else one for no reason but its caller's,
/// asking what a Rewrite asks: what a Full recovery sidecar writes, and
/// the both-ways tests' Rewrite halves.
pub fn forced_rewrite(input: &SaveInput, target: &SaveTarget) -> SavePlan {
    let planned = plan(input, target);
    match planned.strategy {
        Strategy::Rewrite { .. } => planned,
        _ => rewritten(input, Vec::new(), Vec::new(), None),
    }
}

/// What a recovery sidecar of `input` writes against the saved file
/// (§11.10): the plan of an Append of its path, after the record's pending
/// tail, whatever [`plan`] would decide, asking nothing, with save
/// policy's transforms for it; and what the tail and its section do, for
/// the sidecar's flags: whether they remove or replace text the file has
/// or a transform in them would need a Rewrite, their classes of change
/// and the fields they touch. None when no section can follow the saved
/// file: nothing says what it holds, it can't take an update, or the
/// legacy header and footer is drawn, which only a Rewrite draws (until
/// slice 17).
pub(crate) fn sidecar(input: &SaveInput) -> Option<(SavePlan, TailFlags)> {
    let saved = input.saved.as_ref()?;
    let record = &saved.record;
    if !record.takes_updates() || input.target.header_footer().is_some() {
        return None;
    }
    let path = judge(&saved.state, &input.target, &input.path_keys);
    let append = Appending::of(input, record, &path);
    let flags = TailFlags {
        removes: path.flags.intersects(Flags::REMOVES | Flags::REPLACES_TEXT)
            || append.needs_rewrite.is_some()
            || append.recovered,
        sig: append.class,
        fields: append.fields,
    };
    let plan = SavePlan {
        strategy: Strategy::Append { onto: Onto::Saved },
        asks: Vec::new(),
        transforms: append.transforms,
        notes: Vec::new(),
    };
    Some((plan, flags))
}

/// For tests: an Append of `input` onto `onto`, whatever [`plan`] would
/// decide, asking nothing, as "Append anyway" saves: with save policy's
/// form transforms for an Append of its path, written in the section.
#[cfg(any(test, feature = "fixtures"))]
pub fn forced_append(input: &SaveInput, onto: Onto) -> SavePlan {
    let transforms = match &input.saved {
        Some(saved) => {
            let path = judge(&saved.state, &input.target, &input.path_keys);
            Appending::of(input, &saved.record, &path).transforms
        }
        None => Vec::new(),
    };
    let notes = transform_asks(input, &transforms, &mut Vec::new());
    SavePlan {
        strategy: Strategy::Append { onto },
        asks: Vec::new(),
        transforms,
        notes,
    }
}

/// What the reader is told of save policy's `transforms` of a save of
/// `input`, with what they ask first put in `asks`: dropping an XFA part
/// whose datasets hold values.
fn transform_asks(
    input: &SaveInput,
    transforms: &[SaveTransform],
    asks: &mut Vec<Ask>,
) -> Vec<Note> {
    let mut notes = Vec::new();
    for t in transforms {
        match t {
            SaveTransform::DropXfa { .. } => {
                notes.push(Note(rules::XFA_REMOVED.into()));
                let fields = input.target.form().xfa.values;
                if fields > 0 {
                    asks.push(Ask::XfaHoldsValues { fields });
                }
            }
            SaveTransform::DropUr3 => {
                notes.push(Note(rules::RIGHTS_REMOVED.into()));
            }
            SaveTransform::FixSigFlags => {}
        }
    }
    notes
}

/// The first of `plan`'s asks `answers` doesn't answer: what the reader
/// is asked before the save writes. None when they're all answered.
pub fn unanswered(plan: &SavePlan, answers: &AskAnswers) -> Option<Ask> {
    (plan.asks.iter())
        .find(|ask| match ask {
            Ask::FileChanged { .. } => !answers.overwrite,
            Ask::Signatures { append_anyway, .. } => {
                !(answers.break_signatures
                    || append_anyway.as_ref().is_some_and(|o| answers.takes(o)))
            }
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
