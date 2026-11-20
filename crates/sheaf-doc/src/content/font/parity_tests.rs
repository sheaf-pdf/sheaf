//! The standard 14 as sheaf's reader of files' fonts still has them,
//! beside the PDF layer's, while both are in the tree: their metrics and
//! names. It goes with what it compares against. What text is drawn in a
//! font as was compared here too, for every character there is, until
//! sheaf drew in the PDF layer's fonts;
//! `every_character_encodes_as_it_always_has` (`tests.rs`) holds that
//! since.

use std::collections::BTreeSet;

use super::std14::{NAMES, Std14 as Old};
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
