//! sheaf's use of pdf-content-streams' tokens: its caps, as sheaf's
//! limits map to the family's ([`ContentLimits::family`]), color spaces
//! counted from sheaf's sources ([`res_cx`]), and a page's streams read
//! and read as one, with what stops their tokens said in the stream it's
//! in. How content reads is the family's, held there to PDFium.

use std::ops::Range;
use std::sync::Arc;

use super::*;
use crate::pdf::{Document as Pdf, LazyPdf, Object, ObjectId, Stream, dictionary, literal};
use crate::source::{ContentLimits, Limits};

const LIM: &Limits = &Limits::DEFAULT;

fn toks_of<'a>(s: &'a [u8], res: &ResCx, lim: &Limits) -> Vec<(Range<usize>, Tok<'a>)> {
    tokens(s, res, &lim.content.family())
        .map(Result::unwrap)
        .collect()
}

fn error(s: &[u8], lim: &Limits) -> SyntaxError {
    tokens(s, &ResCx::none(), &lim.content.family())
        .find_map(Result::err)
        .expect("an error")
}

/// Whether `gap` is only white-space and comments, as the family reads
/// them: ISO 32000-2's six bytes of white-space, and 0x80 and 0xFF, as
/// PDFium reads them; a comment to the end of its line.
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

/// The caps sheaf sets reach the reading, as its limits map to the
/// family's: the operands before an operator, and an inline image's data.
#[test]
fn the_caps_hold() {
    let capped = |content: ContentLimits| Limits {
        content,
        ..Limits::DEFAULT
    };
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

/// Color spaces count their components, read from sheaf's sources
/// ([`res_cx`]): a direct space, one behind a reference, and what's no
/// space, each as the family counts it.
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
