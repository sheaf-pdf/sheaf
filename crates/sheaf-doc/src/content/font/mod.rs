//! Fonts sheaf draws text in: what codes a string is in a font, how far
//! they advance, and the font's resource for a page; and the fonts files
//! draw in, as read.
//!
//! - [`DrawFont`]: a font sheaf draws in, the PDF layer's [`Std14`] (the
//!   standard 14, with Adobe's widths and boxes) and sheaf's own
//!   [`Glyphless`], OCR's invisible text; [`DrawText`] writes text in one.
//! - [`Font`]: a file's font, its codes, widths and heights as PDFium
//!   reads them, by the PDF layer; [`Fonts`], a source's, each read once.
//!
//! Text a font can't encode draws `?` and is counted ([`Unencoded`]), so
//! the writer can say so once; the text itself (`/Contents`, a recipe) is
//! kept exactly. M10 slice 7's fallback font ends that, behind
//! [`DrawFont`].

mod draw;
mod glyphless;
#[cfg(test)]
mod tests;

use std::sync::OnceLock;

use sheaf_edit::FontChoice;
use unicode_normalization::UnicodeNormalization;

use crate::pdf::fonts::LayoutFont;

pub use draw::DrawText;
pub use glyphless::Glyphless;

pub use crate::pdf::fonts::{
    Descendant, Error as FontError, Font, FontApprox, FontId, FontKind, FontMetrics, FontProgram,
    FontScope, Fonts, Std14, VerticalMetrics,
};

use super::Res;

/// Characters a font couldn't encode: each drew `?`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unencoded(pub usize);

/// A font sheaf draws text in, by the PDF layer's own fonts: what
/// [`LayoutFont`] gives of one (a character's codes, how far it
/// advances), the font as a page resource, and sheaf's rules for text
/// over them, written here once: a font says what it is ([`DrawFont::res`],
/// [`DrawFont::composed`]) and takes the rest as given.
///
/// Any font sheaf embeds is GID-preserving: a subset keeps each glyph's
/// number from the whole font (CID = GID, `/CIDToGIDMap /Identity`), so
/// the codes a string is drawn with depend only on the font and the text,
/// and re-subsetting never changes them. [`Glyphless`], embedded whole,
/// is the one exception: its CIDs are the text's UTF-16, all drawing its
/// one glyph.
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

    /// `s` as this font's codes. A character it can't encode is drawn as
    /// `?`, or left out by a font without one (ZapfDingbats), and counted.
    fn encode(&self, s: &str) -> (Vec<u8>, Unencoded) {
        let mut out = Vec::with_capacity(s.len());
        let mut unencoded = Unencoded::default();
        let mut put = |c: char| {
            if !self.encode_char(c, &mut out) {
                unencoded.0 += 1;
                self.encode_char('?', &mut out);
            }
        };
        match self.composed() {
            true => s.nfc().for_each(&mut put),
            false => s.chars().for_each(&mut put),
        }
        (out, unencoded)
    }

    /// How far `s` advances at `size`, in text space, drawn as
    /// [`DrawFont::encode`] draws it: the sum of its codes' widths,
    /// without character or word spacing.
    fn width(&self, s: &str, size: f32) -> f32 {
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

    /// Whether it encodes `c`, as [`DrawFont::encode`] would: `c`
    /// composed, where the font composes text.
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
    /// breaks them.
    fn wrap(&self, s: &str, size: f32, width: f32) -> Vec<String> {
        wrap_with(s, width, |t| self.width(t, size))
    }
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
