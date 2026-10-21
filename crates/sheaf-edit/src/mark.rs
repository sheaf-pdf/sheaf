//! Marks as an op makes and changes them: their kinds and shapes, their
//! style, and the comments an import brings with their own names.
//!
//! What the host owns isn't here (M10.5 §6.8): a mark's id and `/NM`, its
//! dates, a connector's partner, the comment a reply is to, and the stems
//! naming the stamp page or file it holds. sheaf-doc sets those as it
//! makes the mark, the same for every caller.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geom::{Box2, Pt};
use crate::measure::{Measured, Scale};
use crate::page::Rotation;
use crate::patch::Patch;
use crate::refs::{BlobSel, PageRef};
use crate::tape::{Tape, TapeLook};

/// A color: red, green and blue, 0–255.
pub type Color = [u8; 3];

/// A face text is drawn in: one of the standard 14 fonts' text faces,
/// which every reader has, so none is embedded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FontChoice {
    Helvetica,
    #[default]
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
}

/// How a mark is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub color: Color,
    /// 0–1.
    pub opacity: f32,
    /// Stroke width in points, where it applies.
    pub stroke: f32,
    /// Interior color (`/IC`), for kinds with an inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    /// A dashed stroke (`/BS /S /D`), for kinds with one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dashed: bool,
    /// A cloudy border (`/BE /S /C`): scallops along the outline of a
    /// rectangle, ellipse or polygon.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cloudy: bool,
}

/// What a mark is, and where it is on its page, in PageSpace: every kind
/// sheaf makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MarkKind {
    /// Text markup over glyph boxes.
    Highlight {
        rects: Vec<Box2>,
    },
    /// Sticky note: an icon at `pos` (its box's low corner), text in the
    /// mark's contents.
    Note {
        pos: Pt,
    },
    /// Text drawn in a box.
    FreeText {
        rect: Box2,
        size: f32,
    },
    Square {
        rect: Box2,
    },
    Ellipse {
        rect: Box2,
    },
    Line {
        a: Pt,
        b: Pt,
        /// An arrowhead at `b`.
        arrow: bool,
        /// An arrowhead at `a`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        arrow_start: bool,
    },
    Ink {
        paths: Vec<Vec<Pt>>,
    },
    /// A stamp from the tickmark library: `glyph` is what it shows.
    Stamp {
        rect: Box2,
        glyph: String,
    },
    /// PDFlyer-style connector: one end of a numbered group. Two ends made
    /// together ([`Op::AddConnector`](crate::Op::AddConnector)) link to
    /// each other.
    Connector {
        rect: Box2,
        label: String,
    },
    /// Calculator tape, as PDFlyer prints it: `tape`'s rows, `size` points
    /// tall, drawn on paper as `look` says.
    Tape {
        rect: Box2,
        size: f32,
        #[serde(default, skip_serializing_if = "Tape::is_empty")]
        tape: Tape,
        #[serde(default)]
        look: TapeLook,
    },
    /// Text markup: a line under the text.
    Underline {
        rects: Vec<Box2>,
    },
    /// Text markup: a line through the text.
    StrikeOut {
        rects: Vec<Box2>,
    },
    /// Text markup: a wavy line under the text.
    Squiggly {
        rects: Vec<Box2>,
    },
    /// A suggested replacement (Acrobat's Replace Text): the text `struck`,
    /// over `rects`, is crossed out, and a caret at `at` carries the new
    /// text in the mark's contents.
    Replace {
        rects: Vec<Box2>,
        struck: String,
        at: Box2,
    },
    /// A suggested insertion (Insert Text): a caret at `at`, the text in
    /// the mark's contents.
    Insert {
        at: Box2,
    },
    /// A reply, its text the mark's contents, in the thread of the comment
    /// it's made as a reply to.
    Reply {},
    /// A review state set on the comment it's made as a reply to: `model`
    /// is `Review` (`state` None, Accepted, Rejected, Cancelled or
    /// Completed) or `Marked` (the checkmark: Marked or Unmarked).
    Status {
        model: String,
        state: String,
    },
    /// A stamp from the library: the one-page PDF the mark's asset is,
    /// drawn in `rect`. `name` is what the library calls it, and `text` (a
    /// dynamic stamp's, filled in when placed) is drawn over it.
    PageStamp {
        rect: Box2,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    /// A text stamp in a frame, as Acrobat's dynamic stamps look: the first
    /// line large ("APPROVED"), the rest small ("GE 10/14/2026").
    TextStamp {
        rect: Box2,
        text: String,
        /// Placed by Set Status, so Reset Status can take it off.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        status: bool,
    },
    /// A border around a page, as Set Status draws it; `status` names the
    /// status, so Reset Status finds it.
    PageBorder {
        rect: Box2,
        status: String,
    },
    /// A closed shape through `points`.
    Polygon {
        points: Vec<Pt>,
    },
    /// An open line through `points`.
    Polyline {
        points: Vec<Pt>,
    },
    /// A text box (the mark's contents, `size` points) with a leader line
    /// from its edge through `knee` to an arrowhead at `point`: Acrobat's
    /// callout.
    Callout {
        rect: Box2,
        knee: Pt,
        point: Pt,
        size: f32,
    },
    /// The file the mark's asset is, attached at `pos` (Acrobat's Attach
    /// File), its icon's box's low corner as a note's is. `name` is the
    /// file's; `icon` is one of PushPin, Paperclip, Graph and Tag.
    FileAttachment {
        pos: Pt,
        icon: String,
        name: String,
    },
    /// A measurement (Acrobat's Measure tools): what `points` come to at
    /// `scale`; a distance has two.
    Measure {
        what: Measured,
        points: Vec<Pt>,
        scale: Box<Scale>,
    },
}

/// A mark to make. Its id, `/NM` and dates are the host's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewMark {
    pub kind: MarkKind,
    pub style: Style,
    /// Its text: a note's or a text box's, or a comment on any other kind.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub contents: String,
    /// Who it's by; none is whoever the host says makes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The rotation it reads upright in, absolute: the page's as shown
    /// for a mark made on it, the text's for markup on turned text.
    #[serde(default, skip_serializing_if = "is_upright")]
    pub turn: Rotation,
    /// What it holds: a page stamp's one-page PDF, an attachment's file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<BlobSel>,
}

fn is_upright(turn: &Rotation) -> bool {
    *turn == Rotation::R0
}

/// Changes to marks: each field named is set, the rest kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkPatch {
    /// Its kind and shape, whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MarkKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<f32>,
    #[serde(default, skip_serializing_if = "Patch::is_keep")]
    pub fill: Patch<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloudy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
}

/// A comment as another tool wrote it, brought in by an import: its own
/// `/NM`, dates and author, kept so a second import of the same comments
/// finds them and replies still thread by name. Only the user's own
/// edits and the import job may say this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Imported {
    pub nm: String,
    /// PDF date strings, as `D:20261011153000`.
    pub created: String,
    pub modified: String,
    pub author: String,
}

/// One end of a connector pair: where it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub page: PageRef,
    pub rect: Box2,
}
