//! Transforms: what an export does to the document it writes (M10.5 §6.1,
//! §12). Accepted only by an export, never by an edit: an export lowers
//! them on a transaction of its own that's never committed, so nothing a
//! transform does is the document's. In JSON, outside the envelope, the
//! transform names itself: `{"keep-pages": {...}}`, `"remove-private-data"`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::op::Op;
use crate::refs::PageSel;

/// A step of an export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
#[non_exhaustive]
// An export has a few: an op held as it is costs nothing worth a box.
#[allow(clippy::large_enum_variant)]
pub enum Transform {
    /// An edit, as a commit would lower it: `flatten-annotations` for
    /// print and Export Flattened, a bookmark for a rejected page.
    Op(Op),
    /// Only `pages`, in document order, and what's on them: the other
    /// pages go as a delete takes them, their marks, comments and the
    /// fields shown only there with them. A connector whose other end
    /// goes keeps no partner; a generator stays on the pages it covers,
    /// their Bates numbers as they were. `outline` says what becomes of
    /// the bookmarks, `labels` whether the page labels stay, each range
    /// starting at the first page of it kept, and `numbering` what a
    /// header's page tokens say of the pages kept.
    KeepPages {
        pages: PageSel,
        outline: OutlinePolicy,
        labels: bool,
        numbering: PageNumbering,
    },
    /// Every comment and mark taken off, sheaf's and other tools', and
    /// what else the pages show that's neither a link nor a widget; the
    /// links and the form's widgets stay as `keep` says.
    RemoveMarkup { keep: MarkupKeep },
    /// None of sheaf's private data (`private::KEYS`): what sheaf marks
    /// what it writes with.
    RemovePrivateData,
}

/// What becomes of the bookmarks when pages are left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OutlinePolicy {
    /// A bookmark stays while its page does; one whose page goes, or
    /// that goes nowhere in the file, stays as a heading, going nowhere,
    /// when one under it stays, and goes otherwise.
    Headings,
    /// As a delete leaves them: a bookmark whose page goes goes, the ones
    /// under it taking its place.
    Promote,
    /// None.
    Drop,
}

/// What a header and footer's page tokens say of the pages an export
/// keeps: where each is (`<<1>>`), how many pages there are (`<<N>>`) and
/// its label (`<<label>>`). Bates numbers (`<<bates>>`) are fixed a page
/// when they're set, and say what they said either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PageNumbering {
    /// As the file they make numbers them: pages 5 and 6 of 10 kept say
    /// "Page 1 of 2" and "Page 2 of 2", and the labels the file keeps
    /// (`labels`). An extract's, a split's part's.
    AsKept,
    /// As the document numbers them: pages 5 and 6 of 10 kept say "Page 5
    /// of 10" and "Page 6 of 10", and the document's labels, whether the
    /// file keeps labels or not. A print's, which is pages of the document.
    AsDocument,
}

/// What a markup removal keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkupKeep {
    /// Other tools' links (sheaf's connectors are marks, and go).
    pub links: bool,
    /// The form's widgets, and the fields they show.
    pub widgets: bool,
}

impl MarkupKeep {
    /// Links and widgets: a clean copy's, and print's without markup.
    pub const LINKS_AND_WIDGETS: MarkupKeep = MarkupKeep {
        links: true,
        widgets: true,
    };
}
