//! Every text field of the corpus's forms filled, and the appearance sheaf
//! makes for each widget set beside the one PDFium makes for the same
//! value (M10 slice 6).
//!
//! **Each fill is a change.** sheaf makes an appearance only for a widget
//! whose field's value the model holds changed (`forms::appearance::stale`),
//! so filling a field with its own value would make nothing. So sheaf opens
//! the corpus file with a revision appended (`fixture::try_append_revision`)
//! in which every text field that holds a value has `/V ()`, and fills each
//! text field with a widget on a page with its sample (`fixture::sample`:
//! its own value, else a value of its kind, never past `/MaxLen`) through
//! the model, then saves the whole as a save writes a Rewrite (the
//! primary's objects at their own numbers, the record saying where each key
//! went). PDFium's side is the file with a revision in which each of those
//! fields' `/V` is the sample and their widgets have no `/AP`: PDFium opens
//! it under its form-fill environment, draws each page's widgets once
//! (`FPDF_FFLDraw`, which makes the appearance each lacks) and saves a copy
//! (`FPDF_SaveAsCopy`), whose appearances are its own, as the pixel checks
//! (`fields.rs`) have them. A damaged file (its cross-reference section not
//! where its trailer says) takes its revisions after sheaf's Rewrite of it,
//! on both sides ([`REWRITTEN_FIRST`]).
//!
//! **What the save holds,** read back by its record: every filled widget is
//! made (its `/AP` is `/N` alone, the form the save wrote for it under
//! `AnnotPart(aid, Ap)`, holding `/Tx BMC` .. `EMC`, its content the bytes
//! sheaf's maker gives for the widget's plain data) or unmade (no `/AP`, no
//! form), and the kind of reason why is counted ([`UNMADE`]); made and
//! unmade are the widgets filled. A widget that kept its file's appearance,
//! or holds another, fails. **An overlay of each page** (as the realizer
//! writes one, `realize::overlay_appearances`), with no drawing, holds for
//! each made widget a form equal to the save's in `/BBox`, `/Matrix`,
//! decoded content and resource names, and none for an unmade one: one
//! that differs fails.
//!
//! **The pixels:** each page with a made widget is drawn appearances only
//! from the save and from PDFium's copy, at two pixels a point, and the two
//! compared a widget at a time, inside its box. A widget whose box differs
//! by more than [`TOLERANCE`] is a row, with its facts ([`Facts`]): its plain
//! data and font, what each appearance shows text with, whether the two
//! forms draw alike within 0.001 pt (`common::drawn`) or what differs
//! first, the text each shows, and what the file says that PDFium reads
//! otherwise. Every row is in the first named class its facts meet
//! ([`class`]), each class's count held ([`CLASSES`]); a row in no class
//! fails. So is every form that can't be filled, by name
//! (`fields_unfilled.tsv`). No file is left out. Each form's PDFium part
//! runs in the forms' order ([`each_in_order`]): PDFium's caches are the
//! process's, so what it draws can turn on what it drew before, and in one
//! order it draws the same, run to run. Counts are held on a whole run; `SHEAF_FIELDS_ONLY=<corpus paths, comma-separated>` narrows it, and
//! then they're printed, not held; `SHEAF_FIELDS_ROWS=<dir>` keeps each
//! row's two appearances and its box drawn by each.
//!
//! PDFium is sheaf's linked build (`scripts/fetch-pdfium.sh`). Ignored,
//! about 20 minutes in release, most of it PDFium's: `just tier-fields`.
//! Skips without the corpus.

mod common;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use sheaf_doc::forms::appearance::{self, DaFont, Look, Slot, TextLook};
use sheaf_doc::forms::corpus::Corpus;
use sheaf_doc::forms::fixture::{sample, try_append_revision};
use sheaf_doc::forms::{Field, FieldId, FieldKind, FormClass, Value};
use sheaf_doc::geom::{Geom, Rect};
use sheaf_doc::materialize::key::Part;
use sheaf_doc::materialize::record::ObjMap;
use sheaf_doc::materialize::{
    ObjKey, Profile, SaveInput, SavePlan, SaveTarget, Strategy, materialize,
};
use sheaf_doc::model::{FieldNode, Terminal};
use sheaf_doc::pdf::content::{Res, ResBody};
use sheaf_doc::pdf::forms::{Appearance, FieldFont, Unmade};
use sheaf_doc::pdf::gfx::DefaultAppearance;
use sheaf_doc::pdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, StreamExt, literal};
use sheaf_doc::prim::{Key, Prim, Val};
use sheaf_doc::realize::{ViewInputs, overlay_appearances};
use sheaf_doc::store::BlobKind;
use sheaf_doc::{AnnotId, DocState, Document, PageId, Src, Store};
use sheaf_image::raster::Raster;
use sheaf_ops::NoProgress;
use sheaf_render::PageIndex;
use sheaf_render::pdfium::PdfiumEngine;

use common::drawn;
use common::fields::{differing_in, flat_at, live_at};

/// Corpus paths, comma-separated, to run on alone
/// (`SHEAF_FIELDS_ONLY=forms/irs/f1040.pdf,…`): its counts are then
/// printed, not held.
const ONLY: &str = "SHEAF_FIELDS_ONLY";

/// Pixels a point, as the pixel checks draw.
const SCALE: f32 = 2.0;

/// The pixels inside a widget's box ours may differ from PDFium's by: the
/// pixel checks' (`fields.rs`), measured there, none.
const TOLERANCE: usize = 0;

/// Whether a count of differing pixels is past [`TOLERANCE`].
fn past(count: usize) -> bool {
    count > TOLERANCE
}

/// A directory where each row's two appearances are kept, its content
/// decoded, as `<path>-<object>.sheaf` and `.pdfium` (the path's `/`s
/// `_`s): for reading what a class is. Unset, nothing is written.
const ROWS: &str = "SHEAF_FIELDS_ROWS";

/// How wide PDFium's page view is drawn to make the appearances its
/// widgets lack: what it makes doesn't depend on it.
const MAKING_WIDTH: u32 = 16;

/// Each class of rows past [`TOLERANCE`] ([`class`]), and how many widgets
/// it holds over the whole corpus: 13,254 of the 227,812 compared
/// ([`TOTALS`]).
const CLASSES: &[(&str, usize)] = &[
    (FALLBACK, 4538),
    (ALIKE, 2927),
    (ALIKE_EDGE, 2145),
    (CJK, 1865),
    (ALIKE_BESIDE, 1100),
    (TWO_FONTS, 233),
    (OWN_FONT, 131),
    (WIDGET_MAX_LEN, 91),
    (NAMELESS, 71),
    (FRACTION, 68),
    (CLIP, 50),
    (BEVEL_CMYK, 14),
    (EURO, 9),
    (FEWER, 4),
    (BORDER_ARRAY, 4),
    (OTHER_FIELD, 3),
    (TYPE_UP, 1),
];

/// How much of the corpus the tier filled, made and compared: held, so a
/// reader that finds fewer text widgets, a form that falls among those
/// with no text field, or a widget that drops out of the comparison (its
/// box shows nothing of the page) fails the tier, where [`CLASSES`] alone
/// would count the same rows and pass.
const TOTALS: Totals = Totals {
    forms: 4392,
    no_text: 1577,
    fields: 220_981,
    filled: 229_351,
    made: 228_842,
    two_fonts: 235,
    overlaid: 228_842,
    pages: 8434,
    compared: 227_812,
    off_page: 1030,
};

/// What [`TOTALS`] holds of a whole run.
#[derive(Debug, PartialEq)]
struct Totals {
    /// The corpus's forms.
    forms: usize,
    /// Those that open and have no text field with a widget on a page.
    no_text: usize,
    /// Text fields filled, and their widgets on pages.
    fields: usize,
    filled: usize,
    /// The widgets whose appearance sheaf made, those of them drawn in two
    /// fonts (the field's, and the fallback for what it can't draw), and
    /// how many of those forms an overlay holds as the save does: every
    /// one.
    made: usize,
    two_fonts: usize,
    overlaid: usize,
    /// Pages drawn; the widgets compared with PDFium's on them, and those
    /// left out because their box shows nothing of the page.
    pages: usize,
    compared: usize,
    off_page: usize,
}

/// The filled widgets sheaf makes no appearance for, by the kind of
/// reason (`appearance::unmade_kind`), over the whole corpus.
const UNMADE: &[(&str, usize)] = &[
    // A box of no width or height (494 of them 0 by 0), in the file.
    ("no-box", 497),
    // A character of the field's own value neither its font nor the
    // fallback font has (M10 slice 7, which draws 235 of the 247 that its
    // font alone couldn't in two fonts): a control character, nine of them
    // U+007F DELETE and two in U+0000..U+000F; and one of the Private Use
    // Area, in U+F0D0..U+F0DF. The tier prints each's range of sixteen.
    ("unencodable", 12),
];

/// The forms whose text fields can't be filled, a line each: its path and
/// why, a tab apart.
const UNFILLED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fields_unfilled.tsv");

/// The damaged forms (their cross-reference sections not where their
/// trailers say) whose revisions follow sheaf's Rewrite of the file as it
/// opens, at the same numbers: a section appended after one would name a
/// previous section that isn't there.
const REWRITTEN_FIRST: &[&str] = &[
    "forms/safedocs/0055306.pdf",
    "forms/safedocs/0214456.pdf",
    "forms/safedocs/0290608.pdf",
    "forms/safedocs/0896575.pdf",
    "forms/safedocs/1096333.pdf",
    "forms/safedocs/1127197.pdf",
    "forms/safedocs/1155692.pdf",
    "forms/safedocs/1819139.pdf",
    "forms/safedocs/2498253.pdf",
    "forms/safedocs/4422502.pdf",
    "forms/safedocs/5208359.pdf",
    "forms/safedocs/6561293.pdf",
    "forms/safedocs/6614490.pdf",
    "forms/safedocs/7334180.pdf",
];

/// PDFium makes no appearance: the field has no name, and PDFium finds a
/// widget's field by its name (pdf-forms' stated `forms-nameless-at-the-top`).
const NAMELESS: &str = "no name: PDFium makes none";
/// PDFium makes no appearance: the field's `/FT` is two levels up or
/// more, and PDFium makes a terminal field only where it's the field's own
/// or its parent's (pdf-forms' stated `forms-three-deep`).
const TYPE_UP: &str = "type two levels up: PDFium makes none";
/// Drawn in two fonts (M10 slice 7): what the field's font has no code
/// for in the fallback font sheaf bundles, which PDFium draws in no font a
/// caller gives, so its appearance of the same value is in the field's font
/// alone, or in a font its font map takes for the charset; what's in two
/// fonts has no appearance of PDFium's to be held to (pdf-forms holds it
/// so too): it's held by PDFium's text extraction of what sheaf writes
/// (`fields.rs`).
/// Two of the 235 drawn in two fonts draw as PDFium's: black text on a
/// black `/MK /BG` (`forms/safedocs/0577838.pdf`), and a zero width space
/// past an overflowing box's clip (`0970197.pdf`). Two rows drawn alike
/// beside one of them are [`ALIKE_BESIDE`]'s, as that one differs.
const TWO_FONTS: &str = "two fonts: PDFium draws in no font sheaf gives";
/// A `/DA` font `/DR` lacks falls back to the standard 14 family its name
/// gives; PDFium draws every such font in a Helvetica it adds (pdf-forms'
/// stated `ap-font-missing-*`).
const FALLBACK: &str = "missing font: its family, PDFium's Helvetica";
/// A border's width as `/BS /W` gives it, a fraction; PDFium cuts it to an
/// integer, for the border and the text box inside it (pdf-forms' stated
/// `ap-border-odd`).
const FRACTION: &str = "border width a fraction: PDFium's an integer";
/// The border's width from `/BS`; PDFium takes `/Border`'s first, its
/// third number (pdf-forms' stated `ap-border-array`).
const BORDER_ARRAY: &str = "border from /BS: PDFium's from /Border";
/// The field's text cut by PDFium at a `/MaxLen` of one of its widgets'
/// own, which PDFium's field takes where it has none of its own
/// (`CPDF_FormField::GetMaxLen`) and sheaf's reader, as ISO 32000-2 has
/// `/MaxLen` a field's, doesn't.
const WIDGET_MAX_LEN: &str = "a widget's /MaxLen: PDFium cuts at it";
/// A CJK font, Type 0 and not embedded, in which PDFium's font map draws
/// the text otherwise: Latin text in an ANSI font of `/DR`'s, or a
/// character in a font of its own; pdf-forms draws it in the font the
/// `/DA` names.
const CJK: &str = "CJK font: PDFium's font map";
/// PDFium's font map draws characters of the value in a font of its own,
/// which it takes for a charset other than the `/DA` font's:
/// DroidSansFallback, a face of the machine's, for the quotes and dashes
/// it counts as Chinese (‘ ’ “ ” – —); ChromeSansMM, its own, where the
/// machine has no face of the charset, for Eastern European letters (Š š
/// ž) and ƒ. Their heights move the lines after them; and where that font
/// has no code for one (a bullet, in DroidSansFallback's GBK), PDFium
/// writes its two UTF-16 bytes, which the font reads as two other
/// characters (a space and a quote). pdf-forms draws them in the `/DA`'s
/// font, which has them.
const OWN_FONT: &str = "PDFium's own font for some characters";
/// PDFium's appearance leaves out a character of the value that the font
/// has a code for, which pdf-forms draws (a no-break space, at WinAnsi's
/// 0xA0).
const FEWER: &str = "PDFium leaves out a character";
/// PDFium shows another value: it reads the widget as another field's
/// (the tree read otherwise: in `forms/safedocs/2429543.pdf`, widget kids
/// of a node that has field kids too, which PDFium loads as fields of
/// their own), and that field holds the file's value or another sample.
const OTHER_FIELD: &str = "PDFium shows another field's value";
/// A beveled border's dark band over a CMYK background: pdf-forms halves
/// the color and moves black halfway to full; PDFium halves it (pdf-forms'
/// stated `ap-border-beveled-colors`).
const BEVEL_CMYK: &str = "beveled over CMYK: the dark band";
/// The same glyphs, a centered line set further left or a line broken a
/// word sooner: a euro sign in a standard 14 font drawn without a program
/// or `/Widths`, which pdf-forms measures by Adobe's metrics (556 a
/// thousand in Helvetica) and PDFium by the face it draws in (667, and 722
/// in Helvetica-Bold and Times, whose metrics have 556 and 500): pdf-fonts
/// says its width is guessed (`Font::width_guessed`).
const EURO: &str = "a euro sign: PDFium's width";
/// Drawn alike, each glyph, path and clip within 0.001 pt, and another
/// made widget whose box meets this one's differs itself, and draws here.
const ALIKE_BESIDE: &str = "alike within 0.001 pt, beside a widget that differs";
/// Drawn alike within 0.001 pt, the pixels that differ all within two of
/// the box's edge: a border pdf-forms strokes and PDFium fills as an
/// even-odd ring, the same region, or a background or a clip that edge,
/// each rasterized otherwise where it falls within a pixel (PDFium takes a
/// pixel into a clip whole or not at all, so a clip's edge 0.00003 pt
/// apart flips a row of pixels).
const ALIKE_EDGE: &str = "alike within 0.001 pt, at the box's edge";
/// Drawn alike within 0.001 pt, pixels inside the box differing: PDFium's
/// own drawing of numbers pdf-forms writes to four places and PDFium as
/// `f32`s (and of a comb's cells placed by `Tm` and by summed `Td`s), a
/// glyph whose origin falls that near a half pixel landing a pixel apart,
/// or an edge's anti-aliasing a level or two.
const ALIKE: &str = "alike within 0.001 pt, a glyph a pixel apart";
/// Drawn alike but for a clip only sheaf's writes, which cuts a glyph's
/// box: pdf-forms always clips the text to the box less the border's
/// reach, as Acrobat writes it; PDFium clips only text that passes that box
/// (pdf-forms' stated `forms-ap clip`).
const CLIP: &str = "clip: sheaf's always, PDFium's where text passes";

/// The glyph a drawing's difference starts at and how far it moved, where
/// [`drawn::alike`] says one moved.
fn moved(drawn: &str) -> Option<(usize, f32, f32)> {
    let rest = drawn.strip_prefix("glyph")?;
    let (i, rest) = rest.split_once(":moved:")?;
    let (dx, dy) = rest.split_once(',')?;
    Some((i.parse().ok()?, dx.parse().ok()?, dy.parse().ok()?))
}

/// A row's class: what its difference from PDFium's is, by its facts, the
/// first of these that holds. None for a row no class explains, which
/// fails.
fn class(f: &Facts) -> Option<&'static str> {
    let cut_at = |n: i64| f.text_vs == format!("cut:{n}");
    let cjk = f.kind.starts_with("Type0(") && f.kind.ends_with("/None");
    let alike = f.drawn == "alike";
    let moved = moved(&f.drawn);
    let painted = f.drawn.starts_with("paint") && f.drawn.ends_with(":color");
    let own_font = ["DroidSansFallback", "ChromeSansMM"]
        .iter()
        .any(|font| f.pdfium_tf.contains(font));
    Some(match () {
        _ if f.pdfium_tf == "none" && f.nameless => NAMELESS,
        _ if f.pdfium_tf == "none" && f.ft_up.is_some_and(|up| up >= 2) => TYPE_UP,
        _ if f.fallback > 0 && f.frame == "alike" => TWO_FONTS,
        _ if f.font.starts_with("fallback:") && f.font != "fallback:Helvetica" => FALLBACK,
        _ if f.border.0.fract() != 0.0 => FRACTION,
        _ if f.border_array.is_some_and(|w| w as f32 != f.border.0) => BORDER_ARRAY,
        _ if f.widget_max_len.is_some_and(cut_at) => WIDGET_MAX_LEN,
        _ if cjk && !alike => CJK,
        _ if own_font && !alike => OWN_FONT,
        _ if f.text_vs == "fewer" && !f.ascii => FEWER,
        _ if f.text_vs == "other" => OTHER_FIELD,
        _ if painted && f.border.1 == 'B' && f.bg == "cmyk" => BEVEL_CMYK,
        _ if f.euro && f.kind == "Type1/None" && f.text_vs == "same" && moved.is_some() => EURO,
        _ if alike && f.overlap_differs => ALIKE_BESIDE,
        _ if alike && f.edge_only => ALIKE_EDGE,
        _ if alike => ALIKE,
        _ if f.drawn == "clip-cuts" => CLIP,
        _ => return None,
    })
}

/// What a row is, as its class is told: the widget's plain data, its font,
/// and what PDFium's appearance and sheaf's draw text with.
#[derive(Debug, Clone)]
struct Facts {
    /// The font its `/DA` names, as sheaf reads it from `/DR`: `ref`,
    /// `direct`, `direct-refs`, `fallback:<BaseFont>` where `/DR` lacks it,
    /// `none` where the `/DA` names none (Helvetica), `error:<kind>`.
    font: String,
    /// The font's kind and program, as pdf-fonts reads them.
    kind: String,
    /// The effective `/DA`, escaped.
    da: String,
    /// `/Ff`, `/Q` (0 to 2), `/MaxLen`.
    flags: u32,
    align: u8,
    max_len: Option<u32>,
    /// The box's width and height, `/MK /R`.
    width: f32,
    height: f32,
    rotation: u16,
    /// The border's width and style (`S`, `D`, `B`, `I`, `U`), its dashes.
    border: (f32, char),
    dash: Vec<f32>,
    /// `/MK /BG`'s color space (`-` for none, `g`, `rgb`, `cmyk`), and
    /// whether `/MK /BC` is given.
    bg: &'static str,
    bc: bool,
    /// sheaf's appearance's font size, and whether its text overflowed.
    size: f32,
    overflowed: bool,
    /// How many glyphs sheaf's appearance draws in the fallback font (M10
    /// slice 7), what the field's font can't: 0 for none.
    fallback: usize,
    /// What sheaf's appearance and PDFium's show text with: each `Tf`'s
    /// name and size, and for PDFium's each font's `/BaseFont`; `none`
    /// where PDFium made no appearance.
    ours_tf: String,
    pdfium_tf: String,
    /// The sample's length, in characters, whether it's ASCII, and whether
    /// it holds a euro sign.
    chars: usize,
    ascii: bool,
    euro: bool,
    /// Whether the two forms draw alike within a rounding
    /// ([`drawn::alike`]): `alike`, or the first thing that differs.
    drawn: String,
    /// Whether the two forms draw alike but for their text
    /// ([`drawn::frame_alike`]: the box, the matrix and the paths painted):
    /// `alike`, or the first thing that differs; `-` where either isn't
    /// read.
    frame: String,
    /// Its field's full name is empty: PDFium finds a widget's field by
    /// its name, and finds none for an empty one.
    nameless: bool,
    /// The widget's `/F`.
    annot_flags: u32,
    /// How many other made widgets' boxes meet its box (and draw in it),
    /// and whether any of them differs itself.
    overlaps: usize,
    overlap_differs: bool,
    /// The text PDFium's appearance shows beside sheaf's: `same`, `cut:<n>`
    /// (sheaf's first `n` characters), `fewer` (sheaf's with characters
    /// left out), `other`; `-` where either isn't read.
    text_vs: String,
    /// A `/MaxLen` of a widget's own in its field, which has none: what
    /// PDFium's field takes (`CPDF_FormField::GetMaxLen`) and sheaf's
    /// reader doesn't.
    widget_max_len: Option<i64>,
    /// The border's width as PDFium reads it from `/Border` (its third
    /// number, cut to an integer) where the widget has `/Border` and
    /// `/BS` both: PDFium takes `/Border` first, pdf-forms `/BS`.
    border_array: Option<i64>,
    /// How many levels up from its field's dictionary `/FT` is (0 for its
    /// own), as the file has it; none where none says. PDFium makes a
    /// field only where it's the field's own or its parent's.
    ft_up: Option<u32>,
    /// Its `/DA` asks for an automatic size (`0 Tf`, or none).
    auto: bool,
    /// Every pixel that differs is within two of the box's edge; and the
    /// most any differs by, in one channel's levels.
    edge_only: bool,
    levels: u8,
}

impl std::fmt::Display for Facts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dash: Vec<String> = self.dash.iter().map(f32::to_string).collect();
        write!(
            f,
            "font={} kind={} da={} ff={:x} q={} maxlen={} w={} h={} r={} bw={} bs={} dash={} \
             bg={} bc={} size={} ovf={} fallback={} ours.tf={} pdfium.tf={} chars={} ascii={} \
             euro={} drawn={} frame={} \
             nameless={} f={} overlaps={} overlap_differs={} text={} wmaxlen={} border_array={} \
             ft_up={} auto={} edge_only={} levels={}",
            self.font,
            self.kind,
            self.da,
            self.flags,
            self.align,
            self.max_len.map_or("-".into(), |n| n.to_string()),
            self.width,
            self.height,
            self.rotation,
            self.border.0,
            self.border.1,
            if dash.is_empty() {
                "-".into()
            } else {
                dash.join(",")
            },
            self.bg,
            u8::from(self.bc),
            self.size,
            u8::from(self.overflowed),
            self.fallback,
            self.ours_tf,
            self.pdfium_tf,
            self.chars,
            u8::from(self.ascii),
            u8::from(self.euro),
            self.drawn,
            self.frame,
            u8::from(self.nameless),
            self.annot_flags,
            self.overlaps,
            u8::from(self.overlap_differs),
            self.text_vs,
            self.widget_max_len.map_or("-".into(), |n| n.to_string()),
            self.border_array.map_or("-".into(), |n| n.to_string()),
            self.ft_up.map_or("-".into(), |n| n.to_string()),
            u8::from(self.auto),
            u8::from(self.edge_only),
            self.levels,
        )
    }
}

/// A widget whose box differs from PDFium's past [`TOLERANCE`].
struct Row {
    page: u32,
    object: ObjectId,
    px: usize,
    facts: Facts,
}

/// What became of one form.
#[derive(Default)]
struct Outcome {
    /// Why its text fields can't be filled, where they can't.
    unfilled: Option<String>,
    /// It has no text field with a widget on a page.
    no_text: bool,
    /// It's damaged: its revisions follow sheaf's Rewrite of it.
    rewritten_first: bool,
    /// What fails with no class.
    failed: Vec<String>,
    /// Text fields filled, and their widgets on pages.
    fields: usize,
    filled: usize,
    made: usize,
    unmade: BTreeMap<String, usize>,
    /// Of those made, the widgets drawn in two fonts: the field's and the
    /// fallback ([`Facts::fallback`]).
    two_fonts: usize,
    /// The first character no font draws of each widget unmade as
    /// `unencodable`, by the range of sixteen code points it's in (its
    /// number with the last hex digit 0): the block, never the value.
    unencodable: BTreeMap<u32, usize>,
    /// Made widgets whose forms an overlay and the save hold alike.
    overlaid: usize,
    /// Made widgets compared by pixels, and those whose box shows no pixel
    /// of the page drawn.
    compared: usize,
    off_page: usize,
    /// Pages drawn on each side.
    pages: usize,
    rows: Vec<Row>,
    /// How long it took, but for its wait for its turn at PDFium.
    took: Duration,
}

/// A filled text widget.
struct Filled {
    aid: AnnotId,
    page: PageId,
    /// Its page's index, in the file, its save and PDFium's copy.
    index: u32,
    field: FieldId,
    /// Its dictionary, at its own number in the file and PDFium's copy.
    object: ObjectId,
    /// Where the save wrote it, as its record says: its own number, or its
    /// field's where they're one object, or another where the save writes
    /// a field apart from its widgets (one a page lists twice).
    saved: Option<ObjectId>,
    rect: [f32; 4],
    look: TextLook,
    /// sheaf's appearance for it, made as the emitter makes it, or why
    /// none can be; and how many glyphs it draws in the fallback font.
    made: Result<Appearance, Unmade>,
    fallback: usize,
    /// Its font, as [`Facts::font`] and [`Facts::kind`] say it.
    font: (String, String),
    /// Its field's full name is empty; its `/F`.
    nameless: bool,
    annot_flags: u32,
    /// [`Facts::widget_max_len`], [`Facts::border_array`] and
    /// [`Facts::ft_up`].
    widget_max_len: Option<i64>,
    border_array: Option<i64>,
    ft_up: Option<u32>,
}

/// A field's text as the model reads it: none and an empty one alike.
fn text_of(value: Option<&Value>) -> &str {
    match value {
        Some(Value::Text(text)) => text,
        _ => "",
    }
}

/// Bytes with what isn't printable ASCII, and a backslash, as `\xNN`.
fn escaped(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b' '..=b'~' if b != b'\\' => out.push(b as char),
            b => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

/// What a panic said.
fn said(panic: &(dyn std::any::Any + Send)) -> String {
    match (panic.downcast_ref::<&str>(), panic.downcast_ref::<String>()) {
        (Some(s), _) => s.to_string(),
        (_, Some(s)) => s.clone(),
        _ => "a panic".into(),
    }
}

/// An error on one line, with what caused it.
fn line(e: &dyn std::fmt::Display) -> String {
    format!("{e:#}").replace(['\n', '\t'], " ")
}

/// sheaf's appearance for `look`, a widget of `src`'s, made as the emitter
/// makes one: in the font its `/DA` names in `src`'s `/DR`, named as the
/// emitter names it (its content doesn't depend on the number the font is
/// written at), and the fallback font for what that can't draw; how many
/// glyphs it draws in the fallback; and the font, as [`Facts`] says it.
fn made_by_sheaf(
    src: &Src,
    look: &TextLook,
) -> (Result<Appearance, Unmade>, usize, (String, String)) {
    let name = look.frame.font_name();
    let font = match appearance::font(src, name.as_deref()) {
        Ok(font) => font,
        Err(e) => {
            let kind = appearance::error_kind(&e);
            return (Err(e.into()), 0, (format!("error:{kind}"), "-".into()));
        }
    };
    let said = font_said(&font, look);
    let name = name.unwrap_or_default();
    let res = appearance::font_res(&font, src.id(), 0, &name, |_| ObjectId::new(1, 0));
    let made = appearance::make(&Look::Text(look.clone()), DaFont { font: &font, res });
    match made {
        Ok(made) => {
            let form = made.form(Slot::Normal).expect("a text widget's /N");
            (Ok(form.clone()), made.glyphs.len(), said)
        }
        Err(why) => (Err(why), 0, said),
    }
}

/// `font`, the one `look`'s `/DA` names, as [`Facts::font`] and
/// [`Facts::kind`] say it.
fn font_said(font: &FieldFont, look: &TextLook) -> (String, String) {
    let class = match font.res().as_ref().map(Res::body) {
        _ if look.frame.font_name().is_none() => "none".to_string(),
        _ if font.is_fallback() => {
            let base = match font.res().as_ref().map(Res::body) {
                Some(ResBody::Objects(objects)) => (objects.first())
                    .and_then(Object::as_dict)
                    .and_then(|d| d.get(b"BaseFont"))
                    .and_then(Object::as_name)
                    .map_or("-".into(), escaped),
                _ => "-".into(),
            };
            format!("fallback:{base}")
        }
        None => "direct-refs".into(),
        Some(ResBody::Written(_)) => "ref".into(),
        Some(ResBody::Objects(_)) => "direct".into(),
        Some(_) => "other".into(),
    };
    let f = font.font();
    let kind = format!("{:?}/{:?}", f.kind(), f.program());
    (class, kind.replace(' ', ""))
}

/// Each `Tf` in `content`: its name and size, `;` apart; with each name's
/// `/BaseFont` where `fonts` (a form's `/Resources /Font`, in `pdf`) names
/// it.
fn tfs(content: &[u8], fonts: Option<(&Pdf, &Dictionary)>) -> String {
    let text = String::from_utf8_lossy(content);
    let tokens: Vec<&str> = text.split_ascii_whitespace().collect();
    let mut said = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if *t != "Tf" || i < 2 {
            continue;
        }
        let (name, size) = (tokens[i - 2], tokens[i - 1]);
        let base = fonts.and_then(|(pdf, fonts)| {
            let font = fonts.get(name.trim_start_matches('/').as_bytes())?;
            let font = match font.as_reference() {
                Some(id) => pdf.get_object(id).ok()?,
                None => font,
            };
            let base = font.as_dict()?.get(b"BaseFont")?.as_name()?;
            Some(escaped(base))
        });
        said.push(match base {
            Some(base) => format!("{name}:{size}:{base}"),
            None => format!("{name}:{size}"),
        });
    }
    match said.is_empty() {
        true => "-".into(),
        false => said.join(";"),
    }
}

/// `o`, followed where it's a reference, in `pdf`.
fn reached<'p>(pdf: &'p Pdf, o: &'p Object) -> Option<&'p Object> {
    match o.as_reference() {
        Some(id) => pdf.get_object(id).ok(),
        None => Some(o),
    }
}

/// A widget's `/AP` in a file: its keys, and the object its `/N` names and
/// that object's stream, where it names one.
struct SavedAp {
    keys: Vec<Vec<u8>>,
    n: Option<ObjectId>,
    form: Option<Stream>,
}

/// The `/AP` of the widget whose dictionary is object `widget` of `pdf`;
/// none where it has none. An error where the object isn't a dictionary.
fn saved_ap(pdf: &Pdf, widget: ObjectId) -> anyhow::Result<Option<SavedAp>> {
    let dict = (pdf.get_dictionary(widget)).with_context(|| format!("widget {widget:?}"))?;
    let Some(ap) = dict.get(b"AP").and_then(|ap| reached(pdf, ap)) else {
        return Ok(None);
    };
    let Some(ap) = ap.as_dict() else {
        return Ok(None);
    };
    let n = ap.get(b"N");
    Ok(Some(SavedAp {
        keys: ap.iter().map(|(k, _)| k.to_vec()).collect(),
        n: n.and_then(Object::as_reference),
        form: n.and_then(|n| reached(pdf, n)?.as_stream().cloned()),
    }))
}

/// A form as the overlay check compares it: its box, its matrix, its
/// content decoded, and the names its `/Resources` binds, by kind.
#[derive(Debug, PartialEq)]
struct Form {
    bbox: Option<Vec<f64>>,
    matrix: Option<Vec<f64>>,
    content: Vec<u8>,
    names: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
}

impl Form {
    /// `form`, whose `/Resources` is a direct dictionary of direct ones,
    /// as a made form's is.
    fn of(form: &Stream) -> anyhow::Result<Form> {
        let numbers = |key: &[u8]| -> Option<Vec<f64>> {
            let items = form.dict.get(key)?.as_array()?;
            items.iter().map(Object::as_number).collect()
        };
        let resources = (form.dict.get(b"Resources").and_then(Object::as_dict)).cloned();
        let names = (resources.iter().flat_map(|r| r.iter()))
            .map(|(kind, entries)| {
                let names = (entries.as_dict().iter().flat_map(|d| d.iter()))
                    .map(|(name, _)| name.to_vec())
                    .collect();
                (kind.to_vec(), names)
            })
            .collect();
        Ok(Form {
            bbox: numbers(b"BBox"),
            matrix: numbers(b"Matrix"),
            content: form.decompressed_content().context("a made form decodes")?,
            names,
        })
    }
}

/// The widget's box in a page drawn as `r`, of a page whose geometry is
/// `geom`: its left, top, right and bottom pixels, the last two past it,
/// clamped to the drawing.
fn pixel_box(geom: &Geom, rect: [f32; 4], r: &Raster) -> [usize; 4] {
    let shown = geom.shown();
    let (sx, sy) = (
        r.width as f32 / shown.width.max(1.0),
        r.height as f32 / shown.height.max(1.0),
    );
    let b = (geom.to_view()).bounds(Rect::new(rect[0], rect[1], rect[2], rect[3]));
    let at = |v: f32, max: u32| (v.round().max(0.0) as usize).min(max as usize);
    [
        at(b.x0 * sx, r.width),
        at(b.y0 * sy, r.height),
        at(b.x1 * sx, r.width),
        at(b.y1 * sy, r.height),
    ]
}

/// Object `id` of `pdf`'s dictionary, where it's one.
fn dict_mut(pdf: &mut Pdf, id: ObjectId) -> Option<&mut Dictionary> {
    pdf.objects.get_mut(&id).and_then(Object::as_dict_mut)
}

/// One form's turn at PDFium in a sweep's order ([`each_in_order`]).
struct Turn<'a> {
    at: usize,
    next: &'a (Mutex<usize>, Condvar),
    taken: Cell<bool>,
    /// How long it waited for its turn.
    waited: Cell<Duration>,
}

impl Turn<'_> {
    /// Waits until every form before this one has had its turn: PDFium is
    /// this form's alone until [`pass`](Self::pass).
    fn take(&self) {
        if self.taken.replace(true) {
            return;
        }
        let started = Instant::now();
        let (next, cv) = self.next;
        let mut n = next.lock().unwrap_or_else(PoisonError::into_inner);
        while *n != self.at {
            n = cv.wait(n).unwrap_or_else(PoisonError::into_inner);
        }
        self.waited.set(started.elapsed());
    }

    /// The next form's turn, this one's taken first where it wasn't.
    fn pass(&self) {
        self.take();
        let (next, cv) = self.next;
        *next.lock().unwrap_or_else(PoisonError::into_inner) = self.at + 1;
        cv.notify_all();
    }
}

/// Passes a [`Turn`] when it goes, a panic's unwinding among the ways.
struct Passing<'a>(&'a Turn<'a>);

impl Drop for Passing<'_> {
    fn drop(&mut self) {
        self.0.pass();
    }
}

/// `check` of each of `paths`, in their order, on a thread per core (16 at
/// most), each one's turn at PDFium ([`Turn::take`]) in the paths' order.
/// What PDFium draws can turn on what it drew before (its caches, of
/// glyphs among them, are the process's): drawn in the same order, the
/// same files draw the same, run to run, and the counts held hold.
fn each_in_order<T: Send>(paths: &[String], check: impl Fn(&str, &Turn<'_>) -> T + Sync) -> Vec<T> {
    let queue = Mutex::new(paths.iter().enumerate());
    let next = (Mutex::new(0), Condvar::new());
    let outcomes = Mutex::new(BTreeMap::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(16));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some((at, path)) =
                        queue.lock().unwrap_or_else(PoisonError::into_inner).next()
                    else {
                        break;
                    };
                    let turn = Turn {
                        at,
                        next: &next,
                        taken: Cell::new(false),
                        waited: Cell::new(Duration::ZERO),
                    };
                    let passing = Passing(&turn);
                    let outcome = check(path, &turn);
                    drop(passing);
                    let mut all = outcomes.lock().unwrap_or_else(PoisonError::into_inner);
                    all.insert(at, outcome);
                }
            });
        }
    });
    let all = outcomes
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    all.into_values().collect()
}

/// The form at `path`, filled and compared, its turn at PDFium `turn`.
fn check(corpus: &Corpus, path: &str, turn: &Turn<'_>) -> Outcome {
    let started = Instant::now();
    let mut out = Outcome::default();
    let done = catch_unwind(AssertUnwindSafe(|| {
        fill_and_compare(corpus, path, turn, &mut out)
    }));
    match done {
        Ok(Ok(())) => {}
        Ok(Err(e)) => out.failed.push(line(&e)),
        Err(panic) => out.failed.push(format!("panicked: {}", said(&*panic))),
    }
    out.took = started.elapsed().saturating_sub(turn.waited.get());
    out
}

/// [`check`]'s work.
fn fill_and_compare(
    corpus: &Corpus,
    path: &str,
    turn: &Turn<'_>,
    out: &mut Outcome,
) -> anyhow::Result<()> {
    let bytes = std::fs::read(corpus.root.join(path))?;
    let store = Store::new();
    let kind = BlobKind::file(Path::new(path), None);
    let file = match store.open(bytes.clone().into(), kind) {
        Ok(src) => src,
        Err(e) => {
            out.unfilled = Some(format!("doesn't open: {}", line(&e)));
            return Ok(());
        }
    };
    // A damaged file's revisions follow sheaf's Rewrite of it as it opens.
    let (bytes, file) = match file.base_sound() {
        true => (bytes, file),
        false => {
            out.rewritten_first = true;
            let d = common::ready(store.clone(), file, path).context("the damaged form opens")?;
            let (rewritten, _) = saved_as_saves_are(&d, d.state())?;
            let kind = BlobKind::file(Path::new(path), None);
            let src = (store.open(rewritten.clone().into(), kind)).context("its Rewrite opens")?;
            (rewritten, src)
        }
    };
    let form = file.form();
    if let Some(why) = &form.facts.damaged {
        out.unfilled = Some(format!("its field tree is damaged: {why}"));
        return Ok(());
    }
    let on_pages: HashSet<FieldId> = form.widgets.values().flatten().map(|w| w.field).collect();
    let texts: Vec<&Field> = (form.fields.iter())
        .filter(|f| f.kind == FieldKind::Text && on_pages.contains(&f.id))
        .collect();
    if texts.is_empty() {
        if form.facts.class == FormClass::XfaOnly {
            out.unfilled = Some("XFA only: no text field of the AcroForm's on a page".into());
        }
        out.no_text = true;
        return Ok(());
    }
    // Each text field's sample, by its dictionary; and those that hold a
    // value, which sheaf's revision empties.
    let own = |f: &Field| text_of(f.value.as_ref()).to_string();
    let samples: HashMap<ObjectId, String> = (texts.iter())
        .map(|f| (f.object, sample(&own(f), f.flags, f.max_len)))
        .collect();
    let emptied: Vec<ObjectId> = (texts.iter())
        .filter(|f| !own(f).is_empty())
        .map(|f| f.object)
        .collect();
    let emptying = |pdf: &mut Pdf| {
        for &id in &emptied {
            if let Some(d) = dict_mut(pdf, id) {
                d.set("V", literal(""));
            }
        }
    };
    let ours_file = match try_append_revision(&bytes, &emptied, emptying) {
        Ok(bytes) => bytes,
        Err(e) => {
            out.unfilled = Some(format!("takes no revision: {}", line(&e)));
            return Ok(());
        }
    };
    let store = Store::new();
    let kind = BlobKind::file(Path::new(path), None);
    let src = (store.open(ours_file.into(), kind)).context("the revision emptied opens")?;
    let d = common::ready(store, src, path).context("the revision emptied opens")?;

    // The fill, through the model: each text field with a widget on a
    // page holds its sample.
    let mut st = d.state().clone();
    let mut fields: BTreeMap<FieldId, ObjectId> = BTreeMap::new();
    for (id, node, term) in d.state().terminals() {
        let Some(r) = node.src.as_ref().filter(|_| term.kind == FieldKind::Text) else {
            continue;
        };
        let Some(sample) = samples.get(&r.id) else {
            continue;
        };
        let before = d.state().field(id).expect("a field of the state").clone();
        let after = FieldNode {
            term: Some(std::sync::Arc::new(Terminal {
                value: Some(Value::Text(sample.as_str().into())),
                ..Terminal::clone(term)
            })),
            ..FieldNode::clone(node)
        };
        let prim = Prim {
            key: Key::Field(id),
            before: Some(Val::Field(before)),
            after: Some(Val::Field(std::sync::Arc::new(after))),
        };
        st.apply(&prim)?;
        fields.insert(id, r.id);
    }
    out.fields = fields.len();
    let mut widgets = filled(&st, &fields, out)?;
    out.filled = widgets.len();

    // The save, read back: made or unmade, each.
    let (saved, map) = saved_as_saves_are(&d, &st)?;
    let saved_pdf = Pdf::load_mem(&saved).context("the save reloads")?;
    let revision: HashSet<u32> = d.primary().pdf().ids().map(|id| id.number).collect();
    let written = |key: ObjKey| map.keys.get(&key).copied();
    let mut forms: HashMap<AnnotId, Form> = HashMap::new();
    for w in &mut widgets {
        let at = format!(
            "page {}, widget {} {}",
            w.index + 1,
            w.object.number,
            w.object.generation
        );
        w.saved = written(ObjKey::Annot(w.aid)).or_else(|| written(ObjKey::Field(w.field)));
        let Some(number) = w.saved else {
            out.failed.push(format!("{at}: not in the save's record"));
            continue;
        };
        let ap = saved_ap(&saved_pdf, number)?;
        let made_at = written(ObjKey::AnnotPart(w.aid, Part::Ap));
        match (&w.made, ap) {
            (Err(why), None) if made_at.is_none() => {
                *out.unmade.entry(appearance::unmade_kind(why)).or_default() += 1;
                if let Unmade::Unencodable(c) = why {
                    *out.unencodable
                        .entry(u32::from(c.character) & !0xf)
                        .or_default() += 1;
                }
            }
            (Err(why), _) => out.failed.push(format!(
                "{at}: an appearance in the save, where none can be made ({})",
                appearance::unmade_kind(why)
            )),
            (Ok(_), None) => out.failed.push(format!("{at}: no appearance in the save")),
            (Ok(made), Some(ap)) => {
                let kept = ap.n.is_some_and(|n| revision.contains(&n.number));
                let form = ap.form.as_ref().map(Form::of).transpose()?;
                let content = form.as_ref().map(|f| f.content.as_slice());
                let marked = content.is_some_and(|c| {
                    c.windows(7).any(|w| w == b"/Tx BMC") && c.ends_with(b"EMC\n")
                });
                match () {
                    _ if kept => out.failed.push(format!("{at}: kept its file's appearance")),
                    _ if made_at.is_none() || ap.n != made_at => out.failed.push(format!(
                        "{at}: its /AP /N isn't the form the save made for it"
                    )),
                    _ if ap.keys != [b"N".to_vec()] => {
                        out.failed.push(format!("{at}: an /AP of more than /N"))
                    }
                    _ if content != Some(made.content.as_slice()) || !marked => out
                        .failed
                        .push(format!("{at}: an appearance other than sheaf makes")),
                    _ => {
                        out.made += 1;
                        out.two_fonts += usize::from(w.fallback > 0);
                        forms.insert(w.aid, form.expect("a form"));
                    }
                }
            }
        }
    }

    // An overlay of each page holds the save's forms.
    let pages: BTreeMap<u32, PageId> = widgets.iter().map(|w| (w.index, w.page)).collect();
    let inputs = ViewInputs::screen();
    for (&index, &page) in &pages {
        let made_here: HashMap<AnnotId, &Filled> = (widgets.iter())
            .filter(|w| w.page == page)
            .map(|w| (w.aid, w))
            .collect();
        let mut overlay: HashMap<AnnotId, Stream> = overlay_appearances(&st, page, &inputs)?
            .into_iter()
            .collect();
        for (aid, w) in &made_here {
            let at = format!("page {}, widget {}", index + 1, w.object.number);
            match (forms.get(aid), overlay.remove(aid)) {
                (Some(saved), Some(over)) => match Form::of(&over)? == *saved {
                    true => out.overlaid += 1,
                    false => out
                        .failed
                        .push(format!("{at}: the overlay's form isn't the save's")),
                },
                (None, None) => {}
                (Some(_), None) => out.failed.push(format!("{at}: the overlay makes none")),
                (None, Some(_)) if w.made.is_err() => out
                    .failed
                    .push(format!("{at}: the overlay makes one, the save none")),
                // A widget the save failed on, said already.
                (None, Some(_)) => {}
            }
        }
        for aid in overlay.keys() {
            out.failed.push(format!(
                "page {}: the overlay makes a form for {aid:?}, not filled",
                index + 1
            ));
        }
    }

    let theirs = Theirs {
        bytes: &bytes,
        samples: &samples,
        fields: &fields,
    };
    pixels(path, &theirs, &d, (&saved, &saved_pdf), &widgets, turn, out)
}

/// `st`, a state of `d`'s, Rewritten as a save writes one: the primary's
/// objects at their own numbers, encrypted as it is, nothing asked; and the
/// save's record of where it wrote each key.
fn saved_as_saves_are(
    d: &Document,
    st: &DocState,
) -> anyhow::Result<(Vec<u8>, std::sync::Arc<ObjMap>)> {
    let mut snap = d.snapshot();
    snap.state = st.clone();
    let input = SaveInput::detached(&snap);
    let plan = SavePlan {
        strategy: Strategy::Rewrite { why: Vec::new() },
        asks: Vec::new(),
        transforms: Vec::new(),
        notes: Vec::new(),
    };
    let target = SaveTarget {
        path: Path::new("fields.pdf").into(),
        same_file: false,
        profile: Profile::default(),
        stamp: None,
    };
    let out =
        materialize(&input, &plan, target, &NoProgress).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok((out.bytes, out.record.map.clone()))
}

/// What PDFium's side is made from: the file, each text field's sample by
/// its dictionary, and the fields filled.
struct Theirs<'a> {
    bytes: &'a [u8],
    samples: &'a HashMap<ObjectId, String>,
    fields: &'a BTreeMap<FieldId, ObjectId>,
}

/// The text widgets of `st` (a document's state, filled) whose fields `fields`
/// holds, on its pages in order, each page's in `/Annots` order, with
/// sheaf's appearance for each. A widget that isn't stale fails.
fn filled(
    st: &DocState,
    fields: &BTreeMap<FieldId, ObjectId>,
    out: &mut Outcome,
) -> anyhow::Result<Vec<Filled>> {
    let mut widgets = Vec::new();
    for (index, &page) in st.order().iter().enumerate() {
        for (aid, w) in st.widgets_on(page) {
            if !fields.contains_key(&w.field) {
                continue;
            }
            let r = (w.src.as_ref()).context("a widget read from its file")?;
            if !appearance::stale(st, aid) {
                out.failed.push(format!(
                    "page {}, widget {}: filled, and not stale",
                    index + 1,
                    r.id.number
                ));
                continue;
            }
            let Ok(Look::Text(look)) = appearance::look(st, aid) else {
                anyhow::bail!("a text widget's look");
            };
            let (made, fallback, font) = made_by_sheaf(&r.src, &look);
            widgets.push(Filled {
                aid,
                page,
                index: index as u32,
                field: w.field,
                object: r.id,
                saved: None,
                nameless: st.full_name(w.field).is_none_or(|n| n.is_empty()),
                annot_flags: w.flags,
                widget_max_len: widget_max_len(st, w.field),
                border_array: border_array(r),
                ft_up: ft_up(st, w.field),
                rect: w.rect,
                look,
                made,
                fallback,
                font,
            });
        }
    }
    Ok(widgets)
}

/// A `/MaxLen` of one of field `field`'s widgets' own, the first in the
/// field's order, where the field has none as sheaf reads it.
fn widget_max_len(st: &DocState, field: FieldId) -> Option<i64> {
    let term = st.field(field)?.term.as_deref()?;
    if term.props.max_len.is_some() {
        return None;
    }
    (term.widgets.iter()).find_map(|&aid| {
        let r = st.annot(aid)?.widget()?.src.as_ref()?;
        let dict = r.src.pdf().get_dictionary(r.id).ok()?;
        dict.get(b"MaxLen")?.as_integer()
    })
}

/// The border width PDFium reads from widget `r`'s `/Border` (its third
/// number, cut to an integer; 0 where it's missing), where the widget has
/// `/BS` too.
fn border_array(r: &sheaf_doc::ObjRef) -> Option<i64> {
    let pdf = r.src.pdf();
    let dict = pdf.get_dictionary(r.id).ok()?;
    dict.get(b"BS")?;
    let border = dict.get(b"Border")?;
    let border = match border.as_reference() {
        Some(id) => pdf.get(id).ok()?,
        None => border,
    };
    let width = border.as_array()?.get(2).and_then(Object::as_number);
    Some(width.map_or(0, |w| w.trunc() as i64))
}

/// How many levels up from field `field`'s dictionary, in its file, `/FT`
/// is: [`Facts::ft_up`].
fn ft_up(st: &DocState, field: FieldId) -> Option<u32> {
    let r = st.field(field)?.src.as_ref()?;
    let pdf = r.src.pdf();
    let mut at = r.id;
    for up in 0..32 {
        let dict = pdf.get_dictionary(at).ok()?;
        if dict.has(b"FT") {
            return Some(up);
        }
        at = dict.get(b"Parent")?.as_reference()?;
    }
    None
}

/// Whether every pixel that differs between `a` and `b` inside `px` lies
/// within two of its edge; and the most one differs by, in a channel.
fn where_they_differ(a: &Raster, b: &Raster, px: [usize; 4]) -> (bool, u8) {
    let [l, t, r, bottom] = px;
    let w = a.width as usize;
    let (mut edge, mut levels) = (true, 0u8);
    for y in t..bottom {
        for x in l..r {
            let i = (y * w + x) * 4;
            let most = (0..3)
                .map(|c| a.data[i + c].abs_diff(b.data[i + c]))
                .max()
                .unwrap_or(0);
            if most == 0 {
                continue;
            }
            levels = levels.max(most);
            let near = x < l + 2 || y < t + 2 || x + 2 >= r || y + 2 >= bottom;
            edge &= near;
        }
    }
    (edge, levels)
}

/// How the text `theirs` shows stands to `ours`: [`Facts::text_vs`].
fn text_vs(ours: &str, theirs: &str) -> String {
    let mut left = ours.chars();
    let fewer = theirs.chars().all(|c| left.any(|o| o == c));
    match () {
        _ if ours == theirs => "same".into(),
        _ if ours.starts_with(theirs) => format!("cut:{}", theirs.chars().count()),
        _ if fewer => "fewer".into(),
        _ => "other".into(),
    }
}

/// The pixels: PDFium's appearances made for the same values, and each
/// page with a made widget drawn appearances only from `saved` and from
/// PDFium's copy, each made widget's box compared.
fn pixels(
    path: &str,
    theirs: &Theirs<'_>,
    d: &Document,
    (saved, saved_pdf): (&[u8], &Pdf),
    widgets: &[Filled],
    turn: &Turn<'_>,
    out: &mut Outcome,
) -> anyhow::Result<()> {
    let Theirs {
        bytes,
        samples,
        fields,
    } = *theirs;
    let made: Vec<&Filled> = widgets.iter().filter(|w| w.made.is_ok()).collect();
    if made.is_empty() {
        return Ok(());
    }
    // PDFium's revision: each filled field's `/V` its sample, each filled
    // widget with no `/AP`.
    let valued: BTreeSet<ObjectId> = fields.values().copied().collect();
    let bare: BTreeSet<ObjectId> = widgets.iter().map(|w| w.object).collect();
    let ids: Vec<ObjectId> = valued.union(&bare).copied().collect();
    let theirs_file = try_append_revision(bytes, &ids, |pdf| {
        for &id in &valued {
            if let Some(d) = dict_mut(pdf, id) {
                d.set("V", sheaf_doc::pdf::text::string(&samples[&id]));
            }
        }
        for &id in &bare {
            if let Some(d) = dict_mut(pdf, id) {
                d.remove(b"AP");
            }
        }
    })
    .context("PDFium's revision")?;
    let pages: BTreeMap<u32, PageId> = made.iter().map(|w| (w.index, w.page)).collect();
    // PDFium's from here, in the sweep's order.
    turn.take();
    let copy = {
        let engine = PdfiumEngine::open_bytes(theirs_file.into(), None)?;
        for &index in pages.keys() {
            live_at(&engine, PageIndex(index), MAKING_WIDTH)?;
        }
        engine.save_copy()?
    };
    let copy_pdf = Pdf::load_mem(&copy).context("PDFium's copy reloads")?;
    let ours_engine = PdfiumEngine::open_bytes(saved.to_vec().into(), None)?;
    let theirs_engine = PdfiumEngine::open_bytes(copy.into(), None)?;
    for (&index, &page) in &pages {
        let geom = d.geom(page);
        let width = (geom.shown().width * SCALE).round().max(1.0) as u32;
        let ours_r = flat_at(&ours_engine, PageIndex(index), width)?;
        let theirs_r = flat_at(&theirs_engine, PageIndex(index), width)?;
        out.pages += 1;
        if (ours_r.width, ours_r.height) != (theirs_r.width, theirs_r.height) {
            out.failed
                .push(format!("page {}: drawn at other sizes", index + 1));
            continue;
        }
        // Each made widget's box, and how many of its pixels differ.
        let here: Vec<(&Filled, [usize; 4], usize)> = (made.iter())
            .filter(|w| w.index == index)
            .filter_map(|w| {
                let px = pixel_box(&geom, w.rect, &ours_r);
                let shown = px[0] < px[2] && px[1] < px[3];
                shown.then(|| (*w, px, differing_in(&ours_r, &theirs_r, px)))
            })
            .collect();
        out.off_page += made.iter().filter(|w| w.index == index).count() - here.len();
        out.compared += here.len();
        let meet =
            |a: [usize; 4], b: [usize; 4]| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
        for (i, &(w, px, differ)) in here.iter().enumerate() {
            if !past(differ) {
                continue;
            }
            // The other made widgets whose boxes meet this one's, which draw
            // in it too, and whether any of them differs itself.
            let others: Vec<&(&Filled, [usize; 4], usize)> = (here.iter().enumerate())
                .filter(|&(j, o)| j != i && meet(px, o.1))
                .map(|(_, o)| o)
                .collect();
            let mut facts = facts_of(w, saved_pdf, &copy_pdf);
            (facts.edge_only, facts.levels) = where_they_differ(&ours_r, &theirs_r, px);
            facts.overlaps = others.len();
            facts.overlap_differs = others.iter().any(|o| past(o.2));
            keep_row(path, w, &copy_pdf, [&ours_r, &theirs_r], px)?;
            out.rows.push(Row {
                page: index + 1,
                object: w.object,
                px: differ,
                facts,
            });
        }
    }
    Ok(())
}

/// Widget `w`'s two appearances, its content decoded, kept where [`ROWS`]
/// says: sheaf's, and PDFium's read from `copy`; and `drawn`'s two
/// drawings of its box `px`, sheaf's and PDFium's, side by side with the
/// pixels that differ red, a PPM.
fn keep_row(
    path: &str,
    w: &Filled,
    copy: &Pdf,
    drawn: [&Raster; 2],
    px: [usize; 4],
) -> anyhow::Result<()> {
    let Some(dir) = std::env::var_os(ROWS) else {
        return Ok(());
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir)?;
    let name = format!("{}-{}", path.replace('/', "_"), w.object.number);
    let made = w.made.as_ref().expect("a made widget");
    std::fs::write(dir.join(format!("{name}.sheaf")), &made.content)?;
    let theirs = saved_ap(copy, w.object)?.and_then(|ap| ap.form);
    let content = match theirs {
        Some(form) => form.decompressed_content()?,
        None => b"(none)".to_vec(),
    };
    std::fs::write(dir.join(format!("{name}.pdfium")), content)?;
    let [l, t, r, b] = px;
    let (bw, bh) = (r - l, b - t);
    let mut ppm = format!("P6 {} {} 255\n", bw * 3, bh).into_bytes();
    let at = |d: &Raster, x: usize, y: usize| {
        let i = (y * d.width as usize + x) * 4;
        [d.data[i], d.data[i + 1], d.data[i + 2]]
    };
    for y in t..b {
        for side in 0..3 {
            for x in l..r {
                let [a, c] = drawn.map(|d| at(d, x, y));
                let p = match side {
                    0 => a,
                    1 => c,
                    _ if a == c => [255, 255, 255],
                    _ => [255, 0, 0],
                };
                ppm.extend(p);
            }
        }
    }
    std::fs::write(dir.join(format!("{name}.ppm")), ppm)?;
    Ok(())
}

/// Whether `look`'s `/DA` asks for an automatic size: `0 Tf`, or no size.
fn auto_size(look: &TextLook) -> bool {
    let limits = appearance::limits();
    let da = DefaultAppearance::parse(&look.frame.da, &limits.annots.content);
    da.is_none_or(|da| da.font_size.is_none_or(|size| size == 0.0))
}

/// What a row of widget `w` is, sheaf's appearance read from `saved`, the
/// save, and PDFium's from `copy`.
fn facts_of(w: &Filled, saved: &Pdf, copy: &Pdf) -> Facts {
    let made = w.made.as_ref().expect("a made widget");
    let pdfium = saved_ap(copy, w.object).ok().flatten();
    let ours = (w.saved).and_then(|at| saved_ap(saved, at).ok().flatten());
    let theirs = pdfium.as_ref().and_then(|ap| ap.form.as_ref());
    let said = |alike: Result<(), String>| alike.err().unwrap_or_else(|| "alike".to_string());
    let (alike, frame, text) = match (ours.and_then(|ap| ap.form), theirs) {
        (Some(a), Some(b)) => match (drawn::drawn(saved, &a), drawn::drawn(copy, b)) {
            (Ok(a), Ok(b)) => (
                said(drawn::alike(&a, &b)),
                said(drawn::frame_alike(&a, &b)),
                text_vs(a.text(), b.text()),
            ),
            (Err(e), _) | (_, Err(e)) => (
                format!("unread:{}", line(&e).replace(' ', "_")),
                "-".into(),
                "-".into(),
            ),
        },
        _ => ("none".to_string(), "-".to_string(), "-".to_string()),
    };
    let pdfium_tf = match pdfium.as_ref().and_then(|ap| ap.form.as_ref()) {
        None => "none".to_string(),
        Some(form) => {
            let fonts = (form.dict.get(b"Resources"))
                .and_then(|r| reached(copy, r))
                .and_then(Object::as_dict)
                .and_then(|r| r.get(b"Font"))
                .and_then(|f| reached(copy, f))
                .and_then(Object::as_dict);
            let content = form.decompressed_content().unwrap_or_default();
            tfs(&content, fonts.map(|f| (copy, f)))
        }
    };
    let look_frame = &w.look.frame;
    let style = match look_frame.border.style {
        sheaf_doc::forms::field::BorderStyle::Solid => 'S',
        sheaf_doc::forms::field::BorderStyle::Dashed => 'D',
        sheaf_doc::forms::field::BorderStyle::Beveled => 'B',
        sheaf_doc::forms::field::BorderStyle::Inset => 'I',
        sheaf_doc::forms::field::BorderStyle::Underline => 'U',
    };
    Facts {
        font: w.font.0.clone(),
        kind: w.font.1.clone(),
        da: escaped(&look_frame.da),
        flags: w.look.flags,
        align: w.look.align,
        max_len: w.look.max_len,
        width: look_frame.width,
        height: look_frame.height,
        rotation: look_frame.rotation,
        border: (look_frame.border.width, style),
        dash: look_frame.border.dash.clone(),
        bg: match look_frame.background {
            None => "-",
            Some(sheaf_doc::forms::field::Color::Gray(_)) => "g",
            Some(sheaf_doc::forms::field::Color::Rgb(_)) => "rgb",
            Some(sheaf_doc::forms::field::Color::Cmyk(_)) => "cmyk",
        },
        bc: look_frame.border_color.is_some(),
        size: made.font_size,
        overflowed: made.overflowed,
        fallback: w.fallback,
        ours_tf: tfs(&made.content, None),
        pdfium_tf,
        chars: w.look.text.chars().count(),
        ascii: w.look.text.is_ascii(),
        euro: w.look.text.contains('€'),
        drawn: alike,
        frame,
        nameless: w.nameless,
        annot_flags: w.annot_flags,
        overlaps: 0,
        overlap_differs: false,
        text_vs: text,
        widget_max_len: w.widget_max_len,
        border_array: w.border_array,
        ft_up: w.ft_up,
        auto: auto_size(&w.look),
        edge_only: false,
        levels: 0,
    }
}

/// Every form of the corpus, each text field filled: every filled widget
/// made or unmade by a kind, every overlay's forms the save's, and every
/// made widget drawn as PDFium draws its own appearance for the same value,
/// or a row of a named class, each class's count held.
#[test]
#[ignore = "corpus tier: just tier-fields"]
fn every_text_field_is_drawn_as_pdfium_draws_it_or_its_difference_is_classed() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let all: BTreeSet<String> = (corpus.forms().unwrap().into_iter())
        .map(|e| e.path)
        .collect();
    let narrowed = std::env::var_os(ONLY).is_some();
    let paths = common::narrowed(ONLY, all.into_iter().collect(), |p: &String| p.as_str());
    let started = Instant::now();
    let outcomes = each_in_order(&paths, |path, turn| check(&corpus, path, turn));

    let mut problems = Vec::new();
    let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unmade: BTreeMap<String, usize> = BTreeMap::new();
    let mut unfilled: BTreeMap<&str, &str> = BTreeMap::new();
    let mut rewritten_first: BTreeSet<&str> = BTreeSet::new();
    let (mut no_text, mut fields, mut filled, mut made, mut two_fonts) = (0, 0, 0, 0, 0);
    let mut unencodable: BTreeMap<u32, usize> = BTreeMap::new();
    let (mut overlaid, mut compared, mut off_page, mut pages, mut rows) = (0, 0, 0, 0, 0);
    for (path, o) in paths.iter().zip(&outcomes) {
        if let Some(why) = &o.unfilled {
            eprintln!("unfilled\t{path}\t{why}");
            unfilled.insert(path, why);
        }
        no_text += usize::from(o.no_text);
        if o.rewritten_first {
            eprintln!("rewritten first\t{path}");
            rewritten_first.insert(path);
        }
        for f in &o.failed {
            problems.push(format!("{path}: {f}"));
        }
        fields += o.fields;
        filled += o.filled;
        made += o.made;
        overlaid += o.overlaid;
        compared += o.compared;
        off_page += o.off_page;
        pages += o.pages;
        two_fonts += o.two_fonts;
        for (kind, n) in &o.unmade {
            *unmade.entry(kind.clone()).or_default() += n;
        }
        for (range, n) in &o.unencodable {
            *unencodable.entry(*range).or_default() += n;
        }
        let unmade_here: usize = o.unmade.values().sum();
        if o.made + unmade_here != o.filled {
            problems.push(format!(
                "{path}: {} filled, {} made and {unmade_here} unmade",
                o.filled, o.made
            ));
        }
        for r in &o.rows {
            rows += 1;
            let name = class(&r.facts);
            let shown = name.unwrap_or("(no class)");
            eprintln!(
                "row\t{path}\t{}\t{} {}\t{} px\t{shown}\t{}",
                r.page, r.object.number, r.object.generation, r.px, r.facts
            );
            match name {
                Some(name) => *classes.entry(name).or_default() += 1,
                None => problems.push(format!(
                    "{path}: page {}, widget {}: {} px, in no class",
                    r.page, r.object.number, r.px
                )),
            }
        }
    }
    let mut slowest: Vec<(Duration, &String)> =
        outcomes.iter().map(|o| o.took).zip(paths.iter()).collect();
    slowest.sort_by_key(|&(took, _)| std::cmp::Reverse(took));
    for (took, path) in slowest.iter().take(10) {
        eprintln!("slow\t{path}\t{:.1} s", took.as_secs_f64());
    }
    eprintln!(
        "tier-fields: {} forms; {} can't be filled, {no_text} have no text field on a page, {} \
         damaged ones' revisions follow sheaf's Rewrite; {fields} text fields filled, {filled} \
         widgets: {made} made, {} unmade {unmade:?}",
        paths.len(),
        unfilled.len(),
        rewritten_first.len(),
        unmade.values().sum::<usize>(),
    );
    eprintln!(
        "tier-fields: {overlaid} made forms an overlay holds as the save does; {pages} pages \
         drawn, {compared} widgets compared ({off_page} more show nothing of the page), {rows} \
         past {TOLERANCE} px, by class {classes:?}"
    );
    let ranges: Vec<String> = (unencodable.iter())
        .map(|(range, n)| format!("U+{range:04X}..U+{:04X} {n}", range + 0xf))
        .collect();
    eprintln!(
        "tier-fields: {two_fonts} made in two fonts; unmade as unencodable, by where the first \
         character no font draws is: {}",
        ranges.join(", ")
    );
    eprintln!("tier-fields: {:.0} s", started.elapsed().as_secs_f64());
    if narrowed {
        eprintln!("tier-fields: narrowed by {ONLY}: its counts are printed, not held");
    } else {
        let totals = Totals {
            forms: paths.len(),
            no_text,
            fields,
            filled,
            made,
            two_fonts,
            overlaid,
            pages,
            compared,
            off_page,
        };
        if totals != TOTALS {
            problems.push(format!("the totals {totals:?}, held {TOTALS:?}"));
        }
        let held: BTreeMap<&str, usize> = CLASSES.iter().copied().collect();
        if classes != held {
            problems.push(format!("the classes' counts {classes:?}, held {held:?}"));
        }
        let held: BTreeMap<String, usize> =
            UNMADE.iter().map(|&(k, n)| (k.to_string(), n)).collect();
        if unmade != held {
            problems.push(format!("the unmade {unmade:?}, held {held:?}"));
        }
        let listed = std::fs::read_to_string(UNFILLED).expect("the forms not filled");
        let held: BTreeMap<&str, &str> = (listed.lines())
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .map(|l| l.split_once('\t').expect("a path and why"))
            .collect();
        for (path, why) in unfilled.iter().filter(|(p, w)| held.get(*p) != Some(*w)) {
            problems.push(format!("{path} can't be filled ({why}), and isn't held so"));
        }
        for (path, why) in held.iter().filter(|(p, w)| unfilled.get(*p) != Some(*w)) {
            problems.push(format!(
                "{path} is held as not filled ({why}): now it's filled, or not for that"
            ));
        }
        let held: BTreeSet<&str> = REWRITTEN_FIRST.iter().copied().collect();
        if rewritten_first != held {
            problems.push(format!(
                "damaged, rewritten first {rewritten_first:?}, held {held:?}"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
