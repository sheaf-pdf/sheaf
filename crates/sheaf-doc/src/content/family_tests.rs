//! sheaf's lexer's tests, and those of its balance, marked ranges, names
//! and color spaces' counts, each copied onto pdf-content-streams' calls of
//! the same name, as sheaf would make them (its limits by
//! [`ContentLimits::family`], its sources read through [`Objects`]), with
//! what each expects unchanged: where the family reads content as sheaf's
//! lexer does, a copy passes; where it doesn't, it's ignored, saying which
//! difference it is and the assertion that fails. The functions sheaf's
//! readings of its sources would give the family are here too:
//! [`res_cx`] and [`properties`].
//!
//! The differences, as the ignored copies name them (sheaf's lexer, then
//! the family):
//! - L1: white-space is six bytes; those, 0x80 and 0xFF.
//! - L2: an array or a dictionary is one token to its closer; ended where
//!   PDFium's `ReadNextObject` ends it (an array in an array opens none, a
//!   dictionary ends at a key that's no name or a value that's no object).
//! - L3: arrays and dictionaries nest to the depth cap; past 64 levels,
//!   an error.
//! - L4: a name's `#` escapes only two hex digits; any two bytes, a
//!   non-digit 0, as PDFium's `PDF_NameDecode`.
//! - L5: a raw CR or CR LF in a literal string is LF; kept as written.
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

use super::page::direct;
use super::tests::blank;
use super::{Objects, ReadObjects, Unkept};
use crate::pdf::content::{
    Concat, Pos, Properties, Props, ResCx, ResKind, SpanAcross, SyntaxError, SyntaxErrorKind, Tok,
    balance, marked, name, names_used, object, tokens,
};
use crate::pdf::{
    Dictionary, Document as Pdf, Object, Stream, Unreadable, Value, dictionary, literal,
};
use crate::source::{ContentLimits, Limits};

/// The color spaces `resources` names, a page's or a form's, counted as
/// pdf-content-streams counts them ([`ResCx::read`]), read from `objects`
/// through [`Objects`], with what the source doesn't keep held for this
/// call alone.
pub(super) fn res_cx(
    objects: &impl ReadObjects,
    resources: Option<&Object>,
    lim: &Limits,
) -> Result<ResCx, Unreadable> {
    let unkept = Unkept::default();
    ResCx::read(
        Objects::new(objects, &unkept),
        resources,
        &lim.content.family(),
    )
}

/// The property lists `resources` names, a page's or a form's
/// `/Properties`, each a dictionary, references followed, for
/// pdf-content-streams' marked content: what sheaf's `PropertiesDict::read`
/// reads, a list that doesn't read none.
pub(super) fn properties(objects: &impl ReadObjects, resources: Option<&Object>) -> Properties {
    fn plain_dict(o: &Object) -> Option<&Dictionary> {
        match o.value() {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }
    let direct = |o| direct(objects, o);
    let lists = resources
        .and_then(direct)
        .and_then(plain_dict)
        .and_then(|r| direct(r.get(b"Properties")?))
        .and_then(plain_dict);
    Properties::new(
        (lists.into_iter())
            .flat_map(|lists| lists.iter())
            .filter_map(|(name, list)| Some((name.to_vec(), plain_dict(direct(list)?)?.clone()))),
    )
}

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

/// The copy of `content::tests::tokens_are_as_written`.
#[test]
#[ignore = "L2: `[1 [2 [3]] <</A <</B (c)>>>>]` is one operand to sheaf's lexer; \
            the family ends it at its first `]` (`assert_eq!(toks(PAGE), …)`)"]
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
            Operand(b"[1 [2 [3]] <</A <</B (c)>>>>]"),
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

/// The copy of `content::tests::names_decode_their_escapes`.
#[test]
#[ignore = "L4: `/#zz` is `#zz` to sheaf's lexer, a `#` an escape only before two hex digits; \
            the family reads `#` and the two bytes after it as one byte, a non-digit as 0 \
            (`assert_eq!(name(b\"/#zz\")…, Some(&b\"#zz\"[..]))`)"]
fn names_decode_their_escapes() {
    assert_eq!(name(b"/F#311").as_deref(), Some(&b"F11"[..]));
    assert_eq!(name(b"/A#20B").as_deref(), Some(&b"A B"[..]));
    assert!(matches!(name(b"/Plain"), Some(Cow::Borrowed(b"Plain"))));
    // An escape with no two hex digits stays as written.
    assert_eq!(name(b"/A#2").as_deref(), Some(&b"A#2"[..]));
    assert_eq!(name(b"/#zz").as_deref(), Some(&b"#zz"[..]));
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

/// The copy of `content::tests::operands_read_as_objects`.
#[test]
#[ignore = "L5: a raw CR LF in a literal string read as an object is kept as written by the \
            family, where sheaf's lexer reads it as LF (`assert_eq!(o(b\"(a\\\\(b…\"), …)`); \
            and after it L2 (`[2]` in an array opens none), L7 (a key that's no name makes the \
            dictionary none) and L6 (`--5` is 0, `1.2.3` is 1.2)"]
fn operands_read_as_objects() {
    let o = |s: &[u8]| object(s, &nested_to(8));
    assert_eq!(o(b"12"), Some(Object::from(12)));
    assert_eq!(o(b"-.5"), Some(Object::from(-0.5)));
    assert_eq!(o(b"72."), Some(Object::from(72.0)));
    assert_eq!(o(b"true"), Some(Object::from(true)));
    assert_eq!(o(b"null"), Some(Object::null()));
    assert_eq!(o(b"/A#20B"), Some(Object::name(b"A B")));
    // Escapes, octal codes, a line continued, and an end of line.
    assert_eq!(
        o(b"(a\\(b\\)\\101\\0533\\\nc\r\nd)"),
        Some(literal(&b"a(b)A+3c\nd"[..]))
    );
    assert_eq!(o(b"<48 65 6>"), Some(Object::hex(b"He`".to_vec())));
    assert_eq!(
        o(b"[1 /N (s) [2]]"),
        Some(Object::from(vec![
            1.into(),
            Object::name(b"N"),
            literal("s"),
            Object::from(vec![2.into()]),
        ]))
    );
    // A key that isn't a name is passed over.
    assert_eq!(
        o(b"<</A 1 2 /B /C>>"),
        Some(Object::from(
            dictionary! { "A" => 1, "B" => Object::name("C") }
        ))
    );
    for not in [&b"--5"[..], b")", b"Tj", b"1.2.3"] {
        assert_eq!(o(not), None, "{}", String::from_utf8_lossy(not));
    }
    // Stray closers inside are no elements; one past `depth`, or one that
    // doesn't end, is no object.
    assert_eq!(
        o(b"[1 >> <</A 2 ]>>]"),
        Some(Object::from(vec![
            1.into(),
            Object::from(dictionary! { "A" => 2 }),
        ]))
    );
    assert_eq!(object(b"[[[1]]]", &nested_to(2)), None);
    assert!(object(b"[[1]]", &nested_to(2)).is_some());
    assert_eq!(o(b"[1 (2"), None);
}

/// The copy of `content::tests::deep_operands_read_in_one_pass`.
#[test]
#[ignore = "L2: an array inside an array opens none to the family, so 500 `[` read as one \
            array holding the string (`let Value::Array(mut inner) = o.into_value() else { \
            panic!(\"an array\") }`, at the second level)"]
fn deep_operands_read_in_one_pass() {
    let mut s = b"[".repeat(500);
    s.push(b'<');
    s.resize(s.len() + (8 << 20), b'0');
    s.push(b'>');
    s.extend(b"]".repeat(500));
    let started = std::time::Instant::now();
    let mut o = object(&s, &LIM.content.family()).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    for _ in 0..500 {
        let Value::Array(mut inner) = o.into_value() else {
            panic!("an array")
        };
        o = inner.pop().unwrap();
    }
    assert!(matches!(o.as_bytes(), Some(bytes) if bytes.len() == 4 << 20));
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

/// The copy of `content::tests::unfiltered_data_is_its_exact_size`.
#[test]
#[ignore = "L2 and L9: in `/CS [/DeviceN [/Cyan /Spot] /DeviceCMYK {}]` the family ends the \
            array at its first `]`, as an array in an array opens none, and the image's \
            dictionary where a key should be and isn't a name, its data from there, 6 bytes of a \
            space it can't load, counted 3 (`assert_eq!(found, data, \"{dict}\")`)"]
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
    assert_image(
        "/W 2 /H 1 /BPC 8 /CS [/DeviceN [/Cyan /Spot] /DeviceCMYK {}]",
        b" EI ",
        &none,
    );
    // A name the resources have, by its components; one they don't is
    // none, as in PDFium: one bit of one component.
    let res = ResCx::new([(b"CS0".to_vec(), 3), (b"Sp#1".to_vec(), 1)]);
    assert_image("/W 2 /H 1 /BPC 8 /CS /CS0", b" EI EI", &res);
    assert_image("/W 2 /H 1 /BPC 8 /CS /Sp#231", b"EI", &res);
    assert_image("/W 16 /H 1 /BPC 8 /CS /Absent", b"EI", &res);
    // Reals are their whole part.
    assert_image("/W 4.9 /H 1.0 /BPC 8 /CS /G", b" EI ", &none);
}

/// The copy of `content::tests::filtered_data_ends_at_its_marker`.
#[test]
#[ignore = "L8: `/F /Fl … /L 6` with data `x EI y` ends by `/L` in sheaf's lexer; the family \
            reads no `/L`, its Flate data refused, and ends it at the first `EI` operator, no \
            data kept (`assert_eq!(found, data, \"{dict}\")`); and so `/F /LZW /Length 6`, \
            its data `x` where LZW decoding stops"]
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
    // PDF 2.0's length, abbreviated or not.
    assert_image("/F /Fl /W 9 /H 9 /BPC 8 /CS /G /L 6", b"x EI y", &none);
    assert_image("/F /LZW /Length 6 /W 9 /H 9", b"x EI y", &none);
    // With nothing to go by, the first `EI` between white-space.
    assert_image(
        "/F /CCF /DP <</K -1 /Columns 9>> /W 9 /H 9",
        b"xEI EIy",
        &none,
    );
    assert_image("/F /Fl /W 9 /H 9 /BPC 8 /CS /G", b"", &none);
}

/// The copy of `content::tests::an_image_ends_at_the_next_ei_past_its_data`.
#[test]
#[ignore = "L9: in `BI 7 /W 1 …`, sheaf's lexer passes over the `7` where a key should be; \
            the family ends the dictionary there, and reads the data from \
            after it (`assert_eq!(image(s, …).1, b\"x\")`, the third)"]
fn an_image_ends_at_the_next_ei_past_its_data() {
    // Past the counted data, PDFium reads tokens to the next `EI`.
    let s = b"BI /W 1 /H 1 /BPC 8 /CS /G ID x 1 2 % EI\nEI Q";
    let (r, data) = image(s, &ResCx::none());
    assert_eq!(data, b"x");
    assert_eq!(&s[r.end..], b" Q");
    // And `EI` straight after the data.
    let s = b"BI /W 1 /H 1 /BPC 8 /CS /G ID xEI Q";
    assert_eq!(image(s, &ResCx::none()).1, b"x");
    // A key that isn't a name is passed over.
    let s = b"BI 7 /W 1 /H 1 /BPC 8 /CS /G ID x EI";
    assert_eq!(image(s, &ResCx::none()).1, b"x");
}

/// The copy of `content::tests::errors_say_where`.
#[test]
#[ignore = "L2: `[1 [2]` doesn't end to sheaf's lexer; to the family the inner `[` opens no \
            array, and the first `]` ends it, no error (`assert_eq!(at(b\"[1 [2]\"), (0, \
            Array))`); after it L2 twice (a dictionary ends at `>` or `]` where a key should \
            be) and L8 three times (AHx, A85 and RL data with no end read on to the next `EI`)"]
fn errors_say_where() {
    let lim = LIM;
    let at = |s: &[u8]| {
        let e = error(s, lim);
        (e.at, e.kind)
    };
    use SyntaxErrorKind::*;
    assert_eq!(at(b"0 0 m (a(b)c"), (6, String));
    assert_eq!(at(b"q <414"), (2, HexString));
    assert_eq!(at(b"[1 2 ("), (5, String));
    assert_eq!(at(b"[1 [2]"), (0, Array));
    assert_eq!(at(b"/P << /A [1] >"), (3, Dict));
    // A `]` doesn't close a dictionary: it's passed over.
    assert_eq!(at(b"/P [ << /A 1 ] "), (5, Dict));
    assert_eq!(at(b"q BI /W 1 /H 1"), (2, InlineImage));
    assert_eq!(at(b"q BI /W 1 ID abc"), (2, InlineImage));
    assert_eq!(
        at(b"BI /W 9 /H 9 /BPC 8 /CS /G ID abc EI"),
        (0, InlineImage)
    );
    assert_eq!(at(b"Q BI /F /AHx ID 0E EI"), (2, InlineImage));
    assert_eq!(at(b"BI /F /A85 ID 9jqo EI"), (0, InlineImage));
    assert_eq!(at(b"BI /F /RL ID \x05abc EI"), (0, InlineImage));
    // The tokens before an error come first.
    let read: Vec<_> = tokens(b"q (a", &ResCx::none(), &lim.content.family()).collect();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0], Ok((0..1, Tok::Op(b"q"))));
    assert!(read[1].is_err());
}

/// The copy of `content::tests::the_caps_hold`.
#[test]
#[ignore = "L2: 512 `[` then 512 `]` are one operand to sheaf's lexer; to the family an array \
            in an array opens none, so the first `]` ends it and the rest are 511 stray closers \
            (`assert_eq!(toks(&nested(depth)).len(), 1)`); and so neither 513 `[` nor 513 `<<` \
            (a `<<` where a key should be ends a dictionary) is past the depth"]
fn the_caps_hold() {
    let capped = |content: ContentLimits| Limits {
        content,
        ..Limits::DEFAULT
    };
    let depth = Limits::DEFAULT.content.depth;
    let nested = |n: usize| [b"[".repeat(n), b"]".repeat(n)].concat();
    assert_eq!(toks(&nested(depth)).len(), 1);
    let e = error(&nested(depth + 1), &Limits::DEFAULT);
    assert_eq!((e.at, e.kind), (depth, SyntaxErrorKind::Depth));
    let dicts = [b"<<".repeat(depth + 1), b">>".repeat(depth + 1)].concat();
    assert_eq!(error(&dicts, &Limits::DEFAULT).kind, SyntaxErrorKind::Depth);

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

    /// The copy of `content::tests::tokens_tile_any_content`.
    #[test]
    #[ignore = "L1: a 0xFF or 0x80 between tokens is white-space to the family, and none to \
                sheaf's lexer, so what's between them isn't blank (`assert!(blank(&s[at..\
                r.start]), …)`; proptest's least case `\\xff\\xd9 `)"]
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

/// The copy of `content::tests::color_spaces_count_their_components`.
#[test]
#[ignore = "L10: `/N2 [/DeviceN [/a /b] /DeviceRGB]` has no tint transform: sheaf counts its \
            names, 2; the family 3, as for a space that doesn't load (`assert_eq!(res, \
            ResCx::new(…))`)"]
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
        (b"N2", 2),
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
