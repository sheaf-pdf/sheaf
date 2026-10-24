//! The interpreter over pages built in code ([`fixture`]): each glyph
//! where it was worked out by hand to be, to a thousandth of a point;
//! forms drawn per use, and refused within themselves or too deep; each
//! image, path, clip and shading where it was worked out to be, paths
//! built as PDFium builds them; marked content read as PDFium reads it;
//! each cap on its work held to; operands read as PDFium reads them.

use std::ops::{ControlFlow, Range};

use lopdf::{Document as Pdf, Object, Stream, dictionary};
use proptest::prelude::*;

use super::state::number;
use super::*;
use crate::annot::Pt;
use crate::content::fixture::{self, Fixture};
use crate::content::{FillRule, Paint};
use crate::geom::{Affine, Rect};
use crate::source::{ContentLimits, Limits};

/// A glyph as a test sees it.
#[derive(Debug, Clone)]
struct Shown {
    code: u32,
    origin: Pt,
    quad: Quad,
    invisible: bool,
    stock: bool,
    /// Where it is, or how wide, is guessed ([`FontApprox::widths`]).
    guessed: bool,
}

/// Every event `content`, drawn with `resources`, gives, kept as a test
/// sees it, and the glyphs among them.
#[derive(Debug, Default)]
struct Seen {
    glyphs: Vec<Shown>,
    /// Strings shown in no font.
    fontless: Vec<Vec<u8>>,
    begun: Vec<(ObjectId, UseId, Affine, Option<Rect>)>,
    ended: usize,
    refused: Vec<(ObjectId, Refused)>,
    ops: usize,
}

fn run(
    pdf: &Pdf,
    content: &[u8],
    resources: Option<&Object>,
    lim: &Limits,
    opts: Opts,
) -> Result<(Seen, Summary), InterpError> {
    let mut seen = Seen::default();
    let fonts = Fonts::default();
    let summary = interpret(
        pdf,
        &Concat::new(&[content]),
        resources,
        lim,
        &fonts,
        opts,
        |event| {
            match event.kind {
                Kind::Op => seen.ops += 1,
                Kind::Text(show) if show.font.is_none() => seen.fontless.push(show.string.to_vec()),
                Kind::Text(show) => seen.glyphs.extend(show.glyphs().map(|g| Shown {
                    code: g.code,
                    origin: g.origin,
                    quad: g.quad,
                    invisible: g.invisible,
                    stock: show.stock,
                    guessed: g.approx.widths,
                })),
                Kind::FormBegin(u) => seen.begun.push((u.form, u.id.clone(), u.matrix, u.bbox)),
                Kind::FormEnd(_) => seen.ended += 1,
                Kind::FormRefused { form, why } => seen.refused.push((form, why)),
                _ => {}
            }
            ControlFlow::Continue(())
        },
    )?;
    Ok((seen, summary))
}

fn run_fixture(f: &Fixture, lim: &Limits) -> Result<(Seen, Summary), InterpError> {
    let content = f.content();
    run(
        &f.pdf,
        content.concat().bytes(),
        f.resources(),
        lim,
        Opts::default(),
    )
}

/// Where a glyph was worked out to be: its code, origin, and box.
type Expected = (u8, [f32; 2], [f32; 4]);

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// `glyphs` are `expected`, in order, to a thousandth of a point.
fn assert_glyphs(glyphs: &[Shown], expected: &[Expected]) {
    assert_eq!(glyphs.len(), expected.len(), "{glyphs:#?}");
    for (i, (g, &(code, [x, y], [x0, y0, x1, y1]))) in glyphs.iter().zip(expected).enumerate() {
        let b = g.quad.bounds();
        let at = (g.origin.x, g.origin.y);
        assert_eq!(g.code, u32::from(code), "glyph {i}");
        assert!(
            close(at.0, x) && close(at.1, y),
            "glyph {i}: origin {at:?}, not {x},{y}"
        );
        let worked = [b.x0, b.y0, b.x1, b.y1];
        assert!(
            worked
                .iter()
                .zip([x0, y0, x1, y1])
                .all(|(&a, b)| close(a, b)),
            "glyph {i}: box {worked:?}, not {:?}",
            [x0, y0, x1, y1]
        );
    }
}

/// Each operator of the text state, and `cm`, `q` and `Q`, places each
/// glyph where [`fixture::operators`] says, to a thousandth of a point;
/// the string before any `Tf` has no glyphs, and the leading `Q` is
/// passed over.
#[test]
fn each_operator_places_its_glyphs() {
    let f = fixture::operators();
    let (seen, summary) = run_fixture(&f, &Limits::DEFAULT).unwrap();
    let helvetica = |size: f32| [100.0 + 0.556 * size, 0.718 * size, -0.207 * size];
    let [hv10, ..] = helvetica(10.0);
    let [hv12, ..] = helvetica(12.0);
    #[rustfmt::skip]
    let expected: &[Expected] = &[
        (b'a', [100.0, 700.0], [100.0, 698.0, 110.0, 708.0]),
        (b'b', [110.0, 700.0], [110.0, 698.0, 115.0, 708.0]),
        // 2 Tc
        (b'a', [100.0, 680.0], [100.0, 678.0, 110.0, 688.0]),
        (b'b', [112.0, 680.0], [112.0, 678.0, 117.0, 688.0]),
        // 5 Tw
        (b'a', [100.0, 660.0], [100.0, 658.0, 110.0, 668.0]),
        (b' ', [110.0, 660.0], [110.0, 658.0, 112.5, 668.0]),
        (b'a', [117.5, 660.0], [117.5, 658.0, 127.5, 668.0]),
        // 50 Tz, and TJ through it
        (b'a', [100.0, 640.0], [100.0, 638.0, 105.0, 648.0]),
        (b'b', [105.0, 640.0], [105.0, 638.0, 107.5, 648.0]),
        (b'a', [112.5, 640.0], [112.5, 638.0, 117.5, 648.0]),
        // 12 TL, T*
        (b'a', [100.0, 620.0], [100.0, 618.0, 110.0, 628.0]),
        (b'b', [100.0, 608.0], [100.0, 606.0, 105.0, 616.0]),
        // 3 Ts
        (b'a', [100.0, 593.0], [100.0, 591.0, 110.0, 601.0]),
        // Tm, turned a quarter
        (b'a', [300.0, 500.0], [292.0, 500.0, 302.0, 510.0]),
        (b'b', [300.0, 510.0], [292.0, 510.0, 302.0, 515.0]),
        // TD, TD, T*
        (b'a', [100.0, 560.0], [100.0, 558.0, 110.0, 568.0]),
        (b'b', [100.0, 546.0], [100.0, 544.0, 105.0, 554.0]),
        (b'a', [100.0, 532.0], [100.0, 530.0, 110.0, 540.0]),
        // '
        (b'a', [100.0, 510.0], [100.0, 508.0, 110.0, 518.0]),
        // "
        (b'a', [100.0, 490.0], [100.0, 488.0, 110.0, 498.0]),
        (b' ', [111.0, 490.0], [111.0, 488.0, 113.5, 498.0]),
        (b'a', [118.5, 490.0], [118.5, 488.0, 128.5, 498.0]),
        // TJ
        (b'a', [100.0, 470.0], [100.0, 468.0, 110.0, 478.0]),
        (b'b', [115.0, 470.0], [115.0, 468.0, 120.0, 478.0]),
        (b'a', [118.0, 470.0], [118.0, 468.0, 128.0, 478.0]),
        // cm
        (b'a', [70.0, 450.0], [70.0, 446.0, 90.0, 466.0]),
        // q … Q within BT
        (b'a', [100.0, 420.0], [100.0, 418.0, 110.0, 428.0]),
        // gs /Font [/T3 20]
        (b'b', [100.0, 400.0], [100.0, 396.0, 110.0, 416.0]),
        // gs /Font [reference 10], and /Nope 12 Tf: the stock Helvetica
        (b'a', [100.0, 380.0], [100.0, 380.0 - 2.07, hv10, 380.0 + 7.18]),
        (b'a', [100.0, 360.0], [100.0, 360.0 - 2.484, hv12, 360.0 + 8.616]),
        // A TJ that passes over an empty string loses its last adjustment
        (b'a', [100.0, 340.0], [100.0, 338.0, 110.0, 348.0]),
        (b'b', [110.0, 340.0], [110.0, 338.0, 115.0, 348.0]),
        (b'a', [100.0, 320.0], [100.0, 318.0, 110.0, 328.0]),
        (b'b', [115.0, 320.0], [115.0, 318.0, 120.0, 328.0]),
        (b'b', [120.0, 320.0], [120.0, 318.0, 125.0, 328.0]),
        // and one that doesn't keeps it
        (b'a', [100.0, 300.0], [100.0, 298.0, 110.0, 308.0]),
        (b'b', [120.0, 300.0], [120.0, 298.0, 125.0, 308.0]),
    ];
    assert_glyphs(&seen.glyphs, expected);
    // The quarter-turned `a`'s corners, in its text space's order.
    let corners = seen.glyphs[13].quad.0.map(|p| (p.x, p.y));
    let worked = [
        (302.0, 500.0),
        (302.0, 510.0),
        (292.0, 510.0),
        (292.0, 500.0),
    ];
    assert!(
        corners
            .iter()
            .zip(worked)
            .all(|(a, b)| close(a.0, b.0) && close(a.1, b.1)),
        "{corners:?}"
    );
    let stock: Vec<bool> = seen.glyphs.iter().map(|g| g.stock).collect();
    assert_eq!(stock.iter().filter(|&&s| s).count(), 2);
    assert!(stock[28] && stock[29]);
    assert!(seen.glyphs.iter().all(|g| !g.invisible));
    assert_eq!(seen.fontless, [b"a".to_vec()]);
    assert_eq!(summary.glyphs, 37);
    assert_eq!(summary.deepest_q, 2);
}

/// A width guessed, the Euro's in the standard 14 drawn without
/// `/Widths`, moves what's shown after it on its line: the rest of its
/// string, the next, and what follows a `Q` that restores the line, but
/// nothing once `Td`, `T*`, `'`, `Tm` or `BT` sets the line again; and so
/// does a string in a font whose widths are all guessed.
#[test]
fn a_width_guessed_moves_what_follows_it() {
    let mut pdf = Pdf::with_version("1.7");
    let helvetica = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let unknown = pdf.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "TrueType",
        "BaseFont" => "Nonesuch",
    });
    let fonts = dictionary! { "F1" => helvetica, "F2" => unknown };
    let resources = Object::Dictionary(dictionary! { "Font" => fonts });
    let content = b"BT /F1 10 Tf 12 TL (A\\200B) Tj (C) Tj q 0 -12 Td (D) Tj Q (E) Tj \
        T* (F) Tj (\\200) Tj (G) ' (\\200) Tj 1 0 0 1 0 0 Tm (H) Tj (\\200) Tj ET \
        BT /F1 10 Tf (I) Tj /F2 10 Tf (J) Tj /F1 10 Tf (K) Tj 0 -12 Td (L) Tj ET";
    let (seen, _) = run(
        &pdf,
        content,
        Some(&resources),
        &Limits::DEFAULT,
        Opts::default(),
    )
    .unwrap();
    let shown: Vec<(u8, bool)> = (seen.glyphs.iter())
        .map(|g| (g.code as u8, g.guessed))
        .collect();
    #[rustfmt::skip]
    let expected = [
        (b'A', false), (0x80, true), (b'B', true), (b'C', true), (b'D', false), (b'E', true),
        (b'F', false), (0x80, true), (b'G', false), (0x80, true), (b'H', false), (0x80, true),
        (b'I', false), (b'J', true), (b'K', true), (b'L', false),
    ];
    assert_eq!(shown, expected);
}

/// Each kind of font places its glyphs where [`fixture::fonts`] says:
/// the standard 14 by Adobe's metrics, `/Widths`, CID `/W` through
/// Identity and an embedded CMap (`Tw` on its one-byte space), a Type3
/// `/FontMatrix`, Identity-V's column by `/W2` and `/DW2`, and a code of
/// Identity-H cut off at a string's end, as PDFium reads it.
#[test]
fn each_font_kind_places_its_glyphs() {
    let f = fixture::fonts();
    let (seen, _) = run_fixture(&f, &Limits::DEFAULT).unwrap();
    // Helvetica's ascender and descender, and Helvetica-Bold's.
    let hv = |y: f32| [y - 2.07, y + 7.18];
    let hb = |y: f32| [y - 2.07, y + 7.18];
    let [hv0, hv1] = hv(700.0);
    let [hb0, hb1] = hb(680.0);
    let co = |y: f32| [y - 1.57, y + 6.29];
    let [co0, co1] = co(660.0);
    #[rustfmt::skip]
    let expected: &[Expected] = &[
        (b'H', [100.0, 700.0], [100.0, hv0, 107.22, hv1]),
        (b'i', [107.22, 700.0], [107.22, hv0, 109.44, hv1]),
        (b'H', [100.0, 680.0], [100.0, hb0, 107.22, hb1]),
        (b'i', [107.22, 680.0], [107.22, hb0, 110.0, hb1]),
        (b'a', [100.0, 660.0], [100.0, co0, 106.0, co1]),
        (b'b', [106.0, 660.0], [106.0, co0, 112.0, co1]),
        (b'A', [100.0, 640.0], [100.0, 639.0, 105.0, 649.0]),
        (b'B', [105.0, 640.0], [105.0, 639.0, 111.0, 649.0]),
        (1, [100.0, 620.0], [100.0, 618.0, 104.0, 628.0]),
        (2, [104.0, 620.0], [104.0, 618.0, 109.0, 628.0]),
        (10, [109.0, 620.0], [109.0, 618.0, 116.0, 628.0]),
        (1, [100.0, 600.0], [100.0, 598.0, 104.5, 608.0]),
        (b'A', [100.0, 580.0], [100.0, 578.0, 108.0, 588.0]),
        (b' ', [108.0, 580.0], [108.0, 578.0, 111.0, 588.0]),
        (b'A', [117.0, 580.0], [117.0, 578.0, 125.0, 588.0]),
        (b'a', [100.0, 560.0], [100.0, 558.01, 110.0, 567.99]),
        (1, [297.5, 591.2], [295.0, 590.0, 305.0, 600.0]),
        (2, [297.0, 581.2], [295.0, 580.0, 305.0, 590.0]),
    ];
    assert_glyphs(&seen.glyphs[..18], expected);
    let cut: Vec<(u32, [f32; 3])> = (seen.glyphs[18..].iter())
        .map(|g| {
            let b = g.quad.bounds();
            (g.code, [g.origin.x, b.x1, b.y0])
        })
        .collect();
    #[rustfmt::skip]
    assert_eq!(cut, [
        (1, [100.0, 104.0, 538.0]), (0x0A00, [104.0, 114.0, 538.0]), (97, [114.0, 124.0, 538.01]),
        (1, [100.0, 104.0, 518.0]), (0x2000, [104.0, 114.0, 518.0]), (97, [114.0, 124.0, 518.01]),
    ]);
    let invisible: Vec<usize> = (seen.glyphs.iter().enumerate())
        .filter_map(|(i, g)| g.invisible.then_some(i))
        .collect();
    assert_eq!(invisible, [2, 3]);
}

/// A form whose own resources lack a category finds it in the page's, as
/// PDFium's `FindResourceHolder` does: [`fixture::inherited`]'s `/Fm`
/// shows its `a` in the page's font, not the stock one, opens its range
/// by the page's property list, and finds its inline image's data by the
/// length the page's color space gives, past the ` EI ` in it.
#[test]
fn a_form_finds_what_its_resources_lack_in_the_page_s() {
    let f = fixture::inherited();
    let (mut glyphs, mut mcids, mut images, mut ops) = (Vec::new(), Vec::new(), Vec::new(), 0);
    each(&f, Opts::default(), |event| match event.kind {
        Kind::Op => ops += 1,
        Kind::Text(show) => {
            let stock = show.stock;
            glyphs.extend(
                show.glyphs()
                    .map(|g| (g.code, g.origin, g.quad.bounds(), stock)),
            );
        }
        Kind::MarkedBegin(mark) => mcids.push(mark.mcid),
        Kind::Image(image) => images.push(image),
        _ => {}
    });
    let [(code, origin, bounds, stock)] = glyphs[..] else {
        panic!("{glyphs:?}");
    };
    assert_eq!((code, origin, stock), (97, pt(100.0, 700.0), false));
    assert_rect(bounds, [100.0, 698.0, 110.0, 708.0], "a");
    assert_eq!(mcids, [Some(3)]);
    let [image] = &images[..] else {
        panic!("{images:?}");
    };
    assert!(matches!(&image.of, ImageOf::Inline { data } if data.len() == 12));
    let quad = image.quad.0.map(|p| (p.x, p.y));
    assert_eq!(quad, corners(100.0, 600.0, 20.0, 10.0));
    // `/Fm Do`, then `BDC BT Tf Td Tj ET EMC q cm`, the image, and `Q`.
    assert_eq!(ops, 1 + 9 + 1 + 1);
}

/// A form in a form: each use drawn in its own state, through each
/// `/Matrix`, the inner one twice under ids of its own, and `Tz` not
/// carried in while the font is.
#[test]
fn forms_are_drawn_per_use() {
    let f = fixture::forms();
    let (seen, summary) = run_fixture(&f, &Limits::DEFAULT).unwrap();
    #[rustfmt::skip]
    assert_glyphs(&seen.glyphs, &[
        (b'a', [115.0, 325.0], [115.0, 323.0, 125.0, 333.0]),
        (b'b', [112.0, 322.0], [112.0, 318.0, 122.0, 338.0]),
        (b'b', [162.0, 322.0], [162.0, 318.0, 172.0, 338.0]),
    ]);
    let [
        (outer, top, m1, bbox1),
        (inner, first, m2, bbox2),
        (again, second, m3, _),
    ] = &seen.begun[..]
    else {
        panic!("{:?}", seen.begun);
    };
    assert_eq!(seen.ended, 3);
    assert_eq!(inner, again);
    assert_ne!(outer, inner);
    // Each `Do` where it is: the page's, then each in the outer form's.
    let page = f.content();
    let page = page.concat().bytes();
    let outer_content = f
        .pdf
        .get_object(*outer)
        .unwrap()
        .as_stream()
        .unwrap()
        .content
        .clone();
    let dos = |s: &[u8]| -> Vec<u32> {
        (s.windows(2).enumerate())
            .filter(|(_, w)| w == b"Do")
            .map(|(i, _)| i as u32)
            .collect()
    };
    let in_outer = dos(&outer_content);
    assert_eq!(top.path(), dos(page));
    assert_eq!(first.path(), [dos(page)[0], in_outer[0]]);
    assert_eq!(second.path(), [dos(page)[0], in_outer[1]]);
    assert_eq!(
        (first.depth(), summary.deepest_form, summary.uses),
        (2, 2, 3)
    );
    let translate = |e: f32, f: f32| Affine {
        e,
        f,
        ..Affine::IDENTITY
    };
    assert_eq!(*m1, translate(110.0, 320.0));
    assert_eq!(
        *m2,
        Affine {
            a: 2.0,
            d: 2.0,
            ..translate(110.0, 320.0)
        }
    );
    assert_eq!(
        *m3,
        Affine {
            a: 2.0,
            d: 2.0,
            ..translate(160.0, 320.0)
        }
    );
    assert_eq!(*bbox1, Some(Rect::new(0.0, 0.0, 200.0, 100.0)));
    assert_eq!(*bbox2, Some(Rect::new(0.0, 0.0, 50.0, 50.0)));
}

/// A form drawn within itself, by itself or through another, is refused
/// where it would be, and the rest of the page is drawn.
#[test]
fn a_form_within_itself_is_refused() {
    let f = fixture::cycles();
    let (seen, summary) = run_fixture(&f, &Limits::DEFAULT).unwrap();
    #[rustfmt::skip]
    assert_glyphs(&seen.glyphs, &[
        (b'a', [100.0, 700.0], [100.0, 698.0, 110.0, 708.0]),
        (b'b', [100.0, 650.0], [100.0, 648.0, 105.0, 658.0]),
        (b'a', [100.0, 600.0], [100.0, 598.0, 110.0, 608.0]),
    ]);
    let why: Vec<Refused> = seen.refused.iter().map(|&(_, why)| why).collect();
    assert_eq!(why, [Refused::Cycle, Refused::Cycle]);
    let [own, a, ..] = [seen.begun[0].0, seen.begun[1].0];
    assert_eq!(seen.refused[0].0, own);
    assert_eq!(seen.refused[1].0, a);
    assert_eq!((summary.uses, summary.refused), (3, 2));
}

/// Forms nested `form_depth` deep are drawn, and one deeper is refused,
/// on a thread with the default stack: the recursion is bounded.
#[test]
fn forms_past_form_depth_are_refused() {
    std::thread::spawn(|| {
        let depth = Limits::DEFAULT.content.form_depth;
        let (seen, summary) = run_fixture(&fixture::chain(depth), &Limits::DEFAULT).unwrap();
        assert_eq!(seen.glyphs.len(), 1);
        assert!(seen.refused.is_empty());
        assert_eq!(summary.deepest_form, depth);
        let (seen, summary) = run_fixture(&fixture::chain(depth + 1), &Limits::DEFAULT).unwrap();
        assert!(seen.glyphs.is_empty());
        assert_eq!(seen.refused.len(), 1);
        assert_eq!(seen.refused[0].1, Refused::Depth);
        assert_eq!(summary.deepest_form, depth);
    })
    .join()
    .unwrap();
}

/// `lim` with `content` changed.
fn with(content: impl FnOnce(&mut ContentLimits)) -> Limits {
    let mut lim = Limits::DEFAULT;
    content(&mut lim.content);
    lim
}

fn limit(result: Result<(Seen, Summary), InterpError>) -> Option<Cap> {
    match result {
        Err(InterpError::Limit(cap)) => Some(cap),
        Err(e) => panic!("{e}"),
        Ok(_) => None,
    }
}

/// Each cap on the work holds at its value and refuses one past it:
/// operators, glyphs, states saved, marked content's nesting, the bytes
/// of content lexed, path segments, and what the forms and fonts
/// decompress to.
#[test]
fn each_cap_is_held_to() {
    let pdf = Pdf::with_version("1.7");
    let ops = |n: usize| with(|c| c.operators = n);
    let content = b"q Q q Q";
    assert_eq!(
        limit(run(&pdf, content, None, &ops(4), Opts::default())),
        None
    );
    let past = run(&pdf, content, None, &ops(3), Opts::default());
    assert_eq!(limit(past), Some(Cap::Operators));

    let states = |n: usize| with(|c| c.states = n);
    let content = b"q q q Q Q Q";
    assert_eq!(
        limit(run(&pdf, content, None, &states(3), Opts::default())),
        None
    );
    let past = run(&pdf, content, None, &states(2), Opts::default());
    assert_eq!(limit(past), Some(Cap::States));

    let marked = |n: usize| with(|c| c.depth = n);
    let content = b"/A BMC /A BMC /A BMC EMC EMC EMC";
    assert_eq!(
        limit(run(&pdf, content, None, &marked(3), Opts::default())),
        None
    );
    let past = run(&pdf, content, None, &marked(2), Opts::default());
    assert_eq!(limit(past), Some(Cap::Marked));

    let f = fixture::operators();
    let glyphs = |n: usize| with(|c| c.glyphs = n);
    assert_eq!(limit(run_fixture(&f, &glyphs(37))), None);
    assert_eq!(limit(run_fixture(&f, &glyphs(36))), Some(Cap::Glyphs));

    // The page's content and both forms', each read once.
    let f = fixture::forms();
    let form = |name: &[u8]| -> usize {
        let id = f.pdf.objects.iter().find_map(|(&id, o)| {
            let s = o.as_stream().ok()?;
            (s.dict.get(b"Subtype").ok()?.as_name().ok()? == b"Form"
                && s.content.windows(name.len()).any(|w| w == name))
            .then_some(id)
        });
        f.pdf
            .get_object(id.unwrap())
            .unwrap()
            .as_stream()
            .unwrap()
            .content
            .len()
    };
    // The Type3 font's CharProcs, each read once.
    let char_procs: usize = (f.pdf.objects.values())
        .filter_map(|o| o.as_dict().ok()?.get(b"CharProcs").ok()?.as_dict().ok())
        .flat_map(|procs| procs.iter())
        .map(|(_, id)| f.pdf.get_object(id.as_reference().unwrap()).unwrap())
        .map(|proc| proc.as_stream().unwrap().content.len())
        .sum();
    let content = f.content().concat().bytes().len();
    let all = content + form(b"(a)") + form(b"(b)") + char_procs;
    let decompressed = |n: usize| with(|c| c.decompressed = n);
    assert_eq!(limit(run_fixture(&f, &decompressed(all))), None);
    let past = run_fixture(&f, &decompressed(all - 1));
    assert_eq!(limit(past), Some(Cap::Decompressed));

    // The content, the outer form's once, and the inner form's each of
    // the two times it's drawn.
    let lexed = content + form(b"(a)") + 2 * form(b"(b)");
    let interpreted = |n: usize| with(|c| c.interpreted = n);
    assert_eq!(limit(run_fixture(&f, &interpreted(lexed))), None);
    let past = run_fixture(&f, &interpreted(lexed - 1));
    assert_eq!(limit(past), Some(Cap::Interpreted));

    // Two rectangles of six segments each (a move, four lines, and the
    // close), the first painted and the second made the clip; and a path
    // being built, of twelve.
    let segments = |n: usize| with(|c| c.segments = n);
    let content = b"0 0 1 1 re f 0 0 1 1 re W n";
    assert_eq!(
        limit(run(&pdf, content, None, &segments(12), Opts::default())),
        None
    );
    let past = run(&pdf, content, None, &segments(11), Opts::default());
    assert_eq!(limit(past), Some(Cap::Segments));
    let building = b"0 0 1 1 re 0 0 1 1 re";
    assert_eq!(
        limit(run(&pdf, building, None, &segments(12), Opts::default())),
        None
    );
    let past = run(&pdf, building, None, &segments(11), Opts::default());
    assert_eq!(limit(past), Some(Cap::Segments));
}

/// A form of whitespace, which has one operator, costs its length each
/// time it's drawn: drawn a thousand times by a fan-out of three levels,
/// a MiB form ends the page past the cap on bytes interpreted, quickly,
/// though it has few operators and decompresses once.
#[test]
fn a_form_of_whitespace_costs_its_length_each_use() {
    let mut pdf = Pdf::with_version("1.7");
    let mut leaf = vec![b' '; 1 << 20];
    leaf.extend(b"n");
    let form = |content: Vec<u8>, xobjects: lopdf::Dictionary| {
        Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "Resources" => dictionary! { "XObject" => xobjects },
            },
            content,
        )
    };
    let mut next = pdf.add_object(form(leaf, dictionary! {}));
    for _ in 0..3 {
        next = pdf.add_object(form(b"/F Do ".repeat(10), dictionary! { "F" => next }));
    }
    let resources: Object = dictionary! { "XObject" => dictionary! { "F" => next } }.into();
    let started = std::time::Instant::now();
    let lim = with(|c| c.interpreted = 64 << 20);
    let read = run(&pdf, b"/F Do", Some(&resources), &lim, Opts::default());
    assert_eq!(limit(read), Some(Cap::Interpreted));
    assert!(started.elapsed().as_secs() < 10);
}

/// A page's fonts decompress within its budget, with its content and
/// forms: a thousand fonts written in the resources as dictionaries, each
/// read anew, whose CharProcs are one stream that inflates to 4 MiB, end
/// the page past the cap on what it decompresses, quickly, not at a
/// gigabyte each.
#[test]
fn fonts_decompress_within_the_page_s_budget() {
    use std::io::Write;
    let mut pdf = Pdf::with_version("1.7");
    let mut program = b"100 0 0 0 100 100 d1 ".to_vec();
    program.resize(4 << 20, b' ');
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&program).unwrap();
    let bomb = Stream::new(
        dictionary! { "Filter" => "FlateDecode" },
        z.finish().unwrap(),
    );
    let bomb = pdf.add_object(bomb);
    let font = || {
        dictionary! {
            "Type" => "Font", "Subtype" => "Type3",
            "FontMatrix" => vec![0.001.into(), 0.into(), 0.into(), 0.001.into(), 0.into(), 0.into()],
            "Encoding" => dictionary! { "Differences" => vec![97.into(), "a".into()] },
            "CharProcs" => dictionary! { "a" => bomb },
        }
    };
    let fonts: lopdf::Dictionary = (0..1000)
        .map(|i| (format!("F{i}"), font().into()))
        .collect();
    let resources: Object = dictionary! { "Font" => fonts }.into();
    let content: String = (0..1000).map(|i| format!("/F{i} 1 Tf ")).collect();
    let started = std::time::Instant::now();
    let lim = with(|c| c.decompressed = 64 << 20);
    let read = run(
        &pdf,
        content.as_bytes(),
        Some(&resources),
        &lim,
        Opts::default(),
    );
    assert_eq!(limit(read), Some(Cap::Decompressed));
    assert!(started.elapsed().as_secs() < 10);
    // Fifteen of them fit.
    let fifteen: String = (0..15).map(|i| format!("/F{i} 1 Tf ")).collect();
    let read = run(
        &pdf,
        fifteen.as_bytes(),
        Some(&resources),
        &lim,
        Opts::default(),
    );
    assert_eq!(limit(read), None);
}

/// Geometry past what an `f32` holds fails the interpretation, whether
/// the CTM, the line, a glyph's box, a path, an image or a form's matrix
/// gets there; just short of it, nothing does.
#[test]
fn geometry_past_an_f32_fails() {
    let mut pdf = Pdf::with_version("1.7");
    let e20 = Object::Real(1e20);
    let form = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Matrix" => vec![e20.clone(), 0.into(), 0.into(), e20, 0.into(), 0.into()],
        },
        b"0 0 m 1 1 l S".to_vec(),
    ));
    let resources: Object = dictionary! {
        "Font" => dictionary! {
            "F" => dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
        },
        "XObject" => dictionary! { "Fm" => form },
    }
    .into();
    let big = "100000000000000000000.0";
    let most = "300000000000000000000000000000000000000.0";
    let wide = format!("{big} 0 0 {big} 0 0 cm");
    let cases = [
        format!("{wide} {wide}"),
        format!("BT {most} {most} Td {most} {most} Td ET"),
        format!("{wide} BT /F 1000000000000000000.0 Tf (aaaa) Tj ET"),
        format!("{wide} 0 0 m {big} {big} l S"),
        format!("{most} 0 0 1 {most} 0 cm BI /W 1 /H 1 /BPC 8 /CS /G ID \0 EI"),
        format!("{wide} BT /F 1 Tf {big} Tc (aa) Tj ET"),
        format!("{wide} /Fm Do"),
    ];
    for content in &cases {
        let read = run(
            &pdf,
            content.as_bytes(),
            Some(&resources),
            &Limits::DEFAULT,
            Opts::default(),
        );
        assert!(
            matches!(read, Err(InterpError::NotFinite)),
            "{content}: {read:?}"
        );
    }
    let near = format!("{wide} BT /F 1 Tf (aaaa) Tj ET 0 0 m 1000 1000 l S");
    let read = run(
        &pdf,
        near.as_bytes(),
        Some(&resources),
        &Limits::DEFAULT,
        Opts::default(),
    );
    assert!(read.is_ok(), "{read:?}");
}

/// Forms drawn ten times at each of twenty levels are counted across
/// every use, so the operators cap ends it, quickly, however many uses
/// there would be.
#[test]
fn uses_of_uses_are_counted_across_all() {
    let started = std::time::Instant::now();
    let lim = with(|c| c.operators = 100_000);
    let result = run_fixture(&fixture::fan_out(20, 10), &lim);
    assert_eq!(limit(result), Some(Cap::Operators));
    assert!(started.elapsed().as_secs() < 10);
}

/// A `Q` with nothing saved is passed over, in the page's content and in
/// a form's, which can't restore a state its page saved.
#[test]
fn a_q_with_nothing_saved_is_passed_over() {
    let f = fixture::operators();
    let squares = f
        .page()
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap();
    let squares = squares.as_dict().unwrap().get(b"T3").unwrap().clone();
    let mut pdf = f.pdf;
    let form = pdf.add_object(Stream::new(
        dictionary! { "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()] },
        b"Q BT 10 10 Td (a) Tj ET".to_vec(),
    ));
    let resources: Object = dictionary! {
        "Font" => dictionary! { "T3" => squares },
        "XObject" => dictionary! { "Fm" => form },
    }
    .into();
    let content = b"Q q 2 0 0 2 0 0 cm /T3 10 Tf /Fm Do BT 10 10 Td (b) Tj ET Q Q";
    let (seen, summary) = run(
        &pdf,
        content,
        Some(&resources),
        &Limits::DEFAULT,
        Opts::default(),
    )
    .unwrap();
    // Both drawn at twice the size: the form's `Q` restored nothing.
    #[rustfmt::skip]
    assert_glyphs(&seen.glyphs, &[
        (b'a', [20.0, 20.0], [20.0, 16.0, 40.0, 36.0]),
        (b'b', [20.0, 20.0], [20.0, 16.0, 30.0, 36.0]),
    ]);
    assert_eq!(summary.deepest_q, 1);
}

/// Reported, a form's `Do` is an operator, and nothing it draws is
/// interpreted, nor the form read.
#[test]
fn reported_forms_are_not_drawn() {
    let f = fixture::forms();
    let content = f.content();
    let opts = Opts {
        forms: Forms::Report,
    };
    let (seen, summary) = run(
        &f.pdf,
        content.concat().bytes(),
        f.resources(),
        &Limits::DEFAULT,
        opts,
    )
    .unwrap();
    assert!(seen.glyphs.is_empty() && seen.begun.is_empty());
    assert_eq!((seen.ops, summary.operators, summary.uses), (6, 6, 0));
}

/// A fixture's objects, but for one that can't be read.
struct Unreadable<'a> {
    pdf: &'a Pdf,
    id: ObjectId,
}

impl ReadObjects for Unreadable<'_> {
    fn object(&self, id: ObjectId) -> lopdf::Result<&Object> {
        match id == self.id {
            true => Err(lopdf::Error::InvalidStream("unreadable".into())),
            false => self.pdf.get_object(id),
        }
    }

    fn read<R>(&self, id: ObjectId, read: impl FnOnce(&Object) -> R) -> lopdf::Result<R> {
        self.object(id).map(read)
    }
}

/// A form that can't be read stops what draws it, but not where XObjects
/// are judged on their own, which reads none.
#[test]
fn a_form_that_cant_be_read_stops_only_what_draws_it() {
    let f = fixture::forms();
    let xobjects = f.page().get_deref(b"Resources", &f.pdf).unwrap();
    let xobjects = xobjects.as_dict().unwrap().get(b"XObject").unwrap();
    let fm1 = xobjects.as_dict().unwrap().get(b"Fm1").unwrap();
    let fm1 = fm1.as_reference().unwrap();
    let broken = Unreadable {
        pdf: &f.pdf,
        id: fm1,
    };
    let content = f.content();
    let drawn = |forms| {
        let opts = Opts { forms };
        let (lim, fonts) = (Limits::DEFAULT, Fonts::default());
        interpret(
            &broken,
            content.concat(),
            f.resources(),
            &lim,
            &fonts,
            opts,
            |_| ControlFlow::Continue(()),
        )
    };
    let e = drawn(Forms::Descend).unwrap_err();
    assert!(
        matches!(e, InterpError::XObject { xobject, .. } if xobject == fm1),
        "{e}"
    );
    assert_eq!(drawn(Forms::Report).unwrap().operators, 6);
}

/// The visitor can stop it: the summary says so.
#[test]
fn the_visitor_can_stop_it() {
    let f = fixture::operators();
    let content = f.content();
    let mut texts = 0;
    let summary = interpret(
        &f.pdf,
        content.concat(),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
        Opts::default(),
        |event| {
            texts += usize::from(matches!(event.kind, Kind::Text(_)));
            match texts {
                3 => ControlFlow::Break(()),
                _ => ControlFlow::Continue(()),
            }
        },
    )
    .unwrap();
    assert!(summary.stopped);
    assert_eq!(texts, 3);
}

/// Content that can't be lexed, in the page's or a form's, and a form
/// whose stream can't be read, are errors that say where.
#[test]
fn faults_say_where() {
    let mut pdf = Pdf::with_version("1.7");
    let broken = pdf.add_object(Stream::new(
        dictionary! { "Subtype" => "Form" },
        b"BT (no end".to_vec(),
    ));
    // A filter lopdf can't decode, which a content stream has no business
    // being in.
    let mut unreadable = Stream::new(dictionary! { "Subtype" => "Form" }, b"BT ET".to_vec());
    unreadable.dict.set("Filter", "JPXDecode");
    let unreadable = pdf.add_object(unreadable);
    let resources: Object = dictionary! {
        "XObject" => dictionary! { "B" => broken, "U" => unreadable },
    }
    .into();
    let lim = &Limits::DEFAULT;
    let opts = Opts::default();
    let lexed = |r: Result<(Seen, Summary), InterpError>| match r {
        Err(InterpError::Lex { form, error }) => (form, error.at),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        lexed(run(&pdf, b"q (no end", Some(&resources), lim, opts)),
        (None, 2)
    );
    let in_form = run(&pdf, b"/B Do", Some(&resources), lim, opts);
    assert_eq!(lexed(in_form), (Some(broken), 3));
    match run(&pdf, b"/U Do", Some(&resources), lim, opts) {
        Err(InterpError::XObject { xobject, .. }) => assert_eq!(xobject, unreadable),
        other => panic!("{other:?}"),
    }
}

/// Operands are read as PDFium reads them: what isn't a number, or
/// isn't there, is 0; `Tz` takes one operand only; and a number that
/// isn't finite changes nothing.
#[test]
fn operands_read_as_pdfium_reads_them() {
    for (word, n) in [
        (&b"1.2.3"[..], 1.2),
        (b"--5.0", -5.0),
        (b"+-.5", -0.5),
        (b"5.", 5.0),
        (b".", 0.0),
        (b"-", 0.0),
        (b"007", 7.0),
        (b"4294967295", 4_294_967_295.0),
        (b"4294967296", 0.0),
        (b"+2147483648", 0.0),
        (b"-2147483648", -2_147_483_648.0),
        (b"5-3", 5.0),
        (b"--5", 0.0),
        (b"/Name", 0.0),
        (b"(5)", 0.0),
    ] {
        assert_eq!(number(word), n, "{}", String::from_utf8_lossy(word));
    }
    assert!(number(&[b"9".repeat(40), b".0".to_vec()].concat()).is_infinite());

    let f = fixture::operators();
    let at = |content: &[u8]| -> Vec<Shown> {
        let content = [b"/T3 10 Tf BT ".as_slice(), content, b" (a) Tj ET"].concat();
        run(
            &f.pdf,
            &content,
            f.resources(),
            &Limits::DEFAULT,
            Opts::default(),
        )
        .unwrap()
        .0
        .glyphs
    };
    let origin = |g: &[Shown]| (g[0].origin.x, g[0].origin.y);
    assert_eq!(origin(&at(b"5 Td")), (0.0, 5.0));
    assert_eq!(origin(&at(b"/x 7 Td")), (0.0, 7.0));
    let huge = [b"9".repeat(40).as_slice(), b".0 3 Td"].concat();
    assert_eq!(origin(&at(&huge)), (0.0, 0.0));
    let width = |g: &[Shown]| g[0].quad.bounds().width();
    assert_eq!(width(&at(b"50 Tz")), 5.0);
    assert_eq!(width(&at(b"50 50 Tz")), 10.0);
}

/// A string's text rendering matrix is `Tz`, the size and the rise, then
/// the text matrix and the CTM; and a glyph's own text matrix starts a
/// string where it is.
#[test]
fn matrices_where_text_is() {
    let f = fixture::operators();
    let content = b"2 0 0 2 0 0 cm BT /T3 10 Tf 50 Tz 3 Ts 0 1 -1 0 300 500 Tm (ab) Tj ET";
    let mut seen = Vec::new();
    interpret(
        &f.pdf,
        &Concat::new(&[content]),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
        Opts::default(),
        |event| {
            if let Kind::Text(show) = event.kind {
                seen.push((
                    show.trm(),
                    show.glyphs().map(|g| g.tm()).collect::<Vec<_>>(),
                ));
            }
            ControlFlow::Continue(())
        },
    )
    .unwrap();
    let m = |a, b, c, d, e, f| Affine { a, b, c, d, e, f };
    let [(trm, tms)] = &seen[..] else {
        panic!("{seen:?}")
    };
    assert_eq!(*trm, m(0.0, 10.0, -20.0, 0.0, 594.0, 1000.0));
    let tm = m(0.0, 1.0, -1.0, 0.0, 300.0, 500.0);
    assert_eq!(*tms, [tm, Affine { f: 505.0, ..tm }]);
}

/// Each event `f`'s page gives, in order, handed to `visit`.
fn each(f: &Fixture, opts: Opts, mut visit: impl FnMut(Event<'_>)) -> Summary {
    let content = f.content();
    interpret(
        &f.pdf,
        content.concat(),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
        opts,
        |event| {
            visit(event);
            ControlFlow::Continue(())
        },
    )
    .expect("the fixture interprets")
}

/// `r` is `[x0, y0, x1, y1]`, to a thousandth of a point.
fn assert_rect(r: Rect, [x0, y0, x1, y1]: [f32; 4], what: &str) {
    let worked = [r.x0, r.y0, r.x1, r.y1];
    assert!(
        worked
            .iter()
            .zip([x0, y0, x1, y1])
            .all(|(&a, b)| close(a, b)),
        "{what}: {worked:?}, not {:?}",
        [x0, y0, x1, y1]
    );
}

fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// The corners of the box `x, y, w, h`, in a quad's order.
fn corners(x: f32, y: f32, w: f32, h: f32) -> [(f32, f32); 4] {
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

/// Each image of [`fixture::images`] is the unit square through its CTM,
/// in a form too; each inline image's data is where it's written, by
/// each rule that finds its end; what's said of each is read; and the
/// PostScript XObject is reported, not drawn. Where XObjects are judged
/// on their own ([`Forms::Report`]), none is read, and only the inline
/// images are drawn.
#[test]
fn each_image_is_its_unit_square_through_the_ctm() {
    let f = fixture::images();
    let content = f.content();
    let bytes = content.concat().bytes();
    let mut images = Vec::new();
    let mut ignored = Vec::new();
    each(&f, Opts::default(), |event| match event.kind {
        Kind::Image(image) => {
            if let ImageOf::Inline { data } = &image.of {
                assert_eq!(event.at.tok, Tok::InlineImage(data.clone()));
            }
            images.push((image, event.at.stream.cloned()));
        }
        Kind::Ignored(id) => ignored.push(id),
        _ => {}
    });
    let mut worked: Vec<_> = (0..7)
        .map(|i| corners(100.0 + 30.0 * i as f32, 700.0, 20.0, 10.0))
        .collect();
    worked.push([
        (400.0, 700.0),
        (400.0, 710.0),
        (380.0, 710.0),
        (380.0, 700.0),
    ]);
    for x in [100.0, 200.0, 300.0, 400.0] {
        worked.push(corners(x, 500.0, 50.0, 50.0));
    }
    worked.push([
        (310.0, 400.0),
        (350.0, 400.0),
        (360.0, 440.0),
        (320.0, 440.0),
    ]);
    assert_eq!(images.len(), worked.len(), "{images:#?}");
    for (i, ((image, _), worked)) in images.iter().zip(&worked).enumerate() {
        let quad = image.quad.0.map(|p| (p.x, p.y));
        assert!(
            quad.iter()
                .zip(worked)
                .all(|(a, b)| close(a.0, b.0) && close(a.1, b.1)),
            "image {i}: {quad:?}, not {worked:?}"
        );
    }
    let data = |i: usize| match &images[i].0.of {
        ImageOf::Inline { data } => &bytes[data.clone()],
        other => panic!("{other:?}"),
    };
    assert_eq!(data(0), [0, 255, 0, 255]);
    assert_eq!(data(1), b"00FF00FF>");
    assert_eq!(data(2), b"z~>");
    assert_eq!(data(3), b"\x03\x00\xff\x00\xff\x80");
    assert!(data(4).starts_with(b"\xff\xd8") && data(4).ends_with(b"\xff\xd9"));
    // The Flate data, found by its `/L` and by the `EI` after it.
    assert!(data(5).starts_with(b"\x78"));
    assert_eq!(data(5), data(6));
    assert_eq!(data(7), b"\x40\x80");
    let xobjects = f.page().get(b"Resources").unwrap().as_dict().unwrap();
    let xobjects = xobjects.get(b"XObject").unwrap().as_dict().unwrap();
    let xobject = |name: &[u8]| xobjects.get(name).unwrap().as_reference().unwrap();
    let facts: Vec<_> = (images.iter())
        .map(|(i, _)| {
            (
                i.stencil,
                i.smask.is_some(),
                i.mask.is_some(),
                i.oc.is_some(),
            )
        })
        .collect();
    let none = (false, false, false, false);
    let im = (false, true, false, true);
    #[rustfmt::skip]
    assert_eq!(facts, [
        none, none, none, none, none, none, none, (true, false, false, false),
        im, (true, false, false, false), (false, false, true, false), none, im,
    ]);
    assert_eq!(images[8].0.of, ImageOf::XObject(xobject(b"Im")));
    assert_eq!(images[12].0.of, ImageOf::XObject(xobject(b"Im")));
    assert_eq!(images[12].1.as_ref().map(UseId::depth), Some(1));
    assert!(images[..12].iter().all(|(_, stream)| stream.is_none()));
    assert_eq!(ignored, [xobject(b"PS")]);

    let opts = Opts {
        forms: Forms::Report,
    };
    let mut reported = Vec::new();
    each(&f, opts, |event| match event.kind {
        Kind::Image(image) => reported.push(image.of),
        Kind::Ignored(id) => panic!("{id:?} read"),
        _ => {}
    });
    assert_eq!(reported.len(), 8);
    assert!((reported.iter()).all(|of| matches!(of, ImageOf::Inline { .. })));
}

/// A path as a test sees it: its segments, paint and bounds, and the
/// bounds and parts of the clip it's drawn in.
#[derive(Debug)]
struct Painted {
    segs: Vec<Seg>,
    paint: Paint,
    clip: Option<FillRule>,
    bounds: Rect,
    clip_bounds: Option<Rect>,
    clip_parts: Vec<ClipPart>,
}

fn painted(f: &Fixture) -> (Vec<Painted>, Vec<ShadingDrawn>) {
    let mut paths = Vec::new();
    let mut shadings = Vec::new();
    each(f, Opts::default(), |event| match event.kind {
        Kind::Path(p) => paths.push(Painted {
            segs: p.segments.to_vec(),
            paint: p.paint,
            clip: p.clip,
            bounds: p.bounds,
            clip_bounds: event.state.clip_bounds(),
            clip_parts: event.state.clip.parts().cloned().collect(),
        }),
        Kind::Shading(s) => shadings.push(s),
        _ => {}
    });
    (paths, shadings)
}

/// Each path of [`fixture::clips`] is painted with the bounds worked out
/// for it: half the width past its points, more at a square cap and a
/// miter join, and a closed point with round caps a dot.
#[test]
fn paths_are_painted_with_what_their_strokes_reach() {
    let (paths, _) = painted(&fixture::clips());
    let s = 2.0 * std::f32::consts::SQRT_2;
    #[rustfmt::skip]
    let worked: [(Paint, [f32; 4]); 6] = [
        (Paint::Stroke, [99.0, 699.0, 201.0, 701.0]),
        (Paint::Fill, [100.0, 600.0, 150.0, 650.0]),
        (Paint::Stroke, [300.0 - s, 700.0 - s, 300.0 + s, 750.0 + s]),
        (Paint::Stroke, [390.0, 590.0, 510.0, 660.0]),
        (Paint::Stroke, [47.5, 47.5, 52.5, 52.5]),
        (Paint::Stroke, [95.0, 95.0, 205.0, 205.0]),
    ];
    for (i, (p, (paint, bounds))) in paths.iter().zip(worked).enumerate() {
        assert_eq!((p.paint, p.clip), (paint, None), "path {i}");
        assert_rect(p.bounds, bounds, &format!("path {i}"));
        assert!(p.clip_bounds.is_none(), "path {i}");
    }
    use Seg::*;
    #[rustfmt::skip]
    assert_eq!(paths[1].segs, [
        Move(pt(100.0, 600.0)), Line(pt(150.0, 600.0)), Line(pt(150.0, 650.0)),
        Line(pt(100.0, 650.0)), Line(pt(100.0, 600.0)), Close,
    ]);
    assert_eq!(
        paths[4].segs,
        [Move(pt(50.0, 50.0)), Line(pt(50.0, 50.0)), Close]
    );
    assert_eq!(
        paths[5].segs,
        [
            Move(pt(100.0, 100.0)),
            Curve(pt(100.0, 200.0), pt(200.0, 200.0), pt(200.0, 100.0))
        ]
    );
}

/// Each clip of [`fixture::clips`] cuts what's drawn after it as worked
/// out: a path clip, an even-odd one within it, text in a clipping mode
/// but not in a Type3 font, a path of one point, and a form's `/BBox`;
/// and a shading paints what the clip leaves within its `/BBox`.
#[test]
fn clips_cut_what_is_drawn_after_them() {
    let (paths, shadings) = painted(&fixture::clips());
    let clip = |i: usize| -> &Painted { &paths[i] };
    assert_eq!(
        (clip(6).paint, clip(6).clip),
        (Paint::None, Some(FillRule::NonZero))
    );
    assert_rect(clip(6).bounds, [100.0, 400.0, 300.0, 500.0], "the clip");
    assert_eq!(
        (clip(7).paint, clip(7).clip),
        (Paint::None, Some(FillRule::EvenOdd))
    );
    let probe = [-0.5, -0.5, 612.5, 792.5];
    let probed = |i: usize, bounds: Option<[f32; 4]>| {
        assert_eq!(paths[i].paint, Paint::Stroke, "probe {i}");
        match (paths[i].clip_bounds, bounds) {
            (Some(r), Some(worked)) => assert_rect(r, worked, &format!("probe {i}'s clip")),
            (r, worked) => assert_eq!(r.is_some(), worked.is_some(), "probe {i}: {r:?}"),
        }
    };
    probed(8, Some([150.0, 420.0, 250.0, 480.0]));
    assert_rect(paths[8].bounds, probe, "the probe");
    let rules: Vec<_> = (paths[8].clip_parts.iter())
        .map(|p| match p {
            ClipPart::Path { rule, .. } => Some(*rule),
            ClipPart::Text { .. } => None,
        })
        .collect();
    // The rectangle holds the triangle, which takes its place.
    assert_eq!(rules, [Some(FillRule::EvenOdd)]);
    probed(9, Some([100.0, 400.0, 300.0, 500.0]));
    probed(10, Some([100.0, 300.0 - 2.07, 111.12, 300.0 + 7.18]));
    let [ClipPart::Text { glyphs, .. }] = &paths[10].clip_parts[..] else {
        panic!("{:?}", paths[10].clip_parts)
    };
    assert_eq!(glyphs.len(), 2);
    assert_rect(
        glyphs[1].bounds(),
        [105.56, 297.93, 111.12, 307.18],
        "the clip's b",
    );
    // Type3 text is only filled, so it clips nothing.
    probed(11, None);
    // A path of one point clips everything away.
    assert_eq!(
        (clip(12).paint, clip(12).clip),
        (Paint::None, Some(FillRule::NonZero))
    );
    // The form's `/BBox`, through its `/Matrix` and where it's drawn.
    probed(13, Some([400.0, 100.0, 600.0, 200.0]));
    let [ClipPart::Path { segments, .. }] = &paths[13].clip_parts[..] else {
        panic!("{:?}", paths[13].clip_parts)
    };
    use Seg::*;
    #[rustfmt::skip]
    assert_eq!(&segments[..], [
        Move(pt(400.0, 100.0)), Line(pt(400.0, 200.0)), Line(pt(600.0, 200.0)),
        Line(pt(600.0, 100.0)), Line(pt(400.0, 100.0)), Close,
    ]);
    assert_eq!(paths.len(), 14);

    let [within, nothing, page] = &shadings[..] else {
        panic!("{shadings:?}")
    };
    assert!(within.shading.is_some());
    assert_rect(
        within.bounds.unwrap(),
        [100.0, 400.0, 150.0, 450.0],
        "the shading",
    );
    assert_eq!(nothing.bounds, Some(Rect::default()));
    assert_eq!(page.bounds, None);
}

/// A clip within the rectangle the last one was takes its place, as in
/// PDFium, and cuts what's drawn the same; one that isn't is added.
#[test]
fn a_clip_within_a_rectangle_takes_its_place() {
    let pdf = Pdf::with_version("1.7");
    let clips = |content: &[u8]| -> (usize, Option<Rect>) {
        let mut seen = None;
        let opts = Opts::default();
        interpret(
            &pdf,
            &Concat::new(&[content]),
            None,
            &Limits::DEFAULT,
            &Fonts::default(),
            opts,
            |event| {
                if let Kind::Path(p) = event.kind
                    && p.paint == Paint::Stroke
                {
                    seen = Some((event.state.clip.parts().count(), event.state.clip_bounds()));
                }
                ControlFlow::Continue(())
            },
        )
        .unwrap();
        seen.unwrap()
    };
    let probe = b"0 0 m 1 1 l S";
    let narrowing = [
        b"0 0 612 792 re W n 10 10 100 100 re W n 20 20 50 50 re W n ".as_slice(),
        probe,
    ]
    .concat();
    let (parts, bounds) = clips(&narrowing);
    assert_eq!(parts, 1);
    assert_rect(bounds.unwrap(), [20.0, 20.0, 70.0, 70.0], "narrowed");
    let widening = [
        b"20 20 50 50 re W n 0 0 612 792 re W n 15 15 m 80 15 l 15 80 l h W n ".as_slice(),
        probe,
    ]
    .concat();
    // The page's rectangle isn't within the first, and holds the triangle.
    let (parts, bounds) = clips(&widening);
    assert_eq!(parts, 2);
    assert_rect(bounds.unwrap(), [20.0, 20.0, 70.0, 70.0], "widened");
}

/// Clips as many as content can add without a `Q` go, when the state
/// does, without recursion: on a thread with the default stack.
#[test]
fn a_clip_as_long_as_content_makes_it_is_dropped() {
    std::thread::spawn(|| {
        let pdf = Pdf::with_version("1.7");
        let content: Vec<u8> = (1..=100_000)
            .flat_map(|i| format!("0 0 {i} {i} re W n ").into_bytes())
            .collect();
        let (_, summary) = run(&pdf, &content, None, &Limits::DEFAULT, Opts::default()).unwrap();
        assert_eq!(summary.operators, 300_000);
    })
    .join()
    .unwrap();
}

/// Paths are built as PDFium builds them: after an `m`, path operators
/// read their numbers PDFium's own way; a move after a move takes its
/// place; a line with no path is dropped, and a move it ends with; `n`
/// alone draws nothing; `h` and `b*` close as PDFium closes; and the CTM
/// where a path ends places it.
#[test]
fn paths_are_built_as_pdfium_builds_them() {
    let pdf = Pdf::with_version("1.7");
    let paths = |content: &[u8]| -> Vec<Vec<Seg>> {
        let mut paths = Vec::new();
        interpret(
            &pdf,
            &Concat::new(&[content]),
            None,
            &Limits::DEFAULT,
            &Fonts::default(),
            Opts::default(),
            |event| {
                if let Kind::Path(p) = event.kind {
                    paths.push(p.segments.to_vec());
                }
                ControlFlow::Continue(())
            },
        )
        .unwrap();
        paths
    };
    use Seg::*;
    // Its first two numbers, and what's left in the buffer.
    assert_eq!(
        paths(b"10 10 m 1 2 3 4 l 5 l S"),
        [[Move(pt(10.0, 10.0)), Line(pt(1.0, 2.0)), Line(pt(5.0, 2.0))]]
    );
    // After an operator that isn't a path's, `l` takes two operands only.
    assert_eq!(
        paths(b"10 10 m 1 0 0 1 0 0 cm 1 2 3 4 l 5 6 l S"),
        [[Move(pt(10.0, 10.0)), Line(pt(5.0, 6.0))]]
    );
    assert_eq!(
        paths(b"5 5 l 10 10 m 20 20 m 30 30 l 40 40 m S"),
        [[Move(pt(20.0, 20.0)), Line(pt(30.0, 30.0))]]
    );
    assert!(paths(b"10 10 m 20 20 l n").is_empty());
    assert_eq!(
        paths(b"0 0 m 1 1 2 2 v 3 3 4 4 y S"),
        [[
            Move(pt(0.0, 0.0)),
            Curve(pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 2.0)),
            Curve(pt(3.0, 3.0), pt(4.0, 4.0), pt(4.0, 4.0)),
        ]]
    );
    let (o, x) = (pt(0.0, 0.0), pt(1.0, 0.0));
    assert_eq!(
        paths(b"0 0 m 1 0 l h S 0 0 m 1 0 l 0 0 l h S 0 0 m 1 0 l 0 0 l b*"),
        [
            vec![Move(o), Line(x), Line(o), Close],
            vec![Move(o), Line(x), Line(o), Close],
            vec![Move(o), Line(x), Line(o), Line(o), Close],
        ]
    );
    assert_eq!(
        paths(b"0 0 m 1 1 l 2 0 0 2 0 0 cm S"),
        [[Move(o), Line(pt(2.0, 2.0))]]
    );
}

/// Marked content of [`fixture::marked`] is read as PDFium reads it:
/// each range opened and ended where it says, in a form too, what each
/// glyph is in, and what's said of each.
#[test]
fn marked_content_is_read_as_pdfium_reads_it() {
    let f = fixture::marked();
    let content = f.content();
    let page_len = content.concat().bytes().len();
    let mut log = Vec::new();
    let mut marks = Vec::new();
    let mut ends = Vec::new();
    let mut form_oc = None;
    let mut written = Vec::new();
    let tag = |m: &Mark| String::from_utf8_lossy(&m.tag).into_owned();
    let tags = |m: &Marks| m.iter().map(tag).collect::<Vec<_>>().join(",");
    let summary = each(&f, Opts::default(), |event| match event.kind {
        Kind::MarkedBegin(m) => {
            log.push(format!("begin {} in [{}]", tag(m), tags(event.marks)));
            let bytes = &event.at.content()[m.at.clone()];
            written.push(String::from_utf8_lossy(bytes).into_owned());
            marks.push(m.clone());
        }
        Kind::MarkedEnd(m) => {
            log.push(format!("end {}", tag(m)));
            let empty = event.at.tok == Tok::Op(b"");
            ends.push((event.at.op.clone(), event.at.content().len(), empty));
        }
        Kind::MarkedPoint(m) => {
            log.push(format!("point {}", tag(m)));
            marks.push(m.clone());
        }
        Kind::Text(show) => log.push(format!(
            "{} in [{}]",
            String::from_utf8_lossy(show.string),
            tags(event.marks)
        )),
        Kind::FormBegin(u) => {
            log.push("form".into());
            form_oc = u.oc;
        }
        Kind::FormEnd(_) => log.push("form ends".into()),
        _ => {}
    });
    #[rustfmt::skip]
    assert_eq!(log, [
        "begin P in []", "a in [P]", "begin Span in [P]", "b in [Span,P]", "end Span", "end P",
        "begin OC in []", "a in [OC]",
        "form", "begin Span in [OC]", "a in [Span,OC]", "end Span", "begin Open in [OC]",
        "end Open", "form ends",
        "end OC",
        "begin Artifact in []", "b in [Artifact]", "end Artifact",
        "point Tag", "point Tag",
        "begin Open in []", "a in [Open]", "end Open",
    ]);
    let said: Vec<_> = (marks.iter())
        .map(|m| (m.mcid, m.actual_text.as_deref(), m.oc.is_some()))
        .collect();
    #[rustfmt::skip]
    assert_eq!(said, [
        (Some(0), None, false), (None, Some("A"), false), (None, None, true),
        (Some(7), Some("inside"), false), (None, None, false), (None, None, false),
        (None, None, false), (Some(5), None, false), (None, None, false),
    ]);
    let resources = f.resources().unwrap().as_dict().unwrap();
    let properties = resources.get(b"Properties").unwrap().as_dict().unwrap();
    assert_eq!(
        marks[2].oc,
        properties.get(b"MC0").unwrap().as_reference().ok()
    );
    assert!(form_oc.is_some() && form_oc != marks[2].oc);
    assert_eq!(marks[3].stream.as_ref().map(UseId::depth), Some(1));
    assert!(matches!(&marks[7].props, Some(Props::Named(name, Some(_))) if name == b"P1"));
    assert_eq!(marks[6].props, None);
    // Where each range opens: its tag, its list and its operator.
    #[rustfmt::skip]
    assert_eq!(written, [
        "/P << /MCID 0 >> BDC", "/Span << /ActualText <FEFF0041> >> BDC", "/OC /MC0 BDC",
        "/Span << /ActualText (inside) /MCID 7 >> BDC", "/Open BMC", "/Artifact BMC", "/Open BMC",
    ]);
    assert_eq!(summary.deepest_marked, 2);
    // Each range open where its content ends ends there.
    let ends_at = |(op, len, empty): &(Range<usize>, usize, bool)| *op == (*len..*len) && *empty;
    let at_end: Vec<bool> = ends.iter().map(ends_at).collect();
    #[rustfmt::skip]
    assert_eq!(at_end, [false, false, false, true, false, false, true]);
    assert_eq!(ends[6].1, page_len);
}

/// A spacing, `Tc` or `Tw`: 0 of either sign as often as not.
fn spacing() -> impl Strategy<Value = f32> {
    prop_oneof![Just(0.0f32), Just(-0.0f32), -50.0f32..50.0]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A font whose codes are bytes is laid out without its codes split
    /// ([`text::bytes_end`]) to the bit where its codes are laid out one
    /// at a time ([`text::laid_end`]): with any widths, at any size,
    /// spaced or not, `Tc` and `Tw` 0 of either sign, which the first
    /// leaves out, from where an operator's text starts or on from a
    /// string before it.
    #[test]
    fn byte_codes_end_where_laid_out(
        widths in prop::collection::vec(-2000i64..3000, 256),
        size in prop_oneof![Just(0.0f32), Just(-0.0f32), -100.0f32..100.0, Just(3e38f32)],
        word in spacing(),
        char in spacing(),
        from in prop_oneof![Just(0.0f32), -1000.0f32..1000.0],
        s in prop::collection::vec(prop_oneof![Just(b' '), any::<u8>()], 0..64),
    ) {
        let pdf = Pdf::with_version("1.7");
        let font = dictionary! {
            "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+Some",
            "FirstChar" => 0, "LastChar" => 255,
            "Widths" => widths.iter().map(|&w| w.into()).collect::<Vec<Object>>(),
        };
        let mut budget = 0;
        let font = Foreign::read(&pdf, &font, &Limits::DEFAULT, &mut budget).unwrap();
        let byte_widths = font.byte_widths().expect("a simple font's codes are bytes");
        prop_assert!((0..256u32).all(|c| byte_widths[c as usize] == font.width(c)));
        let mut state = State::default();
        (state.text.size, state.text.word_space, state.text.char_space) = (size, word, char);
        let fast = text::bytes_end(byte_widths, &state.text, &s, from);
        let (laid, glyphs) = text::laid_end(&font, &state.text, &s, from);
        prop_assert!(fast.to_bits() == laid.to_bits() || fast.is_nan() && laid.is_nan(), "{fast} {laid}");
        prop_assert_eq!(glyphs, s.len());
    }
}
