//! Why an op wasn't applied (M10.5 §6.4), and what a commit tells the
//! reader beside it (§6.5): fail, never repair.

use std::fmt;
use std::sync::Arc;

use super::caps::Cap;
use crate::content::ContentError;
use crate::model::VersionId;
use crate::prim::Key;

/// Why an op, a commit, or a move through history didn't happen. `op`
/// names the op as Op JSON does (`delete-pages`). Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// Something it names isn't in the document: a page, a bookmark, an
    /// annotation, a local, a blob or a stamp's page.
    #[error("{op}: {what} isn't there")]
    NotFound { op: &'static str, what: String },
    /// It can't be done as said.
    #[error("{op}: {why}")]
    Invalid { op: &'static str, why: String },
    /// A name it gives fits more than one thing.
    #[error("{op}: {what} names more than one")]
    Ambiguous { op: &'static str, what: String },
    /// A page's content it needs can't be read, or can't be changed as it
    /// asks without changing what else the page draws (§13): the page's
    /// place in the document, from 0, and why, which names it as the
    /// reader counts ([`ContentError`]).
    #[error("{op}: {why}")]
    Content {
        op: &'static str,
        page: u32,
        why: String,
    },
    /// The actor may not make this change.
    #[error("{0}")]
    Refused(Cap),
    /// The file is still opening: its facts aren't read yet, so there's
    /// nothing to edit (M10.5 proposal 28).
    #[error("the file is still opening")]
    NotReady,
    /// Its progress was canceled.
    #[error("canceled")]
    Cancelled,
    /// A transaction made on an earlier version depends on slot `key`,
    /// which no longer holds what it held there, or would break a rule
    /// once its changes are put on the current version (§8.5): changed
    /// last by the version labelled `by`, of those on the way to the
    /// current one, when one is known.
    #[error("{} changed the same data", changed_by(.by))]
    Conflict { key: Key, by: Option<Arc<str>> },
    /// A commit in a group whose edits were undone, or jumped away from,
    /// or which has ended (§8.4): what it adds is never put back.
    #[error("the edits it adds to were undone")]
    GroupEnded,
    /// A version's prims don't fit the state they were made from: a bug,
    /// never the reader's. The version can't be rebuilt.
    #[error("History is damaged at version {}", .version.0)]
    Mismatch { version: VersionId },
    /// A bug in sheaf, such as a cascade that doesn't settle. Debug builds
    /// stop where it's made ([`EditError::internal`]).
    #[error("internal error: {what}")]
    Internal { what: String },
}

/// Who made a [`EditError::Conflict`]'s change, as its text says.
fn changed_by(by: &Option<Arc<str>>) -> String {
    match by {
        Some(label) => format!("“{label}”"),
        None => "another edit".into(),
    }
}

impl EditError {
    /// An [`Internal`](EditError::Internal) error: a debug build panics
    /// with `what`.
    pub(crate) fn internal(what: impl Into<String>) -> EditError {
        let what = what.into();
        debug_assert!(false, "{what}");
        EditError::Internal { what }
    }

    pub(crate) fn not_found(op: &'static str, what: impl fmt::Display) -> EditError {
        EditError::NotFound {
            op,
            what: what.to_string(),
        }
    }

    pub(crate) fn invalid(op: &'static str, why: impl fmt::Display) -> EditError {
        EditError::Invalid {
            op,
            why: why.to_string(),
        }
    }

    /// A [`Content`](EditError::Content) error: `err`, which names a page
    /// of its source, about the page at place `page` of the document.
    pub(crate) fn content(op: &'static str, page: u32, err: ContentError) -> EditError {
        EditError::Content {
            op,
            page,
            why: err.on_page(page).to_string(),
        }
    }
}

impl From<&EditError> for sheaf_edit::Failure {
    /// What a plugin is told of it: its kind, and why, as the user is
    /// told.
    fn from(e: &EditError) -> sheaf_edit::Failure {
        use sheaf_edit::FailureKind as K;
        let kind = match e {
            EditError::NotFound { .. } => K::NotFound,
            EditError::Invalid { .. } => K::Invalid,
            EditError::Ambiguous { .. } => K::Ambiguous,
            EditError::Content { .. } => K::Content,
            EditError::Refused(_) => K::Refused,
            EditError::NotReady => K::NotReady,
            EditError::Cancelled => K::Cancelled,
            EditError::Conflict { .. } => K::Conflict,
            EditError::GroupEnded => K::GroupEnded,
            EditError::Mismatch { .. } => K::Mismatch,
            EditError::Internal { .. } => K::Internal,
        };
        sheaf_edit::Failure {
            kind,
            message: e.to_string(),
        }
    }
}

/// What a commit tells the reader beside its lines (§6.5): the texts M10
/// slices 4–5 pinned, and text the built-in fonts can't draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// An inserted file's fields went under a new parent, `under`, since
    /// their names were taken; `scripted`, its scripts still name them as
    /// they were.
    FormRenamed {
        under: String,
        file: String,
        scripted: bool,
    },
    /// Duplicated pages' fields went under `under`, apart from the
    /// originals'.
    CopiesRenamed { under: String, scripted: bool },
    /// Signed fields came in from `file`: their signatures don't cover
    /// this document (M10 proposal 7).
    SignatureNotValidHere { file: String },
    /// The op turned the form's scripts off: they came in from more than
    /// one form (M10 proposal 5).
    ScriptsOff,
    /// Text it added draws characters the built-in fonts can't as `?`
    /// (until M10 slice 7). Told once a document ([`Document::commit`]).
    ///
    /// [`Document::commit`]: crate::model::Document::commit
    Unencodable,
    /// The recognized text an older sheaf wrote stays on `pages`, which
    /// name them as the reader does: its layer lost the `q` it was written
    /// with, so its `Q` ends the page's own. What a save says of such a
    /// layer beside text it takes out, for a page it takes nothing out of.
    OcrStays { pages: String },
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let still = |f: &mut fmt::Formatter<'_>, scripted: bool| match scripted {
            true => f.write_str("; its scripts still use the old names"),
            false => Ok(()),
        };
        match self {
            Notice::FormRenamed {
                under,
                file,
                scripted,
            } => {
                write!(
                    f,
                    "fields from {file} are under “{under}”, since their names were taken"
                )?;
                still(f, *scripted)
            }
            Notice::CopiesRenamed { under, scripted } => {
                write!(
                    f,
                    "the copies' fields are under “{under}”, apart from the originals"
                )?;
                still(f, *scripted)
            }
            Notice::SignatureNotValidHere { file } => {
                write!(f, "the signature from {file} is no longer valid here")
            }
            Notice::ScriptsOff => {
                f.write_str("scripts are off: fields from more than one form have them")
            }
            Notice::Unencodable => f.write_str(
                "some characters can't be drawn in the built-in fonts and show as “?” in the \
                 file; the text itself is kept",
            ),
            Notice::OcrStays { pages } => write!(
                f,
                "recognized text sheaf wrote earlier stays on {pages}: taking it out would \
                 change what else the page draws"
            ),
        }
    }
}

/// `notices` as the reader is told them: each a sentence, capitalized, and
/// joined with ". ". None when there are none.
pub fn notice_text(notices: &[Notice]) -> Option<String> {
    if notices.is_empty() {
        return None;
    }
    let sentences: Vec<String> = notices
        .iter()
        .map(|n| {
            let text = n.to_string();
            let mut chars = text.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect();
    Some(sentences.join(". "))
}
