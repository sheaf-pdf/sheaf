//! The font OCR text is written in: Tesseract's approach
//! (`pdfrenderer.cpp`), a Type0 font over an embedded TrueType with one
//! empty glyph. Every CID draws that glyph and advances half an em, so a
//! word is fitted to its box by horizontal scaling alone, and the
//! ToUnicode CMap reads each CID back as the UTF-16 code unit it is: any
//! text round-trips.

use std::sync::{Arc, OnceLock};

use crate::pdf::{Object, ObjectId, Stream, dictionary, literal};

use super::{DrawFont, Font, FontRes, Unencoded};
use crate::content::{Res, ResId};
use crate::pdf::fonts::{FontMetrics, GlyphBox, LayoutFont};

/// Tesseract's glyphless font.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Glyphless;

impl Glyphless {
    /// The embedded program: Tesseract's glyphless TrueType (Apache-2.0;
    /// see `assets/LICENSE-pdf.ttf`).
    pub const PROGRAM: &[u8] = include_bytes!("../../../assets/pdf.ttf");
}

/// Each CID is the UTF-16 code unit it reads as.
const TO_UNICODE: &str = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo
<<
  /Registry (Adobe)
  /Ordering (UCS)
  /Supplement 0
>> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
1 beginbfrange
<0000> <FFFF> <0000>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end
";

/// Half an em for each UTF-16 code unit of a character, which is one CID
/// each: nothing is refused.
impl LayoutFont for Glyphless {
    /// Its descriptor's: `/Ascent`, `/Descent` and `/FontBBox`.
    fn font_metrics(&self) -> FontMetrics {
        FontMetrics::new(1000.0, -1.0, Some(GlyphBox::new(0.0, 0.0, 500.0, 1000.0)))
    }

    fn char_advance(&self, ch: char) -> Option<f32> {
        Some(500.0 * ch.len_utf16() as f32)
    }

    fn encode_char(&self, ch: char, out: &mut Vec<u8>) -> bool {
        let mut units = [0u16; 2];
        out.extend(
            ch.encode_utf16(&mut units)
                .iter()
                .flat_map(|u| u.to_be_bytes()),
        );
        true
    }
}

impl DrawFont for Glyphless {
    /// As written: its CIDs are the text's UTF-16, and a word is fitted
    /// to its box by how many it has.
    fn composed(&self) -> bool {
        false
    }

    /// Half an em a CID, by their count, as it has always been reckoned:
    /// the same as the sum of each character's advance, without its
    /// rounding in a text of tens of thousands of characters, and nothing
    /// for no text.
    fn width(&self, s: &str, size: f32) -> f32 {
        s.encode_utf16().count() as f32 * 500.0 * size / 1000.0
    }

    /// The same six objects as [`Font::resource`]'s.
    fn res(&self) -> Res {
        static RES: OnceLock<Res> = OnceLock::new();
        RES.get_or_init(|| {
            Res::of(Font::resource(self).objects)
                .expect("a font's objects are one at least, their streams in memory")
        })
        .clone()
    }
}

impl Font for Glyphless {
    /// UTF-16 code units, each a two-byte CID, as written: nothing is
    /// unencoded.
    fn encode(&self, s: &str) -> (Vec<u8>, Unencoded) {
        let codes = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
        (codes, Unencoded::default())
    }

    fn advance(&self, b: &[u8], size: f32) -> f32 {
        (b.len() / 2) as f32 * 500.0 * size / 1000.0
    }

    fn covers(&self, _: char) -> bool {
        true
    }

    /// The Type0 font, its CIDFontType2, descriptor and program, the
    /// CIDToGIDMap that sends every CID to glyph 1, and the ToUnicode
    /// CMap.
    fn resource(&self) -> FontRes {
        static RESOURCE: OnceLock<FontRes> = OnceLock::new();
        RESOURCE
            .get_or_init(|| {
                // Every CID maps to glyph 1, the font's one (empty) glyph.
                let map = [0u8, 1].repeat(65536);
                let objects: Arc<[Object]> = Arc::new([
                    dictionary! {
                        "Type" => Object::name("Font"),
                        "Subtype" => Object::name("Type0"),
                        "BaseFont" => Object::name("GlyphLessFont"),
                        "Encoding" => Object::name("Identity-H"),
                        "DescendantFonts" => vec![Object::from(ObjectId::new(1, 0))],
                        "ToUnicode" => Object::from(ObjectId::new(5, 0)),
                    }
                    .into(),
                    dictionary! {
                        "Type" => Object::name("Font"),
                        "Subtype" => Object::name("CIDFontType2"),
                        "BaseFont" => Object::name("GlyphLessFont"),
                        "CIDSystemInfo" => dictionary! {
                            "Registry" => literal("Adobe"),
                            "Ordering" => literal("Identity"),
                            "Supplement" => 0i64,
                        },
                        "FontDescriptor" => Object::from(ObjectId::new(2, 0)),
                        "DW" => 500i64,
                        "CIDToGIDMap" => Object::from(ObjectId::new(4, 0)),
                    }
                    .into(),
                    dictionary! {
                        "Type" => Object::name("FontDescriptor"),
                        "FontName" => Object::name("GlyphLessFont"),
                        "Flags" => 5i64, // FixedPitch + Symbolic
                        "FontBBox" => vec![0.into(), 0.into(), 500.into(), 1000.into()],
                        "Ascent" => 1000i64,
                        "Descent" => -1i64,
                        "CapHeight" => 1000i64,
                        "ItalicAngle" => 0i64,
                        "StemV" => 80i64,
                        "FontFile2" => Object::from(ObjectId::new(3, 0)),
                    }
                    .into(),
                    Stream::new(
                        dictionary! { "Length1" => Glyphless::PROGRAM.len() as i64 },
                        Glyphless::PROGRAM.to_vec(),
                    )
                    .into(),
                    Stream::new(dictionary! {}, map).into(),
                    Stream::new(dictionary! {}, TO_UNICODE.as_bytes().to_vec()).into(),
                ]);
                FontRes {
                    id: ResId::of(&objects),
                    objects,
                }
            })
            .clone()
    }
}
