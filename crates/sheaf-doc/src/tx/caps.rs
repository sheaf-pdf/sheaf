//! Who may make which change (M10.5 §6.7). An op's capability comes from
//! the op as resolved and the kinds of what it acts on, not its name
//! alone: deleting sheaf's own marks is a plugin's to do, deleting another
//! tool's comment needs the user's approval. Both tables are total
//! matches, so a new op or actor doesn't compile until it has a row.

use std::fmt;
use std::sync::Arc;

use sheaf_edit::Transform;

use super::resolve::ResolvedOp;
use crate::model::{AnnotBody, DocState};

/// Who submits a transaction. Only [`Document::lower_cx`] binds one to
/// it, so a caller can't claim to be another.
///
/// [`Document::lower_cx`]: crate::model::Document::lower_cx
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Actor {
    /// The user, at the app.
    User,
    /// A plugin, by its id and version.
    Plugin { id: Arc<str>, version: Arc<str> },
    /// The command line.
    Cli,
    /// A job sheaf runs for the user.
    Job(JobKind),
}

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    /// Text recognition: it sets and clears OCR text, nothing else.
    Ocr,
    /// Trim White: it sets page boxes, nothing else.
    Trim,
    /// A comment import.
    Import,
    /// Pages inserted from a file.
    Insert,
}

/// What a change needs of whoever makes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cap {
    /// What any plugin may do: pages moved, turned, deleted or copied,
    /// labels, bookmarks, sheaf's own marks.
    Plugin,
    /// What changes or removes content the user didn't make in sheaf: the
    /// user approves it.
    Approval,
    /// What only the user and the host's own jobs do: content from files,
    /// recognized text, imported comments' names and dates.
    User,
}

impl fmt::Display for Cap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Cap::Plugin => "refused: a plugin's edits are its own to make",
            Cap::Approval => "refused: that needs your approval",
            Cap::User => "refused: only you can make that change",
        })
    }
}

/// The kind of annotation an op acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    /// One of sheaf's marks.
    Mark,
    /// Another tool's comment.
    Comment,
    /// A link.
    Link,
    /// A form field's widget.
    Widget,
}

impl Target {
    /// Every kind, for the totality test.
    #[cfg(test)]
    pub(crate) const ALL: [Target; 4] =
        [Target::Mark, Target::Comment, Target::Link, Target::Widget];

    /// What annotation `id` is in `st`.
    fn of(st: &DocState, id: crate::annot::AnnotId) -> Option<Target> {
        Some(match &st.annot(id)?.body {
            AnnotBody::Mark(_) => Target::Mark,
            AnnotBody::Foreign(f) if f.summary.is_link() => Target::Link,
            AnnotBody::Foreign(_) => Target::Comment,
            AnnotBody::Widget(_) => Target::Widget,
        })
    }
}

/// The capability `op` needs, acting on annotations of `targets`' kinds.
pub fn cap_of(op: &ResolvedOp, targets: &[Target]) -> Cap {
    use ResolvedOp as R;
    match op {
        R::MovePages { .. }
        | R::RotatePages { .. }
        | R::DeletePages { .. }
        | R::DuplicatePages { .. }
        | R::SetPageLabels { .. }
        | R::AddBookmark { .. }
        | R::PatchBookmarks { .. }
        | R::MoveBookmarks { .. }
        | R::DeleteBookmarks { .. }
        | R::AddConnector { .. }
        | R::MoveMarks { .. } => Cap::Plugin,
        R::AddMark { imported, .. } | R::PatchMarks { imported, .. } => match imported {
            Some(_) => Cap::User,
            None => Cap::Plugin,
        },
        R::DeleteAnnotations { .. } => {
            let theirs = targets.iter().any(|t| match t {
                Target::Mark => false,
                Target::Comment | Target::Link | Target::Widget => true,
            });
            match theirs {
                true => Cap::Approval,
                false => Cap::Plugin,
            }
        }
        R::SetPageBoxes { .. }
        | R::AdoptAnnotations { .. }
        | R::ClearOcrText { .. }
        | R::SetWatermark { .. }
        | R::RemoveWatermark { .. }
        | R::SetHeaderFooter { .. }
        | R::RemoveHeaderFooter { .. }
        | R::FlattenAnnotations { .. } => Cap::Approval,
        R::InsertPages { .. }
        | R::ReplacePages { .. }
        | R::SetOcrText { .. }
        | R::Revert { .. } => Cap::User,
    }
}

/// The kinds of annotation `op` acts on in `st`.
pub(crate) fn targets(op: &ResolvedOp, st: &DocState) -> Vec<Target> {
    match op {
        ResolvedOp::DeleteAnnotations { annots } | ResolvedOp::AdoptAnnotations { annots } => {
            annots.iter().filter_map(|&id| Target::of(st, id)).collect()
        }
        _ => Vec::new(),
    }
}

/// The capability an export's `transform` needs, beside what an op it
/// makes needs (an [`Transform::Op`]'s is its op's, checked as it's
/// lowered): leaving pages out is a delete's, which a plugin may make;
/// taking markup or sheaf's private data off changes what the user didn't
/// make in sheaf. One sheaf-edit adds needs the user's until it has a row.
pub fn transform_cap(transform: &Transform) -> Option<Cap> {
    match transform {
        Transform::Op(_) => None,
        Transform::KeepPages { .. } => Some(Cap::Plugin),
        Transform::RemoveMarkup { .. } | Transform::RemovePrivateData => Some(Cap::Approval),
        _ => Some(Cap::User),
    }
}

/// Whether `actor` may make an export's transform that needs `cap`
/// ([`transform_cap`]): the user and the command line any, a plugin those
/// a plugin may, and the jobs that make one kind of edit none.
pub fn actor_exports(actor: &Actor, cap: Cap) -> bool {
    match actor {
        Actor::User | Actor::Cli | Actor::Job(JobKind::Import | JobKind::Insert) => true,
        Actor::Job(JobKind::Ocr | JobKind::Trim) => false,
        Actor::Plugin { .. } => cap == Cap::Plugin,
    }
}

/// Whether `actor` may make `op`, which needs `cap`.
pub fn actor_allows(actor: &Actor, cap: Cap, op: &ResolvedOp) -> bool {
    match actor {
        Actor::User | Actor::Cli | Actor::Job(JobKind::Import | JobKind::Insert) => true,
        Actor::Job(JobKind::Ocr) => matches!(
            op,
            ResolvedOp::SetOcrText { .. } | ResolvedOp::ClearOcrText { .. }
        ),
        Actor::Job(JobKind::Trim) => matches!(op, ResolvedOp::SetPageBoxes { .. }),
        // A manifest grants nothing more until plugin API 0.4's
        // permissions (slice 20).
        Actor::Plugin { .. } => cap == Cap::Plugin,
    }
}
