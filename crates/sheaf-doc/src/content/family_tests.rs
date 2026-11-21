//! sheaf's lexer's tests, and those of its balance, marked ranges, names
//! and color spaces' counts, each copied onto pdf-content-streams' calls of
//! the same name, as sheaf would make them (its limits by
//! [`ContentLimits::family`], its sources read through [`Objects`]): where
//! the family reads content as sheaf's lexer does, a copy expects what the
//! lexer's test did; where it doesn't, PDFium (chromium/8152) reads it as
//! the family does, and the copy expects the family's reading, saying which
//! difference it is and where PDFium's answer is: a case the family holds
//! to PDFium (pdf-content-streams' `tests/pdfium`, or pdf-color's, by
//! name), PDFium having been asked of the copy's own input too. sheaf's
//! sources are read for the family by [`res_cx`] and [`properties`].
//!
//! The differences, as the copies name them (sheaf's lexer, then the
//! family, which is PDFium's):
//! - L1: white-space is six bytes; those, 0x80 and 0xFF.
//! - L2: an array or a dictionary is one token to its closer; ended where
//!   PDFium's `ReadNextObject` ends it (an array in an array opens none, a
//!   dictionary ends at a key that's no name or a value that's no object).
//! - L3: arrays and dictionaries nest to the depth cap; past 64 levels,
//!   an error (PDFium reads what's nested deeper as no object and reads
//!   on: the family's case `deep-65`, which it reads otherwise on
//!   purpose).
//! - L4: a name's `#` escapes only two hex digits; any two bytes, a
//!   non-digit 0, as PDFium's `PDF_NameDecode`.
//! - L5: a raw CR or CR LF in a literal string is LF; kept as written.
//!   And a string read as an object is whole; its first 32,767 bytes.
//! - L6: a number is ISO's; PDFium's (`--5` is 0, `1.2.3` is 1.2).
//! - L7: a dictionary passes over a key that's no name and keeps a key's
//!   last value; it's none, and keeps both.
//! - L8: an inline image's data ends by `/L`, and filters' ends by
//!   markers; no `/L`, filters decoded to where PDFium's decoders stop.
//! - L9: an inline image's dictionary passes over what's no key; ends
//!   there, or makes `BI` an operator of its own.
//! - L10: a color space's components are counted by sheaf; as PDFium
//!   counts them (pdf-color).

use std::borrow::Cow;
use std::ops::Range;

use proptest::prelude::*;

use super::page::{properties, res_cx};
use crate::pdf::content::{
    Concat, Pos, Properties, Props, ResCx, ResKind, SpanAcross, SyntaxError, SyntaxErrorKind, Tok,
    balance, marked, name, names_used, object, tokens,
};
use crate::pdf::{Document as Pdf, Object, Stream, Value, dictionary, literal};
use crate::source::{ContentLimits, Limits};

const LIM: &Limits = &Limits::DEFAULT;

fn toks_of<'a>(s: &'a [u8], res: &ResCx, lim: &Limits) -> Vec<(Range<usize>, Tok<'a>)> {
    tokens(s, res, &lim.content.family())
        .map(Result::unwrap)
        .collect()
}

/// `s`'s tokens under the default caps, with no resources.
fn toks(s: &[u8]) -> Vec<Tok<'_>> {
    toks_of(s, &ResCx::none(), LIM)
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

fn error(s: &[u8], lim: &Limits) -> SyntaxError {
    tokens(s, &ResCx::none(), &lim.content.family())
        .find_map(Result::err)
        .expect("an error")
}

/// Whether `gap` is only white-space and comments, as the family reads
/// them: ISO 32000-2's six bytes of white-space, and 0x80 and 0xFF, as
/// PDFium reads them (L1); a comment to the end of its line.
fn blank(gap: &[u8]) -> bool {
    let mut comment = false;
    gap.iter().all(|&c| {
        comment = match c {
            b'%' => true,
            b'\r' | b'\n' => false,
            _ => comment,
        };
        comment
            || matches!(
                c,
                b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ' | b'\x80' | b'\xff'
            )
    })
}

/// Read until the end or an error, `s`'s tokens are in order, each
/// within it, and only white-space and comments are between them; to the
/// end, after the last one too. The tokens stop after an error.
fn assert_tiles(s: &[u8], res: &ResCx, lim: &Limits) {
    let mut at = 0;
    let mut read = tokens(s, res, &lim.content.family());
    for token in read.by_ref() {
        let Ok((r, _)) = token else {
            assert!(read.next().is_none(), "nothing after an error");
            return;
        };
        assert!(
            at <= r.start && r.start < r.end && r.end <= s.len(),
            "{r:?} after {at}"
        );
        assert!(
            blank(&s[at..r.start]),
            "{:?}",
            String::from_utf8_lossy(&s[at..r.start])
        );
        at = r.end;
    }
    assert!(blank(&s[at..]), "{:?}", String::from_utf8_lossy(&s[at..]));
}

const PAGE: &[u8] = b"%!comment (not a string\n\
    q 0.5 0 0 -1.25 +.5 72. cm /Artifact <</Type /Pagination /Attached [/Top]>> BDC\n\
    BT /F#311 12 Tf [(a\\)(b) c) -250 <4142 43>] TJ (\\\\)Tj ET EMC\n\
    [1 [2 [3]] <</A <</B (c)>>>>] true false null Q { } ) ] >> > %end";

/// The copy of `content::tests::tokens_are_as_written`, on the family's
/// reading of the page's last array (L2): an array inside an array opens
/// none, so `[1 [2 [3]]` ends at its first `]`, and the second `]`, the
/// dictionary and the last `]` are operands of their own, as PDFium reads
/// them (the family's PDFium cases `nested-array-operand` and
/// `triple-array`).
#[test]
fn tokens_are_as_written() {
    use Tok::{Op, Operand};
    assert_eq!(
        toks(PAGE),
        vec![
            Op(b"q"),
            Operand(b"0.5"),
            Operand(b"0"),
            Operand(b"0"),
            Operand(b"-1.25"),
            Operand(b"+.5"),
            Operand(b"72."),
            Op(b"cm"),
            Operand(b"/Artifact"),
            Operand(b"<</Type /Pagination /Attached [/Top]>>"),
            Op(b"BDC"),
            Op(b"BT"),
            Operand(b"/F#311"),
            Operand(b"12"),
            Op(b"Tf"),
            Operand(b"[(a\\)(b) c) -250 <4142 43>]"),
            Op(b"TJ"),
            Operand(b"(\\\\)"),
            Op(b"Tj"),
            Op(b"ET"),
            Op(b"EMC"),
            Operand(b"[1 [2 [3]"),
            Operand(b"]"),
            Operand(b"<</A <</B (c)>>>>"),
            Operand(b"]"),
            Operand(b"true"),
            Operand(b"false"),
            Operand(b"null"),
            Op(b"Q"),
            Operand(b"{"),
            Operand(b"}"),
            Operand(b")"),
            Operand(b"]"),
            Operand(b">>"),
            Operand(b">"),
        ]
    );
    assert_tiles(PAGE, &ResCx::none(), LIM);
}

/// The copy of `content::tests::names_decode_their_escapes`, on the
/// family's reading of `/#zz` (L4): a `#` and the two bytes after it are
/// one byte, a byte that's no hex digit read as 0, as PDFium's
/// `PDF_NameDecode` reads it (the family's PDFium case `name-lax-both`).
#[test]
fn names_decode_their_escapes() {
    assert_eq!(name(b"/F#311").as_deref(), Some(&b"F11"[..]));
    assert_eq!(name(b"/A#20B").as_deref(), Some(&b"A B"[..]));
    assert!(matches!(name(b"/Plain"), Some(Cow::Borrowed(b"Plain"))));
    // An escape without two bytes after it stays as written; with two
    // that aren't hex digits, it's a byte, each read as 0.
    assert_eq!(name(b"/A#2").as_deref(), Some(&b"A#2"[..]));
    assert_eq!(name(b"/#zz").as_deref(), Some(&b"\0"[..]));
    assert_eq!(name(b"/").as_deref(), Some(&b""[..]));
    assert_eq!(name(b"(F1)"), None);
}

/// Sheaf's limits but arrays and dictionaries nested `depth` deep, as
/// the family is given them.
fn nested_to(depth: usize) -> crate::pdf::content::Limits {
    let content = ContentLimits {
        depth,
        ..LIM.content
    };
    content.family()
}

/// The copy of `content::tests::operands_read_as_objects`, on the family's
/// reading where PDFium's `ReadNextObject` parts from sheaf's lexer's: a
/// raw CR LF in a literal string is kept (L5; the family's PDFium cases
/// `tj-cr`, `tj-cr-cr-lf`); an array inside an array, outside any
/// dictionary, opens none, and a `]` that ends a dictionary ends the array
/// it's in too (L2; `nested-array-dash`, `triple-array`,
/// `array-dict-ended-by-bracket`); a dictionary with a key that's no name
/// is none (L7; `bdc-broken`); and a word of digits, signs and points is a
/// number, `--5` 0 and `1.2.3` 1.2 (L6; `mcid---5`, `dash---5`,
/// `dash-1.2.3`).
#[test]
fn operands_read_as_objects() {
    let o = |s: &[u8]| object(s, &nested_to(8));
    assert_eq!(o(b"12"), Some(Object::from(12)));
    assert_eq!(o(b"-.5"), Some(Object::from(-0.5)));
    assert_eq!(o(b"72."), Some(Object::from(72.0)));
    assert_eq!(o(b"true"), Some(Object::from(true)));
    assert_eq!(o(b"null"), Some(Object::null()));
    assert_eq!(o(b"/A#20B"), Some(Object::name(b"A B")));
    // Escapes, octal codes, a line continued, and a raw end of line, kept.
    assert_eq!(
        o(b"(a\\(b\\)\\101\\0533\\\nc\r\nd)"),
        Some(literal(&b"a(b)A+3c\r\nd"[..]))
    );
    assert_eq!(o(b"<48 65 6>"), Some(Object::hex(b"He`".to_vec())));
    // An array inside an array opens none: its `[` is dropped, its
    // elements are the outer one's, and its `]` ends that.
    assert_eq!(
        o(b"[1 /N (s) [2]]"),
        Some(Object::from(vec![
            1.into(),
            Object::name(b"N"),
            literal("s"),
            2.into(),
        ]))
    );
    // A key that isn't a name makes the dictionary none.
    assert_eq!(o(b"<</A 1 2 /B /C>>"), None);
    // Signs and points make a number, as PDFium reads one.
    assert_eq!(o(b"--5"), Some(Object::from(0)));
    assert_eq!(o(b"1.2.3"), Some(Object::from(1.2)));
    for not in [&b")"[..], b"Tj"] {
        assert_eq!(o(not), None, "{}", String::from_utf8_lossy(not));
    }
    // A stray `>>` inside is no element; the `]` that ends the dictionary
    // where a value should be ends the array too.
    assert_eq!(o(b"[1 >> <</A 2 ]>>]"), Some(Object::from(vec![1.into()])));
    // Arrays nest no deeper; dictionaries do, and one past `depth`, or one
    // that doesn't end, is no object.
    assert_eq!(
        object(b"[[[1]]]", &nested_to(2)),
        Some(Object::from(vec![1.into()]))
    );
    assert_eq!(object(b"<</A <</B <</C 1>>>>>>", &nested_to(2)), None);
    assert!(object(b"<</A <</B 1>>>>", &nested_to(2)).is_some());
    assert_eq!(o(b"[1 (2"), None);
}

/// The copy of `content::tests::deep_operands_read_in_one_pass`, on the
/// family's reading of what nests (L2): an array inside an array opens
/// none, so 500 `[` around a string are one array holding it, as PDFium
/// reads them (the family's PDFium case `triple-array`); and deep by
/// dictionaries, which nest, 60 levels, under PDFium's 64, each operand
/// read in one pass. The string is its first 32,767 bytes (L5), as PDFium
/// reads a string (`hex-32768-bytes`, `literal-32768-bytes`).
#[test]
fn deep_operands_read_in_one_pass() {
    let string = |s: &mut Vec<u8>| {
        s.push(b'<');
        s.resize(s.len() + (8 << 20), b'0');
        s.push(b'>');
    };
    let mut s = b"[".repeat(500);
    string(&mut s);
    s.extend(b"]".repeat(500));
    let started = std::time::Instant::now();
    let o = object(&s, &LIM.content.family()).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    let Value::Array(inner) = o.into_value() else {
        panic!("an array")
    };
    assert!(matches!(&inner[..], [o] if o.as_bytes().is_some_and(|b| b.len() == 32_767)));

    let mut s = b"<</A ".repeat(60);
    string(&mut s);
    s.extend(b">>".repeat(60));
    let started = std::time::Instant::now();
    let mut o = object(&s, &LIM.content.family()).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    for _ in 0..60 {
        let Value::Dict(mut inner) = o.into_value() else {
            panic!("a dictionary")
        };
        o = inner.remove(b"A").unwrap();
    }
    assert!(matches!(o.as_bytes(), Some(bytes) if bytes.len() == 32_767));
}

/// `s`'s one inline image: where the token is, and its data.
fn image<'a>(s: &'a [u8], res: &ResCx) -> (Range<usize>, &'a [u8]) {
    toks_of(s, res, LIM)
        .into_iter()
        .find_map(|(r, t)| match t {
            Tok::InlineImage(data) => Some((r, &s[data])),
            _ => None,
        })
        .expect("an inline image")
}

/// An inline image with `dict`, `data` and `EI`, between `q` and `Q`:
/// `data` is what's found, and `Q` follows it.
fn assert_image(dict: &str, data: &[u8], res: &ResCx) {
    let mut s = format!("q BI {dict} ID ").into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nEI Q");
    let (r, found) = image(&s, res);
    assert_eq!(found, data, "{dict}");
    assert_eq!(&s[r.clone()][..2], b"BI");
    assert_eq!(&s[r.end - 2..r.end], b"EI");
    let ops: Vec<Tok> = toks_of(&s, res, LIM).into_iter().map(|(_, t)| t).collect();
    assert_eq!(ops.len(), 3, "{dict}");
    assert_eq!(ops[2], Tok::Op(b"Q"), "{dict}");
    assert_tiles(&s, res, LIM);
}

/// The copy of `content::tests::unfiltered_data_is_its_exact_size`, on the
/// family's reading of an image whose color space nests an array (L2,
/// L9), which PDFium reads as it does (the family's PDFium case
/// `devicen-nested-names`).
#[test]
fn unfiltered_data_is_its_exact_size() {
    let none = ResCx::none();
    // Data with ` EI ` in it: 4 bytes of gray.
    assert_image("/W 4 /H 1 /BPC 8 /CS /G", b" EI ", &none);
    assert_image(
        "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace /DeviceGray",
        b"a EI",
        &none,
    );
    // RGB and CMYK.
    assert_image("/W 2 /H 1 /BPC 8 /CS /RGB", b" EI EI", &none);
    assert_image("/W 1 /H 1 /BPC 8 /CS /DeviceCMYK", b" EI ", &none);
    // Rows padded to a byte: 3 one-bit samples are a byte a row.
    assert_image("/W 3 /H 2 /BPC 1 /CS /G", b"EI", &none);
    // 16 bits a sample.
    assert_image("/W 1 /H 1 /BPC 16 /CS /G", b"EI", &none);
    // An image mask has no color space: a bit a sample.
    assert_image("/IM true /W 10 /H 2 /D [1 0]", b" EI ", &none);
    // An indexed image: one component, whatever its base.
    assert_image(
        "/W 2 /H 2 /BPC 8 /CS [/I /RGB 1 <000000FFFFFF>] /I true",
        b"EI E",
        &none,
    );
    // An array inside an array opens none, so the DeviceN array ends at
    // its first `]`, and the dictionary at the `}` where a key should be,
    // the data from there: `[/DeviceN /Cyan /Spot]` doesn't load, so 3
    // components, 6 bytes, then the first `EI`.
    let s = b"q BI /W 2 /H 1 /BPC 8 /CS [/DeviceN [/Cyan /Spot] /DeviceCMYK {}] ID  EI \nEI Q";
    let (r, found) = image(s, &none);
    assert_eq!((r, found), (2..72, &b"] ID  "[..]));
    let ops: Vec<Tok> = toks_of(s, &none, LIM).into_iter().map(|(_, t)| t).collect();
    assert_eq!(ops[2..], [Tok::Op(b"EI"), Tok::Op(b"Q")]);
    assert_tiles(s, &none, LIM);
    // A name the resources have, by its components; one they don't is
    // none, as in PDFium: one bit of one component.
    let res = ResCx::new([(b"CS0".to_vec(), 3), (b"Sp#1".to_vec(), 1)]);
    assert_image("/W 2 /H 1 /BPC 8 /CS /CS0", b" EI EI", &res);
    assert_image("/W 2 /H 1 /BPC 8 /CS /Sp#231", b"EI", &res);
    assert_image("/W 16 /H 1 /BPC 8 /CS /Absent", b"EI", &res);
    // Reals are their whole part.
    assert_image("/W 4.9 /H 1.0 /BPC 8 /CS /G", b" EI ", &none);
}

/// The copy of `content::tests::filtered_data_ends_at_its_marker`, on the
/// family's reading of an image's length (L8): neither `/L` nor `/Length`
/// is read, as PDFium reads neither (the family's PDFium cases `flate-L`,
/// `lzw-no-end-x`).
#[test]
fn filtered_data_ends_at_its_marker() {
    let none = ResCx::none();
    assert_image("/F /AHx /W 2 /H 2 /BPC 8 /CS /G", b"0E EI 0>", &none);
    assert_image(
        "/Filter /ASCIIHexDecode /W 2 /H 2 /BPC 8 /CS /G",
        b"0E EI 0>",
        &none,
    );
    assert_image(
        "/F [/A85 /Fl] /W 9 /H 9 /BPC 8 /CS /G",
        b"9jqo^ EI ~>",
        &none,
    );
    // RunLength: a run of 4 bytes, ` EI `, then its end.
    assert_image("/F /RL /W 4 /H 1 /BPC 8 /CS /G", b"\x03 EI \x80", &none);
    assert_image(
        "/F /DCT /W 9 /H 9 /BPC 8 /CS /RGB",
        b"\xff\xd8\xff\xe0 EI \x00\xff\xd9",
        &none,
    );
    // PDF 2.0's length, abbreviated or not, isn't read: Flate data that
    // doesn't decode finds no end, and LZW's decodes on to the content's
    // end, so each image ends at its first `EI`, and what's after it is
    // content. (PDFium keeps the Flate image's `x`, and no stream of the
    // LZW one's, which reads on from its start to the same `EI`.)
    for (dict, data) in [
        ("/F /Fl /W 9 /H 9 /BPC 8 /CS /G /L 6", &b""[..]),
        ("/F /LZW /Length 6 /W 9 /H 9", b"x"),
    ] {
        let s = format!("q BI {dict} ID x EI y\nEI Q").into_bytes();
        let (r, found) = image(&s, &none);
        assert_eq!(found, data, "{dict}");
        let first_ei = s.windows(2).position(|w| w == b"EI").unwrap();
        assert_eq!(r.end, first_ei + 2, "{dict}");
        let ops: Vec<Tok> = toks_of(&s, &none, LIM)
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert_eq!(
            ops[2..],
            [Tok::Op(b"y"), Tok::Op(b"EI"), Tok::Op(b"Q")],
            "{dict}"
        );
        assert_tiles(&s, &none, LIM);
    }
    // With nothing to go by, the first `EI` between white-space.
    assert_image(
        "/F /CCF /DP <</K -1 /Columns 9>> /W 9 /H 9",
        b"xEI EIy",
        &none,
    );
    assert_image("/F /Fl /W 9 /H 9 /BPC 8 /CS /G", b"", &none);
}

/// The copy of `content::tests::an_image_ends_at_the_next_ei_past_its_data`,
/// on the family's reading of a key that isn't a name (L9), which PDFium
/// reads as it does (the family's PDFium case `number-in-key-place`).
#[test]
fn an_image_ends_at_the_next_ei_past_its_data() {
    // Past the counted data, PDFium reads tokens to the next `EI`.
    let s = b"BI /W 1 /H 1 /BPC 8 /CS /G ID x 1 2 % EI\nEI Q";
    let (r, data) = image(s, &ResCx::none());
    assert_eq!(data, b"x");
    assert_eq!(&s[r.end..], b" Q");
    // And `EI` straight after the data.
    let s = b"BI /W 1 /H 1 /BPC 8 /CS /G ID xEI Q";
    assert_eq!(image(s, &ResCx::none()).1, b"x");
    // A key that isn't a name ends the dictionary: no size counts, so the
    // image is read on to the next `EI`, all before it its data (of which
    // PDFium keeps a stream of no bytes).
    let s = b"BI 7 /W 1 /H 1 /BPC 8 /CS /G ID x EI";
    assert_eq!(image(s, &ResCx::none()).1, b"/W 1 /H 1 /BPC 8 /CS /G ID x");
}

/// The copy of `content::tests::errors_say_where`, on the family's reading
/// of what ends an array or a dictionary (L2) and of image data that
/// doesn't end (L8): where sheaf's lexer found no end, PDFium finds one,
/// and reads on (the family's PDFium cases `nested-array-dash`,
/// `bad-dict-operand`, `array-dict-ended-by-bracket`, `ahx-no-end`,
/// `a85-to-end`, `rl-no-end`); what doesn't end even so is the error.
#[test]
fn errors_say_where() {
    let lim = LIM;
    let at = |s: &[u8]| {
        let e = error(s, lim);
        (e.at, e.kind)
    };
    let read = |s: &[u8]| tokens(s, &ResCx::none(), &lim.content.family()).all(|t| t.is_ok());
    use SyntaxErrorKind::*;
    assert_eq!(at(b"0 0 m (a(b)c"), (6, String));
    assert_eq!(at(b"q <414"), (2, HexString));
    assert_eq!(at(b"[1 2 ("), (5, String));
    // An array inside an array opens none: its `]` ends the outer one.
    assert!(read(b"[1 [2]"));
    assert_eq!(at(b"[1 [2"), (0, Array));
    // A `>` or a `]` where a key should be ends a dictionary, the `]` the
    // array it's in too.
    assert!(read(b"/P << /A [1] >"));
    assert_eq!(at(b"/P << /A [1]"), (3, Dict));
    assert!(read(b"/P [ << /A 1 ] "));
    assert_eq!(at(b"/P [ << /A 1"), (5, Dict));
    assert_eq!(at(b"q BI /W 1 /H 1"), (2, InlineImage));
    assert_eq!(at(b"q BI /W 1 ID abc"), (2, InlineImage));
    assert_eq!(
        at(b"BI /W 9 /H 9 /BPC 8 /CS /G ID abc EI"),
        (0, InlineImage)
    );
    // Filtered data with no end reads on to the next `EI`, and with none,
    // doesn't end.
    assert!(read(b"Q BI /F /AHx ID 0E EI"));
    assert_eq!(at(b"Q BI /F /AHx ID 0E"), (2, InlineImage));
    assert!(read(b"BI /F /A85 ID 9jqo EI"));
    assert_eq!(at(b"BI /F /A85 ID 9jqo"), (0, InlineImage));
    assert!(read(b"BI /F /RL ID \x05abc EI"));
    assert_eq!(at(b"BI /F /RL ID \x05abc"), (0, InlineImage));
    // The tokens before an error come first.
    let read: Vec<_> = tokens(b"q (a", &ResCx::none(), &lim.content.family()).collect();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0], Ok((0..1, Tok::Op(b"q"))));
    assert!(read[1].is_err());
}

/// The copy of `content::tests::the_caps_hold`, on the family's reading of
/// what nests (L2): an array inside an array opens none, so 512 or 513 `[`
/// and as many `]` are an array and stray closers, and a `<<` where a key
/// should be ends a dictionary, so 513 `<<` nest none either, as PDFium
/// reads them (the family's PDFium cases `triple-array`,
/// `bad-dict-operand`). Dictionaries nest: past 64 levels, PDFium's, it's
/// an error, whatever the cap (L3), where PDFium reads what's deeper as no
/// object (the family's `deep-65`, read otherwise than PDFium on purpose).
#[test]
fn the_caps_hold() {
    let capped = |content: ContentLimits| Limits {
        content,
        ..Limits::DEFAULT
    };
    let depth = Limits::DEFAULT.content.depth;
    let nested = |n: usize| [b"[".repeat(n), b"]".repeat(n)].concat();
    assert_eq!(toks(&nested(depth)).len(), depth);
    assert_eq!(toks(&nested(depth + 1)).len(), depth + 1);
    // `<<<<` is a dictionary that's none: 256 of them, `<<>>`, 512 `>>`.
    let pairs = [b"<<".repeat(depth + 1), b">>".repeat(depth + 1)].concat();
    assert_eq!(toks(&pairs).len(), 769);
    let dicts = |n: usize| [b"<</A ".repeat(n), b"1".to_vec(), b">>".repeat(n)].concat();
    assert_eq!(toks(&dicts(64)).len(), 1);
    let e = error(&dicts(65), &Limits::DEFAULT);
    assert_eq!((e.at, e.kind), (64 * 5, SyntaxErrorKind::Depth));

    let three = capped(ContentLimits {
        operands: 3,
        ..Limits::DEFAULT.content
    });
    assert_eq!(toks_of(b"1 2 3 m 4 5 6 l", &ResCx::none(), &three).len(), 8);
    let e = error(b"1 2 3 4 m", &three);
    assert_eq!((e.at, e.kind), (6, SyntaxErrorKind::Operands));

    let small = capped(ContentLimits {
        inline_image: 3,
        ..Limits::DEFAULT.content
    });
    let s = b"q BI /W 3 /H 1 /BPC 8 /CS /G ID abc EI BI /W 4 /H 1 /BPC 8 /CS /G ID abcd EI";
    let e = error(s, &small);
    assert_eq!((e.at, e.kind), (39, SyntaxErrorKind::InlineImageSize));
}

/// The copy of `content::tests::unended_images_dont_search_again`.
#[test]
fn unended_images_dont_search_again() {
    let s = b"BI /F /DCT ID x EI ".repeat(50_000);
    let started = std::time::Instant::now();
    let read = tokens(&s, &ResCx::none(), &LIM.content.family()).count();
    assert_eq!(read, 50_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

/// Tokens made of what content is made of, run together.
fn content() -> impl Strategy<Value = Vec<u8>> {
    let pieces = prop::sample::select(vec![
        &b" "[..],
        b"\n",
        b"0",
        b"-1.5",
        b"/N",
        b"/W 2 /H 1 /BPC 8 /CS /G",
        b"/F /AHx",
        b"/F /DCT",
        b"/L 3",
        b"(",
        b")",
        b"\\",
        b"<",
        b">",
        b"<<",
        b">>",
        b"[",
        b"]",
        b"{",
        b"%",
        b"BI",
        b"ID",
        b"EI",
        b"~>",
        b"\xff\xd9",
        b"\x80",
        b"Tj",
        b"q",
        b"Q",
    ]);
    prop::collection::vec(pieces, 0..48).prop_map(|p| p.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The copy of `content::tests::tokens_tile_any_content`, 0x80 and
    /// 0xFF white-space between tokens, as PDFium reads them (L1; the
    /// family's PDFium cases `byte-80-before-re`, `byte-ff-before-re`).
    #[test]
    fn tokens_tile_any_content(s in content()) {
        let res = ResCx::new([(b"N".to_vec(), 3)]);
        assert_tiles(&s, &res, LIM);
    }
}

/// The copy of `content::tests::streams_lex_as_one`.
#[test]
fn streams_lex_as_one() {
    let first: &[u8] = b"q /P <</MCID 0>> BDC (ab";
    let second: &[u8] = b"c) Tj";
    let third: &[u8] = b"EMC Q";
    let c = Concat::new(&[first, second, third]);
    assert_eq!(c.bytes(), b"q /P <</MCID 0>> BDC (ab c) Tj EMC Q");
    assert_eq!(c.stream_count(), 3);
    assert_eq!(c.stream(1), second);
    assert_eq!(c.range(2), 31..36);
    let read = toks_of(c.bytes(), &ResCx::none(), LIM);
    // The string runs from the first stream into the second, as in PDFium.
    let (string, _) = &read[4];
    assert_eq!(
        c.span(string.clone()),
        SpanAcross {
            start: Pos { stream: 0, at: 21 },
            end: Pos { stream: 1, at: 2 },
        }
    );
    let (emc, _) = &read[6];
    assert_eq!(c.span(emc.clone()).start, Pos { stream: 2, at: 0 });
    // The space between is the end of the stream before it.
    assert_eq!(c.pos(24), Pos { stream: 0, at: 24 });
    assert_eq!(c.pos(36), Pos { stream: 2, at: 5 });
    // One stream is read where it is.
    let one = Concat::new(&[first]);
    assert!(std::ptr::eq(one.bytes(), first));
}

/// The copy of `content::tests::color_spaces_count_their_components`, on
/// the family's count of a DeviceN space with no tint transform (L10): it
/// doesn't load, so it's 3, as PDFium counts it (pdf-color's PDFium case
/// `device-n-no-function`; pdf-content-streams' `cs-device-n-no-function`).
#[test]
fn color_spaces_count_their_components() {
    let mut pdf = Pdf::with_version("1.7");
    let icc = pdf.add_object(Stream::new(dictionary! { "N" => 4 }, Vec::new()));
    let gray = pdf.add_object(Object::name(b"DeviceGray"));
    let spaces = pdf.add_object(dictionary! {
        "Icc" => vec![Object::name("ICCBased"), icc.into()],
        "Cmyk" => Object::name("DeviceCMYK"),
        "Gray" => gray,
        "Idx" => vec![
            Object::name("Indexed"),
            Object::name("DeviceRGB"),
            1.into(),
            literal("abcdef"),
        ],
        "N2" => vec![
            Object::name("DeviceN"),
            vec![Object::name("a"), Object::name("b")].into(),
            Object::name("DeviceRGB"),
        ],
        "Lab" => vec![Object::name("Lab"), dictionary! {}.into()],
        "Odd" => 7,
    });
    let resources: Object = dictionary! { "ColorSpace" => spaces }.into();
    let res = res_cx(&pdf, Some(&resources), LIM).unwrap();
    let want: [(&[u8], u32); 7] = [
        (b"Icc", 4),
        (b"Cmyk", 4),
        (b"Gray", 1),
        (b"Idx", 1),
        (b"N2", 3),
        (b"Lab", 3),
        (b"Odd", 3),
    ];
    assert_eq!(res, ResCx::new(want.map(|(n, c)| (n.to_vec(), c))));
    assert_eq!(res_cx(&pdf, None, LIM).unwrap(), ResCx::none());
    // An image in one is sized by it.
    assert_image("/W 1 /H 1 /BPC 8 /CS /Icc", b" EI ", &res);
}

fn found<'p>(
    streams: &[&[u8]],
    props: &'p Properties,
) -> Result<Vec<crate::pdf::content::Marked<'p>>, SyntaxError> {
    marked(
        &Concat::new(streams),
        props,
        &ResCx::none(),
        &LIM.content.family(),
    )
}

fn span(start: (u32, u32), end: (u32, u32)) -> SpanAcross {
    let pos = |(stream, at)| Pos { stream, at };
    SpanAcross {
        start: pos(start),
        end: pos(end),
    }
}

/// The span of `part` in `s`, one stream.
fn span_of(s: &[u8], part: &[u8]) -> SpanAcross {
    let at = s.windows(part.len()).position(|w| w == part).unwrap() as u32;
    span((0, at), (0, at + part.len() as u32))
}

/// The copy of `content::blocks_tests::marked_ranges_have_their_lists`, its
/// lists read by [`properties`].
#[test]
fn marked_ranges_have_their_lists() {
    let mut pdf = Pdf::with_version("1.7");
    let ocg = pdf.add_object(dictionary! { "Type" => Object::name("OCG") });
    let resources: Object = dictionary! {
        "Properties" => dictionary! {
            "P0" => dictionary! { "Subtype" => Object::name("Header") },
            "oc1" => ocg,
        },
    }
    .into();
    let props = properties(&pdf, Some(&resources));
    let first: &[u8] =
        b"/Artifact <</Type /Pagination /S#75btype /Footer /A [1 (b\\)) <0a>]>> BDC q";
    let second: &[u8] =
        b"/OC /oc1 BDC /Span BMC EMC EMC Q EMC /Artifact /P0 BDC EMC /X /None BDC EMC";
    let m = found(&[first, second], &props).unwrap();
    let tags: Vec<&[u8]> = m.iter().map(|m| m.tag.as_slice()).collect();
    assert_eq!(tags, [&b"Artifact"[..], b"OC", b"Span", b"Artifact", b"X"]);
    assert_eq!(
        m.iter().map(|m| m.depth).collect::<Vec<_>>(),
        [0, 1, 2, 0, 0]
    );
    // The first runs from the first stream into the second.
    assert_eq!(m[0].span, span((0, 0), (1, 36)));
    let Some(Props::Inline(list)) = &m[0].props else {
        panic!("{:?}", m[0].props)
    };
    assert_eq!(list.get(b"Subtype").unwrap(), &Object::name(b"Footer"));
    assert_eq!(
        list.get(b"A").unwrap(),
        &Object::from(vec![1.into(), literal("b)"), Object::hex(vec![10]),])
    );
    // Named lists, found or not.
    assert_eq!(
        m[1].props,
        Some(Props::Named(
            b"oc1".to_vec(),
            Some(&dictionary! { "Type" => Object::name("OCG") })
        ))
    );
    assert_eq!(m[2].props, None);
    assert_eq!(
        m[3].props.as_ref().and_then(Props::dict),
        Some(&dictionary! { "Subtype" => Object::name("Header") })
    );
    assert_eq!(m[4].props, Some(Props::Named(b"None".to_vec(), None)));
}

/// The copy of `content::blocks_tests::marked_content_must_balance`.
#[test]
fn marked_content_must_balance() {
    let props = Properties::default();
    let kind = |s: &[u8]| found(&[s], &props).unwrap_err();
    assert_eq!(
        kind(b"q /A BMC Q EMC EMC"),
        SyntaxError {
            at: 15,
            kind: SyntaxErrorKind::Emc
        }
    );
    assert_eq!(
        kind(b"/A BMC /B BMC EMC"),
        SyntaxError {
            at: 0,
            kind: SyntaxErrorKind::Marked
        }
    );
    let deep = Limits {
        content: ContentLimits {
            depth: 2,
            ..LIM.content
        },
        ..*LIM
    };
    let e = marked(
        &Concat::new(&[b"/A BMC /B BMC /C BMC EMC EMC EMC"]),
        &props,
        &ResCx::none(),
        &deep.content.family(),
    )
    .unwrap_err();
    assert_eq!(e.kind, SyntaxErrorKind::Depth);
    // Marked content inside a string or an inline image isn't.
    assert!(found(&[b"(EMC) Tj BI /W 3 /H 1 /CS /G /BPC 8 ID EMC EI"], &props).is_ok());
}

/// The copy of `content::blocks_tests::balance_is_the_q_count`, each
/// balance its two counts, as the family's can't be written whole.
#[test]
fn balance_is_the_q_count() {
    let counts = |c: &Concat| {
        let b = balance(c, &ResCx::none(), &LIM.content.family()).unwrap();
        (b.net, b.min_prefix)
    };
    let of = |s: &[u8]| counts(&Concat::new(&[s]));
    // §3.4's worked bases.
    let b = |net, min_prefix| (net, min_prefix);
    assert_eq!(of(b"q 1 0 0 1 0 0 cm"), b(1, 0));
    assert_eq!(of(b"Q q"), b(0, -1));
    assert_eq!(of(b"Q"), b(-1, -1));
    assert_eq!(of(b"q q Q Q Q q"), b(0, -1));
    // Not in strings, names or an image's data.
    assert_eq!(
        of(b"(Q) Tj /Q gs BI /W 1 /H 1 /CS /G /BPC 8 ID Q EI"),
        b(0, 0)
    );
    // Across streams.
    let c = Concat::new(&[b"q", b"Q Q"]);
    assert_eq!(counts(&c), b(-1, -1));
}

/// The copy of `content::blocks_tests::names_used_are_by_category`.
#[test]
fn names_used_are_by_category() {
    let s: &[u8] = b"/GS0 gs /F1 9 Tf /CS0 cs /DeviceRGB CS /P0 scn 0.5 /P1 SCN /Im1 Do \
        /Sh0 sh /OC /oc0 BDC EMC /X /Named DP /Y <<>> BDC EMC \
        BI /W 1 /H 1 /CS /Idx /BPC 8 ID x EI BI /W 1 /H 1 /CS /RGB /BPC 8 ID xyz EI \
        /Artifact BMC /F9 1 Tf EMC";
    let c = Concat::new(&[s]);
    let except = [span_of(s, b"/Artifact BMC /F9 1 Tf EMC")];
    let names = names_used(&c, &ResCx::none(), &LIM.content.family(), &except).unwrap();
    let of = |kind| names.of(kind).collect::<Vec<_>>();
    assert_eq!(of(ResKind::ExtGState), [b"GS0"]);
    assert_eq!(of(ResKind::Font), [b"F1"]);
    assert_eq!(of(ResKind::ColorSpace), [&b"CS0"[..], b"Idx"]);
    assert_eq!(of(ResKind::Pattern), [b"P0", b"P1"]);
    assert_eq!(of(ResKind::XObject), [b"Im1"]);
    assert_eq!(of(ResKind::Shading), [b"Sh0"]);
    assert_eq!(of(ResKind::Properties), [&b"Named"[..], b"oc0"]);
    assert_eq!(names.len(), 10);
    assert!(!names.contains(ResKind::Font, b"F9"));
}
