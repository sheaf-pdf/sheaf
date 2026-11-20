//! Marked content, balance, names and blocks: ranges found across streams
//! and their lists, the cuts that take an artifact out and the keys a list
//! loses, as pdf-content-streams' splice makes them, and pages classified
//! by who drew their pagination artifacts.

use std::sync::Arc;

use crate::pdf::{
    Dictionary, Document as Pdf, Object, ObjectId, Stream, StreamExt, dictionary, literal,
};

use super::*;
use crate::source::Limits;

const LIM: &Limits = &Limits::DEFAULT;

fn found(streams: &[&[u8]], props: &PropertiesDict) -> Result<Vec<Marked>, LexError> {
    marked(&Concat::new(streams), props, &ResCx::none(), LIM)
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
    let props = PropertiesDict::read(&pdf, Some(&resources));
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
            Some(dictionary! { "Type" => Object::name("OCG") })
        ))
    );
    assert_eq!(m[2].props, None);
    assert_eq!(
        m[3].props.as_ref().and_then(Props::dict),
        Some(&dictionary! { "Subtype" => Object::name("Header") })
    );
    assert_eq!(m[4].props, Some(Props::Named(b"None".to_vec(), None)));
}

#[test]
fn marked_content_must_balance() {
    let props = PropertiesDict::default();
    let kind = |s: &[u8]| found(&[s], &props).unwrap_err();
    assert_eq!(
        kind(b"q /A BMC Q EMC EMC"),
        LexError {
            at: 15,
            kind: LexErrorKind::Emc
        }
    );
    assert_eq!(
        kind(b"/A BMC /B BMC EMC"),
        LexError {
            at: 0,
            kind: LexErrorKind::Marked
        }
    );
    let deep = Limits {
        content: crate::source::ContentLimits {
            depth: 2,
            ..LIM.content
        },
        ..*LIM
    };
    let e = marked(
        &Concat::new(&[b"/A BMC /B BMC /C BMC EMC EMC EMC"]),
        &props,
        &ResCx::none(),
        &deep,
    )
    .unwrap_err();
    assert_eq!(e.kind, LexErrorKind::Depth);
    // Marked content inside a string or an inline image isn't.
    assert!(found(&[b"(EMC) Tj BI /W 3 /H 1 /CS /G /BPC 8 ID EMC EI"], &props).is_ok());
}

#[test]
fn balance_is_the_q_count() {
    let of = |s: &[u8]| balance(&Concat::new(&[s]), &ResCx::none(), LIM).unwrap();
    // §3.4's worked bases.
    let b = |net, min_prefix| Balance { net, min_prefix };
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
    assert_eq!(balance(&c, &ResCx::none(), LIM).unwrap(), b(-1, -1));
}

/// The cuts that take an artifact out are the splice's to make
/// (pdf-content-streams', whose own tests hold its rules). Acrobat's
/// first artifact on a page opens with the `Q` that ends the page's `q`:
/// alone the splice refuses it, and its cuts, the start and what's after
/// that `Q`, it takes together, the `Q` kept.
#[test]
fn an_artifacts_cuts_keep_the_q_it_opens_with() {
    let (res, lim) = (ResCx::none(), LIM.content.family());
    let s: &[u8] = b"q 0 g /Artifact <</Subtype /Header>> BDC Q q 1 0 0 1 9 9 cm /X0 Do Q EMC 1 g";
    let c = Concat::new(&[s]);
    let header = marked(&c, &PropertiesDict::default(), &res, LIM).unwrap()[0].span;
    // Alone, it would end the page's `q`.
    assert!(matches!(
        splice(&c, res.family(), &lim, &[header]),
        Err(SpliceError::Unsafe(_))
    ));
    let pagination = Pagination {
        kind: PaginationKind::Header,
        origin: PagOrigin::Acrobat,
        generator: None,
        span: header,
    };
    let cuts = pagination.cuts(&c, &res, LIM).unwrap();
    assert_eq!(
        cuts,
        [
            span_of(s, b"/Artifact <</Subtype /Header>> BDC "),
            span_of(s, b" q 1 0 0 1 9 9 cm /X0 Do Q EMC")
        ]
    );
    let kept = splice(&c, res.family(), &lim, &cuts).unwrap();
    assert_eq!(kept[0], &b"q 0 g Q 1 g"[..]);
    // A range with no `Q` to keep is one cut.
    let plain = Pagination {
        span: span_of(s, b"/X0 Do"),
        ..pagination
    };
    assert_eq!(plain.cuts(&c, &res, LIM).unwrap(), [plain.span]);
}

/// A list loses the keys sheaf marks it with ([`private::LIST_KEYS`])
/// and keeps every other byte as it was written: comments, spacing, line
/// ends, a key written with an escape, and a dictionary inside it, whose
/// own keys are its own.
///
/// [`private::LIST_KEYS`]: crate::private::LIST_KEYS
#[test]
fn a_list_loses_sheafs_private_keys() {
    let (res, lim) = (ResCx::none(), LIM.content.family());
    // `s` with the list of the range `part` written again without them.
    let made = |s: &[u8], part: &[u8]| {
        let c = Concat::new(&[s]);
        let keys = crate::private::LIST_KEYS;
        let rewrite = without_keys(&c, res.family(), &lim, span_of(s, part), &keys);
        let rewrite = rewrite.unwrap().unwrap();
        let streams = Spliced::default().streams(&c, res.family(), &lim, &[rewrite]);
        streams.unwrap().unwrap().remove(0).into_owned()
    };
    // With no space between entries.
    let s: &[u8] = b"/Artifact <</Type/Pagination/Subtype/Watermark/SHEF_Gen true\
        /SHEF_OcrBlock({\"path\":\"/home/me/logo.png\"})>> BDC\nq\n/GS0 gs Q\nEMC\n";
    assert_eq!(
        made(s, s),
        b"/Artifact <</Type/Pagination/Subtype/Watermark>> BDC\nq\n/GS0 gs Q\nEMC\n"
    );
    let s: &[u8] = b"q /Artifact << /Type /Pagination % its kind\n  /Subtype /Watermark\t\
        /SHEF_Gen true /Inner <</SHEF_Gen 1>> % a note\n /SHEF#5FOcrBlock (x)\n>> BDC Q EMC";
    assert_eq!(
        made(s, &s[2..]),
        &b"q /Artifact << /Type /Pagination % its kind\n  /Subtype /Watermark \
            /Inner <</SHEF_Gen 1>> % a note\n\n>> BDC Q EMC"[..]
    );
}

#[test]
fn names_used_are_by_category() {
    let s: &[u8] = b"/GS0 gs /F1 9 Tf /CS0 cs /DeviceRGB CS /P0 scn 0.5 /P1 SCN /Im1 Do \
        /Sh0 sh /OC /oc0 BDC EMC /X /Named DP /Y <<>> BDC EMC \
        BI /W 1 /H 1 /CS /Idx /BPC 8 ID x EI BI /W 1 /H 1 /CS /RGB /BPC 8 ID xyz EI \
        /Artifact BMC /F9 1 Tf EMC";
    let c = Concat::new(&[s]);
    let except = [span_of(s, b"/Artifact BMC /F9 1 Tf EMC")];
    let names = names_used(&c, &ResCx::none(), LIM, &except).unwrap();
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

/// A page whose `/Contents` is `streams`, each with its dictionary, drawn
/// with `resources`: its blocks.
fn blocks_of(
    pdf: &mut Pdf,
    streams: &[(Dictionary, &[u8])],
    resources: Dictionary,
) -> (PageBlocks, Vec<ObjectId>) {
    let ids: Vec<ObjectId> = streams
        .iter()
        .map(|(dict, data)| pdf.add_object(Stream::new(dict.clone(), data.to_vec())))
        .collect();
    let resources = Object::from(resources);
    let blocks =
        PageBlocks::read(&*pdf, &ids, Some(&resources), LIM, &DrawnForms::default()).unwrap();
    (blocks, ids)
}

fn origins(blocks: &PageBlocks) -> Vec<(PaginationKind, PagOrigin)> {
    blocks
        .pagination
        .iter()
        .map(|p| (p.kind, p.origin))
        .collect()
}

#[test]
fn sheafs_blocks_are_found_by_their_markers() {
    let mut pdf = Pdf::with_version("1.7");
    let gen_list = dictionary! {
        "Type" => Object::name("Pagination"),
        "Subtype" => Object::name("Header"),
        "SHEF_Gen" => literal("g1f"),
    };
    let resources = dictionary! { "Properties" => dictionary! {
        "ShP0" => gen_list,
        "ShP1" => dictionary! { "SHEF_OcrBlock" => literal("b2a") },
    } };
    let block = |k: &str, key: &str, id: &str| {
        dictionary! { "SHEF_Block" => dictionary! { "K" => Object::name(k), key => literal(id) } }
    };
    let gen_stream: &[u8] = b"/Artifact /ShP0 BDC (Head) Tj EMC";
    let (blocks, ids) = blocks_of(
        &mut pdf,
        &[
            (
                dictionary! { "SHEF_Block" => dictionary! { "K" => Object::name("Isolate") } },
                b"q q",
            ),
            (
                dictionary! {},
                b"0 g /Artifact /ShP0 BDC (merged) Tj EMC /ShOCR /ShP1 BDC EMC",
            ),
            (
                dictionary! { "SHEF_Block" => dictionary! { "K" => Object::name("Isolate") } },
                b"Q Q",
            ),
            (block("Gen", "G", "g1f"), gen_stream),
            (block("Ocr", "B", "b2b"), b""),
            (block("Flat", "B", "b2c"), b"q /Fm0 Do Q"),
        ],
        resources,
    );
    use PaginationKind::Header;
    assert_eq!(
        origins(&blocks),
        [(Header, PagOrigin::Sheaf), (Header, PagOrigin::Sheaf)]
    );
    let sheaf: Vec<_> = blocks.sheaf.iter().map(|b| (b.block, b.at)).collect();
    assert_eq!(
        sheaf,
        [
            (SheafBlock::Gen(GenId(0x1f)), BlockAt::Stream(ids[3])),
            (SheafBlock::Ocr(BlockId(0x2b)), BlockAt::Stream(ids[4])),
            (SheafBlock::Flat(BlockId(0x2c)), BlockAt::Stream(ids[5])),
            (
                SheafBlock::Gen(GenId(0x1f)),
                BlockAt::Marked(span((1, 4), (1, 39)))
            ),
            (
                SheafBlock::Ocr(BlockId(0x2a)),
                BlockAt::Marked(span((1, 40), (1, 60)))
            ),
        ]
    );
    assert_eq!(&*blocks.isolation, &[ids[0], ids[2]]);
    // A stream's dictionary alone says what it is.
    let marker = |id: ObjectId| Marker::of(&pdf.get_object(id).unwrap().as_stream().unwrap().dict);
    let header = Marker::Gen {
        id: GenId(0x1f),
        layout: None,
        recipe: None,
        number: None,
    };
    assert_eq!(
        ids.iter().map(|&id| marker(id)).collect::<Vec<_>>(),
        [
            Some(Marker::Isolate),
            None,
            Some(Marker::Isolate),
            Some(header),
            Some(Marker::Ocr(BlockId(0x2b))),
            Some(Marker::Flat(BlockId(0x2c))),
        ]
    );
    assert_eq!(
        blocks.balance,
        Balance {
            net: 0,
            min_prefix: 0
        }
    );
    // Ids written otherwise are no ids.
    for bad in [&b"g"[..], b"b1", b"gx1", b"g12345678901234567"] {
        assert_eq!(GenId::parse(bad), None);
    }
    assert_eq!(BlockId::parse(b"bFF"), Some(BlockId(255)));
}

/// A block's id as sheaf writes it reads back as itself, the largest it
/// draws too; one drawn is in [1, 2^53) and none `taken` says is taken.
#[test]
fn a_block_id_round_trips() {
    let mut rng = fastrand::Rng::with_seed(7);
    let first = BlockId::draw(&mut rng, |_| false);
    let mut rng = fastrand::Rng::with_seed(7);
    let other = BlockId::draw(&mut rng, |id| id == first);
    assert_ne!(other, first);
    for id in [
        BlockId(1),
        BlockId(0x2a),
        BlockId((1 << 53) - 1),
        first,
        other,
    ] {
        assert!((1..1 << 53).contains(&id.0), "{id}");
        assert_eq!(BlockId::parse(id.to_string().as_bytes()), Some(id));
    }
    assert_eq!(BlockId(0x2a).to_string(), "b2a");
}

/// A form of Acrobat's, its `/PieceInfo` naming `private`, and resources
/// that name it `/Xi0`, and a form of no one's `/Fm0` holding `inner`.
fn acrobat_resources(pdf: &mut Pdf, private: &str, inner: &[u8]) -> Dictionary {
    let piece = pdf.add_object(dictionary! {
        "ADBE_CompoundType" => dictionary! {
            "DocSettings" => dictionary! {},
            "Private" => Object::name(private),
        },
    });
    let form = |pdf: &mut Pdf, extra: Dictionary, content: &[u8]| {
        let mut dict = dictionary! {
            "Type" => Object::name("XObject"),
            "Subtype" => Object::name("Form"),
            "BBox" => vec![0.into(), 0.into(), 9.into(), 9.into()],
        };
        for (k, v) in &extra {
            dict.set(k, v.clone());
        }
        pdf.add_object(Stream::new(dict, content.to_vec()))
    };
    let xi0 = form(
        pdf,
        dictionary! { "PieceInfo" => piece },
        b"BT (Head) Tj ET",
    );
    let fm0 = form(pdf, dictionary! {}, inner);
    dictionary! { "XObject" => dictionary! { "Xi0" => xi0, "Fm0" => fm0 } }
}

#[test]
fn acrobats_draw_its_forms_only() {
    const HEADER: &str = "/Artifact <</Type /Pagination /Subtype /Header>> BDC";
    let page =
        |inside: &str| format!("{HEADER} q 0 0 612 792 re W n /Xi0 Do Q {inside} EMC").into_bytes();
    let mut pdf = Pdf::with_version("1.7");
    let resources = acrobat_resources(&mut pdf, "Header", b"");
    let classify = |pdf: &mut Pdf, content: &[u8], resources: &Dictionary| {
        let (blocks, _) = blocks_of(pdf, &[(dictionary! {}, content)], resources.clone());
        origins(&blocks)
    };
    use PagOrigin::{Acrobat, Authored};
    use PaginationKind::Header;
    assert_eq!(
        classify(&mut pdf, &page(""), &resources),
        [(Header, Acrobat)]
    );
    // Drawing anything else, or a form of no one's, is an author's.
    for inside in ["(x) Tj", "0 0 m 9 9 l S", "/Fm0 Do"] {
        assert_eq!(
            classify(&mut pdf, &page(inside), &resources),
            [(Header, Authored)],
            "{inside}"
        );
    }
    assert_eq!(
        classify(
            &mut pdf,
            format!("{HEADER} BT (Head) Tj ET EMC").as_bytes(),
            &resources
        ),
        [(Header, Authored)]
    );
    // A `/PieceInfo` that names something else isn't Acrobat's pagination.
    let other = acrobat_resources(&mut pdf, "Stamp", b"");
    assert_eq!(classify(&mut pdf, &page(""), &other), [(Header, Authored)]);
}

#[test]
fn artifacts_in_forms_are_reported_left_in_place() {
    let inner = b"/Artifact <</Type /Pagination /Subtype /Footer>> BDC (f) Tj EMC";
    let mut pdf = Pdf::with_version("1.7");
    let resources = acrobat_resources(&mut pdf, "Header", inner);
    let nested = |pdf: &mut Pdf, content: &[u8]| {
        blocks_of(pdf, &[(dictionary! {}, content)], resources.clone())
            .0
            .nested_in_forms
    };
    assert!(nested(&mut pdf, b"q /Fm0 Do Q"));
    // Drawn inside a pagination artifact, it goes with it.
    assert!(!nested(
        &mut pdf,
        b"/Artifact <</Subtype /Header>> BDC /Fm0 Do EMC"
    ));
    assert!(!nested(&mut pdf, b"/Xi0 Do /Absent Do"));
}

#[test]
fn a_pages_blocks_are_read_once_and_name_the_page() {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let good = pdf.add_object(Stream::new(
        dictionary! {},
        b"/Artifact <</Subtype /Header>> BDC (h) Tj EMC".to_vec(),
    ));
    let unended = pdf.add_object(Stream::new(dictionary! {}, b"/A BMC".to_vec()));
    let kids: Vec<Object> = [good, unended]
        .map(|c| {
            pdf.add_object(dictionary! {
                "Type" => Object::name("Page"),
                "Parent" => pages,
                "Contents" => c,
            })
            .into()
        })
        .to_vec();
    pdf.objects.insert(
        pages,
        dictionary! {
            "Type" => Object::name("Pages"),
            "Kids" => kids,
            "Count" => 2,
            "MediaBox" => vec![0.into(), 0.into(), 9.into(), 9.into()],
        }
        .into(),
    );
    let catalog = pdf.add_object(dictionary! {
        "Type" => Object::name("Catalog"),
        "Pages" => pages,
    });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let src = crate::forms::fixture::source(bytes);
    // Never at open; once read, kept, and the streams aren't.
    assert!(!src.blocks_known(0));
    let blocks = src.blocks(0).unwrap();
    assert!(src.blocks_known(0) && !src.blocks_known(1));
    assert!(blocks.has(&[PaginationKind::Header]));
    assert!(!src.pdf().is_read(good));
    assert!(Arc::ptr_eq(&blocks, &src.blocks(0).unwrap()));
    let e = src.blocks(1).unwrap_err();
    assert_eq!(
        e.to_string(),
        "page 2's content can't be read; nothing was changed"
    );
    assert_eq!(e.on_page(6).page(), 6);
    assert_eq!(*src.blocks(9).unwrap(), PageBlocks::default());
}

/// A form is read once, whatever names and pages draw it: 2,000 names of
/// one 8 MiB form on 50 pages. Past the page's cap, or once what the
/// page's streams leave of it is spent, a form may hold an artifact, and
/// counts as holding one.
#[test]
fn forms_are_read_once_and_within_the_pages_cap() {
    let mut pdf = Pdf::with_version("1.7");
    let form = |pdf: &mut Pdf, content: Vec<u8>| {
        let dict = dictionary! {
            "Type" => Object::name("XObject"),
            "Subtype" => Object::name("Form"),
            "Resources" => dictionary! {},
            "BBox" => vec![0.into(), 0.into(), 9.into(), 9.into()],
        };
        let mut stream = Stream::new(dict, content);
        stream.compress();
        pdf.add_object(stream)
    };
    let big = form(&mut pdf, b"0 0 m 9 9 l S\n".repeat(600_000));
    let mut names = Dictionary::new();
    let mut draws = Vec::new();
    for i in 0..2000 {
        names.set(format!("X{i}"), big);
        draws.extend(format!("/X{i} Do ").bytes());
    }
    let page = pdf.add_object(Stream::new(dictionary! {}, draws.clone()));
    let resources = Object::from(dictionary! { "XObject" => names });
    let known = DrawnForms::default();
    let started = std::time::Instant::now();
    for _ in 0..50 {
        let blocks = PageBlocks::read(&pdf, &[page], Some(&resources), LIM, &known).unwrap();
        assert!(!blocks.nested_in_forms);
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(known.nested_known(big), Some(false));

    // Past the cap.
    let cap = |decompressed| Limits {
        content: crate::source::ContentLimits {
            decompressed,
            ..LIM.content
        },
        ..*LIM
    };
    let fresh = DrawnForms::default();
    let blocks = PageBlocks::read(
        &pdf,
        &[page],
        Some(&resources),
        &cap(draws.len() + 99),
        &fresh,
    );
    assert!(blocks.unwrap().nested_in_forms);
    assert_eq!(fresh.nested_known(big), Some(true));
    // The page's share spent on the first form, the second isn't read.
    let small = [0, 1].map(|_| form(&mut pdf, b"0 0 m 9 9 l S\n".repeat(20)));
    let resources = Object::from(dictionary! {
        "XObject" => dictionary! { "A" => small[0], "B" => small[1] },
    });
    let content = [&b"/A Do /B Do %"[..], &[b'-'; 400]].concat();
    let page = pdf.add_object(Stream::new(dictionary! {}, content.clone()));
    let fresh = DrawnForms::default();
    let blocks = PageBlocks::read(
        &pdf,
        &[page],
        Some(&resources),
        &cap(content.len() + 99),
        &fresh,
    );
    assert!(blocks.unwrap().nested_in_forms);
    assert_eq!(fresh.nested_known(small[0]), Some(false));
    assert_eq!(fresh.nested_known(small[1]), None);
}
