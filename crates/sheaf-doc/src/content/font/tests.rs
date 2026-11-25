//! Fonts sheaf draws in: widths that are Adobe's, encoders that are
//! exact, `?` for what a font can't draw, wrapping, and fonts as
//! resources; and hostile Type 1 programs read through a source's
//! objects. What a file's font reads as, kind by kind, is held by the
//! PDF layer's own tests.

use std::collections::HashMap;
use std::sync::Arc;

use crate::pdf::{Dictionary, Document as Pdf, Object, Stream, StreamExt, Value, dictionary};

use super::*;
use crate::content::{Objects, Unkept};
use crate::content::{ResBody, ResId};
use crate::pdf::fonts::{Encoding, GlyphBox};
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

/// The encoding sheaf writes a standard 14 font in.
fn written_in(font: Std14) -> Encoding {
    match font {
        Std14::Symbol => Encoding::Symbol,
        Std14::ZapfDingbats => Encoding::ZapfDingbats,
        _ => Encoding::WinAnsi,
    }
}

/// Text advances by Adobe's widths: for every code of the encoding a
/// font is written in, the character written as that code advances by
/// its glyph's width in the font's AFM.
#[test]
fn text_advances_by_the_afm_s_widths() {
    for font in Std14::ALL {
        let (widths, _) = afm(font.afm_text());
        let encoding = written_in(font);
        let mut mapped = 0;
        let mut written = 0;
        for code in 0..=255u8 {
            let Some(glyph) = encoding.glyph(code) else {
                continue;
            };
            let width = widths.get(glyph).copied();
            assert!(width.is_some(), "{font:?} {code:#x} {glyph}");
            mapped += 1;
            // The code's character, where it has one and it's written as
            // this code (the lower one, where two draw it).
            let text = encoding.text(code).unwrap_or_default();
            if font.encode_alone(&text) == (vec![code], Unencoded(0)) {
                let advance = font.width_alone(&text, 1000.0);
                assert_eq!(Some(advance), width, "{font:?} {code:#x} {glyph}");
                written += 1;
            }
        }
        // But for the codes whose character a lower code draws: in the
        // Latin fonts the bullet's six more (0x7F and Windows-1252's
        // five empty ones), written as 0x95.
        let twice = match font {
            Std14::Symbol | Std14::ZapfDingbats => 8,
            _ => 8 + 6,
        };
        assert!(written + twice >= mapped, "{font:?}: {written} of {mapped}");
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

/// The twelve Latin fonts write Windows-1252 exactly, all 256 codes: each
/// character it draws encodes to its code; a control code draws nothing,
/// so its character is `?`.
#[test]
fn the_latin_fonts_write_windows_1252() {
    for code in 0..=255u8 {
        let Some(c) = cp1252(code) else { continue };
        let drawn = !c.is_control();
        for font in Std14::ALL
            .into_iter()
            .filter(|&f| written_in(f) == Encoding::WinAnsi)
        {
            let encoded = font.encode_alone(&c.to_string());
            match drawn {
                true => assert_eq!(encoded, (vec![code], Unencoded(0)), "{font:?} {c:?}"),
                false => assert_eq!(encoded, (b"?".to_vec(), Unencoded(1)), "{font:?} {c:?}"),
            }
            assert_eq!(font.covers(c), drawn);
        }
    }
    // The ten cp1252 characters outside Latin-1, which the old header
    // encoder drew as `?`.
    let (codes, none) = Std14::Helvetica.encode_alone("Š š Ž ž Œ œ Ÿ ƒ ˆ ˜");
    assert_eq!(codes, b"\x8a \x9a \x8e \x9e \x8c \x9c \x9f \x83 \x88 \x98");
    assert_eq!(none, Unencoded(0));
    // The second space and the soft hyphen are their own.
    assert_eq!(
        Std14::Helvetica.encode_alone("\u{a0}\u{ad} -").0,
        b"\xa0\xad -"
    );
}

/// A font alone draws what it can't encode as `?`, counted: what's drawn
/// of a cluster neither it nor the fallback has ([`DrawFont::encode`]).
#[test]
fn what_a_font_cant_encode_draws_a_question_mark_and_is_counted() {
    assert_eq!(
        Std14::Helvetica.encode_alone("a中文b€"),
        (b"a??b\x80".to_vec(), Unencoded(2))
    );
    assert_eq!(Std14::Helvetica.width_alone("中", 10.0), 5.56);
    // Composed first: a decomposed é is one character.
    assert_eq!(
        Std14::TimesRoman.encode_alone("e\u{301}"),
        (vec![0xe9], Unencoded(0))
    );
    assert!(!Std14::Helvetica.covers('α'));
    // Symbol has a question mark; ZapfDingbats hasn't, so it leaves the
    // character out.
    assert_eq!(
        Std14::Symbol.encode_alone("αx"),
        (b"a?".to_vec(), Unencoded(1))
    );
    assert_eq!(
        Std14::ZapfDingbats.encode_alone("✔x✘"),
        (b"48".to_vec(), Unencoded(1))
    );
}

#[test]
fn symbol_and_zapf_dingbats_encode_their_own() {
    let symbol = Std14::Symbol;
    assert_eq!(symbol.encode_alone("αβ∑≠ ").0, b"ab\xe5\xb9 ");
    assert!(symbol.covers('π') && symbol.covers('€') && !symbol.covers('A'));
    // Omega, Delta and mu, as the Greek letters and as the AGL reads
    // Symbol's glyphs: the ohm sign, the increment and the micro sign.
    let greek = (
        symbol.encode_alone("\u{3a9}\u{394}\u{3bc}"),
        symbol.encode_alone("\u{2126}\u{2206}\u{b5}"),
    );
    assert_eq!(greek.0, (b"WDm".to_vec(), Unencoded(0)));
    assert_eq!(greek.1, greek.0);
    assert!(symbol.covers('\u{2126}') && symbol.covers('\u{3a9}'));
    // The checks and crosses: ✓ is a19, ✔ a20, ✗ a23, ✘ a24.
    let dingbats = Std14::ZapfDingbats;
    assert_eq!(dingbats.encode_alone("✓✔✗✘").0, b"3478");
    assert_eq!(dingbats.encode_alone("☎ ").0, b"\x25 ");
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
    let (codes, none) = Glyphless.encode_alone("Hé𝄞");
    assert_eq!(codes, [0x00, 0x48, 0x00, 0xe9, 0xd8, 0x34, 0xdd, 0x1e]);
    assert_eq!(none, Unencoded(0));
    assert_eq!(Glyphless.width("Hé𝄞", 10.0), 20.0);
    // Decomposed text stays as it was recognized.
    assert_eq!(Glyphless.encode_alone("e\u{301}").0, [0, 0x65, 0x03, 0x01]);
    assert!(Glyphless.covers('中'));
    // Any font goes where a `dyn DrawFont` does.
    let fonts: [&dyn DrawFont; 2] = [&Std14::Courier, &Glyphless];
    assert_eq!(fonts.map(|f| f.width("ab", 10.0)), [12.0, 10.0]);
}

/// A font as a resource is its dictionary and what that reaches, written
/// into a document with numbers of its own.
#[test]
fn fonts_as_resources() {
    let helvetica = Std14::Helvetica.res();
    let objects = |res: &Res| match res.body() {
        ResBody::Objects(objects) => objects.clone(),
        other => panic!("{other:?}"),
    };
    let [obj] = &objects(&helvetica)[..] else {
        panic!("one dictionary")
    };
    let Value::Dict(dict) = obj.value() else {
        panic!("one dictionary")
    };
    assert_eq!(
        dict,
        &dictionary! {
            "Type" => Object::name("Font"),
            "Subtype" => Object::name("Type1"),
            "BaseFont" => Object::name("Helvetica"),
            "Encoding" => Object::name("WinAnsiEncoding"),
        }
    );
    let symbol = objects(&Std14::Symbol.res());
    assert!(!symbol[0].as_dict().unwrap().has(b"Encoding"));
    // Named by what they are.
    assert_eq!(helvetica.id(), Std14::Helvetica.res().id());
    let ids: std::collections::HashSet<_> = Std14::ALL.map(|f| f.res().id()).into();
    assert_eq!(ids.len(), 14);
    let mut reordered = Dictionary::new();
    for (key, value) in dict.iter().rev() {
        reordered.set(key, value.clone());
    }
    assert_eq!(ResId::of(&[reordered.into()]), helvetica.id());
    assert_ne!(Glyphless.res().id(), helvetica.id());

    let mut pdf = Pdf::with_version("1.7");
    pdf.add_object(dictionary! {});
    let mut res = ResObjects::default();
    let font = res.object(&mut pdf, &Glyphless.res());
    res.finish(&mut pdf);
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

/// A Type 1 font over `program`, read through a source's objects with
/// its program, within a page's budget.
fn type1_over(program: Vec<u8>) -> Font {
    let mut pdf = Pdf::with_version("1.7");
    let program = pdf.add_object(Stream::new(dictionary! {}, program));
    let descriptor = pdf.add_object(dictionary! {
        "Type" => Object::name("FontDescriptor"),
        "Flags" => 32,
        "Ascent" => 700,
        "Descent" => -200,
        "FontFile" => program,
    });
    let font = dictionary! {
        "Type" => Object::name("Font"),
        "Subtype" => Object::name("Type1"),
        "BaseFont" => Object::name("Fix"),
        "FirstChar" => 65,
        "LastChar" => 65,
        "Widths" => vec![600.into()],
        "FontDescriptor" => descriptor,
    };
    let mut budget = Limits::DEFAULT.content.decompressed;
    let lim = Limits::DEFAULT.fonts();
    Font::read(
        Objects::new(&pdf, &Unkept::default()),
        &font,
        &lim,
        &mut budget,
        &mut (),
    )
    .expect("within the budget")
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

/// `A`'s outline in [`fixture::type1_program`](crate::content::fixture::type1_program).
const TYPE1_A: GlyphBox = GlyphBox {
    x0: 0.0,
    y0: 0.0,
    x1: 600.0,
    y1: 750.0,
};

/// A Type 1 program whose procedures nest, however deep, is read through
/// a source's objects by the PDF layer's reader, its glyphs' outlines
/// the program's: a procedure in its clear text is skipped, not run.
#[test]
fn a_type1_program_s_nested_procedures_are_read_past() {
    for levels in [0, 16, 17, 1000] {
        let font = type1_over(type1_nested(levels));
        assert_eq!(font.program(), FontProgram::Read, "{levels} deep");
        assert_eq!(font.char_box(b'A'.into()), Some(TYPE1_A), "{levels} deep");
        assert!(!font.approx().boxes, "{levels} deep");
    }
}

/// A Type 1 program whose integers run, however long, is read through a
/// source's objects, within the reader's work.
#[test]
fn a_type1_program_s_run_of_integers_is_read_past() {
    use crate::content::fixture::type1_integers_program as type1_integers;
    for run in [0, 256, 257, 100_000] {
        let font = type1_over(type1_integers(run));
        assert_eq!(font.program(), FontProgram::Read, "{run} integers");
        assert_eq!(font.char_box(b'A'.into()), Some(TYPE1_A), "{run} integers");
        assert!(!font.approx().boxes, "{run} integers");
    }
}

/// Text drawn in a font through the writer ([`DrawText`]): the font is
/// named as its resource, the string written as that font's codes,
/// literal and escaped, or in hex where a code is a control (Glyphless's
/// two-byte codes), and a character a font can't encode is a `?`,
/// counted. Text names the font it's in at each call, so two fonts can
/// take turns under one `Tf` each.
#[test]
fn text_is_written_in_its_font() {
    use crate::content::{ContentWriter, DrawText, ResKind, Reserved};

    let mut w = ContentWriter::new();
    w.set_font(&Std14::Helvetica, 9.0);
    assert_eq!(
        w.show_text(&Std14::Helvetica, 9.0, "(a\\b) é").1,
        Unencoded(0)
    );
    assert_eq!(w.show_text(&Std14::Helvetica, 9.0, "中文").1, Unencoded(2));
    w.save();
    w.set_font(&Glyphless, 10.0);
    assert_eq!(w.show_text(&Glyphless, 10.0, "Hi").1, Unencoded(0));
    w.restore();
    w.show_text(&Std14::Helvetica, 9.0, "é");
    // A font set again is the one need, under the one name.
    w.set_font(&Std14::Helvetica, 12.0);
    let (bytes, needs) = w.finish(&Reserved::default());
    let [helvetica, glyphless] = &needs.0[..] else {
        panic!("{needs:?}")
    };
    let name = |need: &crate::content::Need| String::from_utf8_lossy(&need.name).into_owned();
    let mut want = format!("/{} 9 Tf\n", name(helvetica)).into_bytes();
    want.extend_from_slice(b"(\\(a\\\\b\\) \xe9) Tj\n(??) Tj\nq\n");
    want.extend(format!("/{} 10 Tf\n", name(glyphless)).bytes());
    want.extend_from_slice(b"<00480069> Tj\nQ\n(\xe9) Tj\n");
    want.extend(format!("/{} 12 Tf\n", name(helvetica)).bytes());
    assert_eq!(bytes, want, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(helvetica.kind, ResKind::Font);
    assert_eq!(helvetica.res, Std14::Helvetica.res());
    assert_eq!(
        helvetica.res.body(),
        &ResBody::Objects(Arc::new([Std14::Helvetica.dict().into()]))
    );
    assert_eq!(glyphless.kind, ResKind::Font);
    assert_eq!(glyphless.res, Glyphless.res());
}

/// What [`every_character_encodes_as_it_always_has`] holds a font to:
/// the xxh3-128, over every character in order, of whether it's covered,
/// its codes, how many characters weren't encoded and the bits of its
/// width at a size of 1000.
fn drawn_digest(
    covers: impl Fn(char) -> bool,
    encode: impl Fn(&str) -> (Vec<u8>, Unencoded),
    width: impl Fn(&str, f32) -> f32,
) -> u128 {
    let mut digest = sheaf_util::Checksum::new();
    let mut buf = [0u8; 4];
    for c in '\0'..=char::MAX {
        let s = &*c.encode_utf8(&mut buf);
        let (codes, unencoded) = encode(s);
        digest.write(&[u8::from(covers(c)), codes.len() as u8]);
        digest.write(&codes);
        digest.write(&(unencoded.0 as u32).to_le_bytes());
        digest.write(&width(s, 1000.0).to_bits().to_le_bytes());
    }
    digest.finish()
}

/// Each standard 14 font's digest, in [`Std14::ALL`]'s order, and
/// Glyphless's: computed from the fonts sheaf drew in before it drew in
/// the PDF layer's (M18 slice 22), and constants since. A digest that
/// moves is a byte some mark, header, stamp or text layer is written
/// with that moved, or a width a line was broken or placed by.
const DRAWN: [u128; 15] = [
    0xd4aed3fc76f711b95bfc874d1e8fd29b, // Courier
    0xd4aed3fc76f711b95bfc874d1e8fd29b, // Courier-Bold
    0xd4aed3fc76f711b95bfc874d1e8fd29b, // Courier-Oblique
    0xd4aed3fc76f711b95bfc874d1e8fd29b, // Courier-BoldOblique
    0xadc31ca352224f2bb11fee3e8feac903, // Helvetica
    0xe124332fa457838cc59362e61ec7483a, // Helvetica-Bold
    0xadc31ca352224f2bb11fee3e8feac903, // Helvetica-Oblique
    0xe124332fa457838cc59362e61ec7483a, // Helvetica-BoldOblique
    0x50abd98349831cce31061e543291f2de, // Times-Roman
    0x65e96874b8297d5e69088cd8471134f4, // Times-Bold
    0xdb0f491ad92642e6311ac032ee834d9c, // Times-Italic
    0x3fcdca091b9621c3e0b57009846a744a, // Times-BoldItalic
    0xd60b6896ae4494e13cda342ddaf70667, // Symbol
    0xbcccbb42c27bd834d2b199efb1d83b7f, // ZapfDingbats
    0xd35d404c39425fa1eb37a225ec49c33d, // Glyphless
];

/// Every font sheaf draws in covers, encodes and measures every character
/// there is as it always has: the goldens of what sheaf writes draw
/// nothing in Symbol or ZapfDingbats, and little of what a font has.
#[test]
fn every_character_encodes_as_it_always_has() {
    let digests = (0..15).map(|i| {
        let font: &dyn DrawFont = match Std14::ALL.get(i) {
            Some(font) => font,
            None => &Glyphless,
        };
        // Each font alone: what it draws of what it has, and of what
        // neither it nor the fallback has.
        drawn_digest(
            |c| font.covers(c),
            |s| font.encode_alone(s),
            |s, z| font.width_alone(s, z),
        )
    });
    // Every font is said before any fails, each as it is now.
    let mut moved = Vec::new();
    for (i, drawn) in digests.enumerate() {
        let name = Std14::ALL.get(i).map_or("Glyphless", |font| font.name());
        if drawn != DRAWN[i] {
            moved.push(format!("{name}: {drawn:#034x}"));
        }
    }
    assert!(moved.is_empty(), "drawn otherwise:\n{}", moved.join("\n"));
}

// ----- the fallback font ---------------------------------------------------

/// The fallback's program is the Noto project's Noto Sans Regular, opened
/// once: its glyphs, a licence that lets it be embedded and cut, and how
/// many characters its `cmap` maps.
#[test]
fn the_fallback_program_opens() {
    let program = Fallback::SANS.program();
    assert!(
        std::ptr::eq(program, Fallback::SANS.program()),
        "opened once"
    );
    assert_eq!(program.glyph_count(), 3884);
    let embedding = program.embedding();
    assert!(embedding.allows(&crate::pdf::fonts::EmbedOptions::default()));
    assert!(embedding.subsetting && !embedding.bitmaps_only);
    let mapped = ('\0'..=char::MAX).filter(|&c| program.glyph(c).is_some());
    assert_eq!(mapped.count(), 2965);
}

/// Content names the fallback by a marker, the same resource every time
/// and so the same name, which no resource of its objects is taken for.
#[test]
fn the_fallback_is_named_by_a_marker() {
    let sans = Fallback::SANS;
    let marker = sans.res();
    assert_eq!(marker.id(), sans.res().id());
    assert!(sans.is(&marker));
    assert!(!sans.is(&Std14::Helvetica.res()) && !sans.is(&Glyphless.res()));
    let ResBody::Objects(objects) = marker.body() else {
        panic!("a marker is its dictionary")
    };
    assert!(!sans.is(&crate::content::Res::of(objects.clone()).unwrap()));
    let named = || {
        let mut w = crate::content::ContentWriter::new();
        w.font(sans.res(), 12.0);
        w.finish(&crate::content::Reserved::default())
    };
    assert_eq!(named(), named());
}

/// A subset is named by what it holds: the same glyphs, joined in any
/// order, give the same resource; another glyph, another.
#[test]
fn equal_glyphs_give_an_equal_subset() {
    let sans = Fallback::SANS;
    let program = sans.program();
    let (mut one, mut other) = (Glyphs::new(), Glyphs::new());
    one.add(program, "\u{141}ukasz").unwrap();
    one.add(program, "\u{D6}l\u{E7}er").unwrap();
    other.add(program, "\u{D6}l\u{E7}er").unwrap();
    let mut apart = Glyphs::new();
    apart.add(program, "zsaku\u{141}").unwrap();
    other.merge(&apart);
    assert_eq!(one, other);
    assert_eq!(sans.subset(&one).id(), sans.subset(&other).id());
    let mut more = one.clone();
    more.add(program, "\u{1EC5}").unwrap();
    assert_ne!(sans.subset(&more).id(), sans.subset(&one).id());
}

/// A subset draws all of what it holds, each glyph with the text it keeps
/// for it: the same glyphs, fewer, or none; not one more; and not a glyph
/// it holds drawn for a text that would take the place of its own (a
/// smaller, which [`Glyphs`] keeps), as its `/ToUnicode` would read it
/// otherwise, while one drawn for a later text reads back as it does.
#[test]
fn a_subset_draws_all_of_what_it_holds() {
    let program = Fallback::SANS.program();
    let glyphs = |text: &str| {
        let mut glyphs = Glyphs::new();
        glyphs.add(program, text).unwrap();
        glyphs
    };
    let held = glyphs("\u{141}\u{143}");
    for drawn in [glyphs("\u{141}"), held.clone(), Glyphs::new()] {
        assert!(draws_all(&held, &drawn), "{drawn:?}");
    }
    assert!(!draws_all(&held, &glyphs("\u{141}\u{1EC5}")));
    assert!(!draws_all(&glyphs("\u{141}"), &held));
    let l = program.glyph('\u{141}').unwrap();
    let (mut smaller, mut later) = (Glyphs::new(), Glyphs::new());
    smaller.insert(l, "A");
    later.insert(l, "\u{179}");
    assert!(!draws_all(&held, &smaller));
    assert!(draws_all(&held, &later));
}

/// Text a font can't draw all of is drawn in two (M10 slice 7): composed,
/// a grapheme cluster at a time, each in the first of the font and the
/// fallback that has all of it, the fallback's glyphs said with their
/// text; a cluster neither has is `?` in the font, counted; a control
/// character stays the font's, though the fallback maps two (a carriage
/// return, a NUL); and text the font draws whole is its own codes, one
/// run, no glyph.
#[test]
fn text_the_font_lacks_is_drawn_in_the_fallback() {
    use crate::content::fixture::FALLBACK_TEXT;

    let program = Fallback::SANS.program();
    let glyph = |c: char| program.glyph(c).unwrap();
    let code = |c: char| glyph(c).to_be_bytes().to_vec();
    let helvetica = Std14::Helvetica;
    let e = helvetica.encode(FALLBACK_TEXT);
    assert_eq!(
        e.runs,
        [
            (InFont::Fallback, code('\u{141}')),
            (InFont::Own, b"ukasz Nguy".to_vec()),
            (InFont::Fallback, code('\u{1ec5}')),
            (InFont::Own, b"n \xd6l\xe7er".to_vec()),
        ]
    );
    assert_eq!(e.unencoded, Unencoded(0));
    let drawn: Vec<(u16, &str)> = e.glyphs.iter().collect();
    let mut want = vec![
        (glyph('\u{141}'), "\u{141}"),
        (glyph('\u{1ec5}'), "\u{1ec5}"),
    ];
    want.sort();
    assert_eq!(drawn, want);
    // Decomposed, the same: composed first.
    let decomposed: String = FALLBACK_TEXT.nfd().collect();
    assert_eq!(helvetica.encode(&decomposed), e);
    // As wide as the two draw it: Helvetica's widths, the program's.
    let units: f32 = (FALLBACK_TEXT.chars())
        .map(|c| (helvetica.char_advance(c)).unwrap_or_else(|| program.width(glyph(c))))
        .sum();
    assert_eq!(helvetica.width(FALLBACK_TEXT, 10.0), units * 10.0 / 1000.0);
    assert_eq!(helvetica.width(&decomposed, 10.0), units * 10.0 / 1000.0);
    assert!(!helvetica.covers('\u{141}'), "covers is the font alone");

    // What neither has is `?` in the font, counted, beside what one has.
    let e = helvetica.encode("\u{141}\u{6f22}");
    assert_eq!(
        e.runs,
        [
            (InFont::Fallback, code('\u{141}')),
            (InFont::Own, b"?".to_vec())
        ]
    );
    assert_eq!((e.glyphs.len(), e.unencoded), (1, Unencoded(1)));
    assert_eq!(
        helvetica.width("\u{6f22}", 10.0),
        helvetica.width_alone("\u{6f22}", 10.0)
    );
    // A control character is the font's, `?`, as it always was.
    for control in ["a\rb", "a\0b"] {
        let e = helvetica.encode(control);
        assert_eq!(e.runs, [(InFont::Own, b"a?b".to_vec())], "{control:?}");
        assert_eq!((e.glyphs.len(), e.unencoded), (0, Unencoded(1)));
        assert_eq!(
            helvetica.width(control, 10.0),
            helvetica.width_alone(control, 10.0)
        );
    }
    // Symbol and ZapfDingbats fall back too.
    assert_eq!(
        Std14::Symbol.encode("\u{3b1}x").runs,
        [(InFont::Own, b"a".to_vec()), (InFont::Fallback, code('x'))]
    );
    assert_eq!(
        Std14::ZapfDingbats.encode("✔x✘").runs,
        [
            (InFont::Own, b"4".to_vec()),
            (InFont::Fallback, code('x')),
            (InFont::Own, b"8".to_vec())
        ]
    );

    // Text a font draws whole is its own codes, as they always were.
    for whole in ["plain \u{e9} \u{20ac}", "", "e\u{301}"] {
        let (codes, unencoded) = helvetica.encode_alone(whole);
        let e = helvetica.encode(whole);
        assert_eq!(e.runs, [(InFont::Own, codes)], "{whole:?}");
        assert_eq!((e.glyphs.len(), e.unencoded), (0, unencoded));
        assert_eq!(
            helvetica.width(whole, 10.0),
            helvetica.width_alone(whole, 10.0)
        );
    }
    // Glyphless never falls back: it draws every character.
    let e = Glyphless.encode("\u{141}ukasz \u{6f22}");
    assert_eq!(
        e.runs,
        [(
            InFont::Own,
            Glyphless.encode_alone("\u{141}ukasz \u{6f22}").0
        )]
    );
    assert!(e.glyphs.is_empty());
}

/// Lines are broken where the text, in both fonts, is as wide as it's
/// drawn: a word of `Ł`s measures the fallback's widths, not `?`'s.
#[test]
fn wrap_measures_text_in_both_fonts() {
    let program = Fallback::SANS.program();
    let l = program.width(program.glyph('\u{141}').unwrap());
    assert_ne!(l, 600.0, "not Courier's");
    let courier = Std14::Courier;
    let three = "\u{141}\u{141}\u{141}";
    assert_eq!(courier.width(three, 10.0), 3.0 * l * 10.0 / 1000.0);
    let line = courier.width(three, 10.0);
    assert_eq!(
        courier.wrap(&format!("{three} {three}"), 10.0, line),
        [three, three]
    );
}

/// Text in two fonts is written in runs ([`DrawText::show_text`]): the
/// fallback's set at the size under its marker's name, then the font set
/// again, so what follows is the font's; what it drew in the fallback and
/// what neither font draws come back. Text the font draws whole is the
/// bytes it always was.
#[test]
fn text_in_two_fonts_is_written_in_runs() {
    use crate::content::{ContentWriter, DrawText, Reserved};

    let helvetica = Std14::Helvetica;
    let sans = Fallback::SANS;
    let program = sans.program();
    let mut w = ContentWriter::new();
    w.set_font(&helvetica, 9.0);
    let (glyphs, unencoded) = w.show_text(&helvetica, 9.0, "\u{141}u\u{6f22}");
    assert_eq!((glyphs.len(), unencoded), (1, Unencoded(1)));
    let (_, none) = w.show_text(&helvetica, 9.0, "plain");
    assert_eq!(none, Unencoded(0));
    let written = w.finish(&Reserved::default());

    let mut by_hand = ContentWriter::new();
    by_hand.font(helvetica.res(), 9.0);
    by_hand.font(sans.res(), 9.0);
    by_hand.show(&program.glyph('\u{141}').unwrap().to_be_bytes());
    by_hand.font(helvetica.res(), 9.0);
    by_hand.show(b"u?");
    by_hand.show(b"plain");
    assert_eq!(written, by_hand.finish(&Reserved::default()));
    let (_, needs) = written;
    let named: Vec<bool> = needs.0.iter().map(|n| sans.is(&n.res)).collect();
    assert_eq!(
        named,
        [false, true],
        "Helvetica, then the fallback's marker"
    );
}

/// Wherever a standard 14 font draws a character itself, the two-font
/// drawing is the font alone's, its codes and its width to the bit; and
/// wherever it draws one in the fallback, the font alone couldn't: over
/// every character of the Basic Multilingual Plane, in each of the 14.
#[test]
fn what_a_font_draws_itself_is_drawn_as_it_always_was() {
    let mut buf = [0u8; 4];
    for font in Std14::ALL {
        let mut fallen = 0;
        // A char range skips the surrogates.
        for c in '\0'..='\u{ffff}' {
            let s = &*c.encode_utf8(&mut buf);
            let drawn = font.encode(s);
            let alone = font.encode_alone(s);
            let width = font.width(s, 1000.0).to_bits();
            match &drawn.runs[..] {
                [(InFont::Own, codes)] => {
                    assert_eq!(
                        (codes, drawn.unencoded),
                        (&alone.0, alone.1),
                        "{font:?} {c:?}"
                    );
                    assert_eq!(
                        width,
                        font.width_alone(s, 1000.0).to_bits(),
                        "{font:?} {c:?}"
                    );
                    assert!(drawn.glyphs.is_empty());
                }
                _ => {
                    assert_ne!(alone.1, Unencoded(0), "{font:?} {c:?}");
                    assert_eq!(drawn.unencoded, Unencoded(0), "{font:?} {c:?}");
                    fallen += 1;
                }
            }
        }
        assert!(fallen > 2000, "{font:?}: {fallen} in the fallback");
    }
}
