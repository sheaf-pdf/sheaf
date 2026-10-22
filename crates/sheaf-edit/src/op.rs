//! Ops: edits as a caller states them (M10.5 §6.1).

use std::borrow::Cow;
use std::sync::Arc;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::geom::Vec2;
use crate::header_footer::HeaderFooterSpec;
use crate::ids::Local;
use crate::labels::PageLabelRange;
use crate::mark::{Anchor, Color, Imported, MarkPatch, NewMark, Style};
use crate::page::{BoxSpec, DestView, Rotation, TextStyle};
use crate::patch::Patch;
use crate::refs::{AnnotRef, BlobSel, BookmarkRef, PageRef, PageSel, RangeSet, ScopeSel};
use crate::text_layer::TextLayer;
use crate::watermark::{WatermarkSpec, Z};

/// An edit, as a caller states it ("delete pages 3–5"). A document lowers
/// it into changes of its model, inside a transaction; whatever it makes
/// it can name with locals, for the ops after it. In JSON, `"op"` names
/// it, kebab-case: `{"op": "delete-pages", "pages": {"positions": "3-5"}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Op {
    /// Move `pages`, kept in document order, to just before `before`, or
    /// to the end.
    MovePages {
        pages: PageSel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<PageRef>,
    },
    /// Turn `pages` from where each is.
    RotatePages { pages: PageSel, by: Turn },
    /// Delete `pages`, and what's only on them: their marks, fields and
    /// bookmarks (whose children move up).
    DeletePages { pages: PageSel },
    /// A copy of each page right after it; `locals` name the copies, in
    /// order.
    DuplicatePages {
        pages: PageSel,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locals: Vec<Local>,
    },
    /// Pages of `from` (`pages`, 1-based; none is all of them) before
    /// `before`, or at the end, with what `bring` says comes with them.
    /// `locals` name the new pages, in order.
    InsertPages {
        from: PageSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pages: Option<RangeSet>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<PageRef>,
        #[serde(default)]
        bring: Bring,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locals: Vec<Local>,
    },
    /// `pages` shown from `from`'s pages `with` (1-based; none is all of
    /// them), in place. The first as many as both have keep their ids,
    /// and so their bookmarks; extra pages are deleted, extra
    /// replacements inserted after.
    ReplacePages {
        pages: Vec<PageRef>,
        from: PageSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        with: Option<RangeSet>,
        #[serde(default)]
        bring: Bring,
    },
    /// The boxes of `pages`: a crop hides what's outside it and moves
    /// nothing. Cleared, the crop is the media box and the others are
    /// gone.
    SetPageBoxes {
        pages: PageSel,
        #[serde(default, skip_serializing_if = "Patch::is_keep")]
        crop: Patch<BoxSpec>,
        #[serde(default, skip_serializing_if = "Patch::is_keep")]
        trim: Patch<BoxSpec>,
        #[serde(default, skip_serializing_if = "Patch::is_keep")]
        bleed: Patch<BoxSpec>,
        #[serde(default, skip_serializing_if = "Patch::is_keep")]
        art: Patch<BoxSpec>,
    },
    /// The page labels, in place of any there are; none removes them.
    SetPageLabels {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ranges: Option<Vec<PageLabelRange>>,
    },
    /// A bookmark under `parent` (the top level when none), going to
    /// `target`, at `view` there (the top of the page when none).
    AddBookmark {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<BookmarkRef>,
        at: Sibling,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<PageRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<DestView>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<Color>,
        #[serde(default, skip_serializing_if = "TextStyle::is_plain")]
        style: TextStyle,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local: Option<Local>,
    },
    PatchBookmarks {
        bookmarks: Vec<BookmarkRef>,
        patch: BookmarkPatch,
    },
    /// Move `bookmarks` and what's under them under `parent` (the top
    /// level when none), in order. `with_pages` moves the pages each spans
    /// (from its page up to the next bookmark's) with it.
    MoveBookmarks {
        bookmarks: Vec<BookmarkRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<BookmarkRef>,
        at: Sibling,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        with_pages: bool,
    },
    /// Delete `bookmarks` and what's under them.
    DeleteBookmarks { bookmarks: Vec<BookmarkRef> },
    /// A mark on `page`; a reply or status in the thread of `reply_to`.
    /// `imported` keeps an imported comment's own name, dates and author.
    AddMark {
        page: PageRef,
        mark: NewMark,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<AnnotRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local: Option<Local>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        imported: Option<Imported>,
    },
    /// Two connectors labeled `label`, linked to each other: one at `a`,
    /// one at `b`. `locals` name them, `a`'s first.
    AddConnector {
        a: Anchor,
        b: Anchor,
        label: String,
        style: Style,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        locals: Vec<Local>,
    },
    /// Change `marks`: what `patch` names, the rest as they are.
    /// `imported` is an imported comment's name (its `/NM`, which must be
    /// the mark's), dates and author.
    PatchMarks {
        marks: Vec<AnnotRef>,
        patch: MarkPatch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        imported: Option<Imported>,
    },
    /// Move `marks` by `by`, onto `to_page` when one is given.
    MoveMarks {
        marks: Vec<AnnotRef>,
        by: Delta,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_page: Option<PageRef>,
    },
    /// Delete annotations, and their threads: replies and statuses, ours
    /// and other tools'. Marks, other tools' comments and links; a field's
    /// widget goes only with its page.
    DeleteAnnotations { annots: Vec<AnnotRef> },
    /// Make other tools' comments marks, editable here, keeping their ids
    /// and `/NM`s.
    AdoptAnnotations { annots: Vec<AnnotRef> },
    /// `page`'s OCR text, in place of any it has.
    SetOcrText {
        page: PageRef,
        layer: Arc<TextLayer>,
    },
    /// Take the OCR text off `pages`: what's not saved yet and what's in
    /// the file.
    ClearOcrText { pages: PageSel },
    /// The watermark (`z` over the pages' content) or background (under
    /// it) `spec` says, on the pages `scope` says, in place of the one of
    /// that `z` there is, and of those of its kind Acrobat or an earlier
    /// sheaf drew there. The author's own stay.
    SetWatermark {
        spec: WatermarkSpec,
        z: Z,
        scope: ScopeSel,
    },
    /// Take the watermark (`z` over), the background (under) or both (no
    /// `z`) off `pages`. `include_authored` takes the author's own off too:
    /// those the program that made the file drew.
    RemoveWatermark {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        z: Option<Z>,
        pages: PageSel,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        include_authored: bool,
    },
    /// The header and footer, on the pages `scope` says, in place of any
    /// there is, and of those Acrobat or an earlier sheaf drew there. The
    /// author's own stay.
    SetHeaderFooter {
        spec: HeaderFooterSpec,
        scope: ScopeSel,
    },
    /// Take the header and footer off `pages`, and those Acrobat, an older
    /// sheaf or another file drew there. `include_authored` takes the
    /// author's own off too: those the program that made the file drew.
    RemoveHeaderFooter {
        pages: PageSel,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        include_authored: bool,
    },
}

/// A turn of a page from where it is, clockwise. In JSON, degrees: 90,
/// 180 or 270, or -90 for a quarter turn back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    Quarter,
    Half,
    Back,
}

impl Turn {
    /// The turn that adds `rotation`: none for none.
    pub fn by(rotation: Rotation) -> Option<Turn> {
        match rotation {
            Rotation::R0 => None,
            Rotation::R90 => Some(Turn::Quarter),
            Rotation::R180 => Some(Turn::Half),
            Rotation::R270 => Some(Turn::Back),
        }
    }

    /// The rotation it adds to a page's.
    pub fn rotation(self) -> Rotation {
        match self {
            Turn::Quarter => Rotation::R90,
            Turn::Half => Rotation::R180,
            Turn::Back => Rotation::R270,
        }
    }
}

impl Serialize for Turn {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i32(self.rotation().degrees())
    }
}

impl<'de> Deserialize<'de> for Turn {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Turn, D::Error> {
        match i64::deserialize(d)? {
            90 => Ok(Turn::Quarter),
            180 => Ok(Turn::Half),
            270 | -90 => Ok(Turn::Back),
            n => Err(de::Error::invalid_value(
                Unexpected::Signed(n),
                &"a turn in degrees: 90, 180, 270 or -90",
            )),
        }
    }
}

impl JsonSchema for Turn {
    fn schema_name() -> Cow<'static, str> {
        "Turn".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Degrees clockwise from where the page is; -90 is a quarter turn back.",
            "enum": [90, 180, 270, -90],
        })
    }
}

/// Where among its siblings a bookmark goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Sibling {
    First,
    Last,
    /// Right after this one, which must be among them.
    After(BookmarkRef),
}

/// How far marks move: in PageSpace, or as a view of their page shows it
/// (x right, y down), which each mark's page turns into PageSpace.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum Delta {
    Page(Vec2),
    View(Vec2),
}

/// Changes to bookmarks: each field named is set or cleared, the rest kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BookmarkPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Where it goes; a new page is at its top unless `view` says.
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub target: Patch<PageRef>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub view: Patch<DestView>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub color: Patch<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<TextStyle>,
}

/// Where inserted pages come from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PageSource {
    /// A PDF: `file`, by the `name` the user picked it by, which renames
    /// and notices say (never a name the bytes came with).
    File { file: BlobSel, name: String },
}

/// What comes in with inserted pages besides the pages. Other tools'
/// annotations and links always come.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bring {
    /// sheaf's marks on them.
    #[serde(default = "yes")]
    pub marks: bool,
    #[serde(default)]
    pub form: FormBring,
    #[serde(default)]
    pub outline: OutlineBring,
}

fn yes() -> bool {
    true
}

impl Default for Bring {
    fn default() -> Bring {
        Bring {
            marks: true,
            form: FormBring::default(),
            outline: OutlineBring::default(),
        }
    }
}

/// What becomes of the file's form fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FormBring {
    /// A form of their own, renamed apart where names clash.
    #[default]
    New,
}

/// What becomes of the file's bookmarks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OutlineBring {
    /// They're left out.
    None,
    /// A top-level bookmark whose title one at the top here has adds its
    /// children to that one; the rest go at the end.
    #[default]
    MergeByTitle,
}
