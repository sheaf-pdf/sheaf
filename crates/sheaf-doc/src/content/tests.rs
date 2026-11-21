//! Content's tokens as sheaf reads them, pdf-content-streams': tokens
//! that tile what they're read from, inline images by each of PDFium's
//! rules, errors that say where, the caps, as sheaf's limits map to the
//! family's ([`ContentLimits::family`]), and color spaces counted from
//! sheaf's sources ([`res_cx`]); and a page's streams read and read as
//! one. Where sheaf's own lexer, before M18 slice 17, read content
//! otherwise, PDFium (chromium/8152) reads it as the family does, and a
//! test says which difference it is and where PDFium's answer is: a case
//! the family holds to PDFium (pdf-content-streams' `tests/pdfium`, or
//! pdf-color's, by name), PDFium having been asked of the test's own
//! input too.
//!
//! The differences, as the tests name them (sheaf's old lexer, then the
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
use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::pdf::{Document as Pdf, LazyPdf, Object, ObjectId, Stream, Value, dictionary, literal};
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
pub(super) fn blank(gap: &[u8]) -> bool {
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

/// Tokens as written, the page's last array as the family reads it (L2):
/// an array inside an array opens
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

/// Names decoded, `/#zz` as the family reads it (L4): a `#` and the two
/// bytes after it are
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

/// Operands read as objects, as the family reads them where PDFium's
/// `ReadNextObject` parts from sheaf's old lexer: a
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

/// Operands nested deep, read in one pass, what nests as the family reads
/// it (L2): an array inside an array opens
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

/// Unfiltered data is its exact size; an image whose color space nests an
/// array as the family reads it (L2, L9), which PDFium reads as it does (the family's PDFium case
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

/// Filtered data ends at its marker, an image's length as the family reads
/// it (L8): neither `/L` nor `/Length`
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

/// An image ends at the next `EI` past its data; a key that isn't a name
/// as the family reads it (L9), which PDFium
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

/// Errors say where; what ends an array or a dictionary (L2) and image
/// data that doesn't end (L8) as the family reads them: where sheaf's old
/// lexer found no end, PDFium finds one,
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

/// The caps hold, as sheaf's limits map to the family's; what nests as
/// the family reads it (L2): an array inside an array opens none, so 512
/// or 513 `[` and as many `]` are an array and stray closers, and a `<<`
/// where a key
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

/// Content of many inline images that don't end is read once: the end
/// of the first is searched for, not each one's.
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

    /// Whatever the bytes, the tokens tile them, up to an error, 0x80 and
    /// 0xFF white-space between tokens, as PDFium reads them (L1; the
    /// family's PDFium cases `byte-80-before-re`, `byte-ff-before-re`).
    #[test]
    fn tokens_tile_any_content(s in content()) {
        let res = ResCx::new([(b"N".to_vec(), 3)]);
        assert_tiles(&s, &res, LIM);
    }
}

/// A page's streams are read as one, as PDFium reads them.
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

/// Color spaces count their components, read from sheaf's sources
/// ([`res_cx`]); a DeviceN space with no tint transform as the family
/// counts it (L10): it
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

/// A one-page document whose page's `/Contents` is what `contents` adds,
/// numbered from 1.
fn page_with(contents: impl FnOnce(&mut Pdf) -> Object) -> (Pdf, ObjectId) {
    let mut pdf = Pdf::with_version("1.7");
    let contents = contents(&mut pdf);
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {
        "Type" => Object::name("Page"),
        "Parent" => pages,
        "Contents" => contents,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
    });
    pdf.objects.insert(
        pages,
        dictionary! {
            "Type" => Object::name("Pages"),
            "Kids" => vec![page.into()],
            "Count" => 1,
        }
        .into(),
    );
    let catalog = pdf.add_object(dictionary! {
        "Type" => Object::name("Catalog"),
        "Pages" => pages,
    });
    pdf.trailer.set("Root", catalog);
    (pdf, page)
}

fn flate(data: &[u8]) -> Stream {
    Stream::new(
        dictionary! { "Filter" => Object::name("FlateDecode") },
        crate::pdf::deflate(data),
    )
}

fn read(
    pdf: &impl ReadObjects,
    page: &crate::pdf::Dictionary,
) -> Result<PageContent, ContentFault> {
    PageContent::read(pdf, &contents(page), &Limits::DEFAULT)
}

fn page_dict(pdf: &Pdf, page: ObjectId) -> crate::pdf::Dictionary {
    pdf.get_dictionary(page).unwrap().clone()
}

#[test]
fn a_pages_streams_read_as_one() {
    let (mut pdf, page) = page_with(|pdf| {
        let first = pdf.add_object(flate(b"q 1 0 0 rg"));
        let second = pdf.add_object(Stream::new(dictionary! {}, b"0 0 9 9 re f Q".to_vec()));
        // A missing object draws nothing.
        vec![first.into(), ObjectId::new(90, 0).into(), second.into()].into()
    });
    let content = read(&pdf, &page_dict(&pdf, page)).unwrap();
    assert_eq!(content.concat().bytes(), b"q 1 0 0 rg 0 0 9 9 re f Q");
    assert_eq!(content.ids(), &[ObjectId::new(1, 0), ObjectId::new(2, 0)]);

    // Through a reference to an array, from a source.
    let array = pdf.add_object(Object::from(vec![
        ObjectId::new(2, 0).into(),
        ObjectId::new(1, 0).into(),
    ]));
    pdf.get_dictionary_mut(page).unwrap().set("Contents", array);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let lazy = LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap();
    let content = read(&lazy, &page_dict(&pdf, page)).unwrap();
    assert_eq!(content.concat().bytes(), b"0 0 9 9 re f Q q 1 0 0 rg");
    // Read, not kept.
    assert!(
        !lazy.is_read(ObjectId::new(1, 0))
            && !lazy.is_read(ObjectId::new(2, 0))
            && !lazy.is_read(array)
    );
    assert_eq!(content.into_bytes(), b"0 0 9 9 re f Q q 1 0 0 rg");
}

#[test]
fn a_stream_that_wont_decode_is_a_fault() {
    let (pdf, page) = page_with(|pdf| {
        let first = pdf.add_object(Stream::new(dictionary! {}, b"q".to_vec()));
        let bad = pdf.add_object(Stream::new(
            dictionary! { "Filter" => Object::name("JBIG2Decode") },
            b"Q".to_vec(),
        ));
        vec![first.into(), bad.into()].into()
    });
    let fault = read(&pdf, &page_dict(&pdf, page)).unwrap_err();
    assert!(
        matches!(
            fault,
            ContentFault::Stream {
                stream: ObjectId {
                    number: 2,
                    generation: 0
                },
                ..
            }
        ),
        "{fault}"
    );
    assert!(
        fault.to_string().starts_with("stream 2 0 R can't be read"),
        "{fault}"
    );
}

#[test]
fn streams_past_the_cap_are_a_fault() {
    let (pdf, page) = page_with(|pdf| {
        let ids: Vec<Object> = (0..3)
            .map(|_| pdf.add_object(flate(&[b' '; 1000])).into())
            .collect();
        ids.into()
    });
    let capped = |decompressed| Limits {
        content: ContentLimits {
            decompressed,
            ..Limits::DEFAULT.content
        },
        ..Limits::DEFAULT
    };
    let dict = page_dict(&pdf, page);
    // Three streams and the two spaces between.
    let content = PageContent::read(&pdf, &contents(&dict), &capped(3002)).unwrap();
    assert_eq!(content.concat().bytes().len(), 3002);
    for cap in [3001, 2500, 999] {
        let fault = PageContent::read(&pdf, &contents(&dict), &capped(cap)).unwrap_err();
        assert!(
            matches!(fault, ContentFault::TooLarge(c) if c == cap),
            "{fault}"
        );
    }
}

/// What pdf-content-streams is given is what sheaf reads with: its
/// defaults under sheaf's caps.
#[test]
fn the_familys_limits_are_sheafs() {
    use crate::pdf::content as family;
    // The defaults are the same caps already.
    assert_eq!(Limits::DEFAULT.content.family(), family::Limits::DEFAULT);
    let content = ContentLimits {
        decompressed: 3002,
        operands: 7,
        depth: 5,
        inline_image: 11,
        ..Limits::DEFAULT.content
    };
    let mut want = family::Limits::DEFAULT;
    want.content = 3002;
    want.operands = 7;
    want.depth = 5;
    want.inline_image = 11;
    assert_eq!(content.family(), want);
}

/// Content whose tokens stop is a fault that says where, in the stream
/// it's in.
#[test]
fn a_syntax_error_names_its_stream() {
    let (pdf, page) = page_with(|pdf| {
        let first = pdf.add_object(Stream::new(dictionary! {}, b"q".to_vec()));
        let second = pdf.add_object(Stream::new(dictionary! {}, b"0 g (open".to_vec()));
        vec![first.into(), second.into()].into()
    });
    let content = read(&pdf, &page_dict(&pdf, page)).unwrap();
    let lim = Limits::DEFAULT.content.family();
    let e = tokens(content.concat().bytes(), &ResCx::none(), &lim)
        .find_map(Result::err)
        .unwrap();
    assert_eq!(e.at, 6);
    let fault = content.fault(e);
    assert_eq!(
        fault.to_string(),
        "stream 2 0 R: a string that doesn't end at byte 4"
    );
}
