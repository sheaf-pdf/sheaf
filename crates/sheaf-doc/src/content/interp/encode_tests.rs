//! Content written again ([`encode`]): each fixture of [`fixture`], and
//! random runs of their operators, draw as their encoding draws, event
//! for event but for the operators themselves: each glyph and its box,
//! each path, image and shading, the clip each is drawn in, the marked
//! ranges each is in, and each form drawn, refused, begun and ended, to
//! the bit. The encoding lexes, and its `q`s and `Q`s and the resource
//! names it uses are the content's.

use std::cell::RefCell;
use std::ops::ControlFlow;

use proptest::prelude::*;

use super::*;
use crate::content::fixture::{self, Fixture};
use crate::content::{Balance, NameSet, balance, lex, names_used};
use crate::geom::{Affine, Rect};
use crate::source::Limits;

/// An event as the round trip compares it: what it is, but for its
/// numbers, and its numbers.
#[derive(Debug)]
pub(super) struct Seen {
    what: String,
    numbers: Vec<f32>,
}

/// What `content` draws on `f`'s page, with its resources: each event but
/// the operators, as the round trip compares them.
fn drawn(f: &Fixture, content: &[u8]) -> Result<Vec<Seen>, InterpError> {
    let mut seen = Vec::new();
    interpret(
        &f.pdf,
        &Concat::new(&[content]),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
        Opts::default(),
        |event| {
            seen.extend(self::seen(&event));
            ControlFlow::Continue(())
        },
    )?;
    Ok(seen)
}

/// `event` as the round trip compares it: None for an operator, which it
/// writes.
fn seen(event: &Event<'_>) -> Option<Seen> {
    (!matches!(event.kind, Kind::Op)).then(|| described(event))
}

/// `event` as [`seen`] compares it, and an operator as what it is, where,
/// and in what text state.
pub(super) fn described(event: &Event<'_>) -> Seen {
    let mut n = Vec::new();
    let what = match &event.kind {
        Kind::Op => {
            let text = &event.state.text;
            affine(&mut n, text.matrix);
            n.extend([text.line.x, text.line.y, text.pen.x, text.pen.y]);
            format!("op {:?} at {:?}", event.at.tok, event.at.op)
        }
        Kind::Text(show) => {
            let text = &event.state.text;
            n.extend([show.size, text.char_space, text.word_space, text.scale]);
            n.push(text.rise);
            affine(&mut n, show.start);
            let mut glyphs = Vec::new();
            for g in show.glyphs() {
                n.extend(g.quad.0.iter().flat_map(|p| [p.x, p.y]));
                n.extend([g.origin.x, g.origin.y, g.advance]);
                affine(&mut n, g.tm());
                glyphs.push((g.bytes.clone(), g.code, g.cid, g.approx, g.invisible));
            }
            let font = show.font.map(|f| f.id());
            format!(
                "text {:?} in {font:?}, stock {}, {:?}: {glyphs:?}",
                show.string, show.stock, show.mode
            )
        }
        Kind::FormBegin(u) | Kind::FormEnd(u) => {
            affine(&mut n, u.matrix);
            n.extend(u.bbox.iter().flat_map(rect));
            let begins = matches!(event.kind, Kind::FormBegin(_));
            let depth = u.id.depth();
            format!(
                "form {:?} at {depth}, begins {begins}, oc {:?}",
                u.form, u.oc
            )
        }
        Kind::FormRefused { form, why } => format!("refused {form:?}, {why:?}"),
        Kind::Path(p) => {
            let kinds = segments(&mut n, p.segments);
            n.extend(rect(&p.bounds));
            format!("path {kinds} {:?}, clips {:?}", p.paint, p.clip)
        }
        Kind::Image(image) => {
            n.extend(image.quad.0.iter().flat_map(|p| [p.x, p.y]));
            let of = match &image.of {
                ImageOf::XObject(id) => format!("{id:?}"),
                ImageOf::Inline { data } => format!("{:?}", &event.at.content()[data.clone()]),
            };
            let ImageDrawn {
                stencil,
                smask,
                mask,
                oc,
                ..
            } = image;
            format!("image {of}, stencil {stencil}, {smask:?} {mask:?} {oc:?}")
        }
        Kind::Shading(shading) => {
            n.extend(shading.bounds.iter().flat_map(rect));
            let bounded = shading.bounds.is_some();
            format!("shading {:?}, bounded {bounded}", shading.shading)
        }
        Kind::Ignored(id) => format!("ignored {id:?}"),
        Kind::MarkedBegin(m) => format!("begins {}", mark(m)),
        Kind::MarkedEnd(m) => format!("ends {}", mark(m)),
        Kind::MarkedPoint(m) => format!("point {}", mark(m)),
    };
    affine(&mut n, event.state.ctm);
    let clip: Vec<String> = (event.state.clip.parts())
        .map(|part| {
            n.extend(rect(&part.bounds()));
            match part {
                ClipPart::Path {
                    segments: s, rule, ..
                } => {
                    format!("{} {rule:?}", self::segments(&mut n, s))
                }
                ClipPart::Text { glyphs, .. } => {
                    n.extend(glyphs.iter().flat_map(|q| q.0).flat_map(|p| [p.x, p.y]));
                    format!("{} glyphs", glyphs.len())
                }
            }
        })
        .collect();
    let marks: Vec<String> = event.marks.iter().map(mark).collect();
    Seen {
        what: format!("{what}; in {marks:?}; clipped by {clip:?}"),
        numbers: n,
    }
}

/// What's said of a mark, but where it's written.
fn mark(m: &Mark) -> String {
    let depth = m.stream.as_ref().map_or(0, UseId::depth);
    format!(
        "{} {:?}, mcid {:?}, {:?}, oc {:?}, at {depth}",
        String::from_utf8_lossy(&m.tag),
        m.props,
        m.mcid,
        m.actual_text,
        m.oc
    )
}

fn affine(n: &mut Vec<f32>, m: Affine) {
    n.extend([m.a, m.b, m.c, m.d, m.e, m.f]);
}

fn rect(r: &Rect) -> [f32; 4] {
    [r.x0, r.y0, r.x1, r.y1]
}

/// `segs`' points, onto `n`, and what each is.
fn segments(n: &mut Vec<f32>, segs: &[Seg]) -> String {
    let mut kinds = String::new();
    for seg in segs {
        let (kind, points) = match *seg {
            Seg::Move(p) => ('m', vec![p]),
            Seg::Line(p) => ('l', vec![p]),
            Seg::Curve(a, b, c) => ('c', vec![a, b, c]),
            Seg::Close => ('h', Vec::new()),
        };
        kinds.push(kind);
        n.extend(points.iter().flat_map(|p| [p.x, p.y]));
    }
    kinds
}

/// Whether `a` and `b` are the same, to the bit but for a 0's sign: the
/// encoding places the line where the content did, by the same sums.
fn same(a: f32, b: f32) -> bool {
    a == b || a.is_nan() && b.is_nan()
}

/// `after` is `before`, event for event.
pub(super) fn assert_alike(before: &[Seen], after: &[Seen], what: &str) {
    assert_eq!(before.len(), after.len(), "{what}: events");
    for (i, (b, a)) in before.iter().zip(after).enumerate() {
        assert_eq!(b.what, a.what, "{what}: event {i}");
        let numbers = b.numbers.len() == a.numbers.len()
            && b.numbers.iter().zip(&a.numbers).all(|(&b, &a)| same(b, a));
        assert!(
            numbers,
            "{what}: event {i}, {}: {:?} written again {:?}",
            b.what, b.numbers, a.numbers
        );
    }
}

/// How `content`'s `q`s and `Q`s balance, and the names it uses, lexed
/// with `res`; and that it lexes.
fn facts(content: &[u8], res: &ResCx) -> (Balance, NameSet) {
    let lim = &Limits::DEFAULT;
    assert!(lex(content, res, lim).all(|t| t.is_ok()));
    let content = Concat::new(&[content]);
    let balance = balance(&content, res, lim).expect("it lexes");
    let names = names_used(&content, res, lim, &[]).expect("it lexes");
    (balance, names)
}

/// `content`, drawn on `f`'s page, draws as its encoding, which lexes, and
/// whose forms' do; each balances as it did and uses the names it did.
fn round_trip(f: &Fixture, content: &[u8], what: &str) -> Encoded {
    let lim = &Limits::DEFAULT;
    let concat = Concat::new(&[content]);
    let encoded = encode(&f.pdf, &concat, f.resources(), lim, &Fonts::default())
        .unwrap_or_else(|e| panic!("{what}: {e}"));
    let again = f.encoded(&encoded);
    let before = drawn(f, content).unwrap();
    let after = drawn(&again, &encoded.content).unwrap();
    assert_alike(&before, &after, what);
    let res = ResCx::read(&f.pdf, f.resources());
    assert_eq!(
        facts(content, &res),
        facts(&encoded.content, &res),
        "{what}"
    );
    for (id, written) in &encoded.forms {
        let form = f.pdf.get_object(*id).and_then(Object::as_stream).unwrap();
        let own = form.dict.get(b"Resources").ok();
        let res = ResCx::read(&f.pdf, own.or(f.resources()));
        let data = form.decompressed_content().unwrap();
        assert_eq!(facts(&data, &res), facts(written, &res), "{what}: {id:?}");
    }
    encoded
}

/// Each fixture draws as its encoding, a form's written once however
/// often it's drawn; those that move text to a line are written as the
/// text matrix's `Tm` and a `Td` from its origin to where they move it,
/// and every other operator as it was.
#[test]
fn every_fixture_draws_as_its_encoding() {
    for (name, f) in fixture::every() {
        let content = f.content();
        let encoded = round_trip(&f, content.concat().bytes(), name);
        let forms: Vec<ObjectId> = encoded.forms.iter().map(|(id, _)| *id).collect();
        let unique: std::collections::BTreeSet<_> = forms.iter().collect();
        assert_eq!(unique.len(), forms.len(), "{name}: a form written twice");
        for written in [&encoded.content]
            .into_iter()
            .chain(encoded.forms.iter().map(|(_, w)| w))
        {
            let text = String::from_utf8_lossy(written);
            for moved in [" TD\n", "T*\n", " '\n", " \"\n"] {
                assert!(!text.contains(moved), "{name}: {moved:?} in {text}");
            }
            let lines: Vec<&str> = text.lines().collect();
            for (i, _) in lines.iter().enumerate().filter(|(_, l)| l.ends_with(" Td")) {
                assert!(
                    lines[i - 1].ends_with(" Tm"),
                    "{name}: a Td moved from another line"
                );
            }
        }
    }
    let f = fixture::operators();
    let written = encode_of(&f);
    // `100 560 TD`, `0 -14 TD` and `T*`: the lines they move to, at 560,
    // 546 and 532 by the leading the second set.
    for line in ["560.0", "546.0", "532.0"] {
        assert!(written.contains(&format!("1.0 0.0 0.0 1.0 0.0 0.0 Tm\n100.0 {line} Td\n")));
    }
    // `10 TL 100 500 Td 4 1 (a a) "`: `Tw`, `Tc`, the next line, its string.
    let quote = "4.0 Tw\n1.0 Tc\n1.0 0.0 0.0 1.0 0.0 0.0 Tm\n100.0 490.0 Td\n(a a) Tj\n";
    assert!(written.contains(quote));
    // The rest as written, inline images whole.
    assert!(written.contains("[(ab) -1000 (a)] TJ\n"));
    let f = fixture::images();
    assert!(encode_of(&f).contains("BI /W 2 /H 2 /BPC 8 /CS /G /F /AHx ID 00FF00FF> EI\n"));
}

/// `f`'s page's content written again, as text.
fn encode_of(f: &Fixture) -> String {
    let content = f.content();
    let encoded = encode(
        &f.pdf,
        content.concat(),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
    )
    .unwrap();
    String::from_utf8_lossy(&encoded.content).into_owned()
}

/// A `Td` that moves nothing is written as it is, but one that moves the
/// pen back to the line is a `Tm`, the line at the text matrix's origin;
/// a `TD`'s leading is in the `Td` of the `T*` after it; and a line past
/// what a number holds can't be written.
#[test]
fn a_line_is_written_where_it_moves_to() {
    let f = fixture::operators();
    let written = |content: &[u8]| {
        let concat = Concat::new(&[content]);
        let encoded = encode(
            &f.pdf,
            &concat,
            f.resources(),
            &Limits::DEFAULT,
            &Fonts::default(),
        );
        encoded.map(|e| String::from_utf8(e.content).unwrap())
    };
    let past_f32 = format!("1{}.0", "0".repeat(39));
    let nowhere = format!("BT 0 0 Td {past_f32} 0 Td ET");
    assert_eq!(
        written(nowhere.as_bytes()).unwrap(),
        format!("BT\n0 0 Td\n{past_f32} 0 Td\nET\n")
    );
    // As it is, it ends PDFium's own reading of the path operators after
    // an `m`, so the `l` after it takes none of the `m`'s numbers, and
    // nothing is drawn.
    let paths = |content: &[u8]| drawn(&f, content).unwrap().len();
    let moved = written(b"100 100 m 0 0 Td l S").unwrap();
    assert_eq!([b"100 100 m l S", moved.as_bytes()].map(paths), [1, 0]);
    assert_eq!(
        written(b"BT /T3 10 Tf (a) Tj 0 0 Td ET").unwrap(),
        "BT\n/T3 10 Tf\n(a) Tj\n1.0 0.0 0.0 1.0 0.0 0.0 Tm\nET\n"
    );
    assert_eq!(
        written(b"BT 2 0 0 2 0 0 Tm 5 10 TD T* ET").unwrap(),
        "BT\n2 0 0 2 0 0 Tm\n2.0 0.0 0.0 2.0 0.0 0.0 Tm\n5.0 10.0 Td\n2.0 0.0 0.0 2.0 0.0 0.0 \
         Tm\n5.0 20.0 Td\nET\n"
    );
    // 3e38 is an `f32`, and twice it isn't.
    let far = format!("3{}.0", "0".repeat(38));
    let far = format!("BT {far} 0 Td {far} 0 Td ET");
    let second = far.rfind("Td").unwrap();
    let line = written(far.as_bytes());
    assert!(
        matches!(line, Err(EncodeError::Unwritable { form: None, at, why: Unwritable::Line }) if at == second),
        "{line:?}"
    );
}

/// An inline image whose `/L` is its data's length is written as it is;
/// one whose `/L` runs past the content, its data found by its `EI`,
/// isn't: content after it could make the `/L` reach, so the image would
/// end elsewhere.
#[test]
fn an_inline_image_is_written_where_it_ends_whatever_follows() {
    let f = fixture::operators();
    let written = |content: &[u8]| {
        let concat = Concat::new(&[content]);
        encode(
            &f.pdf,
            &concat,
            f.resources(),
            &Limits::DEFAULT,
            &Fonts::default(),
        )
    };
    let fits = written(b"q BI /W 2 /H 2 /F /Fl /L 2 ID xy EI Q");
    assert_eq!(
        fits.unwrap().content,
        b"q\nBI /W 2 /H 2 /F /Fl /L 2 ID xy EI\nQ\n"
    );
    let past = written(b"q BI /W 2 /H 2 /F /Fl /L 29 ID xy EI Q");
    assert!(
        matches!(
            past,
            Err(EncodeError::Unwritable {
                form: None,
                at: 2,
                why: Unwritable::InlineLength
            })
        ),
        "{past:?}"
    );
    // Written with a dozen `Q`s after it, the `/L` reaches past the `EI`.
    let bytes = [
        &b"q BI /W 2 /H 2 /F /Fl /L 29 ID xy EI\n"[..],
        &b"Q\n".repeat(12),
    ]
    .concat();
    let lexed: Vec<_> = lex(&bytes, &ResCx::none(), &Limits::DEFAULT).collect();
    assert!(lexed[1].is_err(), "{lexed:?}");
}

thread_local! {
    /// Each fixture, and the pieces of content its operators make: each
    /// operator with its operands, and each token on its own.
    static PIECES: RefCell<Vec<(Fixture, Vec<Vec<u8>>)>> = RefCell::new(
        fixture::every().into_iter().map(|(_, f)| {
            let pieces = pieces(&f);
            (f, pieces)
        }).collect()
    );
}

/// `f`'s pieces: of its page's content, and each form's.
fn pieces(f: &Fixture) -> Vec<Vec<u8>> {
    let lim = &Limits::DEFAULT;
    let res = ResCx::read(&f.pdf, f.resources());
    let forms = (f.pdf.objects.values())
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form"))
        .map(|s| s.decompressed_content().unwrap());
    let mut pieces = Vec::new();
    for content in std::iter::once(f.content().into_bytes()).chain(forms) {
        let mut from = None;
        for token in lex(&content, &res, lim) {
            let (at, tok) = token.expect("the fixture lexes");
            pieces.push(content[at.clone()].to_vec());
            let start = *from.get_or_insert(at.start);
            if !matches!(tok, Tok::Operand(_)) {
                pieces.push(content[start..at.end].to_vec());
                from = None;
            }
        }
    }
    pieces
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Runs of a fixture's operators and tokens, in any order, drawn on its
    /// page, draw as their encoding.
    #[test]
    fn runs_of_operators_draw_as_their_encoding(
        fixture in any::<prop::sample::Index>(),
        picks in prop::collection::vec(any::<prop::sample::Index>(), 0..48),
    ) {
        PIECES.with(|cases| {
            let cases = cases.borrow();
            let (f, pieces) = &cases[fixture.index(cases.len())];
            let content: Vec<u8> = picks
                .iter()
                .flat_map(|p| [&pieces[p.index(pieces.len())][..], b" "].concat())
                .collect();
            if drawn(f, &content).is_ok() {
                round_trip(f, &content, &String::from_utf8_lossy(&content));
            }
        });
    }
}
