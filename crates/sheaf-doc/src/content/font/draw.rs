//! Text drawn in a [`DrawFont`] through the content writer: the writer
//! takes a font as a resource and text as the codes a font already
//! encoded, and this is where a font gives both, from one value. It's the
//! whole of what stands between the fonts sheaf draws in and the writer:
//! every caller draws text through [`DrawText`], naming the font it draws
//! in at each call.

use super::{DrawFont, Fallback, Glyphs, InFont, TextRuns, Unencoded};
use crate::content::ContentWriter;

/// Text in a [`DrawFont`], written to content.
pub trait DrawText {
    /// `Tf`: text draws in `font` at `size`, its resource named when the
    /// content is finished.
    fn set_font(&mut self, font: &dyn DrawFont, size: f32);

    /// `Tj`: `s` as `font` draws it ([`DrawFont::encode`]), at `size`:
    /// its runs in the fallback font each set (`Tf`) and shown, then
    /// `font` set again, so the font set after it is `font`'s whatever it
    /// drew. What it drew in the fallback, which whatever binds the
    /// content's resources joins, and the characters neither font could
    /// draw, each drawn as `?`. `font` and `size` are the ones
    /// [`set_font`] set: the writer keeps none, so text names the font
    /// it's encoded in. Nothing checks that it is, nor which font a `Q`
    /// brings back: the caller pairs them.
    ///
    /// [`set_font`]: DrawText::set_font
    fn show_text(&mut self, font: &dyn DrawFont, size: f32, s: &str) -> (Glyphs, Unencoded);
}

impl DrawText for ContentWriter {
    fn set_font(&mut self, font: &dyn DrawFont, size: f32) {
        self.font(font.res(), size);
    }

    fn show_text(&mut self, font: &dyn DrawFont, size: f32, s: &str) -> (Glyphs, Unencoded) {
        let TextRuns {
            runs,
            glyphs,
            unencoded,
        } = font.encode(s);
        for (face, codes) in &runs {
            match face {
                InFont::Own => self.show(codes),
                InFont::Fallback => {
                    self.font(Fallback::SANS.res(), size);
                    self.show(codes);
                    self.set_font(font, size);
                }
            }
        }
        (glyphs, unencoded)
    }
}
