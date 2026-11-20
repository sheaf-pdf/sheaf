//! A source's objects as the PDF layer's crates read them ([`Objects`]):
//! what's lent and what isn't kept, how a stream decodes and why it
//! doesn't, and a font read through it.

use std::sync::Arc;

use super::*;
use crate::pdf::fonts::Font;
use crate::pdf::{Document, deflate, dictionary};

/// What stream 3 decodes to.
const PROGRAM: &[u8] = b"a font's program, which a reader mustn't hold once the font is read";

/// Font 8's `/ToUnicode`, stream 9: code 0x41 reads as "sheaf".
const TO_UNICODE: &[u8] = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<00> <FF>
endcodespacerange
1 beginbfchar
<41> <00730068006500610066>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end
";

fn id(number: u32) -> ObjectId {
    ObjectId::new(number, 0)
}

/// A file of nine objects, by its bytes:
///
/// - 3: a deflated stream, [`PROGRAM`], its dictionary marked `/Marker 7`;
/// - 4: a reference to 3, and 5: a reference to 4;
/// - 6: a stream whose filter no reader has;
/// - 7: a dictionary;
/// - 8: a Type 1 font whose `/ToUnicode` is stream 9, deflated.
fn file() -> Vec<u8> {
    let stream = |dict: &str, data: &[u8]| {
        let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\nendstream");
        body
    };
    let objects: [Vec<u8>; 9] = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [] /Count 0 >>".to_vec(),
        stream("/Filter /FlateDecode /Marker 7", &deflate(PROGRAM)),
        b"3 0 R".to_vec(),
        b"4 0 R".to_vec(),
        stream("/Filter /NoSuchDecode", b"whatever"),
        b"<< /Plain true >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 9 0 R >>".to_vec(),
        stream("/Filter /FlateDecode", &deflate(TO_UNICODE)),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn lazy() -> LazyPdf {
    LazyPdf::open(Arc::<[u8]>::from(file()), None, &Limits::DEFAULT.reader).unwrap()
}

fn marker(object: &Object) -> Option<i64> {
    object.as_stream()?.dict.get(b"Marker")?.as_integer()
}

/// A source lends a stream's dictionary and keeps nothing of the stream;
/// its data comes decoded, as sheaf decodes it, and still nothing is
/// kept.
#[test]
fn a_stream_s_dictionary_is_lent_and_its_data_isn_t_kept() {
    let pdf = lazy();
    let objects = Objects(&pdf);

    let lent = objects.resolve(id(3)).expect("stream 3");
    assert_eq!(marker(lent), Some(7));
    assert!(!pdf.is_kept(id(3)), "peeking kept the stream");

    let decoded = objects.decoded(id(3), 1 << 20).unwrap();
    assert_eq!(&*decoded, PROGRAM);
    let sheaf_s = pdf
        .object_unkept(id(3))
        .unwrap()
        .as_stream()
        .unwrap()
        .decompressed_content_with_limit(1 << 20)
        .unwrap();
    assert_eq!(&*decoded, sheaf_s);
    assert!(!pdf.is_kept(id(3)), "decoding kept the stream");

    // The same object every time it's asked for.
    assert!(std::ptr::eq(lent, objects.resolve(id(3)).unwrap()));
}

/// What isn't a stream is read and kept as ever, and a stream already
/// kept is lent as kept, with its data.
#[test]
fn what_s_kept_is_lent_as_kept() {
    let pdf = lazy();
    let objects = Objects(&pdf);

    let plain = objects.resolve(id(7)).expect("dictionary 7");
    assert!(plain.as_dict().is_some_and(|d| d.has(b"Plain")));
    assert!(pdf.is_kept(id(7)));
    assert!(std::ptr::eq(plain, pdf.get_object(id(7)).unwrap()));

    let kept = pdf.get_object(id(3)).unwrap();
    assert!(pdf.is_kept(id(3)));
    let lent = objects.resolve(id(3)).unwrap();
    assert!(std::ptr::eq(kept, lent));
    assert_eq!(&*objects.decoded(id(3), 1 << 20).unwrap(), PROGRAM);
}

/// A reference to a reference is followed, as sheaf follows it, by what
/// lends an object and by what decodes one; in a source and in a document
/// in memory.
#[test]
fn a_chain_of_references_is_followed() {
    let pdf = lazy();
    let objects = Objects(&pdf);
    for chained in [4, 5] {
        assert_eq!(marker(objects.resolve(id(chained)).unwrap()), Some(7));
        assert_eq!(&*objects.decoded(id(chained), 1 << 20).unwrap(), PROGRAM);
    }
    assert!(!pdf.is_kept(id(3)));

    let mut doc = Document::new();
    let stream = doc.add_object(Stream::new(
        dictionary! { "Marker" => 7i64 },
        PROGRAM.to_vec(),
    ));
    let first = doc.add_object(Object::from(stream));
    let second = doc.add_object(Object::from(first));
    let objects = Objects(&doc);
    for at in [stream, first, second] {
        assert_eq!(marker(objects.resolve(at).unwrap()), Some(7));
        assert_eq!(&*objects.decoded(at, 1 << 20).unwrap(), PROGRAM);
    }
}

/// A stream past the limit is [`Why::Limit`]; one with no data to give
/// (a filter nothing decodes, an object that isn't a stream, one that
/// isn't there) is [`Why::Damaged`], whatever sheaf's reader called it.
#[test]
fn why_a_stream_doesn_t_decode() {
    let pdf = lazy();
    let objects = Objects(&pdf);
    let why = |at: u32, limit: usize| objects.decoded(id(at), limit).map_err(|e| (e.id, e.why));

    assert_eq!(why(3, PROGRAM.len()).as_deref(), Ok(PROGRAM));
    assert_eq!(why(3, PROGRAM.len() - 1), Err((id(3), Why::Limit)));
    assert_eq!(why(6, 1 << 20), Err((id(6), Why::Damaged)));
    assert_eq!(why(7, 1 << 20), Err((id(7), Why::Damaged)));
    assert_eq!(why(99, 1 << 20), Err((id(99), Why::Damaged)));

    assert!(objects.resolve(id(99)).is_none());
    // The stream nothing decodes is still lent: its dictionary reads.
    assert!(
        objects
            .resolve(id(6))
            .is_some_and(|o| o.as_stream().is_some())
    );

    let doc = Document::new();
    let objects = Objects(&doc);
    assert!(objects.resolve(id(1)).is_none());
    assert_eq!(
        objects.decoded(id(1), 1 << 20).map_err(|e| e.why),
        Err(Why::Damaged)
    );
}

/// A font read through a source: its `/ToUnicode` is decoded and read,
/// charged to the budget, and the reader holds neither stream afterwards.
#[test]
fn a_font_is_read_through_a_source_that_keeps_none_of_its_streams() {
    let pdf = lazy();
    let objects = Objects(&pdf);
    let dict = objects.resolve(id(8)).and_then(Object::as_dict).unwrap();
    let limits = Limits::DEFAULT.fonts();
    let mut budget = 1 << 20;
    let font = Font::read(objects, dict, &limits, &mut budget, &mut ()).unwrap();
    assert_eq!(font.code_text(0x41).as_deref(), Some("sheaf"));
    assert_eq!(font.text(b"AA").as_deref(), Some("sheafsheaf"));
    assert_eq!(budget, (1 << 20) - TO_UNICODE.len());
    assert!(!pdf.is_kept(id(9)), "reading the font kept its /ToUnicode");

    // Past its budget, the font is too large: the limit is told as one.
    let mut budget = TO_UNICODE.len() - 1;
    let too_large = Font::read(objects, dict, &limits, &mut budget, &mut ());
    assert!(matches!(too_large, Err(crate::pdf::fonts::Error::TooLarge)));
}

/// pdf-fonts' limits are its defaults with content's as sheaf sets them.
#[test]
fn a_font_s_limits_are_the_family_s_with_sheaf_s_content_caps() {
    let mut lim = Limits::DEFAULT;
    lim.content.depth = 7;
    lim.content.operands = 11;
    let fonts = lim.fonts();
    assert_eq!(fonts.content, lim.content.family());
    assert_eq!(fonts.content.depth, 7);
    let family = crate::pdf::fonts::Limits::DEFAULT;
    assert_eq!(
        (
            fonts.cmaps,
            fonts.type1_work,
            fonts.font_fuel,
            fonts.glyph_fuel
        ),
        (
            family.cmaps,
            family.type1_work,
            family.font_fuel,
            family.glyph_fuel
        )
    );
    assert_eq!(
        (fonts.composite_glyphs, fonts.composite_depth),
        (family.composite_glyphs, family.composite_depth)
    );
}
