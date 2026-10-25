//! A file's fonts as read (M11 slice 3), for the content interpreter: the
//! codes a string is in one, the CID and text each code is, how far each
//! advances and how high its glyphs reach, read as PDFium reads them, so
//! a glyph's box is where PDFium draws it. The program a font embeds is
//! read for what only it says, each glyph's outline box and the heights a
//! descriptor lacks ([`program`]); what's known of neither is guessed, and
//! the font says what it guessed ([`FontApprox`]). A predefined CMap is
//! Adobe's, as hayro-cmap bundles it, split as PDFium splits it
//! ([`predefined`]).
//!
//! Every metric is in glyph units: thousandths of text space at a font
//! size of 1, as `/Widths` gives them; a Type3 font's are its glyph space
//! through its `/FontMatrix`. What PDFium reads as whole numbers (widths,
//! `Ascent` and `Descent`, a `/FontBBox`) is read whole, cut to its whole
//! part, as PDFium cuts it.
//!
//! Every array is read within its own length: a `/W` range of 2³¹ CIDs
//! is one range, as it's written, not 2³¹ widths. What a font's streams
//! decompress to (its `/ToUnicode`, its CMap's chain, its CharProcs, its
//! program and its `/CIDToGIDMap`) is charged to a budget it's read
//! within, a page's in the interpreter, so a page's fonts are bounded with
//! its content ([`Foreign::read`], [`FontScope::font`]).

use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use lopdf::{Dictionary, Object, ObjectId, dictionary};

use super::encoding::{Encoding, glyph_text};
use super::predefined::{self, Collection, Split};
use super::program::{self, CidGlyphs, EncodingObject, Face, FontProgram, Simple};
use super::{FontMetrics, Std14};
use crate::content::lex::{self, LexError, ResCx, Tok};
use crate::content::page::{ReadObjects, direct};
use crate::geom::{Affine, Rect};
use crate::source::Limits;

/// What kind of font a font dictionary is: its `/Subtype`, and a Type0
/// font's descendant's. A subtype PDFium doesn't know reads as Type1, as
/// in PDFium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    Type1,
    MMType1,
    TrueType,
    Type3,
    Type0(Descendant),
}

/// A Type0 font's CIDFont: its glyphs CFF's, or TrueType's. Any other
/// subtype reads as CIDFontType2, as in PDFium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Descendant {
    CidFontType0,
    CidFontType2,
}

/// What of a font's metrics is guessed: each is exact where it's false.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FontApprox {
    /// Its codes' CIDs may not be PDFium's: a predefined CMap that sheaf
    /// hasn't or PDFium doesn't know, its codes split by its `/ToUnicode`
    /// codespace and their CIDs unknown, or none; or an embedded CMap that
    /// uses a predefined one but Identity, which sheaf reads and PDFium
    /// doesn't, or whose chain sheaf can't read whole.
    pub codes: bool,
    /// Its widths: a simple font without `/Widths` that isn't one of the
    /// standard 14 drawn without a program, or a Type0 font without a
    /// CIDFont.
    pub widths: bool,
    /// Its ascent and descent: PDFium takes them from the face it draws
    /// in, one it substitutes, which may be the system's, as no descriptor
    /// gives them with a `/FontBBox` and the font embeds no program sheaf
    /// reads them from (a Type3 font: no `/FontBBox`, or an `A` or `g`
    /// whose `d1` gives no box).
    pub heights: bool,
    /// Its glyphs' outlines, so a glyph's box ([`Glyph::quad`]) may not
    /// hold its outline as PDFium's does: it embeds no program sheaf read
    /// (none, or one read for its text alone, [`Fonts::for_text`]), as for
    /// the standard 14, which PDFium draws in a face of its own or the
    /// system's; a program sheaf can't read ([`FontProgram::Malformed`]);
    /// or one whose glyphs' boxes it can't say as PDFium's: a tricky
    /// TrueType font, which FreeType draws hinted, one whose own matrix
    /// turns its glyphs, or a Type 1 font whose charstrings sheaf can't
    /// find as read-fonts reads them. In a Type3 font, one of whose
    /// glyphs' CharProc's `d1` gives no box, and that has no `/FontBBox`.
    /// A glyph's ([`Glyph::approx`]) is also set where the program doesn't
    /// say its code's outline: which glyph the code draws rests on what
    /// sheaf doesn't model, or drawing it passes its bound.
    ///
    /// [`Glyph::quad`]: crate::content::interp::Glyph::quad
    /// [`Glyph::approx`]: crate::content::interp::Glyph::approx
    pub boxes: bool,
}

impl FontApprox {
    pub fn any(self) -> bool {
        self.codes || self.widths || self.heights || self.boxes
    }
}

/// How a code's glyph is placed when the font writes vertically (PDF
/// 32000-1 9.7.4.3), in glyph units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerticalMetrics {
    /// How far it advances down the line: negative.
    pub w1: f32,
    /// Where its vertical origin is from its horizontal one.
    pub vx: f32,
    pub vy: f32,
}

/// The ascent and descent a font that says nothing of its heights is
/// taken to have: an em above the baseline and a quarter below, where
/// most fonts' glyphs fit.
const GUESSED: FontMetrics = FontMetrics {
    ascent: 1000.0,
    descent: -250.0,
    bbox: None,
};

/// A file's font, as read.
#[derive(Debug)]
pub struct Foreign {
    /// The object it's read from; None for one written in the resources
    /// as a dictionary.
    id: Option<ObjectId>,
    kind: FontKind,
    codes: Codes,
    widths: Widths,
    /// Where it writes vertically: `/W2` by CID, and `/DW2`.
    vertical: Option<VerticalWidths>,
    metrics: FontMetrics,
    /// A simple font's glyphs' outline boxes by code, where sheaf knows
    /// them: a Type3 font's, and a standard 14 font's drawn without a
    /// program.
    boxes: Option<Box<[Option<Rect>; 256]>>,
    /// What sheaf read of its program.
    program: FontProgram,
    /// The boxes PDFium gives its glyphs from its program
    /// ([`Foreign::char_box`]).
    outlines: Outlines,
    /// The codes whose widths are guessed where its others' aren't
    /// ([`Foreign::width_guessed`]), a bit each.
    guessed: [u64; 4],
    /// The most any of its metrics is from 0, in glyph units: widths,
    /// vertical advances and origins, ascent and descent; infinite where
    /// one isn't finite ([`Foreign::extent`]).
    extent: f32,
    /// What reading its streams decompressed, in bytes: its `/ToUnicode`,
    /// its CMap's chain and its CharProcs, each once.
    cost: usize,
    decoder: Decoder,
    /// A Type0 font's codes' text where it has no `/ToUnicode`.
    cid_text: Option<CidText>,
    approx: FontApprox,
}

/// The boxes a font's program gives its glyphs, as PDFium has them.
#[derive(Debug)]
enum Outlines {
    None,
    /// A simple font's, by code: None for a code sheaf can't say of.
    Codes(Box<[Option<Rect>; 256]>),
    /// A CIDFont's, by CID, drawn as they're asked for.
    Glyphs(Box<CidGlyphs>),
}

/// A font whose streams decompress past what's left of the budget it's
/// read within ([`Foreign::read`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a font's streams decompress past what's left to decompress")]
pub struct TooLarge;

/// How a font's strings are its codes, and its codes CIDs.
#[derive(Debug)]
enum Codes {
    /// A byte each: a simple font.
    One,
    /// Two bytes each, each its CID: Identity-H or Identity-V.
    Identity,
    /// By an embedded CMap's codespace, to its CIDs.
    Embedded(CidMap),
    /// By a predefined CMap's scheme, as PDFium reads it, to the CIDs of
    /// Adobe's CMap of its name.
    Predefined {
        map: &'static hayro_cmap::CMap,
        scheme: predefined::Scheme,
    },
    /// As the decoder splits them, CIDs unknown.
    Unknown,
}

/// A Type0 font's codes' text where it has no `/ToUnicode`, as PDFium
/// reads it (`CPDF_CIDFont::GetUnicodeFromCharCode`): a UCS2 or UTF16
/// CMap's codes are their own; Identity's and another predefined CMap's
/// CIDs, of a collection of Adobe's, are what that collection's UCS2 CMap
/// maps them to, the collection the predefined CMap's or, for Identity,
/// what its CIDFont's `/CIDSystemInfo` orders; an embedded CMap's have
/// none, as PDFium's on Linux haven't.
#[derive(Debug, Clone, Copy)]
enum CidText {
    Code,
    Cid(&'static hayro_cmap::CMap),
}

#[derive(Debug)]
enum Widths {
    /// A simple font's, by code.
    Simple(Box<[f32; 256]>),
    /// A CID font's `/W`, by CID, and `/DW` for the CIDs it doesn't give.
    Cid { w: Ranges<[f32; 1]>, dw: f32 },
}

#[derive(Debug)]
struct VerticalWidths {
    /// Each range's `w1`, `vx` and `vy`.
    w2: Ranges<[f32; 3]>,
    /// `/DW2`: the vertical origin's height and the advance, for the CIDs
    /// `/W2` doesn't give.
    vy: f32,
    w1: f32,
}

impl Foreign {
    /// The font `font`, a font dictionary, reads as, what its streams
    /// decompress to charged to `budget`. What it can't read it guesses,
    /// and says so; it fails only where its streams decompress past
    /// `budget`, which is then as it was.
    pub fn read(
        objects: &impl ReadObjects,
        font: &Dictionary,
        lim: &Limits,
        budget: &mut usize,
    ) -> Result<Foreign, TooLarge> {
        Foreign::read_as(objects, font, lim, budget, true)
    }

    /// [`Foreign::read`], its program read where `programs` says.
    fn read_as(
        objects: &impl ReadObjects,
        font: &Dictionary,
        lim: &Limits,
        budget: &mut usize,
        programs: bool,
    ) -> Result<Foreign, TooLarge> {
        let mut left = *budget;
        let decoder = Decoder::read(objects, font, lim, &mut left)?;
        let mut foreign = match get_name(objects, font, b"Subtype") {
            Some(b"Type0") => type0(objects, font, decoder, lim, &mut left, programs)?,
            Some(b"Type3") => type3(objects, font, decoder, lim, &mut left)?,
            Some(b"MMType1") => simple(
                objects,
                font,
                FontKind::MMType1,
                decoder,
                &mut left,
                programs,
            )?,
            Some(b"TrueType") => simple(
                objects,
                font,
                FontKind::TrueType,
                decoder,
                &mut left,
                programs,
            )?,
            _ => simple(objects, font, FontKind::Type1, decoder, &mut left, programs)?,
        };
        foreign.cost = *budget - left;
        foreign.extent = foreign.extent_of_metrics();
        *budget = left;
        Ok(foreign)
    }

    /// The object it's read from, where it's one ([`Fonts::get`]).
    pub fn id(&self) -> Option<ObjectId> {
        self.id
    }

    pub fn kind(&self) -> FontKind {
        self.kind
    }

    /// Whether it writes vertically: a Type0 font whose CMap does.
    pub fn is_vertical(&self) -> bool {
        self.vertical.is_some()
    }

    /// What of its metrics is guessed.
    pub fn approx(&self) -> FontApprox {
        self.approx
    }

    /// What sheaf read of the program it embeds.
    pub fn program(&self) -> FontProgram {
        self.program
    }

    /// The codes `s`, a string shown in it, is, in order, each with where
    /// it is in `s`, as PDFium splits them (`CPDF_CMap::GetNextChar`):
    /// - a simple font's are a byte each;
    /// - Identity-H's and Identity-V's two;
    /// - an embedded CMap's as its codespace says ([`Scheme`]);
    /// - a predefined CMap's two bytes each, or a byte but where it leads
    ///   two, as PDFium's scheme for it says ([`Split`]);
    /// - one sheaf hasn't as its `/ToUnicode`'s codespace says, as text is
    ///   read, else two bytes; a code that would run past the end is
    ///   what's left.
    ///
    /// A code of two bytes that runs past the end has a 0 for the byte
    /// it hasn't, as PDFium reads it, and is where its one byte is.
    pub fn codes<'s>(&'s self, s: &'s [u8]) -> impl Iterator<Item = (Range<usize>, u32)> + 's {
        let mut at = 0;
        std::iter::from_fn(move || {
            let rest = s.get(at..).filter(|r| !r.is_empty())?;
            let (len, code) = match &self.codes {
                Codes::One => (1, u32::from(rest[0])),
                Codes::Identity => two_bytes(rest),
                Codes::Embedded(map) => map.next(rest),
                Codes::Predefined { scheme, .. } if scheme.split.leads(rest[0]) => two_bytes(rest),
                Codes::Predefined { .. } => (1, u32::from(rest[0])),
                Codes::Unknown => {
                    let len = self.decoder.code_len(rest).clamp(1, rest.len());
                    (len, number(&rest[..len]))
                }
            };
            at += len;
            Some((at - len..at, code))
        })
    }

    /// Whether `Tw` is added after `code`, `len` bytes of its string, as
    /// PDFium adds it: a code 32 in a simple font, or in a Type0 font
    /// whose CMap sizes it a byte (`CPDF_CMap::GetCharSize`), by its value
    /// where its codes are mixed, and by `len` where sheaf doesn't know its
    /// CMap.
    pub(crate) fn word_spaced(&self, code: u32, len: usize) -> bool {
        code == 32
            && match &self.codes {
                Codes::One => true,
                Codes::Identity => false,
                Codes::Embedded(map) => map.scheme != Scheme::TwoBytes,
                Codes::Predefined { scheme, .. } => scheme.split != Split::TwoBytes,
                Codes::Unknown => len == 1,
            }
    }

    /// The CID `code` selects in a Type0 font: itself in Identity, or
    /// what the CMap maps it to, 0 where it maps it to none: an embedded
    /// one, over a predefined one it uses; a predefined one by the code's
    /// length as PDFium sizes it (`CPDF_CMap::GetCharSize`), two bytes in
    /// one of two bytes each, else by its value. None for a simple font,
    /// or where the CMap is unknown.
    pub fn cid(&self, code: u32) -> Option<u32> {
        match &self.codes {
            Codes::Identity => Some(code),
            Codes::Embedded(map) => Some(match map.cids.get(code) {
                Some((lo, cid)) => cid.wrapping_add(code - lo),
                None => map.base_cid(code).unwrap_or(0),
            }),
            Codes::Predefined { map, scheme } => {
                let len = match scheme.split {
                    Split::Lead(_) if code < 0x100 => 1,
                    _ => 2,
                };
                Some(map.lookup_cid_code(code, len).unwrap_or(0))
            }
            Codes::One | Codes::Unknown => None,
        }
    }

    /// How far `code` advances in horizontal writing, before character and
    /// word spacing: its `/Widths` entry, or its CID's `/W` or `/DW`.
    pub fn width(&self, code: u32) -> f32 {
        match &self.widths {
            Widths::Simple(widths) => widths.get(code as usize).copied().unwrap_or(0.0),
            Widths::Cid { w, dw } => match self.cid(code).and_then(|c| w.get(c)) {
                Some([width]) => width,
                None => *dw,
            },
        }
    }

    /// Whether `code`'s width is guessed where its font's others aren't
    /// ([`FontApprox::widths`]): in a standard 14 font drawn without a
    /// program or `/Widths`, a code whose glyph Adobe's metrics haven't,
    /// which PDFium draws as the face it draws in has it, or as the space
    /// where that hasn't either; give a width for and no box (the Euro,
    /// which Adobe's fonts haven't); or give a width PDFium's own face
    /// doesn't ([`Std14::drawn_otherwise`]): the face PDFium draws in,
    /// its own or the system's, says how wide. sheaf knows no box for it
    /// ([`Foreign::glyph_box`]).
    pub fn width_guessed(&self, code: u32) -> bool {
        code < 256 && self.guessed[code as usize / 64] >> (code % 64) & 1 == 1
    }

    /// Whether a code of `s`, a string shown in it, has its width
    /// guessed ([`Foreign::width_guessed`]): in a simple font, a byte of
    /// it.
    pub(crate) fn width_guessed_in(&self, s: &[u8]) -> bool {
        match self.codes {
            _ if self.guessed == [0; 4] => false,
            Codes::One => s.iter().any(|&b| self.width_guessed(u32::from(b))),
            _ => self.codes(s).any(|(_, code)| self.width_guessed(code)),
        }
    }

    /// A simple font's widths by code, where each of its codes is a byte:
    /// what [`Foreign::width`] gives each.
    pub(crate) fn byte_widths(&self) -> Option<&[f32; 256]> {
        match (&self.codes, &self.widths, &self.vertical) {
            (Codes::One, Widths::Simple(widths), None) => Some(widths),
            _ => None,
        }
    }

    /// Where it writes vertically, how `code`'s glyph is placed: its
    /// CID's `/W2`, else `/DW2`'s advance and height, centred on its
    /// width as PDFium centres it. None where it writes horizontally.
    pub fn vertical(&self, code: u32) -> Option<VerticalMetrics> {
        let vertical = self.vertical.as_ref()?;
        let given = self.cid(code).and_then(|c| vertical.w2.get(c));
        Some(match given {
            Some([w1, vx, vy]) => VerticalMetrics { w1, vx, vy },
            None => VerticalMetrics {
                w1: vertical.w1,
                vx: (self.width(code) / 2.0).trunc(),
                vy: vertical.vy,
            },
        })
    }

    /// How far above and below the baseline its glyphs reach, and its box.
    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    /// The box `code`'s glyph's outline fits in, from its origin, where
    /// sheaf knows it: a Type3 glyph's from its CharProc's `d1`, or else
    /// its font's `/FontBBox`, empty for a code with no CharProc, which
    /// draws nothing; a standard 14 glyph's from Adobe's AFM, but one
    /// whose width is guessed ([`Foreign::width_guessed`]). None where
    /// only a font program says.
    pub fn glyph_box(&self, code: u32) -> Option<Rect> {
        *self.boxes.as_ref()?.get(code as usize)?
    }

    /// The box PDFium gives `code`'s glyph from the font's program
    /// (`CPDF_Font::GetCharBBox`), in glyph units, from its origin, which
    /// its loose box for the character spans
    /// (`cpdf_textpage.cpp`'s `GetLooseBounds`): its outline's points'
    /// box, `-1` on every side for a simple font's code with no glyph, and
    /// all 0 for a CIDFont's; a CIDFont's top raised by a 64th of itself,
    /// as PDFium raises it. None where it embeds no program sheaf read, or
    /// sheaf can't say which glyph the code draws or what its outline is.
    #[inline]
    pub(crate) fn char_box(&self, code: u32) -> Option<Rect> {
        match &self.outlines {
            Outlines::None => None,
            Outlines::Codes(boxes) => *boxes.get(code as usize)?,
            Outlines::Glyphs(glyphs) => glyphs.char_box(self.cid(code)?),
        }
    }

    /// Each glyph `s` shows drawn, where it's a CIDFont whose program's
    /// outlines sheaf reads, as they're drawn as they're asked for
    /// ([`Foreign::char_box`]); and what drawing each cost charged to
    /// `budget` where `first`, given the glyph, says it's its first charge,
    /// so an interpretation pays for the glyphs it shows whichever drew
    /// them first. Too large past `budget`.
    pub(crate) fn charge_outlines(
        &self,
        s: &[u8],
        mut first: impl FnMut(u32) -> bool,
        budget: &mut usize,
    ) -> Result<(), TooLarge> {
        let Outlines::Glyphs(glyphs) = &self.outlines else {
            return Ok(());
        };
        for (_, code) in self.codes(s) {
            let Some(cid) = self.cid(code) else {
                continue;
            };
            let (glyph, _, cost) = glyphs.drawn(cid);
            if first(glyph) {
                *budget = budget.checked_sub(cost).ok_or(TooLarge)?;
            }
        }
        Ok(())
    }

    /// Whether its program's outlines were read, so [`Foreign::char_box`]
    /// says its codes' boxes, but where it can't say of one.
    pub(crate) fn outlined(&self) -> bool {
        !matches!(self.outlines, Outlines::None)
    }

    /// The text `code` is: its `/ToUnicode` mapping, or a simple font's
    /// encoding's; in a Type0 font without a `/ToUnicode`, the code's, or
    /// its CID's in its collection, as PDFium reads them ([`CidText`]).
    pub fn code_text(&self, code: u32) -> Option<Cow<'_, str>> {
        match self.cid_text {
            Some(cid_text) => self.cid_text(cid_text, code),
            None => self.decoder.code_text(code),
        }
    }

    /// The text `s`, a string shown in it, is: each code's
    /// ([`Foreign::code_text`]), its codes split as text is read, by its
    /// `/ToUnicode`'s codespace ranges where a composite font's encoding
    /// isn't Identity, or as they're drawn ([`Foreign::codes`]) where a
    /// Type0 font's text is its codes' or CIDs'. None where it doesn't say
    /// the text of one of them.
    ///
    /// CID 0, `.notdef`, which a code the CMap maps no CID takes, has none
    /// here, though PDFium reads Adobe's U+FFFD for it
    /// ([`Foreign::code_text`]): a string of such codes is told from
    /// another by its bytes, as before the predefined CMaps were read, not
    /// taken for the same text.
    pub(crate) fn text(&self, s: &[u8]) -> Option<String> {
        let Some(cid_text) = self.cid_text else {
            return self.decoder.text(s);
        };
        let mut text = String::new();
        for (_, code) in self.codes(s) {
            match cid_text {
                CidText::Code => text.push(char::from_u32(code).filter(|&c| c != '\0')?),
                CidText::Cid(to_unicode) => {
                    let cid = self.cid(code).filter(|&cid| cid != 0)?;
                    match to_unicode.lookup_bf_string(cid)? {
                        hayro_cmap::BfString::Char(c) if c != '\0' => text.push(c),
                        hayro_cmap::BfString::String(s) if s != "\0" => text.push_str(&s),
                        _ => return None,
                    }
                }
            }
        }
        Some(text)
    }

    /// The text `code` is by `cid_text`: the code, or its CID's mapping;
    /// none for the code or the character 0, as PDFium has none.
    fn cid_text(&self, cid_text: CidText, code: u32) -> Option<Cow<'_, str>> {
        let text = match cid_text {
            CidText::Code => String::from(char::from_u32(code)?),
            CidText::Cid(to_unicode) => match to_unicode.lookup_bf_string(self.cid(code)?)? {
                hayro_cmap::BfString::Char(c) => String::from(c),
                hayro_cmap::BfString::String(s) => s,
            },
        };
        (text != "\0").then_some(Cow::Owned(text))
    }

    /// The most any of its metrics is from 0, in glyph units: its widths,
    /// vertical advances and origins, ascent and descent, and its glyphs'
    /// outline boxes ([`OUTLINE_REACH`] for a CIDFont's, drawn as they're
    /// asked for); infinite where one isn't finite. A glyph's box is that
    /// far from its origin at most, at a font size of 1000.
    pub(crate) fn extent(&self) -> f32 {
        self.extent
    }

    fn extent_of_metrics(&self) -> f32 {
        let widths: Vec<f32> = match &self.widths {
            Widths::Simple(widths) => widths.to_vec(),
            Widths::Cid { w, dw } => w.pieces.iter().map(|p| p.2[0]).chain([*dw]).collect(),
        };
        let vertical = self.vertical.iter().flat_map(|v| {
            let w2 = v.w2.pieces.iter().flat_map(|p| p.2);
            w2.chain([v.vy, v.w1])
        });
        let heights = [self.metrics.ascent, self.metrics.descent];
        let outlines: Vec<f32> = match &self.outlines {
            Outlines::None => Vec::new(),
            Outlines::Codes(boxes) => (boxes.iter().flatten())
                .flat_map(|r| [r.x0, r.y0, r.x1, r.y1])
                .collect(),
            Outlines::Glyphs(_) => vec![OUTLINE_REACH],
        };
        (widths
            .into_iter()
            .chain(vertical)
            .chain(heights)
            .chain(outlines))
        .fold(0.0, |most, v| match v.is_nan() {
            true => f32::INFINITY,
            false => most.max(v.abs()),
        })
    }
}

/// How far a CIDFont's glyph's outline box may be from its origin, in
/// glyph units: an `int`'s most, which PDFium's normalized boxes are held
/// within ([`program::normalize`]).
const OUTLINE_REACH: f32 = 2_147_483_648.0;

/// The code of two bytes `rest` starts with, a 0 for the second where it
/// hasn't one, as PDFium reads one (`CPDF_CMap::GetNextChar`), and how
/// many bytes it has of it.
fn two_bytes(rest: &[u8]) -> (usize, u32) {
    let second = rest.get(1).copied().unwrap_or(0);
    (
        rest.len().min(2),
        u32::from(rest[0]) << 8 | u32::from(second),
    )
}

/// A Type1, MMType1 or TrueType font, its program read within `budget`
/// where `programs` says.
fn simple(
    objects: &impl ReadObjects,
    font: &Dictionary,
    kind: FontKind,
    decoder: Decoder,
    budget: &mut usize,
    programs: bool,
) -> Result<Foreign, TooLarge> {
    let descriptor = get_dict(objects, font, b"FontDescriptor");
    let embedded = descriptor.is_some_and(|d| {
        [&b"FontFile"[..], b"FontFile2", b"FontFile3"]
            .iter()
            .any(|key| d.has(key))
    });
    let std14 = get_name(objects, font, b"BaseFont").and_then(Std14::named);
    let mut approx = FontApprox::default();
    // A standard 14 glyph's metrics where no program is embedded, by the
    // name its encoding gives it: Symbol and ZapfDingbats in their own
    // encoding always, as PDFium reads them. A glyph Adobe's metrics give
    // no box (but the space's) or haven't, or whose width PDFium's face
    // gives otherwise, is guessed, and has none.
    let afm = std14.map(|std14| {
        let own = match std14 {
            Std14::Symbol => Encoding::Symbol,
            Std14::ZapfDingbats => Encoding::ZapfDingbats,
            _ => Encoding::Standard,
        };
        let mut encoded = Encoded::read(objects, font, Some(own));
        if own != Encoding::Standard {
            encoded.base = Some(own);
        }
        // A code whose glyph the font hasn't draws as the space does, as
        // in PDFium.
        let glyph = |code: u8| {
            let name = encoded.glyph(code)?;
            let bbox = std14.glyph_box(name)?;
            let drawn =
                (name == "space" || bbox != Rect::default()) && !std14.drawn_otherwise(name);
            Some((std14.glyph_width(name)?, drawn.then_some(bbox)))
        };
        let space = glyph(b' ').map_or((0.0, None), |(width, _)| (width, None));
        Box::new(std::array::from_fn::<_, 256, _>(|code| {
            glyph(code as u8).unwrap_or(space)
        }))
    });
    let given = get_array(objects, font, b"Widths");
    let has_widths = given.is_some();
    let widths = match given {
        Some(widths) => {
            let missing = descriptor.and_then(|d| get_whole(objects, d, b"MissingWidth"));
            let mut table = Box::new([missing.unwrap_or(0.0); 256]);
            let first = get_whole(objects, font, b"FirstChar").unwrap_or(0.0);
            let last = get_whole(objects, font, b"LastChar").unwrap_or(0.0);
            // From `/FirstChar` to `/LastChar`, or as far as `/Widths`
            // goes where `/LastChar` is 0 or past it.
            if (0.0..256.0).contains(&first) && !widths.is_empty() {
                let first = first as usize;
                let end = first + widths.len() - 1;
                let last = if last == 0.0 || last >= end as f32 {
                    end
                } else {
                    last.max(0.0) as usize
                };
                for code in first..=last.min(255) {
                    table[code] = whole(objects, &widths[code - first]).unwrap_or(0.0);
                }
            }
            table
        }
        None => match &afm {
            Some(afm) => {
                approx.widths = embedded;
                Box::new(afm.map(|(width, _)| width))
            }
            None => {
                approx.widths = true;
                Box::new([0.0; 256])
            }
        },
    };
    let base_font = get_name(objects, font, b"BaseFont").unwrap_or_default();
    let (program, face) = read_program(objects, descriptor, base_font, budget, programs)?;
    let outlines = match &face {
        Some(face) => {
            let to_unicode = |code| {
                decoder
                    .to_unicode
                    .then(|| decoder.cmap.get(code)?.chars().next())
                    .flatten()
            };
            let read = Simple {
                truetype: kind == FontKind::TrueType,
                flags: descriptor.map_or(0, |d| flags(objects, d)),
                flags_given: descriptor.is_some_and(|d| d.has(b"Flags")),
                std14: std14.filter(|_| kind != FontKind::TrueType),
                base_font: untagged(base_font),
                encoding: EncodingObject::of(objects, font),
                first_char: get_whole(objects, font, b"FirstChar").map_or(0, |c| c as i64),
                has_to_unicode: font.has(b"ToUnicode"),
                to_unicode: &to_unicode,
            };
            approx.boxes = !face.exact();
            let (boxes, cost) = face.simple_boxes(&read);
            *budget = budget.checked_sub(cost).ok_or(TooLarge)?;
            Outlines::Codes(boxes)
        }
        None => {
            approx.boxes = true;
            Outlines::None
        }
    };
    let program_heights = face.as_ref().and_then(|face| {
        let Outlines::Codes(boxes) = &outlines else {
            return None;
        };
        program_heights(face, boxes[usize::from(b'A')], boxes[usize::from(b'g')])
    });
    let (metrics, guessed) = heights(objects, descriptor, std14, program_heights);
    approx.heights = guessed;
    Ok(Foreign {
        id: None,
        kind,
        codes: Codes::One,
        widths: Widths::Simple(widths),
        vertical: None,
        metrics,
        guessed: match (&afm, embedded || has_widths) {
            (Some(afm), false) => bits(afm.iter().map(|(_, bbox)| bbox.is_none())),
            _ => [0; 4],
        },
        boxes: afm
            .filter(|_| !embedded)
            .map(|afm| Box::new(afm.map(|(_, bbox)| bbox))),
        program,
        outlines,
        extent: 0.0,
        cost: 0,
        decoder,
        cid_text: None,
        approx,
    })
}

/// The program `descriptor` embeds, read within `budget` where `programs`
/// says, and opened ([`Face::open`]); `named` is the font's `/BaseFont`.
fn read_program(
    objects: &impl ReadObjects,
    descriptor: Option<&Dictionary>,
    named: &[u8],
    budget: &mut usize,
    programs: bool,
) -> Result<(FontProgram, Option<Face>), TooLarge> {
    if !programs {
        return Ok((FontProgram::None, None));
    }
    Ok(match program::read_file(objects, descriptor, budget)? {
        None => (FontProgram::None, None),
        Some(data) => match Face::open(data, named) {
            Some(face) => (FontProgram::Read, Some(face)),
            None => (FontProgram::Malformed, None),
        },
    })
}

/// What a font's program says of its heights ([`ProgramHeights`]), the
/// codes `A` and `g` boxed as `a` and `g`; None where its glyph space is
/// scaled otherwise than sheaf models.
fn program_heights(face: &Face, a: Option<Rect>, g: Option<Rect>) -> Option<ProgramHeights> {
    let (bbox, ascent, descent) = face.heights()?;
    Some(ProgramHeights {
        bbox,
        ascent,
        descent,
        a,
        g,
    })
}

/// What a font's program says of its heights, as PDFium takes them
/// (`CPDF_Font::CheckFontMetrics`), in glyph units: its face's box,
/// ascender and descender, and the boxes PDFium gives the codes `A` and
/// `g` ([`Foreign::char_box`]), None where sheaf can't say.
struct ProgramHeights {
    bbox: Rect,
    ascent: f32,
    descent: f32,
    a: Option<Rect>,
    g: Option<Rect>,
}

/// A descriptor's `/Flags`, as PDFium reads them (`LoadFontDescriptor`):
/// 32, nonsymbolic, where it gives none, and 0 where they're no number.
fn flags(objects: &impl ReadObjects, descriptor: &Dictionary) -> i64 {
    match descriptor.get(b"Flags") {
        Ok(f) => integer(objects, f).unwrap_or(0),
        Err(_) => 32,
    }
}

/// A font's name past a subset's tag, as PDFium drops one
/// (`MaybeRemoveSubsettedFontPrefix`): six capitals and `+`.
fn untagged(name: &[u8]) -> &[u8] {
    match name.split_at_checked(7) {
        Some((tag, rest)) if tag[6] == b'+' && tag[..6].iter().all(u8::is_ascii_uppercase) => rest,
        _ => name,
    }
}

/// 256 bits, a code each, those `set` says set.
fn bits(set: impl Iterator<Item = bool>) -> [u64; 4] {
    let mut bits = [0; 4];
    for (code, _) in set.enumerate().filter(|(_, set)| *set) {
        bits[code / 64] |= 1 << (code % 64);
    }
    bits
}

/// A Type3 font: its glyphs drawn by its CharProcs, in glyph space its
/// `/FontMatrix` maps to text space, the identity where it has none, as
/// in PDFium. Its CharProcs decompress within `budget`.
fn type3(
    objects: &impl ReadObjects,
    font: &Dictionary,
    decoder: Decoder,
    lim: &Limits,
    budget: &mut usize,
) -> Result<Foreign, TooLarge> {
    let matrix = get_array(objects, font, b"FontMatrix")
        .and_then(|m| affine(objects, m))
        .unwrap_or(Affine::IDENTITY);
    // The box in glyph units as PDFium keeps it (`CPDF_Type3Font::Load`):
    // its left, bottom, right and top as given, scaled by the matrix's `a`
    // and `d` only, in thousandths, each cut to a whole number, so a
    // matrix that turns glyph space over leaves the top below the bottom;
    // none where that leaves all four 0.
    let given = get_array(objects, font, b"FontBBox")
        .and_then(|b| {
            let [l, b, r, t] = b.get(..4)? else {
                return None;
            };
            let n = |o| number_of(objects, o);
            Some([
                (n(l)?, matrix.a),
                (n(b)?, matrix.d),
                (n(r)?, matrix.a),
                (n(t)?, matrix.d),
            ])
        })
        .map(|sides| sides.map(|(v, scale)| (v * scale * 1000.0).trunc()))
        .filter(|sides| *sides != [0.0; 4]);
    let bbox = given.map(|[l, b, r, t]| Rect::new(l, b, r, t));
    let mut approx = FontApprox::default();
    // Each code's CharProc, None where it has none: each stream read once
    // however many codes and names draw it.
    let encoded = Encoded::read(objects, font, None);
    let char_procs = get_dict(objects, font, b"CharProcs");
    let mut read: HashMap<ObjectId, Option<CharProc>> = HashMap::new();
    let mut procs: [Option<Option<CharProc>>; 256] = [None; 256];
    for (code, slot) in procs.iter_mut().enumerate() {
        let id = encoded
            .glyph_name(code as u8)
            .and_then(|name| char_procs?.get(name).ok()?.as_reference().ok());
        let Some(id) = id else {
            continue;
        };
        *slot = Some(match read.get(&id) {
            Some(proc) => *proc,
            None => *read
                .entry(id)
                .or_insert(char_proc(objects, id, lim, budget)?),
        });
    }
    let xunit = matrix.a.hypot(matrix.b);
    let mut widths = Box::new([0.0; 256]);
    let first = get_whole(objects, font, b"FirstChar").unwrap_or(0.0);
    if let Some(given) = get_array(objects, font, b"Widths")
        && (0.0..256.0).contains(&first)
    {
        let first = first as usize;
        for (slot, w) in widths[first..].iter_mut().zip(given) {
            *slot = (number_of(objects, w).unwrap_or(0.0) * matrix.a * 1000.0).round();
        }
    }
    // A code `/Widths` gives no width advances as its CharProc says, in
    // thousandths rounded, then through the matrix, rounded on by a half
    // and cut, as PDFium has it (`CPDF_Type3Char`).
    for (width, proc) in widths.iter_mut().zip(&procs) {
        if *width == 0.0
            && let Some(Some(proc)) = proc
        {
            *width = ((proc.wx * 1000.0).round() * xunit + 0.5).trunc();
        }
    }
    // PDFium's ascent is `A`'s top and its descent `g`'s bottom
    // (`CPDF_Font::CheckFontMetrics`), the box's top and bottom where the
    // font has no such glyph, held within the box where its top is above
    // its bottom (`GetLooseBounds`); a glyph whose `d1` gives no box has
    // the box of what it draws, which sheaf doesn't read. Where the
    // ascent is below the descent, a glyph's box still spans the two.
    let metrics = match given {
        Some([_, bottom, _, top]) => {
            let mut edge = |code: usize, side: fn(Rect) -> f32, of_box: f32| match procs[code] {
                None => of_box,
                Some(proc) => match proc.and_then(|p| glyph_units(matrix, p.bbox?)) {
                    Some(r) => side(r),
                    None => {
                        approx.heights = true;
                        of_box
                    }
                },
            };
            let (ascent, descent) = (edge(65, |r| r.y1, top), edge(103, |r| r.y0, bottom));
            let (ascent, descent) = match top > bottom {
                true => (ascent.min(top), descent.max(bottom)),
                false => (ascent, descent),
            };
            // Where they're one, PDFium bounds a glyph by its outline.
            approx.heights |= ascent == descent;
            FontMetrics {
                ascent,
                descent,
                bbox,
            }
        }
        None => {
            approx.heights = true;
            GUESSED
        }
    };
    let boxes = procs.map(|proc| match proc {
        None => Some(Rect::default()),
        Some(proc) => match proc.and_then(|p| glyph_units(matrix, p.bbox?)) {
            Some(b) => Some(b),
            None => {
                approx.boxes |= bbox.is_none();
                bbox
            }
        },
    });
    Ok(Foreign {
        id: None,
        kind: FontKind::Type3,
        codes: Codes::One,
        widths: Widths::Simple(widths),
        vertical: None,
        metrics,
        boxes: Some(Box::new(boxes)),
        program: FontProgram::None,
        outlines: Outlines::None,
        guessed: [0; 4],
        extent: 0.0,
        cost: 0,
        decoder,
        cid_text: None,
        approx,
    })
}

/// What a Type3 glyph's CharProc says of it first: `wx wy d0`, or `wx wy
/// llx lly urx ury d1`, with its box's numbers in that order.
#[derive(Debug, Clone, Copy)]
struct CharProc {
    wx: f32,
    bbox: Option<[f32; 4]>,
}

/// A `d1` box, `[llx, lly, urx, ury]`, in glyph units as PDFium keeps it
/// (`CPDF_Type3Char`): its numbers in thousandths, rounded, through
/// `matrix`, the font's, its translation as it is, and rounded again.
/// None where the numbers rounded have no area between them, as PDFium
/// takes such a box for none.
fn glyph_units(matrix: Affine, d1: [f32; 4]) -> Option<Rect> {
    let [l, b, r, t] = d1.map(|v| (v * 1000.0).round());
    if r <= l || b >= t {
        return None;
    }
    let to = matrix.bounds(Rect::new(l, b, r, t));
    Some(Rect::new(
        to.x0.round(),
        to.y0.round(),
        to.x1.round(),
        to.y1.round(),
    ))
}

/// CharProc `id`'s first operator, lexed alone, from its content
/// decompressed within `budget`, which it takes its size from. None when
/// it's neither `d0` nor `d1`, or can't be read.
fn char_proc(
    objects: &impl ReadObjects,
    id: ObjectId,
    lim: &Limits,
    budget: &mut usize,
) -> Result<Option<CharProc>, TooLarge> {
    let Some(data) = decompressed(objects, id, budget)? else {
        return Ok(None);
    };
    Ok(first_operator(&data, lim))
}

/// What `data`, a CharProc's content, says first ([`CharProc`]).
fn first_operator(data: &[u8], lim: &Limits) -> Option<CharProc> {
    let depth = lim.content.depth;
    let mut operands = Vec::new();
    for token in lex::lex(data, &ResCx::none(), lim) {
        match token.ok()?.1 {
            Tok::Operand(o) => operands.push(o),
            Tok::Op(op) => {
                let n = |i: usize| {
                    let o = operands.get(operands.len().checked_sub(i + 1)?)?;
                    Some(lex::object(o, depth)?.as_float().ok()? as f32)
                };
                return match op {
                    b"d0" => Some(CharProc {
                        wx: n(1)?,
                        bbox: None,
                    }),
                    b"d1" => Some(CharProc {
                        wx: n(5)?,
                        bbox: Some([n(3)?, n(2)?, n(1)?, n(0)?]),
                    }),
                    _ => None,
                };
            }
            Tok::InlineImage(_) => return None,
        }
    }
    None
}

/// A Type0 font: its codes by its `/Encoding` CMap, its CIDs' widths by
/// its CIDFont. An embedded CMap's chain decompresses within `budget`; a
/// predefined CMap is read where sheaf has it and PDFium knows it; and the
/// CIDFont's program, and its `/CIDToGIDMap`, where `programs` says.
fn type0(
    objects: &impl ReadObjects,
    font: &Dictionary,
    decoder: Decoder,
    lim: &Limits,
    budget: &mut usize,
    programs: bool,
) -> Result<Foreign, TooLarge> {
    let mut approx = FontApprox::default();
    // PDFium reads a Type0 font with one CIDFont, and no other.
    let cid_font = get_array(objects, font, b"DescendantFonts")
        .filter(|fonts| fonts.len() == 1)
        .and_then(|fonts| direct(objects, &fonts[0])?.as_dict().ok());
    let descendant = match cid_font.and_then(|f| get_name(objects, f, b"Subtype")) {
        Some(b"CIDFontType0") => Descendant::CidFontType0,
        _ => Descendant::CidFontType2,
    };
    let (codes, vertical) = match font.get(b"Encoding").ok().map(|e| (e, direct(objects, e))) {
        Some((_, Some(Object::Name(n)))) if n == b"Identity-H" => (Codes::Identity, false),
        Some((_, Some(Object::Name(n)))) if n == b"Identity-V" => (Codes::Identity, true),
        Some((Object::Reference(id), Some(Object::Stream(_)))) => {
            let map = CidMap::read(objects, *id, lim, &mut approx.codes, budget)?;
            let vertical = map.vertical;
            (Codes::Embedded(map), vertical)
        }
        // A predefined CMap writes vertically where its name ends in V,
        // as PDFium reads it.
        Some((_, Some(Object::Name(n)))) => {
            let known = predefined::scheme(n).zip(predefined::bundled(n));
            let codes = match known {
                Some((scheme, map)) => Codes::Predefined { map, scheme },
                None => {
                    approx.codes = true;
                    Codes::Unknown
                }
            };
            (codes, n.ends_with(b"V"))
        }
        _ => {
            approx.codes = true;
            (Codes::Unknown, false)
        }
    };
    let widths = match cid_font {
        Some(f) => Widths::Cid {
            w: cid_ranges(objects, f, b"W"),
            dw: get_whole(objects, f, b"DW").unwrap_or(1000.0),
        },
        None => {
            approx.widths = true;
            Widths::Cid {
                w: Ranges::default(),
                dw: 1000.0,
            }
        }
    };
    let vertical = vertical.then(|| {
        let dw2 = cid_font.and_then(|f| get_array(objects, f, b"DW2"));
        let at = |i: usize| dw2.and_then(|d| whole(objects, d.get(i)?));
        VerticalWidths {
            w2: cid_font
                .map(|f| cid_ranges(objects, f, b"W2"))
                .unwrap_or_default(),
            vy: at(0).unwrap_or(880.0),
            w1: at(1).unwrap_or(-1000.0),
        }
    });
    let descriptor = cid_font.and_then(|f| get_dict(objects, f, b"FontDescriptor"));
    let base_font = cid_font
        .and_then(|f| get_name(objects, f, b"BaseFont"))
        .unwrap_or_default();
    let (program, face) = read_program(objects, descriptor, base_font, budget, programs)?;
    let outlines = match face {
        Some(face) => {
            approx.boxes = !face.exact();
            let to_gid = match cid_font.and_then(|f| f.get(b"CIDToGIDMap").ok()) {
                Some(&Object::Reference(id)) if is_stream(objects, id) => {
                    Some(decompressed(objects, id, budget)?.unwrap_or_default())
                }
                _ => None,
            };
            Outlines::Glyphs(Box::new(CidGlyphs::new(face, to_gid)))
        }
        None => {
            approx.boxes = true;
            Outlines::None
        }
    };
    // Identity's collection is the one its CIDFont orders.
    let ordered = || {
        let info = get_dict(objects, cid_font?, b"CIDSystemInfo")?;
        let ordering = match direct(objects, info.get(b"Ordering").ok()?)? {
            Object::String(s, _) => s.as_slice(),
            Object::Name(n) => n.as_slice(),
            _ => return None,
        };
        Collection::ordered(ordering)
    };
    let cid_text = match &codes {
        _ if decoder.to_unicode => None,
        Codes::Predefined { scheme, .. } if scheme.unicode => Some(CidText::Code),
        Codes::Predefined { scheme, .. } => scheme.collection.to_unicode().map(CidText::Cid),
        Codes::Identity => ordered().and_then(Collection::to_unicode).map(CidText::Cid),
        Codes::One | Codes::Embedded(_) | Codes::Unknown => None,
    };
    let mut foreign = Foreign {
        id: None,
        kind: FontKind::Type0(descendant),
        codes,
        widths,
        vertical,
        metrics: GUESSED,
        boxes: None,
        program,
        outlines,
        guessed: [0; 4],
        extent: 0.0,
        cost: 0,
        decoder,
        cid_text,
        approx,
    };
    // The program's heights, PDFium's `A` and `g` as the CMap maps them,
    // drawn now and charged.
    let program_heights = match &foreign.outlines {
        Outlines::Glyphs(glyphs) => {
            let drawn = |code: u8| {
                let (_, found, cost) = glyphs.drawn(foreign.cid(code.into())?);
                Some((found, cost))
            };
            let (a, g) = (drawn(b'A'), drawn(b'g'));
            let cost = [a, g].iter().flatten().map(|&(_, cost)| cost).sum();
            *budget = budget.checked_sub(cost).ok_or(TooLarge)?;
            let found = |d: Option<(Option<Rect>, usize)>| d.and_then(|(found, _)| found);
            program_heights(glyphs.face(), found(a), found(g))
        }
        _ => None,
    };
    (foreign.metrics, foreign.approx.heights) = heights(objects, descriptor, None, program_heights);
    Ok(foreign)
}

/// Whether object `id` is a stream.
pub(super) fn is_stream(objects: &impl ReadObjects, id: ObjectId) -> bool {
    matches!(objects.read(id, |o| o.as_stream().is_ok()), Ok(true))
}

/// A font's heights, as PDFium bounds a character (`cpdf_textpage.cpp`'s
/// `GetLooseBounds`, from `CPDF_Font::LoadFontDescriptor` and
/// `CheckFontMetrics`), and whether they're guessed: with a descriptor
/// whose `/FontBBox` isn't all 0, its `Ascent` and `Descent`, a `Descent`
/// over 10 read as below the baseline, within the box. Else PDFium takes
/// them from the face it draws in: its box, ascender and descender where
/// the descriptor has no box, and `A`'s top and `g`'s bottom where neither
/// height is given. Where the font embeds a program sheaf read, that's the
/// face ([`ProgramHeights`]). Else the face is one PDFium substitutes, its
/// own for the standard 14 or the system's, which varies by OS, and they're
/// guessed: the descriptor's, else the standard 14 font's AFM's, else the
/// box's top and bottom, else an em above and a quarter below.
fn heights(
    objects: &impl ReadObjects,
    descriptor: Option<&Dictionary>,
    std14: Option<Std14>,
    program: Option<ProgramHeights>,
) -> (FontMetrics, bool) {
    let get = |key: &[u8]| descriptor.and_then(|d| get_whole(objects, d, key));
    let own = descriptor
        .and_then(|d| get_array(objects, d, b"FontBBox"))
        .and_then(|b| rect(objects, b, whole))
        .filter(|b| *b != Rect::default());
    let ascent = get(b"Ascent").unwrap_or(0.0);
    let descent = get(b"Descent").map_or(0.0, |d| if d > 10.0 { -d } else { d });
    let given = ascent != 0.0 || descent != 0.0;
    let within = |bbox: Option<Rect>, (ascent, descent): (f32, f32)| match bbox {
        Some(b) if b.y1 > b.y0 => (ascent.min(b.y1), descent.max(b.y0)),
        _ => (ascent, descent),
    };
    if let Some(program) = program {
        let (bbox, mut ascent, mut descent) = match own {
            Some(b) => (b, ascent, descent),
            None => (program.bbox, program.ascent, program.descent),
        };
        let mut guessed = false;
        if ascent == 0.0 && descent == 0.0 {
            match (program.a, program.g) {
                (Some(a), Some(g)) => {
                    ascent = if a.y0 == a.y1 { bbox.y1 } else { a.y1 };
                    descent = if g.y0 == g.y1 { bbox.y0 } else { g.y0 };
                }
                _ => {
                    guessed = true;
                    (ascent, descent) = (bbox.y1, bbox.y0);
                }
            }
        }
        let (ascent, descent) = within(Some(bbox), (ascent, descent));
        // Where they're one, PDFium bounds a glyph by its outline alone
        // (`GetLooseBounds`).
        let metrics = FontMetrics {
            ascent,
            descent,
            bbox: Some(bbox),
        };
        return (metrics, guessed || ascent == descent);
    }
    let bbox = own.or(std14.and_then(|f| f.metrics().bbox));
    let (ascent, descent) = match (own, given, std14) {
        (Some(_), true, _) => within(own, (ascent, descent)),
        (_, true, _) => (ascent, descent),
        (_, false, Some(std14)) => {
            let m = std14.metrics();
            within(own, (m.ascent, m.descent))
        }
        (Some(b), false, None) => (b.y1, b.y0),
        (None, false, None) => (GUESSED.ascent, GUESSED.descent),
    };
    let exact = own.is_some() && given;
    let metrics = FontMetrics {
        ascent,
        descent,
        bbox,
    };
    (metrics, !exact || ascent == descent)
}

/// A CIDFont's `/W` or `/W2`, the first range given for a CID winning,
/// as in PDFium.
fn cid_ranges<const N: usize>(
    objects: &impl ReadObjects,
    cid_font: &Dictionary,
    key: &[u8],
) -> Ranges<[f32; N]> {
    let array = get_array(objects, cid_font, key).unwrap_or_default();
    Ranges::new(&metrics_array(objects, array), false)
}

/// A `/W` or `/W2` array's entries, read as PDFium reads them: `c [v …]`
/// gives `c` and the CIDs after it values in turn, and `first last v`
/// gives the range one; `N` numbers a value. An array where a range's
/// last CID should be ends it, and a range that starts below 0 starts
/// at 0.
fn metrics_array<const N: usize>(
    objects: &impl ReadObjects,
    array: &[Object],
) -> Vec<(u32, u32, [f32; N])> {
    let cid = |n: i64| n.clamp(0, i64::from(u32::MAX)) as u32;
    let mut out = Vec::new();
    let mut first = None;
    let mut last = None;
    let mut values = Vec::with_capacity(N);
    for item in array {
        let Some(item) = direct(objects, item) else {
            continue;
        };
        if let Object::Array(list) = item {
            let (Some(start), None) = (first.take(), last) else {
                return out;
            };
            let mut at = cid(start);
            for chunk in list.chunks(N) {
                let value = std::array::from_fn(|k| {
                    let v = chunk.get(k).and_then(|v| whole(objects, v));
                    v.unwrap_or(0.0)
                });
                out.push((at, at, value));
                at = at.saturating_add(1);
            }
            continue;
        }
        let n = integer(objects, item).unwrap_or(0);
        match (first, last) {
            (None, _) => first = Some(n),
            (Some(_), None) => last = Some(n),
            (Some(lo), Some(hi)) => {
                values.push(n as f32);
                if values.len() == N {
                    if hi >= 0 {
                        let value = std::array::from_fn(|k| values[k]);
                        out.push((cid(lo), cid(hi), value));
                    }
                    (first, last) = (None, None);
                    values.clear();
                }
            }
        }
    }
    out
}

/// Values for ranges of codes or CIDs, kept as the ranges they're given
/// in, so a range of 2³¹ costs what a range of one does. Where ranges
/// overlap, the first given wins, as PDFium reads `/W`, or the last, as
/// a CMap's later mappings, and those of the CMap that uses another, win.
#[derive(Debug, Clone, Default)]
struct Ranges<T> {
    /// Disjoint, in order: first, last, value.
    pieces: Vec<(u32, u32, T)>,
}

impl<T: Copy> Ranges<T> {
    /// `ranges`, each its first, its last and its value; one whose last
    /// is before its first is none.
    fn new(ranges: &[(u32, u32, T)], last_wins: bool) -> Ranges<T> {
        let ranges: Vec<_> = ranges.iter().filter(|r| r.0 <= r.1).collect();
        let rank = |i: usize| if last_wins { ranges.len() - i } else { i };
        let mut starts: Vec<usize> = (0..ranges.len()).collect();
        starts.sort_by_key(|&i| ranges[i].0);
        let mut bounds: Vec<u64> = (ranges.iter())
            .flat_map(|r| [u64::from(r.0), u64::from(r.1) + 1])
            .collect();
        bounds.sort_unstable();
        bounds.dedup();
        // From each bound to the next, the ranges that cover it, the one
        // that wins on top; those that have ended are passed over.
        let mut covering = BinaryHeap::new();
        let mut next = starts.iter().peekable();
        let mut pieces: Vec<(u32, u32, T)> = Vec::new();
        let mut winners: Vec<usize> = Vec::new();
        for pair in bounds.windows(2) {
            let (at, end) = (pair[0], pair[1]);
            while let Some(&&i) = next.peek()
                && u64::from(ranges[i].0) == at
            {
                covering.push(Reverse((rank(i), i)));
                next.next();
            }
            while let Some(&Reverse((_, i))) = covering.peek()
                && u64::from(ranges[i].1) < at
            {
                covering.pop();
            }
            let Some(&Reverse((_, i))) = covering.peek() else {
                continue;
            };
            let (lo, hi) = (at as u32, (end - 1) as u32);
            let joins = winners.last() == Some(&i)
                && pieces.last().is_some_and(|p| u64::from(p.1) + 1 == at);
            match pieces.last_mut() {
                Some(piece) if joins => piece.1 = hi,
                _ => {
                    pieces.push((lo, hi, ranges[i].2));
                    winners.push(i);
                }
            }
        }
        Ranges { pieces }
    }

    /// The value of the range `code` is in.
    fn get(&self, code: u32) -> Option<T> {
        let i = self.pieces.partition_point(|p| p.1 < code);
        let &(lo, _, value) = self.pieces.get(i)?;
        (lo <= code).then_some(value)
    }
}

/// An embedded CMap: its codespace, how it splits a string into codes,
/// each code's CID, and whether it writes vertically.
#[derive(Debug, Default)]
struct CidMap {
    codespace: Vec<(Vec<u8>, Vec<u8>)>,
    scheme: Scheme,
    /// Each range's first code and the CID it maps to.
    cids: Ranges<(u32, u32)>,
    /// The predefined CMap it uses but Identity, for the codes it maps
    /// none of.
    base: Option<&'static hayro_cmap::CMap>,
    vertical: bool,
}

/// How a CMap's codespace splits a string into codes, as PDFium has it
/// (`CPDF_CMapParser::HandleCodeSpaceRange`): by one range, a byte each,
/// or two where it's of two bytes; by more, each code by the ranges
/// ([`mixed`]); by none, two bytes each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Scheme {
    OneByte,
    #[default]
    TwoBytes,
    Mixed,
}

impl CidMap {
    /// The CMap stream `id`, over the CMaps it uses: the stream its
    /// `/UseCMap` names, followed while they're streams it hasn't read
    /// yet, at most [`ContentLimits::depth`](crate::source::ContentLimits)
    /// of them, each decompressed within `budget`; and the predefined CMap
    /// one names, by name or by `usecmap`. PDFium uses none of them, so
    /// what they map but Identity sets `unknown`, as does a predefined CMap
    /// sheaf hasn't, a stream of the chain that can't be read, or one past
    /// the depth.
    fn read(
        objects: &impl ReadObjects,
        id: ObjectId,
        lim: &Limits,
        unknown: &mut bool,
        budget: &mut usize,
    ) -> Result<CidMap, TooLarge> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut next = Some(id);
        let mut base = None;
        // A chain back to a CMap it's read adds nothing.
        while let Some(id) = next.take().filter(|id| seen.insert(*id)) {
            if chain.len() >= lim.content.depth {
                *unknown = true;
                break;
            }
            let read = objects.read(id, |o| {
                let stream = o.as_stream().ok()?;
                let data = stream.decompressed_content_with_limit(*budget);
                Some((data, stream.dict.get(b"UseCMap").ok().cloned()))
            });
            let Ok(Some((data, uses))) = read else {
                *unknown = true;
                break;
            };
            let Some(data) = charged(data, budget)? else {
                *unknown = true;
                break;
            };
            chain.push(data);
            match uses {
                Some(Object::Reference(id)) => next = Some(id),
                Some(Object::Name(name)) => base = Some(name),
                _ => {}
            }
        }
        let mut map = CidMap::default();
        let mut ranges = Vec::new();
        if let Some(name) = base {
            map.uses(&name, &mut ranges, unknown);
        }
        for data in chain.iter().rev() {
            map.parse(data, lim, &mut ranges, unknown);
        }
        map.cids = Ranges::new(&ranges, true);
        map.scheme = match map.codespace.len() {
            0 => Scheme::TwoBytes,
            1 if map.codespace[0].0.len() == 2 => Scheme::TwoBytes,
            1 => Scheme::OneByte,
            _ => Scheme::Mixed,
        };
        Ok(map)
    }

    /// The CID its predefined base maps `code` to, by the code's length as
    /// its scheme sizes it (`CPDF_CMap::GetCharSize`).
    fn base_cid(&self, code: u32) -> Option<u32> {
        let len = match self.scheme {
            Scheme::OneByte => 1,
            Scheme::TwoBytes => 2,
            Scheme::Mixed => (4 - code.leading_zeros() / 8).max(1) as u8,
        };
        self.base?.lookup_cid_code(code, len)
    }

    /// The code `rest` starts with, by its scheme, and how many bytes it
    /// takes of it.
    fn next(&self, rest: &[u8]) -> (usize, u32) {
        match self.scheme {
            Scheme::OneByte => (1, u32::from(rest[0])),
            Scheme::TwoBytes => two_bytes(rest),
            Scheme::Mixed => mixed(&self.codespace, rest),
        }
    }

    /// The mappings of `data`, a CMap, onto `ranges`: its codespace
    /// ranges, `cidrange`s and `cidchar`s, its `/WMode`, and the
    /// predefined CMap its `usecmap` names; read as far as it lexes.
    fn parse(
        &mut self,
        data: &[u8],
        lim: &Limits,
        ranges: &mut Vec<(u32, u32, (u32, u32))>,
        unknown: &mut bool,
    ) {
        let depth = lim.content.depth;
        let code = |operand: &[u8]| match lex::object(operand, depth)? {
            Object::String(s, _) if (1..=4).contains(&s.len()) => Some(number(&s)),
            _ => None,
        };
        let cid = |operand: &[u8]| u32::try_from(lex::object(operand, depth)?.as_i64().ok()?).ok();
        let _ = lex::steps(data, &ResCx::none(), lim, |step| {
            let args: Vec<&[u8]> = step.args().collect();
            match step.tok {
                Tok::Op(b"endcodespacerange") => {
                    self.codespace.extend(codespace(&args, depth));
                }
                Tok::Op(b"endcidrange") => {
                    for &[lo, hi, first] in args.as_chunks::<3>().0 {
                        if let (Some(lo), Some(hi), Some(first)) = (code(lo), code(hi), cid(first))
                        {
                            ranges.push((lo, hi, (lo, first)));
                        }
                    }
                }
                Tok::Op(b"endcidchar") => {
                    for &[c, to] in args.as_chunks::<2>().0 {
                        if let (Some(c), Some(to)) = (code(c), cid(to)) {
                            ranges.push((c, c, (c, to)));
                        }
                    }
                }
                Tok::Op(b"def") if step.name(1).as_deref() == Some(&b"WMode"[..]) => {
                    let mode = step.arg(0).and_then(|m| lex::object(m, depth));
                    self.vertical = mode.and_then(|m| m.as_i64().ok()).is_some_and(|m| m != 0);
                }
                Tok::Op(b"usecmap") => {
                    if let Some(name) = step.name(0) {
                        self.uses(&name, ranges, unknown);
                    }
                }
                _ => {}
            }
            Ok::<_, LexError>(())
        });
    }

    /// The predefined CMap `name` used, under what's mapped after it:
    /// Identity's codespace and CIDs; another's CIDs, where sheaf has it,
    /// for the codes nothing else maps, which PDFium maps to CID 0, so
    /// that's guessed (`unknown`), as one sheaf hasn't is.
    fn uses(&mut self, name: &[u8], ranges: &mut Vec<(u32, u32, (u32, u32))>, unknown: &mut bool) {
        if matches!(name, b"Identity-H" | b"Identity-V") {
            self.codespace.push((vec![0, 0], vec![0xff, 0xff]));
            ranges.push((0, 0xffff, (0, 0)));
        } else {
            self.base = predefined::bundled(name).or(self.base);
            *unknown = true;
        }
    }
}

/// A `codespacerange`'s ranges, from its operands: pairs of strings of
/// one length, 1 to 4 bytes.
fn codespace<'a>(args: &'a [&[u8]], depth: usize) -> impl Iterator<Item = (Vec<u8>, Vec<u8>)> + 'a {
    let string = move |operand: &[u8]| match lex::object(operand, depth)? {
        Object::String(s, _) => Some(s),
        _ => None,
    };
    args.as_chunks::<2>().0.iter().filter_map(move |&[lo, hi]| {
        let (lo, hi) = (string(lo)?, string(hi)?);
        (lo.len() == hi.len() && (1..=4).contains(&lo.len())).then_some((lo, hi))
    })
}

/// The code `rest` starts with by `space`'s ranges, as PDFium reads a
/// CMap of more than one (`GetNextChar`'s mixed rule): its bytes taken a
/// byte at a time while a range of more bytes holds what's taken so far
/// (`CheckFourByteCodeRange`); 0, the bytes read taken, where none does,
/// or where the code would run past four bytes or the end. How many
/// bytes it takes.
fn mixed(space: &[(Vec<u8>, Vec<u8>)], rest: &[u8]) -> (usize, u32) {
    let mut size = 1;
    loop {
        let codes = &rest[..size];
        match code_range(space, codes) {
            Held::Not => return (size, 0),
            Held::Whole => return (size, number(codes)),
            Held::Begun if size == 4 || size == rest.len() => return (size, 0),
            Held::Begun => size += 1,
        }
    }
}

/// What PDFium's `CheckFourByteCodeRange` says of `codes`.
enum Held {
    /// No range begins with its first byte.
    Not,
    /// It's a code.
    Whole,
    /// It begins one: it takes another byte.
    Begun,
}

/// Whether `space` holds `codes`, as PDFium checks (`CheckFourByteCodeRange`):
/// by the last range of as many bytes or more whose first byte's bounds
/// hold its first, a code where it's as long as that range, or as long as
/// the bytes of it within its bounds; else begun.
fn code_range(space: &[(Vec<u8>, Vec<u8>)], codes: &[u8]) -> Held {
    for (lo, hi) in space.iter().rev().filter(|(lo, _)| lo.len() >= codes.len()) {
        let within = (codes.iter().zip(lo.iter().zip(hi)))
            .take_while(|(c, (lo, hi))| lo <= c && c <= hi)
            .count();
        if within == lo.len() || within > 0 && codes.len() == lo.len() {
            return Held::Whole;
        }
        if within > 0 {
            return Held::Begun;
        }
    }
    Held::Not
}

/// Stream `id`'s content decompressed within `budget`, charged to it:
/// None where it isn't a stream, or can't be read or decoded.
fn decompressed(
    objects: &impl ReadObjects,
    id: ObjectId,
    budget: &mut usize,
) -> Result<Option<Vec<u8>>, TooLarge> {
    let read = objects.read(id, |o| {
        let stream = o.as_stream().ok()?;
        Some(stream.decompressed_content_with_limit(*budget))
    });
    match read {
        Ok(Some(decoded)) => charged(decoded, budget),
        _ => Ok(None),
    }
}

/// `decoded`, a stream's content decompressed within `budget`, charged to
/// it: None where it can't be decoded.
pub(super) fn charged(
    decoded: lopdf::Result<Vec<u8>>,
    budget: &mut usize,
) -> Result<Option<Vec<u8>>, TooLarge> {
    match decoded {
        Ok(data) => {
            *budget = budget.saturating_sub(data.len());
            Ok(Some(data))
        }
        Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. })) => {
            Err(TooLarge)
        }
        Err(_) => Ok(None),
    }
}

/// How many bytes the code `rest` starts with takes, by `space`'s ranges:
/// the one it's in, or the shortest one's. None where there are none.
fn codespace_len(space: &[(Vec<u8>, Vec<u8>)], rest: &[u8]) -> Option<usize> {
    let within = |(lo, hi): &(Vec<u8>, Vec<u8>)| {
        rest.len() >= lo.len() && (0..lo.len()).all(|i| lo[i] <= rest[i] && rest[i] <= hi[i])
    };
    space
        .iter()
        .find(|r| within(r))
        .or_else(|| space.iter().min_by_key(|(lo, _)| lo.len()))
        .map(|(lo, _)| lo.len())
}

/// Bytes as a big-endian number: a code.
fn number(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |c, &b| c << 8 | u32::from(b))
}

/// A simple font's encoding as written: its base encoding, and the glyphs
/// its `/Differences` name over it.
struct Encoded<'a> {
    /// Its `/Encoding`'s or `/BaseEncoding`'s, or the font's own.
    base: Option<Encoding>,
    differences: [Option<&'a [u8]>; 256],
}

impl<'a> Encoded<'a> {
    /// `font`'s encoding, `own` its base where it names none it knows.
    fn read(objects: &'a impl ReadObjects, font: &'a Dictionary, own: Option<Encoding>) -> Self {
        let encoding = font.get(b"Encoding").ok().and_then(|e| direct(objects, e));
        let (base, differences) = match encoding {
            Some(Object::Name(n)) => (Encoding::named(n).or(own), None),
            Some(Object::Dictionary(d)) => {
                let base = d.get(b"BaseEncoding").ok().and_then(|b| b.as_name().ok());
                let differences = d
                    .get(b"Differences")
                    .ok()
                    .and_then(|o| direct(objects, o))
                    .and_then(|o| o.as_array().ok());
                (base.and_then(Encoding::named).or(own), differences)
            }
            _ => (own, None),
        };
        let mut names: [Option<&[u8]>; 256] = [None; 256];
        let mut code = 0;
        for item in differences.into_iter().flatten() {
            match item {
                Object::Integer(c) => code = *c,
                Object::Name(n) => {
                    if let Some(slot) = usize::try_from(code).ok().and_then(|c| names.get_mut(c)) {
                        *slot = Some(n);
                    }
                    code = code.saturating_add(1);
                }
                _ => {}
            }
        }
        Encoded {
            base,
            differences: names,
        }
    }

    /// The name of the glyph `code` draws: the one `/Differences` gives
    /// it, or its base encoding's.
    fn glyph_name(&self, code: u8) -> Option<&'a [u8]> {
        match self.differences[code as usize] {
            Some(name) => Some(name),
            None => Some(self.base?.glyph(code)?.as_bytes()),
        }
    }

    /// [`Encoded::glyph_name`], where it's text.
    fn glyph(&self, code: u8) -> Option<&'a str> {
        std::str::from_utf8(self.glyph_name(code)?).ok()
    }
}

/// The name `dict`'s `key` is, its reference followed.
fn get_name<'a>(
    objects: &'a impl ReadObjects,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a [u8]> {
    direct(objects, dict.get(key).ok()?)?.as_name().ok()
}

/// The dictionary `dict`'s `key` is, its reference followed.
fn get_dict<'a>(
    objects: &'a impl ReadObjects,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Dictionary> {
    direct(objects, dict.get(key).ok()?)?.as_dict().ok()
}

/// The array `dict`'s `key` is, its reference followed.
fn get_array<'a>(
    objects: &'a impl ReadObjects,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a [Object]> {
    let array = direct(objects, dict.get(key).ok()?)?.as_array().ok()?;
    Some(array.as_slice())
}

/// The whole number `dict`'s `key` is ([`whole`]).
fn get_whole(objects: &impl ReadObjects, dict: &Dictionary, key: &[u8]) -> Option<f32> {
    whole(objects, dict.get(key).ok()?)
}

/// A number, cut to its whole part and kept within an `i32`, as PDFium
/// reads one where it wants a whole one.
fn integer(objects: &impl ReadObjects, o: &Object) -> Option<i64> {
    let n = direct(objects, o)?.as_float().ok()?;
    Some(n.trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i64)
}

/// [`integer`], as a metric.
fn whole(objects: &impl ReadObjects, o: &Object) -> Option<f32> {
    integer(objects, o).map(|n| n as f32)
}

/// A number, as written.
fn number_of(objects: &impl ReadObjects, o: &Object) -> Option<f32> {
    let n = direct(objects, o)?.as_float().ok()? as f32;
    n.is_finite().then_some(n)
}

/// An array's first four numbers, read by `read`, as a rectangle.
fn rect<R: ReadObjects>(
    objects: &R,
    array: &[Object],
    read: fn(&R, &Object) -> Option<f32>,
) -> Option<Rect> {
    let [l, b, r, t] = array.get(..4)? else {
        return None;
    };
    let n = |o| read(objects, o);
    Some(Rect::new(n(l)?, n(b)?, n(r)?, n(t)?))
}

/// A matrix's six numbers.
fn affine(objects: &impl ReadObjects, array: &[Object]) -> Option<Affine> {
    let [a, b, c, d, e, f] = array.get(..6)? else {
        return None;
    };
    let n = |o| number_of(objects, o);
    Some(Affine {
        a: n(a)?,
        b: n(b)?,
        c: n(c)?,
        d: n(d)?,
        e: n(e)?,
        f: n(f)?,
    })
}

/// A source's fonts, each read once: those that are objects by number,
/// kept for every page that draws in them, as
/// [`DrawnForms`](crate::content::DrawnForms) keeps the forms they draw.
///
/// Each font's program is read with it, for its glyphs' boxes and its
/// heights, and charged to the budget it's read within; but by fonts made
/// for text ([`Fonts::for_text`]), which read none, as text needs none.
#[derive(Debug, Default)]
pub struct Fonts {
    read: Mutex<HashMap<ObjectId, Arc<Foreign>>>,
    stock: OnceLock<Arc<Foreign>>,
    /// Whether it reads no font's program.
    for_text: bool,
}

impl Fonts {
    /// Fonts read for the text their strings are and where their glyphs
    /// are placed, and no more: no program is read, nor charged to a
    /// budget, so each glyph's box is its metrics' alone, and heights a
    /// program would give are guessed ([`FontApprox`]). The removal check
    /// reads text with these ([`text_bag`](crate::content::text_bag)).
    ///
    /// A page interprets with these as with fonts that read programs, its
    /// text placed past what an `f32` holds refused alike
    /// ([`InterpError::NotFinite`]), but where a string is shown so large,
    /// or so far out, that its glyphs' outline boxes would reach past that
    /// and their metrics wouldn't: a font's extent takes in its outlines,
    /// a CIDFont's as far as any can reach ([`Foreign::extent`]), so such
    /// a string is refused only where programs are read. No page comes
    /// near it: it takes an em past 10³⁰ points.
    ///
    /// [`InterpError::NotFinite`]: crate::content::interp::InterpError::NotFinite
    pub fn for_text() -> Fonts {
        Fonts {
            for_text: true,
            ..Fonts::default()
        }
    }

    /// The font `font`, a font resource as written, is: read once where
    /// it's an object, kept; read anew where it's a dictionary. None for
    /// anything else. What its streams decompress to is charged to
    /// `budget` whether it's read now or was before ([`Foreign::read`]),
    /// so what it costs is the same whichever page read it first.
    pub fn get(
        &self,
        objects: &impl ReadObjects,
        font: &Object,
        lim: &Limits,
        budget: &mut usize,
    ) -> Result<Option<Arc<Foreign>>, TooLarge> {
        let &Object::Reference(id) = font else {
            let Ok(dict) = font.as_dict() else {
                return Ok(None);
            };
            return Ok(Some(Arc::new(self.read(objects, dict, lim, budget)?)));
        };
        if let Some(known) = self.lock().get(&id) {
            *budget = budget.checked_sub(known.cost).ok_or(TooLarge)?;
            return Ok(Some(known.clone()));
        }
        let Some(dict) = objects.object(id).ok().and_then(|o| o.as_dict().ok()) else {
            return Ok(None);
        };
        let foreign = Arc::new(Foreign {
            id: Some(id),
            ..self.read(objects, dict, lim, budget)?
        });
        Ok(Some(self.lock().entry(id).or_insert(foreign).clone()))
    }

    /// What PDFium draws text in where the resources have no font by the
    /// name `Tf` gives: its stock Helvetica, in WinAnsiEncoding.
    pub fn stock(&self) -> Arc<Foreign> {
        let read = || {
            let helvetica = dictionary! {
                "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            };
            let none = lopdf::Document::new();
            let read = Foreign::read(&none, &helvetica, &Limits::DEFAULT, &mut 0);
            Arc::new(read.expect("a font by name alone has no stream to decompress"))
        };
        self.stock.get_or_init(read).clone()
    }

    /// `font` read, its program read but for text.
    fn read(
        &self,
        objects: &impl ReadObjects,
        font: &Dictionary,
        lim: &Limits,
        budget: &mut usize,
    ) -> Result<Foreign, TooLarge> {
        Foreign::read_as(objects, font, lim, budget, !self.for_text)
    }

    /// The fonts one interpretation picks.
    pub fn scope(&self) -> FontScope<'_> {
        FontScope {
            fonts: self,
            picked: Vec::new(),
            by_name: HashMap::new(),
            charged: HashMap::new(),
            stock: None,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ObjectId, Arc<Foreign>>> {
        self.read.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The fonts one interpretation picks (`Tf`), by name in the resources
/// they're picked from: each looked up once however often it's picked,
/// and one written in the resources as a dictionary read once. Each is
/// known by its place among them ([`FontId`]), which the graphics state
/// keeps, so saving the state counts no reference to it.
#[derive(Debug)]
pub struct FontScope<'f> {
    fonts: &'f Fonts,
    /// The fonts picked, each once, by [`FontId`].
    picked: Vec<Arc<Foreign>>,
    /// By the object whose resources they're in, then by name.
    by_name: HashMap<ObjectId, HashMap<Vec<u8>, Option<FontId>>>,
    /// The fonts by object picked, each charged once.
    charged: HashMap<ObjectId, FontId>,
    /// The stock Helvetica, once it's picked.
    stock: Option<FontId>,
}

/// A font an interpretation picked: its place among its [`FontScope`]'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(u32);

impl FontScope<'_> {
    /// Font `name` of `resources`, the `/Resources` as written of `owner`:
    /// the page, form, pattern or Type3 font whose content picks it. None
    /// where they have no such font. What its streams decompress to is
    /// charged to `budget` the first time it's picked, by any name, and a
    /// font written in resources as a dictionary each time it's read.
    pub fn font(
        &mut self,
        objects: &impl ReadObjects,
        owner: ObjectId,
        resources: Option<&Object>,
        name: &[u8],
        lim: &Limits,
        budget: &mut usize,
    ) -> Result<Option<FontId>, TooLarge> {
        if let Some(&font) = self.by_name.get(&owner).and_then(|n| n.get(name)) {
            return Ok(font);
        }
        let written = resources
            .and_then(|r| direct(objects, r)?.as_dict().ok())
            .and_then(|r| get_dict(objects, r, b"Font"))
            .and_then(|f| f.get(name).ok());
        let font = match written {
            Some(Object::Reference(id)) if self.charged.contains_key(id) => {
                self.charged.get(id).copied()
            }
            Some(written) => match self.fonts.get(objects, written, lim, budget)? {
                Some(read) => {
                    let object = read.id;
                    let font = self.add(read);
                    if let Some(id) = object {
                        self.charged.insert(id, font);
                    }
                    Some(font)
                }
                None => None,
            },
            None => None,
        };
        let by_name = self.by_name.entry(owner).or_default();
        by_name.insert(name.to_vec(), font);
        Ok(font)
    }

    /// PDFium's stock Helvetica ([`Fonts::stock`]), picked.
    pub fn stock(&mut self) -> FontId {
        match self.stock {
            Some(font) => font,
            None => {
                let font = self.add(self.fonts.stock());
                self.stock = Some(font);
                font
            }
        }
    }

    /// The font `font` is.
    pub fn get(&self, font: FontId) -> &Foreign {
        &self.picked[font.0 as usize]
    }

    /// `font` among those picked. There are fewer than operators
    /// interpreted, which are capped far below 2^32.
    fn add(&mut self, font: Arc<Foreign>) -> FontId {
        self.picked.push(font);
        FontId((self.picked.len() - 1) as u32)
    }
}

/// How one font's strings read as text.
#[derive(Debug, Clone)]
pub(crate) struct Decoder {
    /// Bytes a code takes where the codespace ranges don't say.
    width: usize,
    /// Whether the codespace ranges say how long a code is.
    ranged: bool,
    /// Whether its font's `/ToUnicode` is a stream it read, whatever that
    /// maps.
    to_unicode: bool,
    cmap: CMap,
    /// A simple font's codes, by its encoding.
    encoded: Option<Box<[Option<String>]>>,
}

impl Decoder {
    /// `font`'s, its `/ToUnicode` decompressed within `budget`, charged to
    /// it.
    fn read(
        objects: &impl ReadObjects,
        font: &Dictionary,
        lim: &Limits,
        budget: &mut usize,
    ) -> Result<Decoder, TooLarge> {
        let cmap = match font
            .get(b"ToUnicode")
            .ok()
            .and_then(|o| o.as_reference().ok())
        {
            Some(id) => decompressed(objects, id, budget)?.map(|bytes| CMap::parse(&bytes, lim)),
            None => None,
        };
        Ok(Decoder {
            to_unicode: cmap.is_some(),
            ..Decoder::new(objects, font, cmap.unwrap_or_default())
        })
    }

    fn new(objects: &impl ReadObjects, font: &Dictionary, cmap: CMap) -> Decoder {
        let name = |key: &[u8]| {
            let object = direct(objects, font.get(key).ok()?)?;
            object.as_name().ok()
        };
        if name(b"Subtype") == Some(b"Type0") {
            let identity = matches!(name(b"Encoding"), Some(b"Identity-H" | b"Identity-V"));
            return Decoder {
                width: 2,
                ranged: !identity && !cmap.codespace.is_empty(),
                to_unicode: false,
                cmap,
                encoded: None,
            };
        }
        // A subset's name has a tag: ABCDEF+Symbol.
        let base = name(b"BaseFont").map(|n| match n.iter().position(|&c| c == b'+') {
            Some(6) => &n[7..],
            _ => n,
        });
        let own = match base {
            Some(b) if b.starts_with(b"Symbol") => Some(Encoding::Symbol),
            Some(b"ZapfDingbats" | b"Dingbats") => Some(Encoding::ZapfDingbats),
            _ => matches!(name(b"Subtype"), Some(b"Type1" | b"MMType1"))
                .then_some(Encoding::Standard),
        };
        let Encoded { base, differences } = Encoded::read(objects, font, own);
        let dingbats = own == Some(Encoding::ZapfDingbats);
        let encoded = (0..=255u8)
            .map(|code| match differences[code as usize] {
                Some(n) => glyph_text(std::str::from_utf8(n).ok()?, dingbats),
                None => base?.text(code),
            })
            .collect();
        Decoder {
            width: 1,
            ranged: false,
            to_unicode: false,
            cmap,
            encoded: Some(encoded),
        }
    }

    /// `s`'s codes as text: None when the font doesn't say the text of
    /// one of them.
    fn text(&self, s: &[u8]) -> Option<String> {
        let mut out = String::new();
        let mut at = 0;
        while at < s.len() {
            let rest = &s[at..];
            let len = self.code_len(rest).clamp(1, rest.len());
            let code = number(&rest[..len]);
            at += len;
            out.push_str(&self.code_text(code)?);
        }
        Some(out)
    }

    /// The text `code` is: its `/ToUnicode` mapping, or its encoding's.
    fn code_text(&self, code: u32) -> Option<Cow<'_, str>> {
        match self.cmap.get(code) {
            Some(text) => Some(text),
            None => Some(Cow::Borrowed(
                self.encoded.as_ref()?.get(code as usize)?.as_deref()?,
            )),
        }
    }

    /// How many bytes the code `rest` starts with takes: the codespace
    /// range it's in, or the shortest one's, or the font's width.
    fn code_len(&self, rest: &[u8]) -> usize {
        if !self.ranged {
            return self.width;
        }
        codespace_len(&self.cmap.codespace, rest).unwrap_or(self.width)
    }
}

/// A `/ToUnicode` CMap.
#[derive(Debug, Clone, Default)]
struct CMap {
    /// Codespace ranges, each its first and last code, both as long as
    /// the codes in it.
    codespace: Vec<(Vec<u8>, Vec<u8>)>,
    /// Codes mapped one at a time.
    chars: HashMap<u32, String>,
    /// Ranges whose text counts up from their first code's, by first
    /// code: first, last, the first code's text.
    ranges: Vec<(u32, u32, Vec<char>)>,
}

impl CMap {
    /// The mappings of `bytes`, a CMap; read as far as it lexes.
    fn parse(bytes: &[u8], lim: &Limits) -> CMap {
        let mut cmap = CMap::default();
        let depth = lim.content.depth;
        let string = |operand: &[u8]| match lex::object(operand, depth)? {
            Object::String(s, _) => Some(s),
            _ => None,
        };
        let code = |operand: &[u8]| {
            let s = string(operand)?;
            (1..=4).contains(&s.len()).then(|| number(&s))
        };
        let _ = lex::steps(bytes, &ResCx::none(), lim, |step| {
            let args: Vec<&[u8]> = step.args().collect();
            match step.tok {
                Tok::Op(b"endcodespacerange") => {
                    cmap.codespace.extend(codespace(&args, depth));
                }
                Tok::Op(b"endbfchar") => {
                    for &[c, text] in args.as_chunks::<2>().0 {
                        if let (Some(c), Some(text)) = (code(c), destination(text, depth)) {
                            cmap.chars.insert(c, text);
                        }
                    }
                }
                Tok::Op(b"endbfrange") => {
                    for &[lo, hi, text] in args.as_chunks::<3>().0 {
                        let (Some(lo), Some(hi)) = (code(lo), code(hi)) else {
                            continue;
                        };
                        if lo > hi {
                            continue;
                        }
                        match lex::object(text, depth) {
                            Some(Object::String(s, _)) => {
                                cmap.ranges.push((lo, hi, utf16(&s).chars().collect()));
                            }
                            Some(Object::Array(items)) => {
                                let texts = items.iter().filter_map(|i| match i {
                                    Object::String(s, _) => Some(utf16(s)),
                                    _ => None,
                                });
                                for (c, text) in (lo..=hi).zip(texts) {
                                    cmap.chars.insert(c, text);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            Ok::<_, LexError>(())
        });
        cmap.ranges.sort_by_key(|r| r.0);
        cmap
    }

    /// The text `code` maps to: its own mapping, or the range it's in,
    /// the range's text with its last character counted up.
    fn get(&self, code: u32) -> Option<Cow<'_, str>> {
        if let Some(text) = self.chars.get(&code) {
            return Some(Cow::Borrowed(text));
        }
        let i = self
            .ranges
            .partition_point(|r| r.0 <= code)
            .checked_sub(1)?;
        let (lo, hi, first) = &self.ranges[i];
        if code > *hi {
            return None;
        }
        let (last, rest) = first.split_last()?;
        let last = char::from_u32(u32::from(*last).checked_add(code - lo)?)?;
        Some(Cow::Owned(rest.iter().chain([&last]).collect()))
    }
}

/// A `bfchar` destination's text: a string's UTF-16, or a glyph name's.
fn destination(operand: &[u8], depth: usize) -> Option<String> {
    match lex::object(operand, depth)? {
        Object::String(s, _) => Some(utf16(&s)),
        Object::Name(n) => glyph_text(std::str::from_utf8(&n).ok()?, false),
        _ => None,
    }
}

/// A CMap destination string's text: UTF-16BE, or one byte's code.
fn utf16(s: &[u8]) -> String {
    if let [b] = s {
        return char::from(*b).to_string();
    }
    let units = s.as_chunks::<2>().0.iter().map(|&u| u16::from_be_bytes(u));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}
