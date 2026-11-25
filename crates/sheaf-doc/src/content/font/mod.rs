//! Fonts sheaf draws text in: what codes a string is in a font, how far
//! they advance, and the font's resource for a page; and the fonts files
//! draw in, as read.
//!
//! - [`DrawFont`]: a font sheaf draws in, the PDF layer's [`Std14`] (the
//!   standard 14, with Adobe's widths and boxes) and sheaf's own
//!   [`Glyphless`], OCR's invisible text; [`DrawText`] writes text in one.
//! - [`Font`]: a file's font, its codes, widths and heights as PDFium
//!   reads them, by the PDF layer; [`Fonts`], a source's, each read once.
//! - [`Fallback`]: the font sheaf bundles to draw what another can't (M10
//!   slice 7), embedded as a subset of the glyphs a file draws in it, and
//!   bound late: content names a marker ([`Fallback::res`]), and what it
//!   draws in it travels with what it draws with ([`Uses`]) to the sink
//!   that writes the subset.
//!
//! Text is drawn in its font and, where that lacks a character, in the
//! fallback ([`DrawFont::encode`], M10 slice 7); what neither draws is `?`
//! and counted ([`Unencoded`]), so the writer can say so once; the text
//! itself (`/Contents`, a recipe) is kept exactly.

mod draw;
mod fallback;
mod glyphless;
#[cfg(test)]
mod tests;

use std::sync::OnceLock;

use sheaf_edit::FontChoice;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::pdf::fonts::LayoutFont;
use crate::pdf::gfx::{WithFallback, face_for};

pub use draw::DrawText;
pub use fallback::{Fallback, Uses, draws_all};
pub use glyphless::Glyphless;

pub use crate::pdf::fonts::{
    Descendant, Font, FontApprox, FontKind, FontMetrics, FontProgram, Fonts, Glyphs, Std14,
    VerticalMetrics,
};

use super::Res;

/// Characters no font sheaf bundles could draw: each drew `?` in the font
/// the text is in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unencoded(pub usize);

/// Which of a drawing's two fonts a run of its codes is in
/// ([`DrawFont::encode`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFont {
    /// The font the text is drawn in.
    Own,
    /// The fallback font sheaf bundles ([`Fallback::SANS`]).
    Fallback,
}

/// Text as a [`DrawFont`] draws it ([`DrawFont::encode`]): its codes in
/// runs, each in the font or the fallback, in order; the glyphs it draws
/// in the fallback, which whatever binds the drawing's resources joins
/// ([`Uses`]); and how many characters neither font could draw, each
/// drawn as `?` in the font.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextRuns {
    pub runs: Vec<(InFont, Vec<u8>)>,
    pub glyphs: Glyphs,
    pub unencoded: Unencoded,
}

/// A font sheaf draws text in, by the PDF layer's own fonts: what
/// [`LayoutFont`] gives of one (a character's codes, how far it
/// advances), the font as a page resource, and sheaf's rules for text
/// over them, written here once: a font says what it is ([`DrawFont::res`],
/// [`DrawFont::composed`], [`DrawFont::falls_back`]) and takes the rest as
/// given.
///
/// **Two fonts.** Text is drawn composed (NFC), a grapheme cluster at a
/// time, each in the first of the font and the fallback font sheaf bundles
/// ([`Fallback`]) that has all of it, as the PDF layer chooses a face
/// ([`face_for`] over [`WithFallback`]); a cluster neither has, or with a
/// control character in it, is drawn in the font as it always was, `?`
/// for each character it lacks, and counted ([`Unencoded`]). Text the
/// font draws whole is its own codes alone, the bytes and widths it
/// always was ([`DrawFont::encode_alone`], [`DrawFont::width_alone`]).
///
/// Any font sheaf embeds draws a glyph by its number in the whole font: a
/// code is that number, whatever subset a file embeds (each CID the
/// glyph's number in the whole font, a `/CIDToGIDMap` stream taking it to
/// the subset's own, as the [`Fallback`]'s does), so the codes a string is
/// drawn with depend only on the font and the text, and re-subsetting
/// never changes them. [`Glyphless`], embedded whole, is the one
/// exception: its CIDs are the text's UTF-16, all drawing its one glyph.
pub trait DrawFont: LayoutFont + Send + Sync {
    /// The font as a page resource: its dictionary and the objects it
    /// reaches, named by a digest of all of them, so a save writes one
    /// font once however many pages draw in it.
    fn res(&self) -> Res;

    /// Whether text is composed (NFC) before it's encoded, so a
    /// decomposed "é" is the one character a font has. Not in a font
    /// whose codes are the text as written ([`Glyphless`]).
    fn composed(&self) -> bool {
        true
    }

    /// Whether what it can't draw is drawn in the fallback font
    /// ([`Fallback`]). Not in a font that draws every character
    /// ([`Glyphless`]).
    fn falls_back(&self) -> bool {
        true
    }

    /// `s` as this font's own codes, nothing drawn in the fallback: a
    /// character it can't encode is drawn as `?`, or left out by a font
    /// without one (ZapfDingbats), and counted. What [`DrawFont::encode`]
    /// draws of text the font draws whole.
    fn encode_alone(&self, s: &str) -> (Vec<u8>, Unencoded) {
        let mut out = Vec::with_capacity(s.len());
        let mut unencoded = Unencoded::default();
        match self.composed() {
            true => encode_chars(self, s.nfc(), &mut out, &mut unencoded),
            false => encode_chars(self, s.chars(), &mut out, &mut unencoded),
        }
        (out, unencoded)
    }

    /// How far `s` advances at `size`, in text space, drawn in this font
    /// alone as [`DrawFont::encode_alone`] draws it: the sum of its codes'
    /// widths, without character or word spacing.
    fn width_alone(&self, s: &str, size: f32) -> f32 {
        // What's drawn for a character the font hasn't: `?`, or nothing.
        let unknown = self.char_advance('?');
        let advance = |c: char| self.char_advance(c).or(unknown);
        // Summed in glyph units and scaled once, so a line's width has
        // the bits it always had.
        let units: f32 = match self.composed() {
            true => s.nfc().filter_map(advance).sum(),
            false => s.chars().filter_map(advance).sum(),
        };
        units * size / 1000.0
    }

    /// `s` as it's drawn, in the font and the fallback (see the trait's
    /// doc): its runs of codes, the fallback's glyphs, and the characters
    /// neither draws, each `?` in the font. Text the font draws whole is
    /// one run of its own codes ([`DrawFont::encode_alone`]), no glyph; so
    /// is the empty text.
    fn encode(&self, s: &str) -> TextRuns {
        if !self.falls_back() || draws_whole(self, s) {
            let (codes, unencoded) = self.encode_alone(s);
            return TextRuns {
                runs: vec![(InFont::Own, codes)],
                glyphs: Glyphs::new(),
                unencoded,
            };
        }
        let text = composed(self, s);
        let program = Fallback::SANS.program();
        let mut out = TextRuns::default();
        for (cluster, fallback) in clusters(self, &text) {
            let face = match fallback {
                true => InFont::Fallback,
                false => InFont::Own,
            };
            if out.runs.last().is_none_or(|(last, _)| *last != face) {
                out.runs.push((face, Vec::new()));
            }
            let (_, codes) = out.runs.last_mut().expect("a run pushed");
            match face {
                InFont::Own => encode_chars(self, cluster.chars(), codes, &mut out.unencoded),
                InFont::Fallback => {
                    let drawn = (out.glyphs.add(program, cluster))
                        .expect("the fallback maps every character of a cluster it draws");
                    codes.extend(drawn);
                }
            }
        }
        out
    }

    /// How far `s` advances at `size`, in text space, drawn as
    /// [`DrawFont::encode`] draws it, in both fonts: the sum of its codes'
    /// widths, without character or word spacing.
    fn width(&self, s: &str, size: f32) -> f32 {
        if !self.falls_back() {
            return self.width_alone(s, size);
        }
        // The font alone's sum, in one walk, while it draws every
        // character itself: [`DrawFont::width_alone`]'s, bit for bit.
        let unknown = self.char_advance('?');
        let mut codes = Vec::new();
        let mut whole = true;
        let mut advance = |c: char| {
            codes.clear();
            let own = self.char_advance(c);
            whole &= own.is_some() && self.encode_char(c, &mut codes);
            own.or(unknown)
        };
        let units: f32 = match self.composed() {
            true => s.nfc().filter_map(&mut advance).sum(),
            false => s.chars().filter_map(&mut advance).sum(),
        };
        if whole {
            return units * size / 1000.0;
        }
        let text = composed(self, s);
        let program = Fallback::SANS.program();
        let unknown = self.char_advance('?');
        let advance = |(c, fallback): (char, bool)| match fallback {
            true => program.char_advance(c),
            false => self.char_advance(c).or(unknown),
        };
        let units: f32 = clusters(self, &text)
            .flat_map(|(cluster, fallback)| cluster.chars().map(move |c| (c, fallback)))
            .filter_map(advance)
            .sum();
        units * size / 1000.0
    }

    /// Whether it encodes `c` itself, as [`DrawFont::encode_alone`] would:
    /// `c` composed, where the font composes text. The fallback aside: what
    /// tells which of two fonts a caller draws a character in.
    fn covers(&self, c: char) -> bool {
        let mut codes = Vec::new();
        if !self.composed() {
            return self.encode_char(c, &mut codes);
        }
        let mut composed = std::iter::once(c).nfc();
        match (composed.next(), composed.next()) {
            (Some(c), None) => self.encode_char(c, &mut codes),
            _ => false,
        }
    }

    /// `s` in lines no wider than `width` at `size`, as [`wrap_with`]
    /// breaks them, each measured as it's drawn ([`DrawFont::width`]).
    fn wrap(&self, s: &str, size: f32, width: f32) -> Vec<String> {
        wrap_with(s, width, |t| self.width(t, size))
    }
}

/// `chars` in `font`'s own codes, into `out`: a character it can't encode
/// as `?`, or nothing in a font without one, counted in `unencoded`.
fn encode_chars<F: DrawFont + ?Sized>(
    font: &F,
    chars: impl Iterator<Item = char>,
    out: &mut Vec<u8>,
    unencoded: &mut Unencoded,
) {
    for c in chars {
        if !font.encode_char(c, out) {
            unencoded.0 += 1;
            font.encode_char('?', out);
        }
    }
}

/// Whether `font` draws every character of `s` itself, composed where it
/// composes text: so every cluster of it is the font's, and nothing falls
/// back.
fn draws_whole<F: DrawFont + ?Sized>(font: &F, s: &str) -> bool {
    let mut codes = Vec::new();
    let mut draws = |c: char| {
        codes.clear();
        font.char_advance(c).is_some() && font.encode_char(c, &mut codes)
    };
    match font.composed() {
        true => s.nfc().all(&mut draws),
        false => s.chars().all(&mut draws),
    }
}

/// `s` as `font` draws it: composed (NFC) where it composes text.
fn composed<F: DrawFont + ?Sized>(font: &F, s: &str) -> String {
    match font.composed() {
        true => s.nfc().collect(),
        false => s.to_string(),
    }
}

/// Each grapheme cluster of `text`, and whether it's drawn in the fallback:
/// where the font hasn't all of it and the fallback has, the first of the
/// two that does ([`face_for`]), every character mapped in its program.
/// A cluster with a control character in it (a line break, a NUL) is the
/// font's, drawn as it always was: a control draws nothing, and the glyphs
/// the fallback maps two of to (a carriage return's, a blank as wide as a
/// space) aren't the text's to draw.
fn clusters<'t, F: DrawFont + ?Sized>(
    font: &'t F,
    text: &'t str,
) -> impl Iterator<Item = (&'t str, bool)> + 't {
    let sans = Fallback::SANS;
    let program = sans.program();
    let both = WithFallback::new(font, program, sans.res());
    text.graphemes(true).map(move |cluster| {
        let fallback = !cluster.chars().any(char::is_control)
            && face_for(&both, cluster) == Ok(1)
            && cluster.chars().all(|c| program.glyph(c).is_some());
        (cluster, fallback)
    })
}

/// A standard 14 font: text composed, in WinAnsiEncoding for the twelve
/// Latin ones and their own for Symbol and ZapfDingbats.
impl DrawFont for Std14 {
    /// Its dictionary, made once.
    fn res(&self) -> Res {
        static RESOURCES: [OnceLock<Res>; 14] = [const { OnceLock::new() }; 14];
        RESOURCES[*self as usize]
            .get_or_init(|| {
                Res::dict(self.dict())
                    .expect("a font's objects are one at least, their streams in memory")
            })
            .clone()
    }
}

/// The face an op chose, which is one of the twelve.
pub fn std14_of(choice: FontChoice) -> Std14 {
    match choice {
        FontChoice::Helvetica => Std14::Helvetica,
        FontChoice::HelveticaBold => Std14::HelveticaBold,
        FontChoice::HelveticaOblique => Std14::HelveticaOblique,
        FontChoice::HelveticaBoldOblique => Std14::HelveticaBoldOblique,
        FontChoice::TimesRoman => Std14::TimesRoman,
        FontChoice::TimesBold => Std14::TimesBold,
        FontChoice::TimesItalic => Std14::TimesItalic,
        FontChoice::TimesBoldItalic => Std14::TimesBoldItalic,
        FontChoice::Courier => Std14::Courier,
        FontChoice::CourierBold => Std14::CourierBold,
        FontChoice::CourierOblique => Std14::CourierOblique,
        FontChoice::CourierBoldOblique => Std14::CourierBoldOblique,
    }
}

/// `s` in lines no wider than `width` by `measure`, which says how wide a
/// line of text is: text in more than one font wraps as one. Each `\n`
/// starts a paragraph; words are what spaces separate, a run of them one,
/// and a word wider than a line is broken between characters.
pub fn wrap_with(s: &str, width: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let fits = |t: &str| measure(t) <= width;
    let mut out = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            let joined = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if fits(&joined) {
                line = joined;
                continue;
            }
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            for c in word.chars() {
                line.push(c);
                if !fits(&line) && line.chars().count() > 1 {
                    line.pop();
                    out.push(std::mem::replace(&mut line, c.to_string()));
                }
            }
        }
        out.push(line);
    }
    out
}
