//! Text shown: each string's glyphs, where PDFium places them
//! (`CPDF_TextObject::CalcPositionDataInternal`) and the box each spans,
//! as PDFium bounds a character (`cpdf_textpage.cpp`'s `GetLooseBounds`):
//! its metrics' box united with its outline's, where the font's program
//! says it.

use std::borrow::Cow;
use std::ops::Range;

use super::state::{State, TextState};
use crate::annot::Pt;
use crate::content::{FontApprox, FontKind, Foreign, TextMode};
use crate::geom::{Affine, Rect};

/// Four corners in PageSpace, in order around a box: a glyph's
/// bottom-left, bottom-right, top-right and top-left, as its text space
/// has them upright.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad(pub [Pt; 4]);

impl Quad {
    /// The smallest rectangle that holds it.
    pub fn bounds(&self) -> Rect {
        let [first, rest @ ..] = &self.0;
        rest.iter()
            .fold(Rect::new(first.x, first.y, first.x, first.y), |r, p| Rect {
                x0: r.x0.min(p.x),
                y0: r.y0.min(p.y),
                x1: r.x1.max(p.x),
                y1: r.y1.max(p.y),
            })
    }

    /// Whether its corners are all finite.
    pub(super) fn is_finite(&self) -> bool {
        self.0.iter().all(|p| p.x.is_finite() && p.y.is_finite())
    }

    /// The box `r`, in a space `m` maps onto PageSpace.
    pub(super) fn of(r: Rect, m: Affine) -> Quad {
        let corners = [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)];
        Quad(corners.map(|(x, y)| m.apply(Pt { x, y })))
    }
}

/// A string shown by `Tj`, one of `TJ`'s, or by `'` or `"`: one that
/// isn't empty, as PDFium shows none that is. A `TJ`'s strings are laid
/// out as PDFium lays them out (`CPDF_TextObject`), one text object: from
/// where the `TJ` starts, each string on from where the one before it and
/// its adjustment end.
#[derive(Debug, Clone)]
pub struct TextShow<'e> {
    /// The string, its escapes read.
    pub string: &'e [u8],
    /// The font it's shown in: the one `Tf`, or an ExtGState's `/Font`,
    /// picked, PDFium's stock Helvetica where the resources have none by
    /// the name it gave ([`Fonts::stock`](crate::content::Fonts::stock));
    /// None before either, where PDFium draws nothing, and it has no
    /// glyphs.
    pub font: Option<&'e Foreign>,
    /// Whether the font is PDFium's stock Helvetica, the resources having
    /// none by the name `Tf` gave.
    pub stock: bool,
    /// The font size.
    pub size: f32,
    /// How it's drawn: `Tr`, but fill for a Type3 font, which PDFium fills
    /// whatever `Tr` says.
    pub mode: TextMode,
    /// Text space where its operator's text starts onto PageSpace, as
    /// glyphs are laid out in it, at the font size and before `Tz`: `Tz`,
    /// then the rise and the pen, the text matrix and the CTM. A glyph `x`
    /// along the line has its origin at `(x, 0)`, in vertical writing a
    /// glyph `y` down the column at `(0, y)`.
    pub start: Affine,
    /// Where along the line, or down the column, from `start`, it starts:
    /// 0 for `Tj`'s, and for a `TJ`'s later strings how far the strings
    /// and adjustments before them move.
    pub offset: f32,
    text: &'e TextState,
}

impl<'e> TextShow<'e> {
    /// `string`, shown in `font` `offset` from where its operator's text
    /// starts, the pen of `state`.
    pub(super) fn new(
        string: &'e [u8],
        state: &'e State,
        font: Option<&'e Foreign>,
        offset: f32,
    ) -> TextShow<'e> {
        let text = &state.text;
        let type3 = font.is_some_and(|f| f.kind() == FontKind::Type3);
        let at = Affine {
            a: text.scale,
            e: text.pen.x,
            f: text.pen.y + text.rise,
            ..Affine::IDENTITY
        };
        TextShow {
            string,
            font,
            stock: text.stock,
            size: text.size,
            mode: if type3 { TextMode::Fill } else { text.mode },
            start: at.then(text.matrix).then(state.ctm),
            offset,
            text,
        }
    }

    /// Whether each of its `n` glyphs in `font`, laid out to `end`, is
    /// placed within what an `f32` holds, its origin, box, advance and
    /// pen: a bound, worked in `f64`, from the font's [extent], the
    /// spacing and how far it starts along the line, through `start`,
    /// below [`PLACED`]. A string whose bound is past that is taken for
    /// one that isn't, as content no real page has.
    ///
    /// [extent]: Foreign::extent
    pub(super) fn bounded(&self, font: &Foreign, n: usize, end: f32) -> bool {
        let text = self.text;
        let size = f64::from(text.size.abs());
        let em = f64::from(font.extent()) * size / 1000.0;
        let step = em + f64::from(text.word_space.abs()) + f64::from(text.char_space.abs());
        // How far any point laid out is from where the operator's text
        // starts, along the line and across it, at most.
        let reach = f64::from(self.offset.abs()) + n as f64 * step + em + size / 2.0;
        let s = self.start;
        let spread = [s.a, s.b, s.c, s.d]
            .iter()
            .map(|v| f64::from(v.abs()))
            .sum::<f64>();
        let corner = f64::from(s.e.abs()) + f64::from(s.f.abs()) + spread * reach;
        let pen = f64::from(text.pen.x.abs()) + f64::from(text.pen.y.abs());
        let along = pen + 2.0 * reach * (1.0 + f64::from(text.scale.abs()));
        end.is_finite() && corner < PLACED && along < PLACED
    }

    /// The text rendering matrix where its operator's text starts (PDF
    /// 32000-1 9.4.4): text space at a font size of 1 onto PageSpace.
    pub fn trm(&self) -> Affine {
        Affine {
            a: self.size,
            d: self.size,
            ..Affine::IDENTITY
        }
        .then(self.start)
    }

    /// Its glyphs, a code each, in order.
    pub fn glyphs(&self) -> impl Iterator<Item = Glyph<'e>> + 'e {
        let TextShow {
            string,
            font,
            mode,
            start,
            offset,
            text,
            ..
        } = *self;
        let invisible = matches!(mode, TextMode::Invisible | TextMode::Clip);
        font.into_iter().flat_map(move |font| {
            let of_font = font.approx();
            let outlined = font.outlined();
            let vertical = font.is_vertical();
            let size = text.size / 1000.0;
            let metrics = font.metrics();
            // Whether a width before the glyph is guessed.
            let mut moved = text.guessed;
            laid(font, text, string, offset).map(move |laid| {
                let guessed = font.width_guessed(laid.code);
                let outline = outlined.then(|| font.char_box(laid.code)).flatten();
                let approx = FontApprox {
                    widths: of_font.widths || moved || guessed,
                    boxes: of_font.boxes || (outlined && outline.is_none()),
                    ..of_font
                };
                moved |= guessed;
                let (origin, cell, pen) = if vertical {
                    let v = font.vertical(laid.code);
                    let (vx, vy) = v.map_or((0.0, 0.0), |v| (v.vx, v.vy));
                    let origin = Pt {
                        x: -text.size * vx / 1000.0,
                        y: laid.at - text.size * vy / 1000.0,
                    };
                    // The em-wide column its advance spans, centred on the
                    // pen, as PDFium bounds a vertical glyph.
                    let half = text.size / 2.0;
                    let cell = Rect::new(-half, laid.at + laid.width, half, laid.at);
                    (origin, cell, Pt { x: 0.0, y: laid.at })
                } else {
                    let origin = Pt { x: laid.at, y: 0.0 };
                    let cell = Rect::new(
                        laid.at,
                        metrics.descent * size,
                        laid.at + laid.width,
                        metrics.ascent * size,
                    );
                    let pen = Pt {
                        x: laid.at * text.scale,
                        y: 0.0,
                    };
                    (origin, cell, pen)
                };
                // United with its outline's box, as PDFium unites them.
                let cell = match outline {
                    Some(r) => {
                        let o = outline_box(r, origin, size, laid.units * size);
                        Rect {
                            x0: cell.x0.min(o.x0),
                            y0: cell.y0.min(o.y0),
                            x1: cell.x1.max(o.x1),
                            y1: cell.y1.max(o.y1),
                        }
                    }
                    None => cell,
                };
                Glyph {
                    cid: font.cid(laid.code),
                    code: laid.code,
                    origin: start.apply(origin),
                    advance: if vertical {
                        laid.end - laid.at
                    } else {
                        (laid.end - laid.at) * text.scale
                    },
                    quad: Quad::of(cell, start),
                    approx,
                    invisible,
                    pen: Pt {
                        x: text.pen.x + pen.x,
                        y: text.pen.y + pen.y,
                    },
                    bytes: laid.bytes,
                    font,
                    text,
                }
            })
        })
    }
}

/// How far from 0 a glyph is placed at most ([`TextShow::bounded`]): a
/// tenth of what an `f32` holds, so what's worked in `f32` on the way,
/// each step rounded, stays within it.
const PLACED: f64 = 3e37;

/// How short a side of a glyph's outline box may be before PDFium takes
/// it out (`kSizeEpsilon`), in text space at the font size.
const SIZE_EPSILON: f32 = 0.01;

/// The box PDFium gives a glyph's outline, its tight box (`CPDF_TextPage::
/// ProcessTextObject`), in the text space it's laid out in: `r`, its
/// font's box for its code in glyph units ([`Foreign::char_box`]), at
/// `size`, a thousandth of the font size, from its origin `origin`; a side
/// shorter than [`SIZE_EPSILON`] taken out, its top to `size` above its
/// bottom, its right to `width` past its left, its advance at the font
/// size, each worked in PDFium's order.
fn outline_box(r: Rect, origin: Pt, size: f32, width: f32) -> Rect {
    let (x0, y0) = (r.x0 * size + origin.x, r.y0 * size + origin.y);
    let (mut x1, mut y1) = (r.x1 * size + origin.x, r.y1 * size + origin.y);
    if (y1 - y0).abs() < SIZE_EPSILON {
        y1 = y0 + size;
    }
    if (x1 - x0).abs() < SIZE_EPSILON {
        x1 = x0 + width;
    }
    Rect::new(x0, y0, x1, y1)
}

/// A glyph shown: a code of a string.
#[derive(Debug, Clone)]
pub struct Glyph<'e> {
    /// Where its code is in the string.
    pub bytes: Range<usize>,
    pub code: u32,
    /// The CID it selects, in a Type0 font whose CMap sheaf knows.
    pub cid: Option<u32>,
    /// Its origin in PageSpace; in vertical writing, its horizontal
    /// origin, where PDFium puts a character's.
    pub origin: Pt,
    /// How far it moves the pen in the text matrix's space, along the line
    /// or down the column: its width at the font size, `Tc`, and `Tw`
    /// where it's a space, through `Tz` along a line; `TJ`'s adjustments
    /// aren't its.
    pub advance: f32,
    /// The box it spans, as PDFium bounds a character (its loose box):
    /// across its width, from the font's descent to its ascent, or in
    /// vertical writing the em-wide column its advance spans; united with
    /// its outline's box where the font's program says it
    /// ([`Foreign::char_box`]), as PDFium takes that box out where it has
    /// no width or height. United in text space, so in text turned or
    /// skewed its bounds hold PDFium's box, which unites the two boxes'
    /// bounds in PageSpace; and where the font's ascent is its descent,
    /// which PDFium bounds by the outline's box alone, its heights are
    /// guessed ([`FontApprox::heights`]). Where no program says its outline
    /// ([`FontApprox::boxes`]), and in a Type3 font, whose glyphs' `d1`
    /// boxes ([`Foreign::glyph_box`]) aren't united, it holds what the
    /// metrics say, and the outline may reach past it.
    pub quad: Quad,
    /// What of the metrics it's placed by is guessed, so its box may be
    /// elsewhere: its font's ([`Foreign::approx`]); its width, where the
    /// font guesses its code's ([`Foreign::width_guessed`]), or one shown
    /// before it on its line, its string's or since the line was set
    /// ([`TextState::guessed`]), which moves it; and its outline, where
    /// the font's program doesn't say its code's.
    pub approx: FontApprox,
    /// Whether it draws nothing: shown in `3 Tr` or `7 Tr`.
    pub invisible: bool,
    /// Where it starts in the text matrix's space: where a string of its
    /// own would start, after a `Td` to here from `BT` in that matrix.
    pub pen: Pt,
    font: &'e Foreign,
    text: &'e TextState,
}

impl Glyph<'_> {
    /// The text its code is, where the font says ([`Foreign::code_text`]).
    pub fn text(&self) -> Option<Cow<'_, str>> {
        self.font.code_text(self.code)
    }

    /// The text matrix a string of its own would be shown with to start
    /// where it is: what `Tm` writes it at absolutely, the rest of the
    /// state unchanged.
    pub fn tm(&self) -> Affine {
        Affine {
            e: self.pen.x,
            f: self.pen.y,
            ..Affine::IDENTITY
        }
        .then(self.text.matrix)
    }
}

/// A code of a string, laid out along the line (or down the column) in
/// text space at the font size, before `Tz`, from where its operator's
/// text starts.
struct Laid {
    bytes: Range<usize>,
    code: u32,
    /// Where it starts.
    at: f32,
    /// Its width (`w0`), or its vertical advance (`w1`, negative), in
    /// glyph units.
    units: f32,
    /// That at the font size.
    width: f32,
    /// Where the pen goes past it: on by the width, then `Tw` where it's a
    /// space, then `Tc`, added in PDFium's order.
    end: f32,
}

/// `s`'s codes in `font`, laid out with `text`'s spacing, from `from`.
fn laid<'a>(
    font: &'a Foreign,
    text: &'a TextState,
    s: &'a [u8],
    from: f32,
) -> impl Iterator<Item = Laid> + 'a {
    let vertical = font.is_vertical();
    let mut at = from;
    font.codes(s).map(move |(bytes, code)| {
        let units = match vertical {
            true => font.vertical(code).map_or(0.0, |v| v.w1),
            false => font.width(code),
        };
        let width = units * text.size / 1000.0;
        let mut next = at + width;
        if font.word_spaced(code, bytes.len()) {
            next += text.word_space;
        }
        next += text.char_space;
        let laid = Laid {
            bytes,
            code,
            at,
            units,
            width,
            end: next,
        };
        at = next;
        laid
    })
}

/// Where showing `s` in `font` from `from` ends ([`laid`]), and how many
/// glyphs it shows.
pub(super) fn advance(font: &Foreign, text: &TextState, s: &[u8], from: f32) -> (f32, usize) {
    match font.byte_widths() {
        Some(widths) => (bytes_end(widths, text, s, from), s.len()),
        None => laid_end(font, text, s, from),
    }
}

/// Where [`laid`]'s last code of `s` from `from` ends, `from` where it
/// has none, and how many codes it lays out.
pub(super) fn laid_end(font: &Foreign, text: &TextState, s: &[u8], from: f32) -> (f32, usize) {
    let mut end = from;
    let mut glyphs = 0;
    for laid in laid(font, text, s, from) {
        end = laid.end;
        glyphs += 1;
    }
    (end, glyphs)
}

/// [`laid_end`]'s end where each code is a byte, its width `widths`':
/// the same sums, in the same order, without the codes split, as most
/// text is in such a font. A sum of widths from `from`, which is never
/// -0 (an operator's text starts at 0, and its adjustments subtract), is
/// never -0, to which adding 0 would make a difference, so where `Tc`
/// and `Tw` are both 0 neither is added.
pub(super) fn bytes_end(widths: &[f32; 256], text: &TextState, s: &[u8], from: f32) -> f32 {
    let mut at = from;
    if text.word_space == 0.0 && text.char_space == 0.0 {
        for &b in s {
            at += widths[usize::from(b)] * text.size / 1000.0;
        }
        return at;
    }
    for &b in s {
        let mut next = at + widths[usize::from(b)] * text.size / 1000.0;
        if b == 32 {
            next += text.word_space;
        }
        next += text.char_space;
        at = next;
    }
    at
}
