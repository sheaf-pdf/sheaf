//! The standard 14 fonts: every PDF reader has them, so sheaf draws in
//! them without embedding anything. The twelve Latin ones are written in
//! WinAnsiEncoding, Symbol and ZapfDingbats in their own encodings, and
//! every width and box is Adobe's, from the font's AFM. A file's font
//! names one by any of the names PDFium takes for it ([`Std14::named`]).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use lopdf::{Object, dictionary};
use sheaf_edit::FontChoice;
use unicode_normalization::UnicodeNormalization;

use super::afm::Afm;
use super::encoding::{Encoding, glyph_text};
use super::{Font, FontMetrics, FontRes, Unencoded};
use crate::content::ResId;
use crate::geom::Rect;

/// A standard 14 font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Std14 {
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
    Helvetica,
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Symbol,
    ZapfDingbats,
}

impl Std14 {
    pub const ALL: [Std14; 14] = [
        Std14::Courier,
        Std14::CourierBold,
        Std14::CourierOblique,
        Std14::CourierBoldOblique,
        Std14::Helvetica,
        Std14::HelveticaBold,
        Std14::HelveticaOblique,
        Std14::HelveticaBoldOblique,
        Std14::TimesRoman,
        Std14::TimesBold,
        Std14::TimesItalic,
        Std14::TimesBoldItalic,
        Std14::Symbol,
        Std14::ZapfDingbats,
    ];

    /// Its `/BaseFont`.
    pub fn name(self) -> &'static str {
        match self {
            Std14::Courier => "Courier",
            Std14::CourierBold => "Courier-Bold",
            Std14::CourierOblique => "Courier-Oblique",
            Std14::CourierBoldOblique => "Courier-BoldOblique",
            Std14::Helvetica => "Helvetica",
            Std14::HelveticaBold => "Helvetica-Bold",
            Std14::HelveticaOblique => "Helvetica-Oblique",
            Std14::HelveticaBoldOblique => "Helvetica-BoldOblique",
            Std14::TimesRoman => "Times-Roman",
            Std14::TimesBold => "Times-Bold",
            Std14::TimesItalic => "Times-Italic",
            Std14::TimesBoldItalic => "Times-BoldItalic",
            Std14::Symbol => "Symbol",
            Std14::ZapfDingbats => "ZapfDingbats",
        }
    }

    /// The standard 14 font `base_font`, a font's `/BaseFont`, names, as
    /// PDFium reads one (`CFX_StandardFont::GetStandardFontIndex`): after
    /// a subset's tag (`ABCDEF+`), one of the 14 names or an alternate
    /// PDFium takes for one (`Arial,Bold`, `TimesNewRomanPSMT`), in any
    /// case. None for any other name.
    pub fn named(base_font: &[u8]) -> Option<Std14> {
        let name = match base_font.iter().position(|&c| c == b'+') {
            Some(6) => &base_font[7..],
            _ => base_font,
        };
        (NAMES.iter())
            .find(|(alias, _)| alias.as_bytes().eq_ignore_ascii_case(name))
            .map(|&(_, font)| font)
    }

    /// How far its glyphs reach, per 1000 em, from its AFM: its
    /// `Ascender` and `Descender`, or, for Symbol and ZapfDingbats, which
    /// give neither, its `FontBBox`'s top and bottom; and its `FontBBox`.
    pub fn metrics(self) -> FontMetrics {
        let afm = self.afm();
        let [left, bottom, right, top] = afm.bbox.map(f32::from);
        FontMetrics {
            ascent: afm.ascender.map_or(top, f32::from),
            descent: afm.descender.map_or(bottom, f32::from),
            bbox: Some(Rect::new(left, bottom, right, top)),
        }
    }

    /// Glyph `name`'s advance, per 1000 em, from its AFM; None for a glyph
    /// it hasn't.
    pub fn glyph_width(self, name: &str) -> Option<f32> {
        self.afm().width(name).map(f32::from)
    }

    /// The box glyph `name`'s outline fits in, from its origin, per 1000
    /// em, from its AFM; None for a glyph it hasn't.
    pub fn glyph_box(self, name: &str) -> Option<Rect> {
        let [left, bottom, right, top] = self.afm().glyph_box(name)?.map(f32::from);
        Some(Rect::new(left, bottom, right, top))
    }

    /// Whether glyph `name`'s width, where a file draws the font without
    /// a program or `/Widths`, is the face's PDFium draws it in rather
    /// than Adobe's. PDFium's own faces (Foxit's, measured in PDFium
    /// 8fdce8e1) haven't the glyphs Adobe's metrics added in their second
    /// version ([`ADDED`]), and draw each as the space, and draw a few
    /// others at widths of their own; a system's face may draw any of
    /// them otherwise again. Courier's are 600 whatever draws them, and
    /// Symbol's and ZapfDingbats' are Adobe's.
    pub fn drawn_otherwise(self, name: &str) -> bool {
        let own: &[&str] = match self {
            Std14::Helvetica => &["guillemotleft", "guillemotright", "guilsinglright"],
            Std14::HelveticaOblique => &["at", "guilsinglleft"],
            Std14::HelveticaBold => &["at"],
            Std14::HelveticaBoldOblique => &["at", "five", "oslash"],
            _ => &[],
        };
        let latin = !matches!(
            self,
            Std14::Courier
                | Std14::CourierBold
                | Std14::CourierOblique
                | Std14::CourierBoldOblique
                | Std14::Symbol
                | Std14::ZapfDingbats
        );
        (latin && ADDED.contains(&name)) || own.contains(&name)
    }

    /// The encoding sheaf writes it in.
    pub(crate) fn encoding(self) -> Encoding {
        match self {
            Std14::Symbol => Encoding::Symbol,
            Std14::ZapfDingbats => Encoding::ZapfDingbats,
            _ => Encoding::WinAnsi,
        }
    }

    /// Its AFM, as Adobe wrote it but for the kerning.
    pub(crate) fn afm_text(self) -> &'static str {
        match self {
            Std14::Courier => include_str!("../../../assets/afm/Courier.afm"),
            Std14::CourierBold => include_str!("../../../assets/afm/Courier-Bold.afm"),
            Std14::CourierOblique => include_str!("../../../assets/afm/Courier-Oblique.afm"),
            Std14::CourierBoldOblique => {
                include_str!("../../../assets/afm/Courier-BoldOblique.afm")
            }
            Std14::Helvetica => include_str!("../../../assets/afm/Helvetica.afm"),
            Std14::HelveticaBold => include_str!("../../../assets/afm/Helvetica-Bold.afm"),
            Std14::HelveticaOblique => include_str!("../../../assets/afm/Helvetica-Oblique.afm"),
            Std14::HelveticaBoldOblique => {
                include_str!("../../../assets/afm/Helvetica-BoldOblique.afm")
            }
            Std14::TimesRoman => include_str!("../../../assets/afm/Times-Roman.afm"),
            Std14::TimesBold => include_str!("../../../assets/afm/Times-Bold.afm"),
            Std14::TimesItalic => include_str!("../../../assets/afm/Times-Italic.afm"),
            Std14::TimesBoldItalic => include_str!("../../../assets/afm/Times-BoldItalic.afm"),
            Std14::Symbol => include_str!("../../../assets/afm/Symbol.afm"),
            Std14::ZapfDingbats => include_str!("../../../assets/afm/ZapfDingbats.afm"),
        }
    }

    /// Its metrics, read once.
    pub(super) fn afm(self) -> &'static Afm {
        static AFMS: [OnceLock<Afm>; 14] = [const { OnceLock::new() }; 14];
        AFMS[self as usize].get_or_init(|| Afm::parse(self.afm_text()))
    }

    /// Its codes in [`Std14::encoding`], read once.
    fn face(self) -> &'static Face {
        static FACES: [OnceLock<Face>; 14] = [const { OnceLock::new() }; 14];
        FACES[self as usize].get_or_init(|| Face::new(self))
    }
}

/// The glyphs Adobe's metrics for the Helvetica and Times fonts added in
/// their second version, the letters of Central European languages and
/// MacRoman's mathematical signs, which PDFium's own faces haven't
/// ([`Std14::drawn_otherwise`]).
const ADDED: [&str; 86] = [
    "Abreve",
    "Amacron",
    "Aogonek",
    "Cacute",
    "Ccaron",
    "Dcaron",
    "Dcroat",
    "Delta",
    "Ecaron",
    "Edotaccent",
    "Emacron",
    "Eogonek",
    "Gbreve",
    "Gcommaaccent",
    "Idotaccent",
    "Imacron",
    "Iogonek",
    "Kcommaaccent",
    "Lacute",
    "Lcaron",
    "Lcommaaccent",
    "Nacute",
    "Ncaron",
    "Ncommaaccent",
    "Ohungarumlaut",
    "Omacron",
    "Racute",
    "Rcaron",
    "Rcommaaccent",
    "Sacute",
    "Scedilla",
    "Scommaaccent",
    "Tcaron",
    "Tcommaaccent",
    "Uhungarumlaut",
    "Umacron",
    "Uogonek",
    "Uring",
    "Zacute",
    "Zdotaccent",
    "abreve",
    "amacron",
    "aogonek",
    "cacute",
    "ccaron",
    "commaaccent",
    "dcaron",
    "dcroat",
    "ecaron",
    "edotaccent",
    "emacron",
    "eogonek",
    "gbreve",
    "gcommaaccent",
    "greaterequal",
    "imacron",
    "iogonek",
    "kcommaaccent",
    "lacute",
    "lcaron",
    "lcommaaccent",
    "lessequal",
    "lozenge",
    "nacute",
    "ncaron",
    "ncommaaccent",
    "notequal",
    "ohungarumlaut",
    "omacron",
    "partialdiff",
    "racute",
    "radical",
    "rcaron",
    "rcommaaccent",
    "sacute",
    "scedilla",
    "scommaaccent",
    "summation",
    "tcaron",
    "tcommaaccent",
    "uhungarumlaut",
    "umacron",
    "uogonek",
    "uring",
    "zacute",
    "zdotaccent",
];

/// The names PDFium reads as the standard 14 fonts: the 14 and the
/// alternates it takes for them, from `cfx_standardfont.cpp`'s
/// `kAltFontNames` (PDFium 8fdce8e1).
const NAMES: [(&str, Std14); 89] = [
    ("Arial", Std14::Helvetica),
    ("Arial,Bold", Std14::HelveticaBold),
    ("Arial,BoldItalic", Std14::HelveticaBoldOblique),
    ("Arial,Italic", Std14::HelveticaOblique),
    ("Arial-Bold", Std14::HelveticaBold),
    ("Arial-BoldItalic", Std14::HelveticaBoldOblique),
    ("Arial-BoldItalicMT", Std14::HelveticaBoldOblique),
    ("Arial-BoldMT", Std14::HelveticaBold),
    ("Arial-Italic", Std14::HelveticaOblique),
    ("Arial-ItalicMT", Std14::HelveticaOblique),
    ("ArialBold", Std14::HelveticaBold),
    ("ArialBoldItalic", Std14::HelveticaBoldOblique),
    ("ArialItalic", Std14::HelveticaOblique),
    ("ArialMT", Std14::Helvetica),
    ("ArialMT,Bold", Std14::HelveticaBold),
    ("ArialMT,BoldItalic", Std14::HelveticaBoldOblique),
    ("ArialMT,Italic", Std14::HelveticaOblique),
    ("ArialRoundedMTBold", Std14::HelveticaBold),
    ("Courier", Std14::Courier),
    ("Courier,Bold", Std14::CourierBold),
    ("Courier,BoldItalic", Std14::CourierBoldOblique),
    ("Courier,Italic", Std14::CourierOblique),
    ("Courier-Bold", Std14::CourierBold),
    ("Courier-BoldOblique", Std14::CourierBoldOblique),
    ("Courier-Oblique", Std14::CourierOblique),
    ("CourierBold", Std14::CourierBold),
    ("CourierBoldItalic", Std14::CourierBoldOblique),
    ("CourierItalic", Std14::CourierOblique),
    ("CourierNew", Std14::Courier),
    ("CourierNew,Bold", Std14::CourierBold),
    ("CourierNew,BoldItalic", Std14::CourierBoldOblique),
    ("CourierNew,Italic", Std14::CourierOblique),
    ("CourierNew-Bold", Std14::CourierBold),
    ("CourierNew-BoldItalic", Std14::CourierBoldOblique),
    ("CourierNew-Italic", Std14::CourierOblique),
    ("CourierNewBold", Std14::CourierBold),
    ("CourierNewBoldItalic", Std14::CourierBoldOblique),
    ("CourierNewItalic", Std14::CourierOblique),
    ("CourierNewPS-BoldItalicMT", Std14::CourierBoldOblique),
    ("CourierNewPS-BoldMT", Std14::CourierBold),
    ("CourierNewPS-ItalicMT", Std14::CourierOblique),
    ("CourierNewPSMT", Std14::Courier),
    ("CourierStd", Std14::Courier),
    ("CourierStd-Bold", Std14::CourierBold),
    ("CourierStd-BoldOblique", Std14::CourierBoldOblique),
    ("CourierStd-Oblique", Std14::CourierOblique),
    ("Helvetica", Std14::Helvetica),
    ("Helvetica,Bold", Std14::HelveticaBold),
    ("Helvetica,BoldItalic", Std14::HelveticaBoldOblique),
    ("Helvetica,Italic", Std14::HelveticaOblique),
    ("Helvetica-Bold", Std14::HelveticaBold),
    ("Helvetica-BoldItalic", Std14::HelveticaBoldOblique),
    ("Helvetica-BoldOblique", Std14::HelveticaBoldOblique),
    ("Helvetica-Italic", Std14::HelveticaOblique),
    ("Helvetica-Oblique", Std14::HelveticaOblique),
    ("HelveticaBold", Std14::HelveticaBold),
    ("HelveticaBoldItalic", Std14::HelveticaBoldOblique),
    ("HelveticaItalic", Std14::HelveticaOblique),
    ("Symbol", Std14::Symbol),
    ("SymbolMT", Std14::Symbol),
    ("Times-Bold", Std14::TimesBold),
    ("Times-BoldItalic", Std14::TimesBoldItalic),
    ("Times-Italic", Std14::TimesItalic),
    ("Times-Roman", Std14::TimesRoman),
    ("TimesBold", Std14::TimesBold),
    ("TimesBoldItalic", Std14::TimesBoldItalic),
    ("TimesItalic", Std14::TimesItalic),
    ("TimesNewRoman", Std14::TimesRoman),
    ("TimesNewRoman,Bold", Std14::TimesBold),
    ("TimesNewRoman,BoldItalic", Std14::TimesBoldItalic),
    ("TimesNewRoman,Italic", Std14::TimesItalic),
    ("TimesNewRoman-Bold", Std14::TimesBold),
    ("TimesNewRoman-BoldItalic", Std14::TimesBoldItalic),
    ("TimesNewRoman-Italic", Std14::TimesItalic),
    ("TimesNewRomanBold", Std14::TimesBold),
    ("TimesNewRomanBoldItalic", Std14::TimesBoldItalic),
    ("TimesNewRomanItalic", Std14::TimesItalic),
    ("TimesNewRomanPS", Std14::TimesRoman),
    ("TimesNewRomanPS-Bold", Std14::TimesBold),
    ("TimesNewRomanPS-BoldItalic", Std14::TimesBoldItalic),
    ("TimesNewRomanPS-BoldItalicMT", Std14::TimesBoldItalic),
    ("TimesNewRomanPS-BoldMT", Std14::TimesBold),
    ("TimesNewRomanPS-Italic", Std14::TimesItalic),
    ("TimesNewRomanPS-ItalicMT", Std14::TimesItalic),
    ("TimesNewRomanPSMT", Std14::TimesRoman),
    ("TimesNewRomanPSMT,Bold", Std14::TimesBold),
    ("TimesNewRomanPSMT,BoldItalic", Std14::TimesBoldItalic),
    ("TimesNewRomanPSMT,Italic", Std14::TimesItalic),
    ("ZapfDingbats", Std14::ZapfDingbats),
];

/// A font's encoding, both ways, with each code's advance.
struct Face {
    /// Per 1000 em; 0 where the encoding has no glyph.
    widths: [u16; 256],
    /// The code each character it draws has.
    codes: HashMap<char, u8>,
    /// What it draws for a character it hasn't: `?` where it has one.
    unknown: Option<u8>,
}

impl Face {
    fn new(font: Std14) -> Face {
        let encoding = font.encoding();
        let afm = font.afm();
        let mut face = Face {
            widths: [0; 256],
            codes: HashMap::new(),
            unknown: None,
        };
        // From the top, so where two codes draw one character the lower
        // one stays.
        for code in (0..=255u8).rev() {
            let Some(glyph) = encoding.glyph(code) else {
                continue;
            };
            let Some(width) = afm.width(glyph) else {
                continue;
            };
            face.widths[code as usize] = width;
            // The AGL reads three of Symbol's Greek letters as the math
            // signs (∆ U+2206, Ω U+2126, µ U+00B5); the letters, its
            // `…greek` names, are written with them too.
            let greek = (encoding == Encoding::Symbol)
                .then(|| glyph_text(&format!("{glyph}greek"), false))
                .flatten()
                .filter(|t| t.chars().all(char::is_alphabetic));
            for text in [encoding.text(code), greek].into_iter().flatten() {
                let mut chars = text.nfc();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    face.codes.insert(c, code);
                }
            }
        }
        face.unknown = face.codes.get(&'?').copied();
        face
    }
}

/// The face an op chose, which is one of the twelve.
impl From<FontChoice> for Std14 {
    fn from(choice: FontChoice) -> Std14 {
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
}

impl Font for Std14 {
    /// Composed first (NFC), so a decomposed "é" is WinAnsi's 0xE9. Where
    /// the encoding has a character twice, the lower code is written.
    fn encode(&self, s: &str) -> (Vec<u8>, Unencoded) {
        let face = self.face();
        let mut unencoded = Unencoded::default();
        let mut out = Vec::with_capacity(s.len());
        for c in s.nfc() {
            match face.codes.get(&c) {
                Some(&code) => out.push(code),
                None => {
                    unencoded.0 += 1;
                    out.extend(face.unknown);
                }
            }
        }
        (out, unencoded)
    }

    fn advance(&self, b: &[u8], size: f32) -> f32 {
        let widths = &self.face().widths;
        b.iter()
            .map(|&c| f32::from(widths[c as usize]))
            .sum::<f32>()
            * size
            / 1000.0
    }

    /// As [`Font::encode`] would: `c` composed.
    fn covers(&self, c: char) -> bool {
        let mut composed = std::iter::once(c).nfc();
        match (composed.next(), composed.next()) {
            (Some(c), None) => self.face().codes.contains_key(&c),
            _ => false,
        }
    }

    /// Its dictionary, made once.
    fn resource(&self) -> FontRes {
        static RESOURCES: [OnceLock<FontRes>; 14] = [const { OnceLock::new() }; 14];
        RESOURCES[*self as usize]
            .get_or_init(|| {
                let mut dict = dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => self.name(),
                };
                if self.encoding() == Encoding::WinAnsi {
                    dict.set("Encoding", "WinAnsiEncoding");
                }
                let objects: Arc<[Object]> = Arc::new([dict.into()]);
                FontRes {
                    id: ResId::of(&objects),
                    objects,
                }
            })
            .clone()
    }
}
