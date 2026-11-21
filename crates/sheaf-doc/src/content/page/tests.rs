//! A source's objects as the PDF layer's crates read them ([`Objects`]):
//! what's lent and what isn't kept, how a stream decodes, why an object
//! or a stream doesn't read and what that stops, and a font read through
//! it.

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

/// pdf-fonts' limits are its defaults with content's as sheaf sets them,
/// and a `/UseCMap` chain's cap is content's depth, as it was when sheaf
/// read the chain itself.
#[test]
fn a_font_s_limits_are_the_family_s_with_sheaf_s_content_caps() {
    let mut lim = Limits::DEFAULT;
    assert_eq!(lim.fonts().cmaps, lim.content.depth);
    lim.content.depth = 7;
    lim.content.operands = 11;
    let fonts = lim.fonts();
    assert_eq!(fonts.content, lim.content.family());
    assert_eq!(fonts.content.depth, 7);
    assert_eq!(fonts.cmaps, 7);
    let family = crate::pdf::fonts::Limits::DEFAULT;
    assert_eq!(
        (fonts.type1_work, fonts.font_fuel, fonts.glyph_fuel),
        (family.type1_work, family.font_fuel, family.glyph_fuel)
    );
    assert_eq!(
        (fonts.composite_glyphs, fonts.composite_depth),
        (family.composite_glyphs, family.composite_depth)
    );
}

/// A CMap's `/UseCMap` chain is read as far as content's depth and no
/// further, whatever a caller sets it to: a chain within it is read
/// whole, and one CMap more leaves the font's codes guessed, as sheaf's
/// own reader left them.
#[test]
fn a_cmap_chain_is_cut_at_content_s_depth() {
    let mut lim = Limits::DEFAULT;
    lim.content.depth = 3;
    let depth = lim.content.depth;
    let guessed = |chain: usize| {
        let mut doc = Document::new();
        // Those it uses take nothing, so what's guessed is only the cut.
        let mut cmap = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        for _ in 2..chain {
            cmap = doc.add_object(Stream::new(dictionary! { "UseCMap" => cmap }, Vec::new()));
        }
        let cmap = doc.add_object(Stream::new(
            dictionary! { "UseCMap" => cmap },
            b"1 begincidrange <00> <FF> 0 endcidrange".to_vec(),
        ));
        let descendant = doc.add_object(dictionary! {
            "Type" => Object::name("Font"),
            "Subtype" => Object::name("CIDFontType2"),
            "BaseFont" => Object::name("Chained"),
        });
        let font = dictionary! {
            "Type" => Object::name("Font"),
            "Subtype" => Object::name("Type0"),
            "BaseFont" => Object::name("Chained"),
            "Encoding" => cmap,
            "DescendantFonts" => vec![Object::from(descendant)],
        };
        let limits = lim.fonts();
        let font = Font::read(Objects(&doc), &font, &limits, &mut (1 << 20), &mut ()).unwrap();
        font.approx().codes
    };
    assert!(!guessed(depth), "a chain of {depth} CMaps is read whole");
    assert!(guessed(depth + 1), "a chain past {depth} CMaps is cut");
}

/// A stream whose data doesn't read (here an encrypted one whose last
/// block doesn't decrypt) is lent as a stream and decodes as locked, its
/// decryption having failed ([`why`]), the same before anything asked
/// for its data as after [`LazyPdf::get`] kept the failure: what a font
/// makes of such a stream doesn't turn on what touched the object first.
#[test]
fn a_stream_whose_data_doesn_t_read_is_lent_the_same_kept_or_not() {
    use crate::pdf::test_crypt::{CipherKind, TestEncryption};

    let mut doc = Document::new();
    let pages = doc.add_object(dictionary! {
        "Type" => Object::name("Pages"),
        "Kids" => Vec::<Object>::new(),
        "Count" => 0i64,
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => Object::name("Catalog"),
        "Pages" => pages,
    });
    doc.trailer.set("Root", catalog);
    let stream = doc.add_object(Stream::new(
        dictionary! { "Marker" => 7i64 },
        PROGRAM.to_vec(),
    ));
    doc.encrypt(&TestEncryption::new(CipherKind::Aes128, "", "owner"))
        .unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();

    // The stream's last byte in the file, which its padding is read from.
    let find = |from: usize, what: &[u8]| {
        from + (bytes[from..].windows(what.len()))
            .position(|w| w == what)
            .expect("in the file")
    };
    let object = find(0, format!("\n{} 0 obj", stream.number).as_bytes());
    let end = find(object, b"endstream");
    let last = (object..end)
        .rev()
        .find(|&at| !bytes[at].is_ascii_whitespace())
        .unwrap();
    let open = |bytes: Vec<u8>| {
        LazyPdf::open(Arc::<[u8]>::from(bytes), Some(""), &Limits::DEFAULT.reader).unwrap()
    };
    // A last byte under which the stream doesn't decrypt.
    let broken = (1..=255u8)
        .map(|flip| {
            let mut bytes = bytes.clone();
            bytes[last] ^= flip;
            bytes
        })
        .find(|bytes| open(bytes.clone()).get(stream).is_err())
        .expect("no last byte fails the stream's padding");

    let lent = |pdf: &LazyPdf| {
        let objects = Objects(pdf);
        (
            objects.resolve(stream).and_then(marker),
            (objects.decoded(stream, 1 << 20))
                .map(|data| data.len())
                .map_err(|e| e.why),
        )
    };
    let fresh = open(broken.clone());
    assert_eq!(lent(&fresh), (Some(7), Err(Why::Locked)));
    assert!(!fresh.is_kept(stream));

    let kept = open(broken);
    assert!(kept.get(stream).is_err());
    assert!(kept.is_kept(stream), "the failure is kept");
    assert_eq!(lent(&kept), (Some(7), Err(Why::Locked)));
}

/// A page interpreted from a source keeps none of its fonts' streams:
/// each font's program, CMap and map of CIDs is read as the font is,
/// through [`Objects`], and the source holds no slot for one afterwards,
/// though every font's program was read and its glyphs laid out.
#[test]
fn a_page_interpreted_keeps_no_font_s_stream() {
    use std::ops::ControlFlow;

    use crate::content::fixture;
    use crate::content::interp::{Kind, Opts, interpret};
    use crate::content::{FontProgram, Fonts};

    let mut built = fixture::programs();
    let page = built.pdf.page_iter().next().expect("one page");
    let content: Vec<ObjectId> = contents(built.page());
    // Every stream of the file but the page's content is a font's.
    let streams: Vec<ObjectId> = (built.pdf.objects.iter())
        .filter(|(id, object)| object.as_stream().is_some() && !content.contains(id))
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(streams.len(), 3, "a TrueType, a CFF and a Type 1 program");

    let lim = &Limits::DEFAULT;
    let pdf = LazyPdf::open(Arc::<[u8]>::from(built.bytes()), None, &lim.reader).unwrap();
    let dict = pdf.get_dictionary(page).expect("the page");
    let resources = dict.get(b"Resources");
    let read = PageContent::read(&pdf, &contents(dict), lim).expect("its content");

    let (mut glyphs, mut programs) = (0, 0);
    let fonts = Fonts::default();
    let done = interpret(
        &pdf,
        read.concat(),
        resources,
        lim,
        &fonts,
        Opts::default(),
        |event| {
            if let Kind::Text(show) = event.kind {
                glyphs += show.glyphs().count();
                let read = show.font.map(|font| font.program());
                programs += usize::from(read == Some(FontProgram::Read));
            }
            ControlFlow::Continue(())
        },
    );
    done.expect("the page is interpreted");
    assert!(glyphs > 20, "{glyphs} glyphs");
    assert!(
        programs >= 6,
        "{programs} strings in a font whose program read"
    );
    for id in &streams {
        assert!(!pdf.is_kept(*id), "{id:?}, a font's stream, was kept");
    }
    // One asked for as an object is kept, so the check above can see one.
    pdf.get(streams[0]).expect("a program");
    assert!(pdf.is_kept(streams[0]));
}

/// A file with `objects` (each written as it's given, numbered from 1)
/// and a catalog, as a source reads it, each dictionary that has the key
/// `broken` made what no parser reads, its length kept.
fn source(objects: &[Object], broken: &[u8]) -> LazyPdf {
    let mut doc = Document::new();
    for object in objects {
        doc.add_object(object.clone());
    }
    let pages = doc.add_object(dictionary! {
        "Type" => Object::name("Pages"),
        "Kids" => Vec::<Object>::new(),
        "Count" => 0i64,
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => Object::name("Catalog"),
        "Pages" => pages,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let key = [b"/".as_slice(), broken].concat();
    let mut from = 0;
    while let Some(at) = (bytes[from..].windows(key.len())).position(|w| w == key) {
        let at = from + at;
        let open = (bytes[..at].windows(2))
            .rposition(|w| w == b"<<")
            .expect("its dictionary");
        bytes[open..open + 2].copy_from_slice(b"]]");
        from = at + key.len();
    }
    LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap()
}

/// An object that doesn't read keeps its reason: one that isn't there is
/// none, one that doesn't parse is damaged, and a chain of references
/// past the reader's depth is capped, while a chain within it is
/// followed; what lends an object (`peek`) says the same as what the PDF
/// layer's crates are told. A stream decodes past its limit as
/// [`Why::Limit`], a filter that fails as damaged, what isn't a stream as
/// damaged, and the chain as capped.
#[test]
fn a_source_s_failure_keeps_its_reason() {
    let depth = Limits::DEFAULT.reader.max_depth;
    // 1: a dictionary, 2: one to break, 3..: a chain past the depth,
    // then two streams.
    let mut objects = vec![
        Object::from(dictionary! { "Marker" => 7i64 }),
        Object::from(dictionary! { "Broken" => 1i64 }),
    ];
    let chain = depth + 2;
    for n in 0..chain {
        objects.push(Object::from(ObjectId::new(4 + n as u32, 0)));
    }
    objects.push(Object::from(dictionary! { "End" => 1i64 }));
    let plain = ObjectId::new(objects.len() as u32 + 1, 0);
    objects.push(Object::from(Stream::new(
        dictionary! {},
        b"0123456789".to_vec(),
    )));
    let filtered = ObjectId::new(objects.len() as u32 + 1, 0);
    let mut bad = Stream::new(dictionary! {}, b"not flate".to_vec());
    bad.dict.set("Filter", Object::name("FlateDecode"));
    objects.push(Object::from(bad));
    let pdf = source(&objects, b"Broken");
    let objects = Objects(&pdf);
    let reason = |id: ObjectId| {
        let told = (objects.try_resolve(id))
            .map(|o| o.is_some())
            .map_err(|e| e.why);
        let lent = (pdf.peek(id)).map(|o| o.is_some()).map_err(|e| why(&e));
        assert_eq!(told, lent, "{id:?}");
        told
    };

    assert_eq!(reason(id(1)), Ok(true));
    assert_eq!(reason(id(9999)), Ok(false));
    assert_eq!(reason(id(2)), Err(Why::Damaged));
    assert_eq!(reason(id(3)), Err(Why::Capped));
    assert!(matches!(
        pdf.peek(id(3)),
        Err(crate::pdf::Error::ReferenceLimit)
    ));
    // Within the depth, the chain is followed.
    assert_eq!(reason(id(3 + chain as u32 - 2)), Ok(true));
    assert_eq!(objects.resolve(id(3)), None);
    assert_eq!(objects.resolve(id(2)), None);

    let decoded = |id, limit| {
        (objects.decoded(id, limit))
            .map(|d| d.len())
            .map_err(|e| e.why)
    };
    assert_eq!(decoded(plain, 100), Ok(10));
    assert_eq!(decoded(plain, 4), Err(Why::Limit));
    assert_eq!(decoded(filtered, 100), Err(Why::Damaged));
    assert_eq!(decoded(id(1), 100), Err(Why::Damaged));
    assert_eq!(decoded(id(3), 100), Err(Why::Capped));

    // A document in memory: what isn't there is none.
    let doc = Document::new();
    assert_eq!(
        Objects(&doc).try_resolve(id(1)).map(|o| o.is_some()),
        Ok(false)
    );
}

/// Each reason a source's error gives is the one [`why`] says: its I/O
/// the source's, a password or decryption that fails locked, the
/// reader's caps capped, and anything else the file's damage.
#[test]
fn each_source_error_is_its_reason() {
    use crate::pdf::{DecryptionError, Error as E, Limit};
    let io = || E::IO(std::io::Error::other("gone"));
    let cases = [
        (io(), Why::Source),
        (E::InvalidPassword, Why::Locked),
        (E::Decryption(DecryptionError::Padding), Why::Locked),
        (E::Limit(Limit::Depth), Why::Capped),
        (E::Limit(Limit::Decompressed), Why::Capped),
        (E::ReferenceLimit, Why::Capped),
        (E::MissingXrefEntry, Why::Damaged),
        (E::ObjectIdMismatch, Why::Damaged),
        (E::IndirectObject { offset: 3 }, Why::Damaged),
        (E::InvalidStream("bad".into()), Why::Damaged),
        (E::Syntax("bad".into()), Why::Damaged),
    ];
    for (error, reason) in cases {
        assert_eq!(why(&error), reason, "{error}");
    }
}

/// What an object that doesn't read stops is its reason's, through
/// [`interpret`](crate::content::interp::interpret): an ExtGState that
/// doesn't parse fails a page drawn with its forms, as damaged, and is
/// none where forms are judged on their own (a text bag among them); one
/// behind a chain past the reader's depth fails either, as capped.
#[test]
fn what_an_object_that_doesn_t_read_stops_is_its_reason_s() {
    use std::ops::ControlFlow;

    use crate::content::interp::{Error, Forms, Opts, interpret};
    use crate::content::{Fonts, text_bag};

    let depth = Limits::DEFAULT.reader.max_depth;
    let mut objects = vec![Object::from(dictionary! { "Broken" => 1i64, "LW" => 3i64 })];
    let chain = depth + 2;
    for n in 0..chain {
        objects.push(Object::from(ObjectId::new(3 + n as u32, 0)));
    }
    objects.push(Object::from(dictionary! { "LW" => 5i64 }));
    let pdf = source(&objects, b"Broken");
    let resources = Object::from(dictionary! {
        "ExtGState" => dictionary! {
            "Bad" => id(1),
            "Deep" => id(2),
        },
    });
    let (lim, fonts) = (Limits::DEFAULT, Fonts::for_text());
    let run = |content: &[u8], forms| {
        let opts = Opts {
            forms,
            ..Opts::default()
        };
        let content = Concat::new(&[content]);
        let resources = Some(&resources);
        interpret(&pdf, &content, resources, &lim, &fonts, opts, |_| {
            ControlFlow::Continue(())
        })
    };
    let failed = |read: Result<_, Error>| match read {
        Err(Error::Unreadable(u)) => Some((u.id, u.why)),
        _ => None,
    };

    let bad = b"/Bad gs 0 0 m 1 1 l S";
    assert_eq!(
        failed(run(bad, Forms::Descend)),
        Some((id(1), Why::Damaged))
    );
    assert!(run(bad, Forms::Skip).is_ok());
    let bag = text_bag(&pdf, &Concat::new(&[bad]), Some(&resources), &lim, &fonts);
    assert!(bag.is_ok());

    let deep = b"/Deep gs";
    for forms in [Forms::Descend, Forms::Skip] {
        let why = failed(run(deep, forms)).map(|(_, why)| why);
        assert_eq!(why, Some(Why::Capped), "{forms:?}");
    }
    let bag = text_bag(&pdf, &Concat::new(&[deep]), Some(&resources), &lim, &fonts);
    assert!(matches!(bag, Err(Error::Unreadable(u)) if u.why == Why::Capped));
}
