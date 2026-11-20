//! Text drawn in a [`Font`] through the content writer: the writer takes a
//! font as a resource and text as the codes a font already encoded, and
//! this is where a sheaf font gives both. With
//! `impl From<FontRes> for Res` it's the whole of what stands between
//! sheaf's fonts and the writer: every caller draws text through
//! [`DrawText`], naming the font it draws in at each call.

use super::{Font, Unencoded};
use crate::content::ContentWriter;

/// Text in a [`Font`], written to content.
pub trait DrawText {
    /// `Tf`: text draws in `font` at `size`, its resource named when the
    /// content is finished.
    fn set_font(&mut self, font: &dyn Font, size: f32);

    /// `Tj`: `s` as `font`'s codes, and the characters it couldn't
    /// encode, drawn as `?`. `font` is the one [`set_font`] set: the
    /// writer keeps none, so text names the font it's encoded in.
    ///
    /// [`set_font`]: DrawText::set_font
    fn show_text(&mut self, font: &dyn Font, s: &str) -> Unencoded;
}

impl DrawText for ContentWriter {
    fn set_font(&mut self, font: &dyn Font, size: f32) {
        self.font(font.resource().into(), size);
    }

    fn show_text(&mut self, font: &dyn Font, s: &str) -> Unencoded {
        let (codes, unencoded) = font.encode(s);
        self.show(&codes);
        unencoded
    }
}
