//! sheaf's own fonts beside the PDF layer's, while both are in the tree:
//! what text is drawn in a font as ([`Font`] against [`DrawFont`]), for
//! every character there is, and the standard 14's metrics and names.
//! It goes with what it compares against;
//! `every_character_encodes_as_it_always_has` (`tests.rs`) holds what a
//! font encodes after that.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::std14::NAMES;
use super::{DrawFont, Font, Glyphless, Std14 as Old};
use crate::content::{Res, ResBody};
use crate::geom::Rect;
use crate::pdf::fonts::{GlyphBox, Std14 as New};

/// The 14, sheaf's beside the PDF layer's.
fn pairs() -> impl Iterator<Item = (Old, New)> {
    assert_eq!(Old::ALL.len(), New::ALL.len());
    Old::ALL.into_iter().zip(New::ALL).inspect(|(old, new)| {
        assert_eq!(old.name(), new.name());
    })
}

fn pair(old: Old) -> New {
    New::ALL[Old::ALL.iter().position(|&f| f == old).unwrap()]
}

/// A font by both traits, to draw `s` in each.
struct Both<'a> {
    name: &'a str,
    old: &'a dyn Font,
    new: &'a dyn DrawFont,
}

impl Both<'_> {
    /// Where the two part on `s`: its codes, what wasn't encoded, and its
    /// width's bits at each size.
    fn parts(&self, s: &str, sizes: &[f32]) -> Option<String> {
        let (old, new) = (self.old.encode(s), self.new.encode(s));
        if old != new {
            return Some(format!("{} {s:?}: codes {old:x?}, now {new:x?}", self.name));
        }
        for &size in sizes {
            let (old, new) = (self.old.width(s, size), self.new.width(s, size));
            if old.to_bits() != new.to_bits() {
                return Some(format!(
                    "{} {s:?} at {size}: width {old:?} ({:#x}), now {new:?} ({:#x})",
                    self.name,
                    old.to_bits(),
                    new.to_bits()
                ));
            }
        }
        None
    }

    /// Every character, alone: whether it's covered, and [`Both::parts`].
    fn every_char(&self) -> Vec<String> {
        let mut parted = Vec::new();
        let mut buf = [0u8; 4];
        for c in '\0'..=char::MAX {
            let (old, new) = (self.old.covers(c), self.new.covers(c));
            if old != new {
                parted.push(format!(
                    "{} U+{:04X}: covered {old}, now {new}",
                    self.name, c as u32
                ));
            }
            parted.extend(self.parts(c.encode_utf8(&mut buf), &[1000.0, 9.5]));
        }
        parted
    }
}

fn none_parted(parted: Vec<String>) {
    assert!(
        parted.is_empty(),
        "{} differences:\n{}",
        parted.len(),
        parted.join("\n")
    );
}

/// Each of the 14 covers, encodes and measures every character as it did.
#[test]
fn the_standard_14_draw_every_character_the_same() {
    let parted = pairs().flat_map(|(old, new)| {
        let both = Both {
            name: old.name(),
            old: &old,
            new: &new,
        };
        both.every_char()
    });
    none_parted(parted.collect());
}

const GLYPHLESS: Both<'static> = Both {
    name: "Glyphless",
    old: &Glyphless,
    new: &Glyphless,
};

/// Glyphless too: every character, text decomposed (which stays as
/// written) and characters outside the BMP (two CIDs, an em).
#[test]
fn glyphless_draws_every_character_the_same() {
    none_parted(GLYPHLESS.every_char());
    let sizes = [1.0, 8.0, 9.5, 1000.0];
    for s in [
        "",
        " ",
        "e\u{301}",
        "é",
        "𝄞",
        "a𝄞e\u{301}\u{fb01}",
        "Ω\u{2126}",
    ] {
        none_parted(GLYPHLESS.parts(s, &sizes).into_iter().collect());
    }
    assert_eq!(
        DrawFont::encode(&Glyphless, "e\u{301}").0,
        [0, 0x65, 0x03, 0x01]
    );
    assert_eq!(
        DrawFont::encode(&Glyphless, "𝄞").0,
        [0xd8, 0x34, 0xdd, 0x1e]
    );
    assert_eq!(DrawFont::width(&Glyphless, "𝄞", 1000.0), 1000.0);
}

/// Text of every kind the fonts tell apart: Latin, marks that compose
/// and marks that don't, Greek, the signs Symbol draws, dingbats,
/// controls, spaces and line ends, and characters outside the BMP.
fn text() -> impl Strategy<Value = String> {
    let c = prop_oneof![
        4 => proptest::char::range(' ', '~'),
        2 => proptest::char::range('\u{a0}', '\u{17f}'),
        2 => proptest::char::range('\u{300}', '\u{36f}'),
        2 => proptest::char::range('\u{370}', '\u{3ff}'),
        2 => proptest::char::range('\u{2000}', '\u{23ff}'),
        2 => proptest::char::range('\u{2700}', '\u{27bf}'),
        1 => proptest::char::range('\0', '\u{1f}'),
        2 => Just(' '),
        1 => Just('\n'),
        1 => proptest::char::range('\u{1d100}', '\u{1d1ff}'),
        1 => any::<char>(),
    ];
    proptest::collection::vec(c, 0..48).prop_map(String::from_iter)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Strings, in the 14 and Glyphless: the same codes, the same count
    /// of what wasn't encoded, widths the same to the bit at every size,
    /// and the same lines.
    #[test]
    fn strings_are_drawn_and_wrapped_the_same(
        s in text(),
        size in prop_oneof![Just(1.0f32), Just(9.5), Just(12.0), Just(1000.0), 0.5f32..72.0],
        width in 0.0f32..400.0,
    ) {
        let sizes = [size, 1.0, 12.0, 1000.0];
        let fonts: Vec<(Old, New)> = pairs().collect();
        let both = fonts.iter().map(|(old, new)| Both { name: old.name(), old, new });
        for both in both.chain([GLYPHLESS]) {
            prop_assert_eq!(both.parts(&s, &sizes), None);
            prop_assert_eq!(
                both.old.wrap(&s, size, width),
                both.new.wrap(&s, size, width),
                "{} wraps {:?} at {} in {}", both.name, &s, size, width
            );
        }
    }
}

/// A font is the same resource: the same id, the same objects.
#[test]
fn the_fonts_are_the_same_resources() {
    let same = |name: &str, old: &dyn Font, new: &dyn DrawFont| {
        let (old, new) = (old.resource(), new.res());
        assert_eq!(old.id, new.id(), "{name}");
        match new.body() {
            ResBody::Objects(objects) => assert_eq!(**objects, *old.objects, "{name}"),
            other => panic!("{name}: {other:?}"),
        }
        assert_eq!(Res::from(old), new, "{name}");
    };
    for (old, new) in pairs() {
        same(old.name(), &old, &new);
    }
    same("Glyphless", &Glyphless, &Glyphless);
}

fn rect(b: GlyphBox) -> Rect {
    Rect::new(b.x0, b.y0, b.x1, b.y1)
}

/// The metrics are the same: each font's heights and box, Adobe's AFM
/// itself (every line but its comments), and every glyph any of the 14 has, in each of them: its
/// width, its box, and whether PDFium's faces draw it otherwise. Names
/// no AFM has, too.
#[test]
fn the_metrics_are_the_same() {
    let mut glyphs: BTreeSet<&str> = [".notdef", "", "nosuchglyph", "A.alt", "afii10017"].into();
    for (old, new) in pairs() {
        // Adobe's AFM, but for the comments each copy says its changes in.
        let data = |afm: &'static str| afm.lines().filter(|l| !l.starts_with("Comment "));
        assert!(data(old.afm_text()).eq(data(new.afm_text())), "{old:?}");
        assert!(data(old.afm_text()).count() > 200, "{old:?}");
        for line in old.afm_text().lines().filter(|l| l.starts_with("C ")) {
            let name = line.split(';').find_map(|f| f.trim().strip_prefix("N "));
            glyphs.insert(name.expect("a glyph's name").trim());
        }
        let (was, now) = (old.metrics(), new.metrics());
        assert_eq!(was.ascent.to_bits(), now.ascent.to_bits(), "{old:?}");
        assert_eq!(was.descent.to_bits(), now.descent.to_bits(), "{old:?}");
        assert_eq!(was.bbox, now.bbox.map(rect), "{old:?}");
    }
    assert!(glyphs.len() > 500, "{} glyphs", glyphs.len());
    for (old, new) in pairs() {
        for &glyph in &glyphs {
            assert_eq!(
                old.glyph_width(glyph).map(f32::to_bits),
                new.glyph_width(glyph).map(f32::to_bits),
                "{old:?} {glyph}"
            );
            assert_eq!(
                old.glyph_box(glyph),
                new.glyph_box(glyph).map(rect),
                "{old:?} {glyph}"
            );
            assert_eq!(
                old.drawn_otherwise(glyph),
                new.drawn_otherwise(glyph),
                "{old:?} {glyph}"
            );
        }
    }
}

/// A `/BaseFont` names the same font: each name PDFium takes for one of
/// the 14, as written, after a subset's tag and in other cases, and
/// names that are none.
#[test]
fn the_names_are_the_same() {
    let same = |name: &[u8]| {
        assert_eq!(
            Old::named(name).map(pair),
            New::named(name),
            "{}",
            name.escape_ascii()
        );
    };
    for (name, font) in NAMES {
        assert_eq!(New::named(name.as_bytes()), Some(pair(font)), "{name}");
        for name in [
            name.to_string(),
            format!("ABCDEF+{name}"),
            format!("ABCDE+{name}"),
            format!("ABCDEFG+{name}"),
            format!("ABCDEF+ABCDEF+{name}"),
            name.to_ascii_lowercase(),
            name.to_ascii_uppercase(),
            format!("{name} "),
            format!("{name},"),
            format!("{name}-"),
            name.replace(',', "-"),
            name.replace('-', ","),
            name.replace("MT", ""),
        ] {
            same(name.as_bytes());
        }
    }
    for none in [
        &b""[..],
        b"+",
        b"ABCDEF+",
        b"Arial\xff",
        b"Helvetica-Narrow",
        b"Symbol,Bold",
    ] {
        same(none);
    }
}
