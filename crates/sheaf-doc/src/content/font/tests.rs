//! Fonts: widths that are Adobe's, encoders that are exact, `?` for what
//! a font can't draw, wrapping, and fonts as resources; and files' fonts
//! as read, built in code, each kind's codes, widths and heights.

use std::collections::HashMap;
use std::sync::Arc;

use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};

use super::encoding::Encoding;
use super::*;
use crate::serialize::built::ResObjects;
use crate::source::Limits;

/// An AFM's widths and codes, read here as plainly as it can be: each
/// `C <code> ; WX <width> ; N <name>` line.
fn afm(text: &str) -> (HashMap<&str, f32>, HashMap<u8, &str>) {
    let mut widths = HashMap::new();
    let mut codes = HashMap::new();
    for line in text.lines().filter(|l| l.starts_with("C ")) {
        let field = |key: &str| {
            line.split(';')
                .find_map(|f| f.trim().strip_prefix(key))
                .map(str::trim)
        };
        let name = field("N ").unwrap();
        widths.insert(name, field("WX ").unwrap().parse().unwrap());
        if let Ok(code) = field("C ").unwrap().parse::<u8>() {
            codes.insert(code, name);
        }
    }
    (widths, codes)
}

/// Windows-1252, from its code chart: None for the five codes it leaves
/// empty.
fn cp1252(code: u8) -> Option<char> {
    const HIGH: &str = "€\u{81}‚ƒ„…†‡ˆ‰Š‹Œ\u{8d}Ž\u{8f}\u{90}‘’“”•–—˜™š›œ\u{9d}žŸ";
    match code {
        0x81 | 0x8d | 0x8f | 0x90 | 0x9d => None,
        0x80..=0x9f => HIGH.chars().nth(code as usize - 0x80),
        _ => Some(code as char),
    }
}

/// Every code a font's encoding maps advances by its glyph's width in
/// Adobe's AFM, and every glyph it names is in the AFM.
#[test]
fn widths_equal_the_afm() {
    for font in Std14::ALL {
        let (widths, codes) = afm(font.afm_text());
        let mut mapped = 0;
        for code in 0..=255u8 {
            let advance = font.advance(&[code], 1000.0);
            let Some(glyph) = font.encoding().glyph(code) else {
                assert_eq!(advance, 0.0, "{font:?} {code:#x}");
                continue;
            };
            let width = widths.get(glyph).copied();
            assert_eq!(Some(advance), width, "{font:?} {code:#x} {glyph}");
            mapped += 1;
        }
        let expected = match font {
            // Printable ASCII, 27 of 0x80–0x9F, and 0xA0–0xFF.
            Std14::Symbol | Std14::ZapfDingbats => codes.len(),
            _ => 95 + 27 + 96,
        };
        assert_eq!(mapped, expected, "{font:?}");
        assert_eq!(font.afm().len(), widths.len(), "{font:?}");
    }
    let width = |font: Std14, s: &str| font.width(s, 1000.0);
    assert_eq!(width(Std14::Helvetica, "A"), 667.0);
    assert_eq!(width(Std14::Helvetica, "€"), 556.0);
    assert_eq!(width(Std14::HelveticaBold, "m"), 889.0);
    assert_eq!(width(Std14::Courier, "Œ€i"), 1800.0);
    assert_eq!(width(Std14::TimesRoman, "ƒ"), 500.0);
    assert_eq!(width(Std14::Symbol, "α"), 631.0);
    assert_eq!(width(Std14::ZapfDingbats, "✔"), 846.0);
    // At a size: per 1000 em.
    assert_eq!(
        Std14::Helvetica.width("AA", 12.0),
        2.0 * 667.0 * 12.0 / 1000.0
    );
}

/// WinAnsiEncoding is Windows-1252 exactly, all 256 codes: each character
/// it draws encodes to its code and its code reads as it; a control code
/// or an empty one draws nothing, so its character is `?`.
#[test]
fn win_ansi_is_cp1252_both_ways() {
    for code in 0..=255u8 {
        let c = cp1252(code);
        let drawn = c.filter(|c| !c.is_control());
        assert_eq!(
            Encoding::WinAnsi.text(code),
            drawn.map(String::from),
            "{code:#x}"
        );
        let Some(c) = c else { continue };
        for font in Std14::ALL
            .into_iter()
            .filter(|f| f.encoding() == Encoding::WinAnsi)
        {
            let encoded = font.encode(&c.to_string());
            match drawn {
                Some(_) => assert_eq!(encoded, (vec![code], Unencoded(0)), "{font:?} {c:?}"),
                None => assert_eq!(encoded, (b"?".to_vec(), Unencoded(1)), "{font:?} {c:?}"),
            }
            assert_eq!(font.covers(c), drawn.is_some());
        }
    }
    // The ten cp1252 characters outside Latin-1, which the old header
    // encoder drew as `?`.
    let (codes, none) = Std14::Helvetica.encode("Š š Ž ž Œ œ Ÿ ƒ ˆ ˜");
    assert_eq!(codes, b"\x8a \x9a \x8e \x9e \x8c \x9c \x9f \x83 \x88 \x98");
    assert_eq!(none, Unencoded(0));
    // The second space and the soft hyphen are their own.
    assert_eq!(Std14::Helvetica.encode("\u{a0}\u{ad} -").0, b"\xa0\xad -");
}

#[test]
fn what_a_font_cant_encode_draws_a_question_mark_and_is_counted() {
    assert_eq!(
        Std14::Helvetica.encode("a中文b€"),
        (b"a??b\x80".to_vec(), Unencoded(2))
    );
    assert_eq!(Std14::Helvetica.width("中", 10.0), 5.56);
    // Composed first: a decomposed é is one character.
    assert_eq!(
        Std14::TimesRoman.encode("e\u{301}"),
        (vec![0xe9], Unencoded(0))
    );
    assert!(!Std14::Helvetica.covers('α'));
    // Symbol has a question mark; ZapfDingbats hasn't, so it leaves the
    // character out.
    assert_eq!(Std14::Symbol.encode("αx"), (b"a?".to_vec(), Unencoded(1)));
    assert_eq!(
        Std14::ZapfDingbats.encode("✔x✘"),
        (b"48".to_vec(), Unencoded(1))
    );
}

#[test]
fn symbol_and_zapf_dingbats_encode_their_own() {
    let symbol = Std14::Symbol;
    assert_eq!(symbol.encode("αβ∑≠ ").0, b"ab\xe5\xb9 ");
    assert!(symbol.covers('π') && symbol.covers('€') && !symbol.covers('A'));
    // Omega, Delta and mu, as the Greek letters and as the AGL reads
    // Symbol's glyphs: the ohm sign, the increment and the micro sign.
    let greek = (
        symbol.encode("\u{3a9}\u{394}\u{3bc}"),
        symbol.encode("\u{2126}\u{2206}\u{b5}"),
    );
    assert_eq!(greek.0, (b"WDm".to_vec(), Unencoded(0)));
    assert_eq!(greek.1, greek.0);
    assert!(symbol.covers('\u{2126}') && symbol.covers('\u{3a9}'));
    // The checks and crosses: ✓ is a19, ✔ a20, ✗ a23, ✘ a24.
    let dingbats = Std14::ZapfDingbats;
    assert_eq!(dingbats.encode("✓✔✗✘").0, b"3478");
    assert_eq!(dingbats.encode("☎ ").0, b"\x25 ");
    assert!(dingbats.covers('❤') && !dingbats.covers('a'));
}

#[test]
fn wrap_breaks_at_spaces_and_inside_long_words() {
    let font = Std14::Courier;
    // Courier is 600 per 1000 em: at 10 pt, 6 a character.
    let wrap = |s: &str, chars: f32| font.wrap(s, 10.0, chars * 6.0);
    assert_eq!(wrap("one two three", 7.0), ["one two", "three"]);
    assert_eq!(wrap("one  two\nthree", 20.0), ["one two", "three"]);
    assert_eq!(wrap("abcdefghij k", 4.0), ["abcd", "efgh", "ij k"]);
    assert_eq!(wrap("", 4.0), [""]);
    assert_eq!(wrap("a\n\nb", 4.0), ["a", "", "b"]);
    // A line holds at least a character, however narrow.
    assert_eq!(wrap("ab", 0.5), ["a", "b"]);
}

#[test]
fn glyphless_encodes_utf16_at_half_an_em() {
    let (codes, none) = Glyphless.encode("Hé𝄞");
    assert_eq!(codes, [0x00, 0x48, 0x00, 0xe9, 0xd8, 0x34, 0xdd, 0x1e]);
    assert_eq!(none, Unencoded(0));
    assert_eq!(Glyphless.advance(&codes, 10.0), 20.0);
    assert!(Glyphless.covers('中'));
    // Decomposed text stays as it was recognized.
    assert_eq!(Glyphless.encode("e\u{301}").0, [0, 0x65, 0x03, 0x01]);
    // Any font goes where a `dyn Font` does.
    let fonts: [&dyn Font; 2] = [&Std14::Courier, &Glyphless];
    assert_eq!(fonts.map(|f| f.width("ab", 10.0)), [12.0, 10.0]);
}

/// A font as a resource is its dictionary and what that reaches, written
/// into a document with numbers of its own.
#[test]
fn fonts_as_resources() {
    let helvetica = Std14::Helvetica.resource();
    let [Object::Dictionary(dict)] = &helvetica.objects[..] else {
        panic!("one dictionary")
    };
    assert_eq!(
        dict,
        &dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        }
    );
    let symbol = Std14::Symbol.resource();
    assert!(!symbol.objects[0].as_dict().unwrap().has(b"Encoding"));
    // Named by what they are.
    assert_eq!(helvetica.id, Std14::Helvetica.resource().id);
    let ids: std::collections::HashSet<_> = Std14::ALL.map(|f| f.resource().id).into();
    assert_eq!(ids.len(), 14);
    let mut reordered = Dictionary::new();
    for (key, value) in dict.iter().rev() {
        reordered.set(key.clone(), value.clone());
    }
    assert_eq!(ResId::of(&[reordered.into()]), helvetica.id);
    assert_ne!(Glyphless.resource().id, helvetica.id);

    let mut pdf = Pdf::with_version("1.7");
    pdf.add_object(dictionary! {});
    let font = ResObjects::default().object(&mut pdf, &Glyphless.resource().into());
    assert_eq!(pdf.objects.len(), 7);
    let type0 = pdf.get_dictionary(font).unwrap();
    let cid = type0.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]
        .as_reference()
        .unwrap();
    let cid = pdf.get_dictionary(cid).unwrap();
    assert_eq!(
        cid.get(b"Subtype").unwrap().as_name().unwrap(),
        b"CIDFontType2"
    );
    let descriptor = cid.get(b"FontDescriptor").unwrap().as_reference().unwrap();
    let program = pdf
        .get_dictionary(descriptor)
        .unwrap()
        .get(b"FontFile2")
        .unwrap()
        .as_reference()
        .unwrap();
    let program = pdf.get_object(program).unwrap().as_stream().unwrap();
    assert!(program.dict.has(b"Filter"), "compressed");
    assert_eq!(program.decompressed_content().unwrap(), Glyphless::PROGRAM);
    let to_unicode = type0.get(b"ToUnicode").unwrap().as_reference().unwrap();
    let to_unicode = pdf.get_object(to_unicode).unwrap().as_stream().unwrap();
    let cmap = to_unicode.decompressed_content().unwrap();
    assert!(cmap.windows(21).any(|w| w == b"<0000> <FFFF> <0000>\n"));
}

/// `font` read over `pdf`'s objects, within a page's budget.
fn foreign(pdf: &Pdf, font: Dictionary) -> Foreign {
    let mut budget = Limits::DEFAULT.content.decompressed;
    Foreign::read(pdf, &font, &Limits::DEFAULT, &mut budget).expect("within the budget")
}

/// The codes `s` is in `font`, each with where it is.
fn codes(font: &Foreign, s: &[u8]) -> Vec<(std::ops::Range<usize>, u32)> {
    font.codes(s).collect()
}

/// What's guessed of a font that embeds no program, and is no Type3
/// font, but whose metrics are all given: its glyphs' outlines.
const OUTLINES: FontApprox = FontApprox {
    codes: false,
    widths: false,
    heights: false,
    boxes: true,
};

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect { x0, y0, x1, y1 }
}

/// A simple font's widths: `/Widths` from `/FirstChar` to `/LastChar`,
/// whole, and `MissingWidth` for every other code; a code a byte.
#[test]
fn simple_widths_from_first_char_and_missing_width() {
    let mut pdf = Pdf::with_version("1.7");
    let descriptor = pdf.add_object(dictionary! {
        "Type" => "FontDescriptor", "MissingWidth" => 500, "Ascent" => 800, "Descent" => -200,
        "FontBBox" => vec![0.into(), (-200).into(), 1000.into(), 800.into()],
    });
    let truetype = |first: i64, last: i64| {
        dictionary! {
            "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "Foo",
            "FirstChar" => first, "LastChar" => last,
            "Widths" => vec![600.into(), Object::Real(700.9)],
            "FontDescriptor" => descriptor,
        }
    };
    let font = foreign(&pdf, truetype(65, 66));
    assert_eq!(font.kind(), FontKind::TrueType);
    let widths = [64, 65, 66, 67, 200].map(|c| font.width(c));
    assert_eq!(widths, [500.0, 600.0, 700.0, 500.0, 500.0]);
    assert_eq!(codes(&font, b"AB"), [(0..1, 65), (1..2, 66)]);
    assert_eq!(font.cid(65), None);
    // Its glyphs' outlines only a program it doesn't embed would say.
    assert_eq!(font.approx(), OUTLINES);
    // `/LastChar` before `/Widths` ends; 0, or past it, reads to its end.
    let short = foreign(&pdf, truetype(65, 65));
    assert_eq!([65, 66].map(|c| short.width(c)), [600.0, 500.0]);
    let open = foreign(&pdf, truetype(65, 0));
    assert_eq!(open.width(66), 700.0);
    // A `/FirstChar` past a byte's codes gives no widths.
    let past = foreign(&pdf, truetype(300, 301));
    assert_eq!(past.width(44), 500.0);
    // Without `MissingWidth`, 0; and a font sheaf has no widths for is a
    // guess.
    let mut bare = truetype(65, 66);
    bare.remove(b"FontDescriptor");
    let bare = foreign(&pdf, bare);
    assert_eq!(bare.width(67), 0.0);
    assert!(!bare.approx().widths);
    let mut unknown = truetype(65, 66);
    unknown.remove(b"Widths");
    assert!(foreign(&pdf, unknown).approx().widths);
}

/// A standard 14 font without `/Widths`: each code's glyph by its
/// encoding, at Adobe's width, with Adobe's box; a code the font has no
/// glyph for as the space, and the Euro, which Adobe gives no box, each
/// guessed, with none; Symbol in its own encoding whatever it names; one
/// with its program embedded, a guess; one with `/Widths`, its own.
#[test]
fn standard_14_without_widths() {
    let mut pdf = Pdf::with_version("1.7");
    let type1 = |base: &str| {
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => base.to_string() }
    };
    let helvetica = foreign(&pdf, type1("Helvetica"));
    assert_eq!(
        [b'A', b' ', 0x01].map(|c| helvetica.width(c.into())),
        [667.0, 278.0, 278.0]
    );
    assert_eq!(
        helvetica.glyph_box(b'A'.into()),
        Some(rect(14.0, 0.0, 654.0, 718.0))
    );
    assert_eq!(helvetica.glyph_box(b' '.into()), Some(Rect::default()));
    assert_eq!(helvetica.glyph_box(0x01), None);
    // Those StandardEncoding names no glyph for, or one PDFium's face is
    // another width of, and only those.
    let guessed = |c: u32| match Encoding::Standard.glyph(c as u8) {
        Some(name) => Std14::Helvetica.drawn_otherwise(name),
        None => true,
    };
    assert!((0..256).all(|c| helvetica.width_guessed(c) == guessed(c)));
    assert!(guessed(0x01) && guessed(0xAB) && !guessed(b'A'.into()));
    // A string has a code whose width is guessed where a byte of it is.
    for s in [
        &b""[..],
        b"A",
        b"\x01",
        b"\x01AB",
        b"AB\x01",
        b"A\xABB",
        b"ABC",
    ] {
        let any = s.iter().any(|&b| guessed(b.into()));
        assert_eq!(helvetica.width_guessed_in(s), any, "{s:?}");
    }
    let metrics = helvetica.metrics();
    assert_eq!((metrics.ascent, metrics.descent), (718.0, -207.0));
    assert_eq!(metrics.bbox, Some(rect(-166.0, -225.0, 1000.0, 931.0)));
    // PDFium takes its heights, and its glyphs' outlines, from the face it
    // substitutes.
    let heights = FontApprox {
        heights: true,
        ..OUTLINES
    };
    assert_eq!(helvetica.approx(), heights);

    let mut encoded = type1("Helvetica");
    encoded.set(
        "Encoding",
        dictionary! {
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![1.into(), "A".into()],
        },
    );
    let encoded = foreign(&pdf, encoded);
    assert_eq!([1, 0x80].map(|c| encoded.width(c)), [667.0, 556.0]);
    assert_eq!(
        [1, 0x80, 0x81, b'A'.into()].map(|c| encoded.width_guessed(c)),
        [false, true, true, false]
    );
    assert_eq!(encoded.glyph_box(0x80), None);
    let mut given = type1("Helvetica");
    given.set("Encoding", "WinAnsiEncoding");
    given.set("FirstChar", 0x80);
    given.set("Widths", vec![600.into()]);
    let given = foreign(&pdf, given);
    assert_eq!(given.width(0x80), 600.0);
    assert!((0..256).all(|c| !given.width_guessed(c)));
    assert!(!given.width_guessed_in(b"\x80\x01"));
    assert!(Std14::Courier.glyph_width("A") == Some(600.0));
    let courier = foreign(&pdf, type1("Courier"));
    assert!((0..=255).all(|c| courier.width(c) == 600.0));
    let mut symbol = type1("Symbol");
    symbol.set("Encoding", "WinAnsiEncoding");
    assert_eq!(foreign(&pdf, symbol).width(b'a'.into()), 631.0);

    let program = pdf.add_object(Stream::new(dictionary! {}, vec![0]));
    let descriptor = pdf.add_object(dictionary! { "FontFile" => program });
    let mut embedded = type1("ABCDEF+Helvetica");
    embedded.set("FontDescriptor", descriptor);
    let embedded = foreign(&pdf, embedded);
    assert_eq!(embedded.width(b'A'.into()), 667.0);
    assert!(embedded.approx().widths);
    assert!((0..256).all(|c| !embedded.width_guessed(c)));
    assert_eq!(embedded.glyph_box(b'A'.into()), None);
}

/// Each standard 14 font's heights and glyphs' boxes are its AFM's, every
/// glyph's within the font's box; Symbol and ZapfDingbats, which give no
/// `Ascender` or `Descender`, reach their box's top and bottom.
#[test]
fn standard_14_heights_and_boxes_are_the_afm_s() {
    for font in Std14::ALL {
        let metrics = font.metrics();
        let bbox = metrics.bbox.unwrap();
        assert!(metrics.descent < 0.0 && 0.0 < metrics.ascent, "{font:?}");
        assert!(
            bbox.y0 <= metrics.descent && metrics.ascent <= bbox.y1,
            "{font:?}"
        );
        for (name, width) in afm(font.afm_text()).0 {
            assert_eq!(font.glyph_width(name), Some(width), "{font:?} {name}");
            let b = font.glyph_box(name).unwrap();
            let within = bbox.x0 <= b.x0 && b.x1 <= bbox.x1 && bbox.y0 <= b.y0 && b.y1 <= bbox.y1;
            assert!(within, "{font:?} {name} {b:?}");
        }
    }
    let helvetica = Std14::Helvetica.metrics();
    assert_eq!((helvetica.ascent, helvetica.descent), (718.0, -207.0));
    let dingbats = Std14::ZapfDingbats.metrics();
    assert_eq!((dingbats.ascent, dingbats.descent), (820.0, -143.0));
    let a = Std14::TimesBold.glyph_box("A");
    assert_eq!(a, Some(rect(9.0, 0.0, 689.0, 690.0)));
    assert_eq!(Std14::Helvetica.glyph_box("nonesuch"), None);
}

/// PDFium's names for the standard 14: the 14, and its alternates, in any
/// case, after a subset's tag.
#[test]
fn standard_14_by_pdfium_s_names() {
    let named = |n: &str| Std14::named(n.as_bytes());
    assert_eq!(named("Helvetica"), Some(Std14::Helvetica));
    assert_eq!(named("Arial,Bold"), Some(Std14::HelveticaBold));
    assert_eq!(named("ABCDEF+ArialMT"), Some(Std14::Helvetica));
    assert_eq!(named("arialmt"), Some(Std14::Helvetica));
    assert_eq!(
        named("TimesNewRomanPS-BoldItalicMT"),
        Some(Std14::TimesBoldItalic)
    );
    assert_eq!(named("CourierNew,Italic"), Some(Std14::CourierOblique));
    assert_eq!(named("SymbolMT"), Some(Std14::Symbol));
    assert_eq!(named("ArialNarrow"), None);
    assert_eq!(named("ABC+Arial"), None);
    for font in Std14::ALL {
        assert_eq!(named(font.name()), Some(font));
    }
    // A file's Arial draws at Helvetica's widths.
    let pdf = Pdf::with_version("1.7");
    let arial = foreign(
        &pdf,
        dictionary! { "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "Arial,Bold" },
    );
    assert_eq!(arial.width(b'A'.into()), 722.0);
    assert!(!arial.approx().widths);
}

/// A Type0 font over `/Encoding`, with a CIDFont of `descendant`'s keys.
fn type0(pdf: &mut Pdf, encoding: Object, descendant: Dictionary) -> Dictionary {
    let cid_font = pdf.add_object(descendant);
    dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Foo",
        "Encoding" => encoding, "DescendantFonts" => vec![cid_font.into()],
    }
}

/// Identity-H: two bytes a code, each its CID, widths from `/W` in both
/// its forms, the first range given for a CID winning, and `/DW` for the
/// rest.
#[test]
fn identity_h_widths_from_w_and_dw() {
    let mut pdf = Pdf::with_version("1.7");
    let descriptor = pdf.add_object(dictionary! {
        "FontBBox" => vec![0.into(), (-200).into(), 1000.into(), 900.into()],
        "Ascent" => 900, "Descent" => -200,
    });
    let w: Vec<Object> = vec![
        1.into(),
        vec![500.into(), 600.into()].into(),
        10.into(),
        20.into(),
        700.into(),
        15.into(),
        vec![900.into()].into(),
    ];
    let font = type0(
        &mut pdf,
        "Identity-H".into(),
        dictionary! {
            "Subtype" => "CIDFontType2", "W" => w, "DW" => 900, "FontDescriptor" => descriptor,
        },
    );
    let font = foreign(&pdf, font);
    assert_eq!(font.kind(), FontKind::Type0(Descendant::CidFontType2));
    let widths = [1, 2, 3, 10, 15, 20, 21].map(|c| font.width(c));
    assert_eq!(widths, [500.0, 600.0, 900.0, 700.0, 700.0, 700.0, 900.0]);
    // A code cut off at the end has a 0 for its second byte, as PDFium
    // reads it.
    let split = codes(&font, &[0, 1, 0, 2, 0, 10, 7]);
    assert_eq!(split, [(0..2, 1), (2..4, 2), (4..6, 10), (6..7, 0x700)]);
    assert_eq!(font.cid(0x1234), Some(0x1234));
    assert_eq!(font.vertical(1), None);
    let metrics = font.metrics();
    assert_eq!((metrics.ascent, metrics.descent), (900.0, -200.0));
    assert_eq!(font.approx(), OUTLINES);
    // No `/DW`: 1000.
    let bare = type0(
        &mut pdf,
        "Identity-H".into(),
        dictionary! { "Subtype" => "CIDFontType0" },
    );
    let bare = foreign(&pdf, bare);
    assert_eq!(bare.kind(), FontKind::Type0(Descendant::CidFontType0));
    assert_eq!(bare.width(5), 1000.0);
    // A predefined CMap sheaf hasn't, or PDFium doesn't know, splits by
    // the `/ToUnicode` codespace, its CIDs unknown: a name outside Table
    // 118; CNS-EUC-V, which PDFium's table hasn't; KSCpc-EUC-V, which
    // Adobe's hasn't.
    for name in ["Adobe-Korea1-V", "CNS-EUC-V", "KSCpc-EUC-V"] {
        let predefined = type0(&mut pdf, name.into(), dictionary! { "DW" => 800 });
        let predefined = foreign(&pdf, predefined);
        assert!(predefined.approx().codes, "{name}");
        assert_eq!(predefined.cid(0x41), None);
        assert_eq!(predefined.width(0x41), 800.0);
        assert!(predefined.vertical(0x41).is_some());
    }
}

/// A predefined CMap sheaf has (hayro-cmap's bundle of Adobe's) and
/// PDFium knows: its codes split by PDFium's scheme for it, a byte, or
/// two where the first leads two (90ms-RKSJ-H), or two each
/// (UniJIS-UCS2-H), a code cut off at the end 0 for its second byte; each
/// code's CID Adobe's, by the code's length, 0 for a code it maps none
/// of; widths by those CIDs; `Tw` after a space of a byte; vertical where
/// its name ends in V. With no `/ToUnicode`, its text is as PDFium reads
/// it: a UCS2 CMap's codes, another's CIDs in Adobe-Japan1-UCS2, none for
/// the code 0.
#[test]
fn predefined_cmaps_are_adobe_s() {
    let mut pdf = Pdf::with_version("1.7");
    let w = |pairs: &[(i64, i64)]| -> Object {
        let w = (pairs.iter()).flat_map(|&(cid, w)| [cid.into(), vec![w.into()].into()]);
        Object::Array(w.collect())
    };
    let rksj = type0(
        &mut pdf,
        "90ms-RKSJ-H".into(),
        dictionary! { "W" => w(&[(264, 500), (843, 1000)]), "DW" => 700 },
    );
    let rksj = foreign(&pdf, rksj);
    assert!(!rksj.approx().codes);
    let split = codes(&rksj, b"A\x82\xa0\xb1\x88\x9f\x82");
    #[rustfmt::skip]
    assert_eq!(split, [(0..1, 0x41), (1..3, 0x82a0), (3..4, 0xb1), (4..6, 0x889f), (6..7, 0x8200)]);
    let cids = [0x41, 0x82a0, 0xb1, 0x889f, 0x8200, 0x20, 0x80].map(|c| rksj.cid(c));
    assert_eq!(cids, [264, 843, 343, 1125, 0, 231, 0].map(Some));
    let widths = [0x41, 0x82a0, 0xb1].map(|c| rksj.width(c));
    assert_eq!(widths, [500.0, 1000.0, 700.0]);
    assert!(rksj.word_spaced(32, 1));
    let text = rksj.text(b"A\x82\xa0\x88\x9f\xb1");
    assert_eq!(text.as_deref(), Some("Aあ亜ｱ"));
    assert_eq!(rksj.code_text(0x82a0).as_deref(), Some("あ"));
    assert!(!rksj.is_vertical());

    let ucs2 = type0(
        &mut pdf,
        "UniJIS-UCS2-H".into(),
        dictionary! { "W" => w(&[(34, 550)]) },
    );
    let ucs2 = foreign(&pdf, ucs2);
    assert!(!ucs2.approx().codes);
    let split = codes(&ucs2, b"\0A\x30\x42\x65");
    assert_eq!(split, [(0..2, 0x41), (2..4, 0x3042), (4..5, 0x6500)]);
    let cids = [0x41, 0x3042, 0x65e5, 0x20].map(|c| ucs2.cid(c));
    assert_eq!(cids, [34, 843, 3284, 1].map(Some));
    assert_eq!((ucs2.width(0x41), ucs2.width(0x3042)), (550.0, 1000.0));
    assert!(!ucs2.word_spaced(32, 2));
    let text = ucs2.text(b"\0A\x30\x42\x65\xe5");
    assert_eq!(text.as_deref(), Some("Aあ日"));
    assert_eq!(ucs2.text(b"\0\0"), None);

    let vertical = type0(&mut pdf, "90ms-RKSJ-V".into(), dictionary! {});
    let vertical = foreign(&pdf, vertical);
    assert!(vertical.is_vertical());
    let cids = (vertical.cid(0x8141), vertical.cid(0x41));
    assert_eq!(cids, (Some(7887), Some(264)));
}

/// A Type0 font's text: its `/ToUnicode`'s where it has one, its codes
/// split by that's codespace; without one, as PDFium reads it, Identity's
/// CIDs in the UCS2 CMap of the collection its CIDFont's
/// `/CIDSystemInfo` orders, by name or string, its registry unread; none
/// for an Identity font of another ordering, nor for an embedded CMap's
/// CIDs.
#[test]
fn type0_text_as_pdfium_reads_it() {
    let mut pdf = Pdf::with_version("1.7");
    let info = |ordering: Object| {
        dictionary! {
            "CIDSystemInfo" => dictionary! {
                "Registry" => Object::string_literal("Other"), "Ordering" => ordering,
            },
        }
    };
    let text = |pdf: &mut Pdf, encoding: Object, descendant: Dictionary| {
        let font = type0(pdf, encoding, descendant);
        foreign(pdf, font).text(b"\x01\x08\x03\x4b")
    };
    let japan1 = info(Object::string_literal("Japan1"));
    let named = info("Japan1".into());
    let identity = info(Object::string_literal("Identity"));
    let ordered = text(&mut pdf, "Identity-H".into(), japan1.clone());
    assert_eq!(ordered.as_deref(), Some("Aあ"));
    let ordered = text(&mut pdf, "Identity-V".into(), named);
    assert_eq!(ordered.as_deref(), Some("Aあ"));
    assert_eq!(text(&mut pdf, "Identity-H".into(), identity), None);
    let cmap = pdf.add_object(Stream::new(
        dictionary! {},
        b"1 begincidrange <0000> <FFFF> 0 endcidrange".to_vec(),
    ));
    assert_eq!(text(&mut pdf, cmap.into(), japan1.clone()), None);
    // CID 0, `.notdef`, is Adobe's U+FFFD, as PDFium reads it, but a
    // string showing it isn't text: it's told from another by its bytes.
    let font = type0(&mut pdf, "Identity-H".into(), japan1.clone());
    let font = foreign(&pdf, font);
    assert_eq!(font.code_text(0).as_deref(), Some("\u{FFFD}"));
    assert_eq!(font.text(b"\x01\x08\x00\x00"), None);
    let font = type0(&mut pdf, "90ms-RKSJ-H".into(), japan1.clone());
    let font = foreign(&pdf, font);
    assert_eq!(font.cid(0x80), Some(0));
    assert_eq!(font.text(b"A"), Some("A".into()));
    assert_eq!(font.text(b"A\x80"), None);

    let to_unicode = pdf.add_object(Stream::new(
        dictionary! {},
        b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
          2 beginbfchar <0108> <005A> <034B> <0059> endbfchar"
            .to_vec(),
    ));
    let mut font = type0(&mut pdf, "UniJIS-UCS2-H".into(), japan1);
    font.set("ToUnicode", to_unicode);
    let font = foreign(&pdf, font);
    assert_eq!(font.text(b"\x01\x08\x03\x4b").as_deref(), Some("ZY"));
    assert_eq!(font.text(b"\x30\x42"), None);
    assert_eq!(font.code_text(0x3042), None);
}

/// Identity-V: the vertical metrics `/W2` gives in both its forms, and
/// `/DW2`'s, centred on the width, for the rest.
#[test]
fn identity_v_metrics_from_w2_and_dw2() {
    let mut pdf = Pdf::with_version("1.7");
    let w2: Vec<Object> = vec![
        1.into(),
        vec![(-900).into(), 300.into(), 800.into()].into(),
        5.into(),
        9.into(),
        (-800).into(),
        250.into(),
        700.into(),
    ];
    let font = type0(
        &mut pdf,
        "Identity-V".into(),
        dictionary! {
            "Subtype" => "CIDFontType0", "W" => vec![3.into(), vec![600.into()].into()],
            "W2" => w2, "DW2" => vec![900.into(), (-1100).into()],
        },
    );
    let font = foreign(&pdf, font);
    let v = |w1, vx, vy| Some(VerticalMetrics { w1, vx, vy });
    assert_eq!(font.vertical(1), v(-900.0, 300.0, 800.0));
    assert_eq!(font.vertical(7), v(-800.0, 250.0, 700.0));
    assert_eq!(font.vertical(3), v(-1100.0, 300.0, 900.0));
    assert_eq!(font.vertical(4), v(-1100.0, 500.0, 900.0));
    let plain = type0(&mut pdf, "Identity-V".into(), dictionary! {});
    assert_eq!(foreign(&pdf, plain).vertical(4), v(-1000.0, 500.0, 880.0));
}

/// An embedded CMap: codes by its codespace, as PDFium splits them, a code
/// cut off at the end 0; CIDs by its `cidrange`s and `cidchar`s over
/// those of the CMap its `/UseCMap` names, `/WMode` from its program; a
/// CMap that uses itself read once; Identity used by `usecmap` known;
/// another predefined CMap, by `usecmap` or `/UseCMap`, read for the codes
/// nothing else maps, by their length, and guessed, as PDFium reads none,
/// as is one sheaf hasn't.
#[test]
fn embedded_cmap_over_the_one_it_uses() {
    let mut pdf = Pdf::with_version("1.7");
    let base = pdf.add_object(Stream::new(
        dictionary! { "Type" => "CMap" },
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
          2 begincodespacerange <00> <7F> <8000> <FFFF> endcodespacerange\n\
          1 begincidrange <20> <7E> 1 endcidrange\n\
          endcmap CMapName currentdict /CMap defineresource pop end end"
            .to_vec(),
    ));
    let own = pdf.add_object(Stream::new(
        dictionary! { "Type" => "CMap", "UseCMap" => base },
        b"begincmap /WMode 1 def\n\
          1 begincidchar <41> 500 endcidchar\n\
          1 begincidrange <8140> <8142> 1000 endcidrange endcmap"
            .to_vec(),
    ));
    let font = type0(
        &mut pdf,
        own.into(),
        dictionary! { "Subtype" => "CIDFontType0" },
    );
    let font = foreign(&pdf, font);
    let split = codes(&font, b"A B\x81\x41\x81");
    let expected = [
        (0..1, 0x41),
        (1..2, 0x20),
        (2..3, 0x42),
        (3..5, 0x8141),
        (5..6, 0),
    ];
    assert_eq!(split, expected);
    let cids = [0x41, 0x20, 0x42, 0x8141, 0x8142, 0x8000].map(|c| font.cid(c));
    assert_eq!(cids, [500, 1, 35, 1001, 1002, 0].map(Some));
    assert!(font.vertical(0x41).is_some());
    assert!(!font.approx().codes);

    let mut looped = Stream::new(
        dictionary! {},
        b"1 begincidchar <0005> 9 endcidchar".to_vec(),
    );
    let id = pdf.new_object_id();
    looped.dict.set("UseCMap", id);
    pdf.objects.insert(id, looped.into());
    let looped = type0(&mut pdf, id.into(), dictionary! {});
    let looped = foreign(&pdf, looped);
    assert_eq!(looped.cid(5), Some(9));
    assert_eq!(looped.vertical(5), None);

    let identity = pdf.add_object(Stream::new(
        dictionary! {},
        b"/Identity-H usecmap 1 begincidchar <0041> 7 endcidchar".to_vec(),
    ));
    let identity = type0(&mut pdf, identity.into(), dictionary! {});
    let identity = foreign(&pdf, identity);
    assert_eq!(codes(&identity, b"\0A\0B"), [(0..2, 0x41), (2..4, 0x42)]);
    assert_eq!(
        (identity.cid(0x41), identity.cid(0x42)),
        (Some(7), Some(0x42))
    );
    assert!(!identity.approx().codes);
    let other = pdf.add_object(Stream::new(
        dictionary! {},
        b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
          /UniJIS-UCS2-H usecmap 1 begincidchar <0041> 7 endcidchar"
            .to_vec(),
    ));
    let other = type0(&mut pdf, other.into(), dictionary! {});
    let other = foreign(&pdf, other);
    let cids = [0x41, 0x3042, 0xd800].map(|c| other.cid(c));
    assert_eq!(cids, [7, 843, 0].map(Some));
    assert!(other.approx().codes);
    let named = pdf.add_object(Stream::new(
        dictionary! { "UseCMap" => "90ms-RKSJ-H" },
        b"1 begincodespacerange <00> <FF> endcodespacerange".to_vec(),
    ));
    let named = type0(&mut pdf, named.into(), dictionary! {});
    let named = foreign(&pdf, named);
    assert_eq!((named.cid(0x41), named.cid(0xb1)), (Some(264), Some(343)));
    assert!(named.approx().codes);
    let unknown = pdf.add_object(Stream::new(
        dictionary! {},
        b"/Adobe-Korea1-2 usecmap".to_vec(),
    ));
    let unknown = type0(&mut pdf, unknown.into(), dictionary! {});
    assert!(foreign(&pdf, unknown).approx().codes);
}

/// An embedded CMap's codes as PDFium splits them
/// (`CPDF_CMapParser::HandleCodeSpaceRange`, `CPDF_CMap::GetNextChar`):
/// by one range a byte each, but two where it's of two bytes, whatever
/// the bytes; by more, a byte at a time while the last range of more
/// bytes whose first byte holds theirs is longer, a code as long as that
/// range though its later bytes are past its bounds, and 0 for a byte no
/// range begins with.
#[test]
fn codes_split_as_pdfium_splits_them() {
    let mut pdf = Pdf::with_version("1.7");
    let mut cmap = |space: &str| {
        let program = format!("begincmap 2 begincodespacerange {space} endcodespacerange endcmap");
        let id = pdf.add_object(Stream::new(dictionary! {}, program.into_bytes()));
        let font = type0(&mut pdf, id.into(), dictionary! {});
        foreign(&pdf, font)
    };
    let four = cmap("<00000000> <FFFFFFFF>");
    assert_eq!(codes(&four, b"\x01\x02"), [(0..1, 1), (1..2, 2)]);
    let two = cmap("<8140> <9FFC>");
    assert_eq!(codes(&two, b"\x01\x02\x03"), [(0..2, 0x102), (2..3, 0x300)]);
    let mixed = cmap("<00> <7F> <8140> <817E>");
    let split = codes(&mixed, b"A\x81\x41\x81\x20\x90\x81");
    #[rustfmt::skip]
    assert_eq!(split, [(0..1, 0x41), (1..3, 0x8141), (3..5, 0x8120), (5..6, 0), (6..7, 0)]);
    assert!(!mixed.approx().codes);
}

/// What reading a font's streams decompresses to is charged to the budget
/// it's read within: its CharProcs, each stream once however many names
/// draw it, and its `/ToUnicode`; and an embedded CMap's chain. Past the
/// budget it fails, and the budget is as it was. A source's font, read
/// before, costs as much again ([`Fonts::get`]), so what a page costs is
/// the same whichever page read it first; within a scope, once by any
/// name, and a font written as a dictionary each time it's read.
#[test]
fn reading_a_font_is_charged_to_its_budget() {
    let mut pdf = Pdf::with_version("1.7");
    let proc = b"100 0 0 0 100 100 d1 0 0 100 100 re f".to_vec();
    let to_unicode = b"1 beginbfchar <61> <0041> endbfchar".to_vec();
    let cost = proc.len() + to_unicode.len();
    let proc = pdf.add_object(Stream::new(dictionary! {}, proc));
    let to_unicode = pdf.add_object(Stream::new(dictionary! {}, to_unicode));
    let type3 = dictionary! {
        "Type" => "Font", "Subtype" => "Type3",
        "FontMatrix" => vec![0.001.into(), 0.into(), 0.into(), 0.001.into(), 0.into(), 0.into()],
        "Encoding" => dictionary! { "Differences" => vec![97.into(), "a".into(), "b".into(), "c".into()] },
        "CharProcs" => dictionary! { "a" => proc, "b" => proc, "c" => proc },
        "ToUnicode" => to_unicode,
    };
    let lim = &Limits::DEFAULT;
    let mut budget = cost;
    let font = Foreign::read(&pdf, &type3, lim, &mut budget).unwrap();
    assert_eq!((font.width(98), budget), (100.0, 0));
    let mut budget = cost - 1;
    assert_eq!(
        Foreign::read(&pdf, &type3, lim, &mut budget).err(),
        Some(TooLarge)
    );
    assert_eq!(budget, cost - 1);

    let by_ref = pdf.add_object(type3.clone());
    let resources: Object = dictionary! {
        "Font" => dictionary! { "R1" => by_ref, "R2" => by_ref, "D1" => type3.clone() },
    }
    .into();
    let fonts = Fonts::default();
    let mut budget = 10 * cost;
    fonts.get(&pdf, &by_ref.into(), lim, &mut budget).unwrap();
    fonts.get(&pdf, &by_ref.into(), lim, &mut budget).unwrap();
    assert_eq!(budget, 8 * cost);
    let mut budget = cost - 1;
    let again = fonts.get(&pdf, &by_ref.into(), lim, &mut budget);
    assert_eq!(again.err(), Some(TooLarge));
    let mut scope = fonts.scope();
    let mut budget = 10 * cost;
    for name in [&b"R1"[..], b"R2", b"R1", b"D1", b"D1"] {
        let picked = scope.font(&pdf, (1, 0), Some(&resources), name, lim, &mut budget);
        assert!(picked.unwrap().is_some());
    }
    let picked = scope.font(&pdf, (2, 0), Some(&resources), b"D1", lim, &mut budget);
    assert!(picked.unwrap().is_some());
    // R1 and R2 once, and D1 once for each owner it's picked by.
    assert_eq!(budget, 7 * cost);

    // An embedded CMap's chain: the CMap, and the one it uses.
    let base_program = b"1 begincodespacerange <00> <FF> endcodespacerange".to_vec();
    let own_program = b"1 begincidrange <00> <FF> 0 endcidrange".to_vec();
    let chain = base_program.len() + own_program.len();
    let base = pdf.add_object(Stream::new(dictionary! {}, base_program));
    let own = pdf.add_object(Stream::new(dictionary! { "UseCMap" => base }, own_program));
    let font = type0(&mut pdf, own.into(), dictionary! {});
    let mut budget = chain;
    assert!(Foreign::read(&pdf, &font, lim, &mut budget).is_ok());
    assert_eq!(budget, 0);
    let mut budget = chain - 1;
    assert_eq!(
        Foreign::read(&pdf, &font, lim, &mut budget).err(),
        Some(TooLarge)
    );
}

/// An embedded CMap whose chain can't be read whole has its codes
/// guessed: a `/UseCMap` that's no stream, and a chain past
/// [`ContentLimits::depth`](crate::source::ContentLimits).
#[test]
fn a_cmap_chain_read_in_part_is_guessed() {
    let mut pdf = Pdf::with_version("1.7");
    let program = || b"1 begincidrange <00> <FF> 0 endcidrange".to_vec();
    let nothing = pdf.add_object(Object::Integer(7));
    let broken = pdf.add_object(Stream::new(dictionary! { "UseCMap" => nothing }, program()));
    let broken = type0(&mut pdf, broken.into(), dictionary! {});
    assert!(foreign(&pdf, broken).approx().codes);
    let mut next = pdf.add_object(Stream::new(dictionary! {}, program()));
    for _ in 0..2 {
        next = pdf.add_object(Stream::new(dictionary! { "UseCMap" => next }, program()));
    }
    let long = type0(&mut pdf, next.into(), dictionary! {});
    let mut lim = Limits::DEFAULT;
    lim.content.depth = 3;
    let read = |lim: &Limits| {
        let mut budget = lim.content.decompressed;
        Foreign::read(&pdf, &long, lim, &mut budget)
            .unwrap()
            .approx()
            .codes
    };
    assert!(!read(&lim));
    lim.content.depth = 2;
    assert!(read(&lim));
}

/// A Type3 font: its widths and boxes through a `/FontMatrix` that isn't
/// a thousandth; a glyph's box from its CharProc's `d1`, else the
/// font's `/FontBBox`; a width `/Widths` doesn't give from its CharProc;
/// a code with no CharProc drawing nothing.
#[test]
fn type3_through_its_font_matrix() {
    let mut pdf = Pdf::with_version("1.7");
    let mut proc = |content: &[u8]| pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let (a, b, c) = (
        proc(b"50 0 10 0 60 80 d1 0 0 50 80 re f"),
        proc(b"40 0 d0 1 0 0 rg 0 0 40 40 re f"),
        proc(b"0 0 0 0 0 0 d1"),
    );
    let type3 = |bbox: Option<Vec<Object>>| {
        let mut font = dictionary! {
            "Type" => "Font", "Subtype" => "Type3",
            "FontMatrix" => vec![0.01.into(), 0.into(), 0.into(), 0.01.into(), 0.into(), 0.into()],
            "FirstChar" => 65, "Widths" => vec![50.into(), 0.into()],
            "Encoding" => dictionary! {
                "Differences" => vec![65.into(), "a".into(), "b".into(), "c".into()],
            },
            "CharProcs" => dictionary! { "a" => a, "b" => b, "c" => c },
        };
        if let Some(bbox) = bbox {
            font.set("FontBBox", bbox);
        }
        font
    };
    let bbox = vec![0.into(), (-10).into(), 100.into(), 90.into()];
    let font = foreign(&pdf, type3(Some(bbox)));
    assert_eq!(font.kind(), FontKind::Type3);
    assert_eq!(
        [65, 66, 67, 68].map(|c| font.width(c)),
        [500.0, 400.0, 0.0, 0.0]
    );
    // The box cut to whole thousandths as PDFium cuts it: -10 × 0.01 ×
    // 1000 is -99.99999 in an `f32`.
    let whole = rect(0.0, -99.0, 1000.0, 900.0);
    assert_eq!(font.glyph_box(65), Some(rect(100.0, 0.0, 600.0, 800.0)));
    assert_eq!(font.glyph_box(66), Some(whole));
    assert_eq!(font.glyph_box(67), Some(whole));
    assert_eq!(font.glyph_box(68), Some(Rect::default()));
    // `A`'s top, its `d1`'s, and the box's bottom, as there's no `g`.
    let metrics = font.metrics();
    assert_eq!((metrics.ascent, metrics.descent), (800.0, -99.0));
    assert_eq!(codes(&font, b"AB"), [(0..1, 65), (1..2, 66)]);
    assert!(!font.approx().any(), "{:?}", font.approx());
    // A matrix that turns glyph space over leaves the box's top below its
    // bottom, as PDFium keeps it, and the ascent, there being no `A`, the
    // box's top, held within it no longer; where `A`'s top is the box's
    // bottom, the heights are one, and a guess.
    let mut over = type3(Some(vec![0.into(), 0.into(), 1000.into(), 1000.into()]));
    let turned = [0.01, 0.0, 0.0, -0.01, 0.0, 0.0].map(Object::Real);
    over.set("FontMatrix", turned.to_vec());
    let mut moved = over.clone();
    let differences = vec![66.into(), "a".into(), "b".into(), "c".into()];
    moved.set("Encoding", dictionary! { "Differences" => differences });
    let moved = foreign(&pdf, moved);
    let m = moved.metrics();
    assert_eq!(
        (m.ascent, m.descent, moved.approx().heights),
        (-10000.0, 0.0, false)
    );
    assert_eq!(moved.glyph_box(66), Some(rect(100.0, -800.0, 600.0, 0.0)));
    let over = foreign(&pdf, over);
    let m = over.metrics();
    assert_eq!(
        (m.ascent, m.descent, over.approx().heights),
        (0.0, 0.0, true)
    );
    // With no `/FontBBox`, a glyph `d1` gives no box for is a guess.
    let unbounded = foreign(&pdf, type3(None));
    assert_eq!(
        unbounded.glyph_box(65),
        Some(rect(100.0, 0.0, 600.0, 800.0))
    );
    assert_eq!(unbounded.glyph_box(66), None);
    assert!(unbounded.approx().boxes && unbounded.approx().heights);
}

/// Ascent and descent: the descriptor's within its `/FontBBox`, a
/// positive `Descent` read as below; else guessed, as PDFium takes them
/// from the face it draws in: the descriptor's, else the standard 14
/// font's AFM's, else the `/FontBBox`'s, else an em above and a quarter
/// below; and guessed where they're one, as PDFium then bounds a glyph by
/// its outline.
#[test]
fn heights_by_each_fallback() {
    let mut pdf = Pdf::with_version("1.7");
    let mut heights = |base: &str, descriptor: Option<Dictionary>| {
        let mut font =
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => base.to_string() };
        if let Some(d) = descriptor {
            font.set("FontDescriptor", pdf.add_object(d));
        }
        let font = foreign(&pdf, font);
        let m = font.metrics();
        (m.ascent, m.descent, font.approx().heights)
    };
    let bbox = |l: i64, b: i64, r: i64, t: i64| -> Object {
        vec![l.into(), b.into(), r.into(), t.into()].into()
    };
    let clamped =
        dictionary! { "Ascent" => 900, "Descent" => -300, "FontBBox" => bbox(0, -200, 1000, 800) };
    assert_eq!(heights("Foo", Some(clamped)), (800.0, -200.0, false));
    let positive = dictionary! {
        "Ascent" => 700, "Descent" => 250, "FontBBox" => bbox(0, -300, 1000, 900),
    };
    assert_eq!(heights("Foo", Some(positive)), (700.0, -250.0, false));
    let unboxed = dictionary! { "Ascent" => 700, "Descent" => -250 };
    assert_eq!(heights("Foo", Some(unboxed)), (700.0, -250.0, true));
    let boxed = dictionary! { "FontBBox" => bbox(-50, -150, 1000, 850) };
    assert_eq!(heights("Foo", Some(boxed)), (850.0, -150.0, true));
    let zeros = dictionary! { "Ascent" => 0, "Descent" => 0, "FontBBox" => bbox(0, 0, 0, 0) };
    assert_eq!(heights("Times-Roman", Some(zeros)), (683.0, -217.0, true));
    assert_eq!(heights("Symbol", None), (1010.0, -293.0, true));
    let short = dictionary! { "FontBBox" => bbox(0, -100, 1000, 600) };
    assert_eq!(heights("Helvetica", Some(short)), (600.0, -100.0, true));
    let tall = dictionary! { "Ascent" => 950, "Descent" => -100 };
    assert_eq!(heights("Helvetica", Some(tall)), (950.0, -100.0, true));
    assert_eq!(heights("Foo", None), (1000.0, -250.0, true));
    assert_eq!(heights("Foo", Some(dictionary! {})), (1000.0, -250.0, true));
    let one = dictionary! { "Ascent" => 5, "Descent" => 5, "FontBBox" => bbox(0, -200, 1000, 800) };
    assert_eq!(heights("Foo", Some(one)), (5.0, 5.0, true));
}

/// Arrays a font is read from never expand past their own length: a
/// `/W` range of 2³¹ CIDs is one range, a hundred thousand of them over
/// each other the first, and `/Widths` far longer than 256 codes, 256.
#[test]
fn hostile_metrics_stay_bounded() {
    let mut pdf = Pdf::with_version("1.7");
    let mut w: Vec<Object> = vec![0.into(), i64::from(i32::MAX).into(), 500.into()];
    for i in 0..100_000 {
        w.extend([0.into(), u32::MAX.into(), (i + 1).into()]);
    }
    let font = type0(
        &mut pdf,
        "Identity-H".into(),
        dictionary! { "W" => w, "DW" => 7 },
    );
    let font = foreign(&pdf, font);
    assert_eq!(
        [0, 12345, i32::MAX as u32].map(|c| font.width(c)),
        [500.0; 3]
    );
    assert_eq!(font.width(u32::MAX), 7.0);
    let widths: Vec<Object> = (0..1_000_000).map(Object::from).collect();
    let simple = dictionary! { "Subtype" => "Type1", "FirstChar" => 0, "Widths" => widths };
    let simple = foreign(&pdf, simple);
    assert_eq!(simple.width(255), 255.0);
    let differences = vec![
        Object::Integer(1 << 40),
        "A".into(),
        Object::Integer(-5),
        "B".into(),
    ];
    let encoded = dictionary! {
        "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => dictionary! { "Differences" => differences },
    };
    assert_eq!(foreign(&pdf, encoded).width(b'A'.into()), 667.0);
}

/// A source's fonts are read once each: an object's for every page, and
/// one written in the resources as a dictionary once an interpretation.
#[test]
fn fonts_are_read_once() {
    let mut pdf = Pdf::with_version("1.7");
    let helvetica = pdf.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let resources: Object = dictionary! {
        "Font" => dictionary! {
            "F1" => dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" },
            "F2" => helvetica,
        },
    }
    .into();
    let lim = &Limits::DEFAULT;
    let budget = &mut lim.content.decompressed.clone();
    let fonts = Fonts::default();
    let get = |font: &Object, budget: &mut usize| fonts.get(&pdf, font, lim, budget).unwrap();
    let by_ref = get(&helvetica.into(), budget).unwrap();
    assert!(Arc::ptr_eq(
        &by_ref,
        &get(&helvetica.into(), budget).unwrap()
    ));
    let (page, other) = ((1, 0), (2, 0));
    let mut pick = |scope: &mut FontScope<'_>, owner, name: &[u8]| {
        let picked = scope.font(&pdf, owner, Some(&resources), name, lim, budget);
        picked.unwrap()
    };
    let mut scope = fonts.scope();
    let f1 = pick(&mut scope, page, b"F1").unwrap();
    assert_eq!(scope.get(f1).width(b'A'.into()), 600.0);
    assert_eq!(pick(&mut scope, page, b"F1"), Some(f1));
    let other_f1 = pick(&mut scope, other, b"F1").unwrap();
    assert!(!std::ptr::eq(scope.get(f1), scope.get(other_f1)));
    let f2 = pick(&mut scope, page, b"F2").unwrap();
    assert!(std::ptr::eq(&*by_ref, scope.get(f2)));
    assert!(pick(&mut scope, page, b"F3").is_none());
    let mut again = fonts.scope();
    let f2 = pick(&mut again, other, b"F2").unwrap();
    assert!(std::ptr::eq(&*by_ref, again.get(f2)));
    assert!(get(&Object::Integer(1), budget).is_none());
}

/// Font `name` of [`fixture::programs`]' page, read within `budget`, its
/// program read where `programs` says.
fn program_font(f: &crate::content::fixture::Fixture, name: &[u8], programs: bool) -> Arc<Foreign> {
    let fonts = f
        .resources()
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap();
    let font = fonts.as_dict().unwrap().get(name).unwrap();
    let read = match programs {
        true => Fonts::default(),
        false => Fonts::for_text(),
    };
    let mut budget = Limits::DEFAULT.content.decompressed;
    read.get(&f.pdf, font, &Limits::DEFAULT, &mut budget)
        .unwrap()
        .unwrap()
}

/// Each glyph's box is its program's, as FreeType loads it unscaled and
/// PDFium normalizes it to thousandths, a side below 0 a unit up
/// (`NormalizeFontMetric`); a code PDFium maps to no glyph has `-1` all
/// round in a simple font; a CIDFont's top is raised by a 64th of itself,
/// and a CID past its glyphs is all 0. TrueType, CFF and Type 1 alike.
#[test]
fn outline_boxes_are_the_program_s() {
    let f = crate::content::fixture::programs();
    let char_box = |font: &Foreign, code: u8| font.char_box(code.into());
    let truetype = program_font(&f, b"PT", true);
    assert_eq!(truetype.program(), FontProgram::Read);
    assert!(!truetype.approx().boxes);
    assert_eq!(
        char_box(&truetype, b'A'),
        Some(rect(0.0, 0.0, 600.0, 750.0))
    );
    // Its off-curve point is a control point the box holds.
    assert_eq!(
        char_box(&truetype, b'g'),
        Some(rect(20.0, -199.0, 480.0, 560.0))
    );
    assert_eq!(char_box(&truetype, b' '), Some(Rect::default()));
    // Drawn from its side bearing, not its outline's left.
    assert_eq!(
        char_box(&truetype, b'B'),
        Some(rect(50.0, 0.0, 350.0, 300.0))
    );
    assert_eq!(
        char_box(&truetype, b'C'),
        Some(rect(100.0, 50.0, 700.0, 800.0))
    );
    // `D` has no glyph, and draws `.notdef`; so does a code of no name.
    let notdef = Some(rect(50.0, 0.0, 450.0, 700.0));
    assert_eq!(
        (char_box(&truetype, b'D'), char_box(&truetype, 1)),
        (notdef, notdef)
    );
    for name in [&b"PC"[..], b"P1"] {
        let font = program_font(&f, name, true);
        assert_eq!(font.program(), FontProgram::Read, "{name:?}");
        assert_eq!(char_box(&font, b'A'), Some(rect(0.0, 0.0, 600.0, 750.0)));
        // The curve's control points.
        assert_eq!(
            char_box(&font, b'g'),
            Some(rect(20.0, -199.0, 480.0, 560.0))
        );
        assert_eq!(char_box(&font, b' '), Some(Rect::default()));
        assert_eq!(char_box(&font, b'D'), Some(Rect::default()), "{name:?}");
        assert_eq!(char_box(&font, 1), Some(rect(-1.0, -1.0, -1.0, -1.0)));
    }
    let cid = program_font(&f, b"PI", true);
    assert_eq!(cid.char_box(1), Some(rect(0.0, 0.0, 600.0, 761.0)));
    assert_eq!(cid.char_box(2), Some(rect(20.0, -199.0, 480.0, 568.0)));
    assert_eq!(cid.char_box(3), Some(Rect::default()));
    assert_eq!(cid.char_box(4), Some(rect(50.0, 0.0, 350.0, 304.0)));
    assert_eq!(cid.char_box(5), Some(rect(100.0, 50.0, 700.0, 812.0)));
    assert_eq!(cid.char_box(9), Some(Rect::default()));
    // Read for text, it has none.
    let text = program_font(&f, b"PT", false);
    assert_eq!(
        (text.program(), char_box(&text, b'A')),
        (FontProgram::None, None)
    );
}

/// Where the descriptor has no `/FontBBox`, the heights are the program's
/// face's, within its box, as PDFium takes them (`CheckFontMetrics`); where
/// it has one but gives `Ascent` and `Descent` 0, `A`'s top and `g`'s
/// bottom. Read for text, both are guessed, as without a program.
#[test]
fn heights_from_the_program_where_the_descriptor_lacks_them() {
    let f = crate::content::fixture::programs();
    let heights = |name: &[u8], programs: bool| {
        let font = program_font(&f, name, programs);
        let m = font.metrics();
        (m.ascent, m.descent, m.bbox, font.approx().heights)
    };
    let given = Some(rect(0.0, -200.0, 700.0, 800.0));
    assert_eq!(heights(b"PT", true), (800.0, -200.0, given, false));
    let face = Some(rect(0.0, -199.0, 700.0, 800.0));
    assert_eq!(heights(b"PN", true), (800.0, -199.0, face, false));
    let own = Some(rect(0.0, -300.0, 1000.0, 1000.0));
    assert_eq!(heights(b"PZ", true), (750.0, -199.0, own, false));
    assert!(heights(b"PN", false).3 && heights(b"PZ", false).3);
}

/// [`fixture::type1_program`](crate::content::fixture::type1_program)
/// with a procedure `levels` deep in its clear text.
fn type1_nested(levels: usize) -> Vec<u8> {
    let program = crate::content::fixture::type1_program();
    let at = program.windows(4).position(|w| w == b"/Pai").unwrap();
    let nested = format!(
        "/Nested {}{}def\n",
        "{ ".repeat(levels),
        "} ".repeat(levels)
    );
    [&program[..at], nested.as_bytes(), &program[at..]].concat()
}

/// A Type 1 program whose procedures nest as deep as the scan allows is
/// read; one a level deeper isn't handed to read-fonts.
#[test]
fn type1_procedures_nest_within_the_scan_s_depth() {
    let depth = super::type1::PROC_DEPTH;
    assert!(super::type1::scan(&type1_nested(depth)).is_some());
    assert!(super::type1::scan(&type1_nested(depth + 1)).is_none());
}

/// A Type 1 program whose integers run as long as the scan reads ahead of
/// is read; one an integer longer isn't handed to read-fonts, nor a run
/// of a hundred thousand, which the scan refuses within its stack.
#[test]
fn type1_integers_run_within_the_scan_s_lookahead() {
    use crate::content::fixture::type1_integers_program as type1_integers;
    let ahead = super::type1::INTS_AHEAD;
    assert!(super::type1::scan(&type1_integers(ahead)).is_some());
    assert!(super::type1::scan(&type1_integers(ahead + 1)).is_none());
    assert!(super::type1::scan(&type1_integers(100_000)).is_none());
}

/// A program that doesn't read, cut short, garbage, of no format, or a
/// Type 1 program whose procedures nest, or whose integers run, past what
/// read-fonts is handed, is malformed: the font's boxes guessed, its heights as without one;
/// never a panic.
#[test]
fn malformed_programs_fall_back() {
    use crate::content::fixture::{
        cff_program, truetype_program, type1_integers_program, type1_program,
    };
    let half = |p: Vec<u8>| p[..p.len() / 2].to_vec();
    let programs = [
        ("TrueType", "FontFile2", half(truetype_program())),
        ("TrueType", "FontFile2", b"\0\x01\0\0garbage".to_vec()),
        ("Type1", "FontFile3", half(cff_program())),
        ("Type1", "FontFile", half(type1_program())),
        ("Type1", "FontFile", type1_nested(17)),
        ("Type1", "FontFile", type1_integers_program(100_000)),
        ("Type1", "FontFile3", b"\xff\xd8\xff\xe0 a JPEG".to_vec()),
    ];
    for (subtype, key, program) in programs {
        let mut pdf = Pdf::with_version("1.7");
        let program = pdf.add_object(Stream::new(dictionary! {}, program));
        let descriptor = pdf.add_object(dictionary! {
            "Type" => "FontDescriptor", "Flags" => 32, "Ascent" => 700, "Descent" => -200,
            key => program,
        });
        let font = foreign(
            &pdf,
            dictionary! {
                "Type" => "Font", "Subtype" => subtype, "BaseFont" => "Bad",
                "FirstChar" => 65, "LastChar" => 65, "Widths" => vec![600.into()],
                "FontDescriptor" => descriptor,
            },
        );
        assert_eq!(font.program(), FontProgram::Malformed, "{subtype} {key}");
        assert!(font.approx().boxes && font.approx().heights);
        assert_eq!(font.char_box(65), None);
        assert_eq!(
            (font.metrics().ascent, font.metrics().descent),
            (700.0, -200.0)
        );
    }
}

/// A CIDFont's program is read once, however many of its glyphs are
/// drawn: two thousand CIDs over a Type 1 program of a MiB, each drawn as
/// it's asked for, are drawn quickly.
#[test]
fn a_cid_font_s_program_is_read_once() {
    let program = crate::content::fixture::type1_program();
    let at = program.windows(4).position(|w| w == b"/Pai").unwrap();
    let comment = [&b"%"[..], &vec![b'x'; 1 << 20], b"\n"].concat();
    let program = [&program[..at], &comment, &program[at..]].concat();
    let mut pdf = Pdf::with_version("1.7");
    let program = pdf.add_object(Stream::new(dictionary! {}, program));
    let descriptor = pdf.add_object(dictionary! {
        "Type" => "FontDescriptor", "Flags" => 32, "Ascent" => 750, "Descent" => -200,
        "FontFile" => program,
    });
    let font = type0(
        &mut pdf,
        "Identity-H".into(),
        dictionary! { "Subtype" => "CIDFontType0", "FontDescriptor" => descriptor },
    );
    let font = foreign(&pdf, font);
    assert_eq!(font.program(), FontProgram::Read);
    let started = std::time::Instant::now();
    assert_eq!(font.char_box(2), Some(rect(0.0, 0.0, 600.0, 761.0)));
    for cid in 0..2000 {
        font.char_box(cid);
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

/// An SFNT whose `loca` record is past its end, which FreeType reads
/// past, and so sheaf: its glyphs taken for none, it reads, over its CFF.
#[test]
fn a_loca_record_past_the_end_is_read_past() {
    let mut pdf = Pdf::with_version("1.7");
    let program = crate::content::fixture::loca_past_end_program();
    let program = pdf.add_object(Stream::new(
        dictionary! { "Subtype" => "OpenType" },
        program,
    ));
    let descriptor = pdf.add_object(dictionary! {
        "Type" => "FontDescriptor", "Flags" => 32, "Ascent" => 750, "Descent" => -200,
        "FontFile3" => program,
    });
    let font = foreign(
        &pdf,
        dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Fix",
            "FirstChar" => 65, "LastChar" => 65, "Widths" => vec![600.into()],
            "Encoding" => "WinAnsiEncoding", "FontDescriptor" => descriptor,
        },
    );
    assert_eq!(font.program(), FontProgram::Read);
}

/// A program's work is bounded: a CFF glyph whose subroutines call each
/// other past its fuel, and a TrueType composite of composites past the
/// glyphs one may draw, are unknown, and found so quickly; one within its
/// fuel is drawn.
#[test]
fn programs_past_their_work_are_unknown() {
    use crate::content::fixture::{composite_bomb_program, heavy_cff_program};
    let font = |subtype: &str, key: &str, program: Vec<u8>| {
        let mut pdf = Pdf::with_version("1.7");
        let program = pdf.add_object(Stream::new(dictionary! {}, program));
        let descriptor = pdf.add_object(dictionary! {
            "Type" => "FontDescriptor", "Flags" => 32, "Ascent" => 750, "Descent" => 0,
            "FontBBox" => vec![0.into(), 0.into(), 600.into(), 750.into()], key => program,
        });
        foreign(
            &pdf,
            dictionary! {
                "Type" => "Font", "Subtype" => subtype, "BaseFont" => "Heavy",
                "FirstChar" => 65, "LastChar" => 65, "Widths" => vec![600.into()],
                "Encoding" => "WinAnsiEncoding", "FontDescriptor" => descriptor,
            },
        )
    };
    let started = std::time::Instant::now();
    let light = font("Type1", "FontFile3", heavy_cff_program(3));
    assert_eq!(light.char_box(65), Some(rect(0.0, 0.0, 640.0, 640.0)));
    let heavy = font("Type1", "FontFile3", heavy_cff_program(10));
    assert_eq!(
        (heavy.program(), heavy.char_box(65)),
        (FontProgram::Read, None)
    );
    let mut pdf = Pdf::with_version("1.7");
    let bomb = pdf.add_object(Stream::new(dictionary! {}, composite_bomb_program()));
    let descriptor = pdf.add_object(dictionary! {
        "Type" => "FontDescriptor", "Flags" => 32, "Ascent" => 750, "Descent" => 0,
        "FontBBox" => vec![0.into(), 0.into(), 600.into(), 750.into()], "FontFile2" => bomb,
    });
    let bomb = type0(
        &mut pdf,
        "Identity-H".into(),
        dictionary! { "Subtype" => "CIDFontType2", "FontDescriptor" => descriptor },
    );
    let bomb = foreign(&pdf, bomb);
    assert_eq!(bomb.char_box(0), None);
    assert_eq!(bomb.char_box(6), Some(rect(0.0, 0.0, 600.0, 761.0)));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

/// What a program decompresses to is charged to the budget its font is
/// read within, a `/CIDToGIDMap` stream's too, and a simple font's what
/// drawing its glyphs costs: past it, the font is too large; read for
/// text, none of it is charged.
#[test]
fn a_program_is_charged_to_the_budget() {
    let f = crate::content::fixture::programs();
    let font = |name: &[u8]| {
        let fonts = f
            .resources()
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .unwrap();
        let font = fonts
            .as_dict()
            .unwrap()
            .get(name)
            .unwrap()
            .as_reference()
            .unwrap();
        f.pdf.get_dictionary(font).unwrap().clone()
    };
    let lim = &Limits::DEFAULT;
    let program = crate::content::fixture::truetype_program().len();
    let mut budget = usize::MAX;
    Foreign::read(&f.pdf, &font(b"PT"), lim, &mut budget).unwrap();
    let charged = usize::MAX - budget;
    assert!(
        charged > program,
        "{charged} charged, the program {program}"
    );
    let mut budget = charged;
    Foreign::read(&f.pdf, &font(b"PT"), lim, &mut budget).unwrap();
    assert_eq!(budget, 0);
    let mut budget = charged - 1;
    assert_eq!(
        Foreign::read(&f.pdf, &font(b"PT"), lim, &mut budget).err(),
        Some(TooLarge)
    );
    assert_eq!(budget, charged - 1);
    let mut budget = 0;
    let text = Fonts::for_text();
    let by_ref = f
        .resources()
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap();
    let pt = by_ref.as_dict().unwrap().get(b"PT").unwrap();
    assert!(text.get(&f.pdf, pt, lim, &mut budget).unwrap().is_some());

    let mut pdf = f.pdf.clone();
    let map = pdf.add_object(Stream::new(dictionary! {}, vec![0, 0, 0, 2, 0, 1]));
    let cid = font(b"PI");
    let descendant = cid.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]
        .as_reference()
        .unwrap();
    pdf.get_dictionary_mut(descendant)
        .unwrap()
        .set("CIDToGIDMap", map);
    let mut budget = program + 6;
    let mapped = Foreign::read(&pdf, &cid, lim, &mut budget).unwrap();
    assert_eq!(budget, 0);
    // CID 1 draws glyph 2, CID 2 glyph 1, and CID 3 is past the map.
    assert_eq!(mapped.char_box(1), Some(rect(20.0, -199.0, 480.0, 568.0)));
    assert_eq!(mapped.char_box(2), Some(rect(0.0, 0.0, 600.0, 761.0)));
    assert_eq!(mapped.char_box(3), Some(Rect::default()));
    let mut budget = program + 5;
    assert_eq!(
        Foreign::read(&pdf, &cid, lim, &mut budget).err(),
        Some(TooLarge)
    );
}
