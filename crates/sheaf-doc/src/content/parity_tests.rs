//! sheaf's content writer and pdf-content-streams', driven by the same
//! calls (M18 slice 19): the same content bytes, and the same resources
//! under the same names, so the writer can be swapped for the family's
//! with every byte sheaf writes held. Also the two digests a resource is
//! named by, over the resources sheaf makes, and the names a resources
//! dictionary already binds, read both ways.
//!
//! Where the two part, a fixed case shows the difference exactly: a whole
//! number of 32 bits or more ([`a_whole_number_past_32_bits_keeps_its_point`]),
//! an alpha that isn't finite ([`an_alpha_that_isnt_finite_is_written_0`])
//! and one resource drawn in two categories
//! ([`a_resource_drawn_in_two_categories_is_a_need_in_each`]).
//!
//! It goes with sheaf's writer, when the family's is the only one.

use std::collections::BTreeSet;

use proptest::prelude::*;
use sheaf_image::io::{PageImage, Pixels};

use crate::annot::{Box2, Pt};
use crate::geom::{Affine, Rect};
use crate::pdf::content as fam;
use crate::pdf::{
    Dictionary, Document as Pdf, Object, ObjectId, Str, Stream, StringFormat, Value, dictionary,
    resolve_fn,
};

use super::sink::{Sink, reserved_shallow};
use super::{
    Blend, Color, ContentWriter, FillRule, Font, GState, Glyphless, LineCap, LineJoin, Paint, Res,
    ResBody, ResId, ResKind, Reserved, ResourceNeeds, Std14, TextMode, Unencoded,
};

// ---- The calls, and each writer driven by them ----

/// A font sheaf draws text in.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Face {
    Std(usize),
    Glyphless,
}

static STD14: [Std14; 14] = Std14::ALL;
static GLYPHLESS: Glyphless = Glyphless;

const HELVETICA: Face = Face::Std(4);
const HELVETICA_BOLD: Face = Face::Std(5);
const ZAPF: Face = Face::Std(13);

impl Face {
    fn font(self) -> &'static dyn Font {
        match self {
            Face::Std(i) => &STD14[i],
            Face::Glyphless => &GLYPHLESS,
        }
    }

    /// Its resource as the family's writer takes it: sheaf's id, and the
    /// objects.
    fn family(self) -> fam::Res {
        let res = self.font().resource();
        fam::Res::named(fam::ResId(res.id.0), fam::ResBody::Objects(res.objects))
            .expect("a font's objects are held in memory")
    }
}

/// A resource content draws with, made the same way for each writer.
#[derive(Debug, Clone)]
enum Drawn {
    Written(ObjectId),
    Objects(Vec<Object>),
}

impl Drawn {
    fn old(&self) -> Res {
        match self {
            Drawn::Written(id) => Res::written(*id),
            Drawn::Objects(objects) => Res::of(objects.clone()),
        }
    }

    fn family(&self) -> fam::Res {
        match self {
            Drawn::Written(id) => fam::Res::written(*id),
            Drawn::Objects(objects) => fam::Res::of(objects.clone()).expect("held in memory"),
        }
    }
}

/// A call on a content writer.
#[derive(Debug, Clone)]
enum Call {
    Save,
    Restore,
    Transform(Affine),
    FillColor(Color),
    StrokeColor(Color),
    GState(GState),
    LineWidth(f32),
    Dash(Vec<f32>, f32),
    LineCap(LineCap),
    LineJoin(LineJoin),
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CurveTo([f32; 6]),
    Rect([f32; 4]),
    Close,
    Paint(Paint),
    Clip(FillRule),
    BeginText,
    EndText,
    Font(Face, f32),
    /// Text, and the font it's in: the one sheaf's writer remembers, and
    /// the one a caller of the family's encodes with. None is text with
    /// no font set, which sheaf's writer doesn't draw and a caller of the
    /// family's has no codes for.
    Text(Option<Face>, String),
    TextMatrix(Affine),
    TextMove(f32, f32),
    NextLine,
    TextMode(TextMode),
    HorizontalScale(f32),
    Leading(f32),
    Draw(Drawn),
    Image(Drawn, Box2),
    PlaceForm(Drawn, Rect, Affine, Box2),
    BeginTagged(String),
    BeginMarked(String, Dictionary),
    EndMarked,
    Operation(Vec<Vec<u8>>, Vec<u8>),
}

fn array(m: Affine) -> [f32; 6] {
    [m.a, m.b, m.c, m.d, m.e, m.f]
}

/// sheaf's writer after `calls`, and what each text left unencoded.
fn old(calls: &[Call]) -> (ContentWriter<'static>, Vec<Unencoded>) {
    let mut w = ContentWriter::new();
    let mut unencoded = Vec::new();
    for call in calls {
        match call.clone() {
            Call::Save => w.save(),
            Call::Restore => w.restore(),
            Call::Transform(m) => w.transform(m),
            Call::FillColor(c) => w.fill_color(c),
            Call::StrokeColor(c) => w.stroke_color(c),
            Call::GState(g) => w.gstate(g),
            Call::LineWidth(x) => w.line_width(x),
            Call::Dash(dashes, phase) => w.dash(&dashes, phase),
            Call::LineCap(cap) => w.line_cap(cap),
            Call::LineJoin(join) => w.line_join(join),
            Call::MoveTo(x, y) => w.move_to(x, y),
            Call::LineTo(x, y) => w.line_to(x, y),
            Call::CurveTo([x1, y1, x2, y2, x3, y3]) => w.curve_to(
                Pt { x: x1, y: y1 },
                Pt { x: x2, y: y2 },
                Pt { x: x3, y: y3 },
            ),
            Call::Rect([x, y, width, height]) => w.rect(x, y, width, height),
            Call::Close => w.close(),
            Call::Paint(paint) => w.paint(paint),
            Call::Clip(rule) => w.clip(rule),
            Call::BeginText => w.begin_text(),
            Call::EndText => w.end_text(),
            Call::Font(face, size) => w.font(face.font(), size),
            Call::Text(face, s) => {
                let left = w.text(&s);
                if face.is_some() {
                    unencoded.push(left);
                }
            }
            Call::TextMatrix(m) => w.text_matrix(m),
            Call::TextMove(x, y) => w.text_move(x, y),
            Call::NextLine => w.next_line(),
            Call::TextMode(mode) => w.text_mode(mode),
            Call::HorizontalScale(x) => w.horizontal_scale(x),
            Call::Leading(x) => w.leading(x),
            Call::Draw(res) => w.draw(res.old()),
            Call::Image(res, target) => w.image(res.old(), target),
            Call::PlaceForm(res, bbox, matrix, target) => {
                w.place_form(res.old(), bbox, matrix, target);
            }
            Call::BeginTagged(tag) => w.begin_tagged(&tag),
            Call::BeginMarked(tag, list) => w.begin_marked(&tag, list),
            Call::EndMarked => w.end_marked(),
            Call::Operation(operands, op) => {
                w.operation(operands.iter().map(Vec::as_slice), &op);
            }
        }
    }
    (w, unencoded)
}

/// The family's writer after `calls`, each made as sheaf's callers will
/// make it: geometry as plain numbers, text as the codes its font encodes
/// it as, a property list as a resource, a form placed by the matrix
/// sheaf's geometry gives.
fn family(calls: &[Call]) -> (fam::ContentWriter, Vec<Unencoded>) {
    let mut w = fam::ContentWriter::new();
    let mut unencoded = Vec::new();
    for call in calls {
        match call.clone() {
            Call::Save => w.save(),
            Call::Restore => w.restore(),
            Call::Transform(m) => w.transform(array(m)),
            Call::FillColor(c) => w.fill_color(color(c)),
            Call::StrokeColor(c) => w.stroke_color(color(c)),
            Call::GState(g) => w.gstate(gstate(g)),
            Call::LineWidth(x) => w.line_width(x),
            Call::Dash(dashes, phase) => w.dash(&dashes, phase),
            Call::LineCap(cap) => w.line_cap(match cap {
                LineCap::Butt => fam::LineCap::Butt,
                LineCap::Round => fam::LineCap::Round,
                LineCap::Square => fam::LineCap::Square,
            }),
            Call::LineJoin(join) => w.line_join(match join {
                LineJoin::Miter => fam::LineJoin::Miter,
                LineJoin::Round => fam::LineJoin::Round,
                LineJoin::Bevel => fam::LineJoin::Bevel,
            }),
            Call::MoveTo(x, y) => w.move_to(x, y),
            Call::LineTo(x, y) => w.line_to(x, y),
            Call::CurveTo([x1, y1, x2, y2, x3, y3]) => w.curve_to(x1, y1, x2, y2, x3, y3),
            Call::Rect([x, y, width, height]) => w.rect(x, y, width, height),
            Call::Close => w.close(),
            Call::Paint(p) => w.paint(paint(p)),
            Call::Clip(rule) => w.clip(match rule {
                FillRule::NonZero => fam::FillRule::NonZero,
                FillRule::EvenOdd => fam::FillRule::EvenOdd,
            }),
            Call::BeginText => w.begin_text(),
            Call::EndText => w.end_text(),
            Call::Font(face, size) => w.font(face.family(), size),
            Call::Text(None, _) => {}
            Call::Text(Some(face), s) => {
                let (codes, left) = face.font().encode(&s);
                w.show(&codes);
                unencoded.push(left);
            }
            Call::TextMatrix(m) => w.text_matrix(array(m)),
            Call::TextMove(x, y) => w.text_move(x, y),
            Call::NextLine => w.next_line(),
            Call::TextMode(mode) => w.text_mode(text_mode(mode)),
            Call::HorizontalScale(x) => w.horizontal_scale(x),
            Call::Leading(x) => w.leading(x),
            Call::Draw(res) => w.draw(res.family()),
            Call::Image(res, to) => w.image(res.family(), to.x, to.y, to.w, to.h),
            Call::PlaceForm(res, bbox, matrix, target) => {
                if let Some(onto) = Affine::onto(matrix.bounds(bbox), target) {
                    w.save();
                    w.transform(array(onto));
                    w.draw(res.family());
                    w.restore();
                }
            }
            Call::BeginTagged(tag) => w.begin_tagged(&tag),
            Call::BeginMarked(tag, list) => {
                let list = fam::Res::dict(list).expect("a property list holds no stream");
                w.begin_marked(&tag, list);
            }
            Call::EndMarked => w.end_marked(),
            Call::Operation(operands, op) => {
                w.operation(operands.iter().map(Vec::as_slice), &op);
            }
        }
    }
    (w, unencoded)
}

fn color(c: Color) -> fam::Color {
    match c {
        Color::Gray(v) => fam::Color::Gray(v),
        Color::Rgb(v) => fam::Color::Rgb(v),
        Color::Cmyk(v) => fam::Color::Cmyk(v),
    }
}

fn gstate(g: GState) -> fam::GState {
    let mut state = fam::GState::default();
    state.fill_alpha = g.fill_alpha;
    state.stroke_alpha = g.stroke_alpha;
    state.blend = g.blend.map(blend);
    state
}

const BLENDS: [Blend; 16] = [
    Blend::Normal,
    Blend::Multiply,
    Blend::Screen,
    Blend::Overlay,
    Blend::Darken,
    Blend::Lighten,
    Blend::ColorDodge,
    Blend::ColorBurn,
    Blend::HardLight,
    Blend::SoftLight,
    Blend::Difference,
    Blend::Exclusion,
    Blend::Hue,
    Blend::Saturation,
    Blend::Color,
    Blend::Luminosity,
];

fn blend(b: Blend) -> fam::Blend {
    match b {
        Blend::Normal => fam::Blend::Normal,
        Blend::Multiply => fam::Blend::Multiply,
        Blend::Screen => fam::Blend::Screen,
        Blend::Overlay => fam::Blend::Overlay,
        Blend::Darken => fam::Blend::Darken,
        Blend::Lighten => fam::Blend::Lighten,
        Blend::ColorDodge => fam::Blend::ColorDodge,
        Blend::ColorBurn => fam::Blend::ColorBurn,
        Blend::HardLight => fam::Blend::HardLight,
        Blend::SoftLight => fam::Blend::SoftLight,
        Blend::Difference => fam::Blend::Difference,
        Blend::Exclusion => fam::Blend::Exclusion,
        Blend::Hue => fam::Blend::Hue,
        Blend::Saturation => fam::Blend::Saturation,
        Blend::Color => fam::Blend::Color,
        Blend::Luminosity => fam::Blend::Luminosity,
    }
}

const PAINTS: [Paint; 9] = [
    Paint::Fill,
    Paint::FillEvenOdd,
    Paint::Stroke,
    Paint::CloseStroke,
    Paint::FillStroke,
    Paint::FillStrokeEvenOdd,
    Paint::CloseFillStroke,
    Paint::CloseFillStrokeEvenOdd,
    Paint::None,
];

fn paint(p: Paint) -> fam::Paint {
    match p {
        Paint::Fill => fam::Paint::Fill,
        Paint::FillEvenOdd => fam::Paint::FillEvenOdd,
        Paint::Stroke => fam::Paint::Stroke,
        Paint::CloseStroke => fam::Paint::CloseStroke,
        Paint::FillStroke => fam::Paint::FillStroke,
        Paint::FillStrokeEvenOdd => fam::Paint::FillStrokeEvenOdd,
        Paint::CloseFillStroke => fam::Paint::CloseFillStroke,
        Paint::CloseFillStrokeEvenOdd => fam::Paint::CloseFillStrokeEvenOdd,
        Paint::None => fam::Paint::None,
    }
}

const TEXT_MODES: [TextMode; 8] = [
    TextMode::Fill,
    TextMode::Stroke,
    TextMode::FillStroke,
    TextMode::Invisible,
    TextMode::FillClip,
    TextMode::StrokeClip,
    TextMode::FillStrokeClip,
    TextMode::Clip,
];

fn text_mode(mode: TextMode) -> fam::TextMode {
    match mode {
        TextMode::Fill => fam::TextMode::Fill,
        TextMode::Stroke => fam::TextMode::Stroke,
        TextMode::FillStroke => fam::TextMode::FillStroke,
        TextMode::Invisible => fam::TextMode::Invisible,
        TextMode::FillClip => fam::TextMode::FillClip,
        TextMode::StrokeClip => fam::TextMode::StrokeClip,
        TextMode::FillStrokeClip => fam::TextMode::FillStrokeClip,
        TextMode::Clip => fam::TextMode::Clip,
    }
}

// ---- What a writer gives, set beside the other's ----

/// What a resource is, as either writer holds it.
#[derive(Debug, Clone, PartialEq)]
enum Body {
    Objects(Vec<Object>),
    Written(ObjectId),
}

/// A need: its category's key, its name, its id and what it is.
type NeedOf = (&'static [u8], Vec<u8>, u128, Body);

/// Content, and the resources it draws with.
type Named = (Vec<u8>, Vec<NeedOf>);

fn old_needs(needs: &ResourceNeeds) -> Vec<NeedOf> {
    (needs.0.iter())
        .map(|n| {
            let body = match &n.res.body {
                ResBody::Objects(objects) => Body::Objects(objects.to_vec()),
                ResBody::Written(id) => Body::Written(*id),
            };
            (n.kind.key(), n.name.clone(), n.res.id.0, body)
        })
        .collect()
}

fn family_needs(needs: &fam::ResourceNeeds) -> Vec<NeedOf> {
    (needs.0.iter())
        .map(|n| {
            let body = match n.res.body() {
                fam::ResBody::Objects(objects) => Body::Objects(objects.to_vec()),
                fam::ResBody::Written(id) => Body::Written(*id),
                other => panic!("a body of a kind this doesn't know: {other:?}"),
            };
            (n.kind.key(), n.name.clone(), n.res.id().0, body)
        })
        .collect()
}

fn old_named(w: &ContentWriter, reserved: &Reserved) -> Named {
    let (content, needs) = w.named(reserved);
    (content, old_needs(&needs))
}

fn family_named(w: &fam::ContentWriter, reserved: &fam::Reserved) -> Named {
    let (content, needs) = w.named(reserved);
    (content, family_needs(&needs))
}

/// What both writers give for `calls` with nothing reserved, by sheaf's
/// and by the family's.
fn both(calls: &[Call]) -> (Named, Named) {
    let (ours, ours_left) = old(calls);
    let (theirs, theirs_left) = family(calls);
    assert_eq!(ours_left, theirs_left, "what text left unencoded");
    (
        old_named(&ours, &Reserved::default()),
        family_named(&theirs, &fam::Reserved::default()),
    )
}

/// What both writers give for `calls`: the same bytes and needs.
#[track_caller]
fn same(calls: &[Call]) -> Named {
    let (ours, theirs) = both(calls);
    assert_eq!(
        String::from_utf8_lossy(&ours.0),
        String::from_utf8_lossy(&theirs.0),
        "content"
    );
    assert_eq!(ours, theirs);
    ours
}

// ---- writer_tests.rs's call sequences ----

fn list(subtype: &str) -> Dictionary {
    dictionary! { "Type" => Object::name("Pagination"), "Subtype" => Object::name(subtype) }
}

fn written(n: u32) -> Drawn {
    Drawn::Written(ObjectId::new(n, 0))
}

fn marked(tag: &str, subtype: &str) -> Call {
    Call::BeginMarked(tag.to_owned(), list(subtype))
}

fn text(face: Face, s: &str) -> Call {
    Call::Text(Some(face), s.to_owned())
}

/// `each_call_writes_its_operator`'s calls.
fn each_call() -> Vec<Call> {
    let mut calls = vec![
        Call::Save,
        Call::Transform(Affine {
            a: 2.0,
            e: -0.00001,
            f: 1.0 / 3.0,
            ..Affine::IDENTITY
        }),
        Call::FillColor(Color::Gray(0.5)),
        Call::StrokeColor(Color::Rgb([1.0, 0.0, 0.25])),
        Call::FillColor(Color::Cmyk([0.0, 0.1, 0.2, 1.0])),
        Call::StrokeColor(Color::rgb8([255, 0, 51])),
        Call::LineWidth(0.75),
        Call::Dash(vec![4.0, 3.0], 0.0),
        Call::Dash(vec![], 0.0),
        Call::LineCap(LineCap::Round),
        Call::LineJoin(LineJoin::Bevel),
        Call::MoveTo(1.0, 2.0),
        Call::LineTo(612.0, 1e7),
        Call::CurveTo([1.0, 2.0, 3.0, 4.0, 5.0, f32::NAN]),
        Call::Rect([0.0, 0.0, 10.0, -0.0]),
        Call::Close,
    ];
    calls.extend(PAINTS[..8].iter().map(|&p| Call::Paint(p)));
    calls.extend([
        Call::Clip(FillRule::NonZero),
        Call::Clip(FillRule::EvenOdd),
        Call::Paint(Paint::None),
        Call::BeginText,
        Call::TextMode(TextMode::Invisible),
        Call::HorizontalScale(160.0),
        Call::Leading(12.5),
        Call::TextMove(72.0, 700.0),
        Call::TextMatrix(Affine::IDENTITY),
        Call::NextLine,
        Call::EndText,
        Call::Restore,
        Call::EndMarked,
    ]);
    calls
}

/// `text_is_written_in_its_font`'s calls: the last text is in the font
/// the `q` saved, which a caller of the family's writer names.
fn text_in_its_font() -> Vec<Call> {
    vec![
        Call::Text(None, "lost".to_owned()),
        Call::Font(HELVETICA, 9.0),
        text(HELVETICA, "(a\\b) é"),
        text(HELVETICA, "中文"),
        Call::Save,
        Call::Font(Face::Glyphless, 10.0),
        text(Face::Glyphless, "Hi"),
        Call::Restore,
        text(HELVETICA, "é"),
    ]
}

/// `an_operator_read_is_written_as_it_is`'s calls.
fn an_operator_read() -> Vec<Call> {
    let operation = |operands: &[&[u8]], op: &[u8]| {
        Call::Operation(operands.iter().map(|o| o.to_vec()).collect(), op.to_vec())
    };
    vec![
        Call::Font(HELVETICA, 9.0),
        operation(&[b"/F1", b"12"], b"Tf"),
        operation(&[b"[(a) -250 <62>]"], b"TJ"),
        operation(&[], b"BI /W 1 /H 1 /CS /G /BPC 8 ID \xff EI"),
        operation(&[], b"Q"),
        text(HELVETICA, "é"),
    ]
}

/// `marked_content_names_its_list`'s calls.
fn marked_content() -> Vec<Call> {
    vec![
        marked("Artifact", "Header"),
        Call::GState(GState {
            fill_alpha: Some(0.5),
            stroke_alpha: Some(2.0),
            blend: Some(Blend::Multiply),
        }),
        Call::EndMarked,
        marked("A b/c", "Header"),
        Call::EndMarked,
    ]
}

/// `names_are_bound_at_the_end`'s calls.
fn two_lists() -> Vec<Call> {
    vec![
        marked("Artifact", "Header"),
        Call::EndMarked,
        marked("Artifact", "Footer"),
        Call::EndMarked,
    ]
}

/// `needs_are_one_a_resource_in_first_use_order`'s calls.
fn drawn_again() -> Vec<Call> {
    vec![
        Call::Font(HELVETICA, 9.0),
        Call::Draw(written(7)),
        Call::Font(ZAPF, 9.0),
        Call::Font(HELVETICA, 10.0),
        Call::Draw(written(7)),
        Call::Draw(written(8)),
    ]
}

/// `place_form_honors_bbox_and_matrix`'s calls, each case after the last.
fn forms_placed() -> Vec<Call> {
    let target = Box2 {
        x: 100.0,
        y: 200.0,
        w: 50.0,
        h: 200.0,
    };
    let bbox = Rect::new(50.0, 50.0, 250.0, 150.0);
    let turned = Affine {
        a: 0.0,
        b: 1.0,
        c: -1.0,
        d: 0.0,
        ..Affine::IDENTITY
    };
    let image = Box2 {
        x: 1.0,
        y: 2.0,
        w: 3.0,
        h: 4.0,
    };
    let empty = Rect::new(0.0, 0.0, 0.0, 9.0);
    vec![
        Call::PlaceForm(written(5), bbox, Affine::IDENTITY, target),
        Call::PlaceForm(written(5), bbox, turned, target),
        Call::PlaceForm(written(5), empty, Affine::IDENTITY, target),
        Call::Image(written(6), image),
    ]
}

/// `identical_calls_give_identical_bytes`'s calls.
fn identical_calls() -> Vec<Call> {
    let turn = 45f32.to_radians();
    vec![
        marked("Artifact", "Footer"),
        Call::Save,
        Call::GState(GState {
            fill_alpha: Some(0.3),
            ..GState::default()
        }),
        Call::BeginText,
        Call::FillColor(Color::rgb8([200, 30, 30])),
        Call::Font(HELVETICA_BOLD, 72.0),
        Call::TextMatrix(Affine {
            a: turn.cos(),
            b: turn.sin(),
            c: -turn.sin(),
            d: turn.cos(),
            e: 123.456,
            f: 789.012,
        }),
        text(HELVETICA_BOLD, "Café “x” – €"),
        Call::Font(Face::Glyphless, 10.0),
        text(Face::Glyphless, "中文"),
        Call::EndText,
        Call::Restore,
        Call::EndMarked,
    ]
}

/// `needs_are_written_once_and_bound`'s calls.
fn written_once() -> Vec<Call> {
    vec![
        Call::Font(Face::Glyphless, 10.0),
        marked("Artifact", "Header"),
        Call::EndMarked,
    ]
}

/// Every call sequence of sheaf's writer's tests gives the same bytes and
/// the same needs through the family's writer; and the bytes are the ones
/// those tests hold, so the sequences here are theirs.
#[test]
fn the_writer_tests_sequences_write_the_same() {
    let name = |named: &Named, i: usize| String::from_utf8_lossy(&named.1[i].1).into_owned();

    let named = same(&each_call());
    assert_eq!(
        String::from_utf8_lossy(&named.0),
        "q\n2 0 0 1 0 0.3333 cm\n0.5 g\n1 0 0.25 RG\n0 0.1 0.2 1 k\n1 0 0.2 RG\n\
         0.75 w\n[4 3] 0 d\n[] 0 d\n1 J\n2 j\n1 2 m\n612 10000000 l\n1 2 3 4 5 0 c\n\
         0 0 10 0 re\nh\nf\nf*\nS\ns\nB\nB*\nb\nb*\nW\nW*\nn\n\
         BT\n3 Tr\n160 Tz\n12.5 TL\n72 700 Td\n1 0 0 1 0 0 Tm\nT*\nET\nQ\nEMC\n"
    );
    assert!(named.1.is_empty());

    let named = same(&text_in_its_font());
    let mut want = format!("/{} 9 Tf\n", name(&named, 0)).into_bytes();
    want.extend_from_slice(b"(\\(a\\\\b\\) \xe9) Tj\n(??) Tj\nq\n");
    want.extend(format!("/{} 10 Tf\n", name(&named, 1)).bytes());
    want.extend_from_slice(b"<00480069> Tj\nQ\n(\xe9) Tj\n");
    assert_eq!(named.0, want);
    assert_eq!(named.1.len(), 2);

    let named = same(&an_operator_read());
    let mut want = format!("/{} 9 Tf\n", name(&named, 0)).into_bytes();
    want.extend_from_slice(
        b"/F1 12 Tf\n[(a) -250 <62>] TJ\nBI /W 1 /H 1 /CS /G /BPC 8 ID \xff EI\n",
    );
    want.extend_from_slice(b"Q\n(\xe9) Tj\n");
    assert_eq!(named.0, want);

    let named = same(&marked_content());
    let (p, g) = (name(&named, 0), name(&named, 1));
    assert_eq!(
        String::from_utf8_lossy(&named.0),
        format!("/Artifact /{p} BDC\n/{g} gs\nEMC\n/A#20b#2Fc /{p} BDC\nEMC\n")
    );
    assert_eq!(
        named.1[1].3,
        Body::Objects(vec![
            dictionary! {
                "Type" => Object::name("ExtGState"),
                "ca" => 0.5,
                "CA" => 1.0,
                "BM" => Object::name("Multiply"),
            }
            .into()
        ])
    );

    let named = same(&two_lists());
    assert_eq!(named.1.len(), 2);
    assert_ne!(named.1[0].1, named.1[1].1);

    let named = same(&drawn_again());
    assert_eq!(
        String::from_utf8_lossy(&named.0),
        format!(
            "/{0} 9 Tf\n/{1} Do\n/{2} 9 Tf\n/{0} 10 Tf\n/{1} Do\n/{3} Do\n",
            name(&named, 0),
            name(&named, 1),
            name(&named, 2),
            name(&named, 3)
        )
    );
    let kinds: Vec<&[u8]> = named.1.iter().map(|n| n.0).collect();
    assert_eq!(kinds, [&b"Font"[..], b"XObject", b"Font", b"XObject"]);

    let named = same(&forms_placed());
    assert_eq!(
        String::from_utf8_lossy(&named.0),
        format!(
            "q\n0.25 0 0 2 87.5 100 cm\n/{0} Do\nQ\nq\n0.5 0 0 1 175 150 cm\n/{0} Do\nQ\n\
             q\n3 0 0 4 1 2 cm\n/{1} Do\nQ\n",
            name(&named, 0),
            name(&named, 1)
        )
    );

    let named = same(&identical_calls());
    assert_eq!(named.1.len(), 4);
    let named = same(&written_once());
    assert_eq!(named.1.len(), 2);
}

// ---- Any calls, over the numbers a float can be ----

/// A number below 2^31 in magnitude, or one that isn't finite: where the
/// two writers write the same.
fn below_2_31(x: f32) -> f32 {
    if x.is_finite() && x.abs() >= 2_147_483_648.0 {
        x % 2_147_483_648.0
    } else {
        x
    }
}

/// Any float, NaN and the infinities among them, with the numbers content
/// holds and the edges of how one is written drawn more often.
fn any_number() -> impl Strategy<Value = f32> {
    prop_oneof![
        4 => proptest::num::f32::ANY,
        3 => -1000f32..1000f32,
        2 => (-100_000i32..100_000).prop_map(|n| n as f32 / 10_000.0),
        2 => (-1_000_000i32..1_000_000).prop_map(|n| n as f32 / 100_000.0),
        1 => (-40i32..40).prop_map(|n| n as f32),
        2 => prop::sample::select(vec![
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            0.0,
            -0.0,
            0.00005,
            -0.00005,
            0.000049,
            0.99995,
            16_777_216.0,
            2_147_483_520.0,
            -2_147_483_520.0,
            f32::MIN_POSITIVE,
            1e-40,
            f32::EPSILON,
        ]),
    ]
}

fn number() -> impl Strategy<Value = f32> {
    any_number().prop_map(below_2_31)
}

/// An alpha that's finite, in 0 to 1 or out of it.
fn alpha() -> impl Strategy<Value = f32> {
    number().prop_map(|x| if x.is_finite() { x } else { 0.5 })
}

fn numbers<const N: usize>() -> impl Strategy<Value = [f32; N]> {
    prop::collection::vec(number(), N).prop_map(|v| std::array::from_fn(|i| v[i]))
}

fn affine() -> impl Strategy<Value = Affine> {
    numbers::<6>().prop_map(|[a, b, c, d, e, f]| Affine { a, b, c, d, e, f })
}

fn a_box() -> impl Strategy<Value = Box2> {
    numbers::<4>().prop_map(|[x, y, w, h]| Box2 { x, y, w, h })
}

fn a_color() -> impl Strategy<Value = Color> {
    prop_oneof![
        number().prop_map(Color::Gray),
        numbers::<3>().prop_map(Color::Rgb),
        numbers::<4>().prop_map(Color::Cmyk),
        any::<[u8; 3]>().prop_map(Color::rgb8),
    ]
}

fn a_gstate() -> impl Strategy<Value = GState> {
    (
        prop::option::of(alpha()),
        prop::option::of(alpha()),
        prop::option::of(prop::sample::select(BLENDS.to_vec())),
    )
        .prop_map(|(fill_alpha, stroke_alpha, blend)| GState {
            fill_alpha,
            stroke_alpha,
            blend,
        })
}

fn a_face() -> impl Strategy<Value = Face> {
    prop_oneof![(0usize..14).prop_map(Face::Std), Just(Face::Glyphless)]
}

/// Text: ASCII, what WinAnsi holds past it, what no standard face holds,
/// control codes, and what a literal string escapes.
fn a_text() -> impl Strategy<Value = String> {
    let chars = prop_oneof![
        4 => prop::sample::select("aZ 09(){}<>\\/%#é€“–中文✓α\u{1F600}\n\r\t\u{7f}\u{1}".chars().collect::<Vec<_>>()),
        1 => any::<char>(),
    ];
    prop::collection::vec(chars, 0..12).prop_map(|c| c.into_iter().collect())
}

/// A tag: any characters but NUL, which sheaf's writer doesn't write.
fn a_tag() -> impl Strategy<Value = String> {
    let chars = prop_oneof![
        4 => prop::sample::select("Artifact Span/#()<>[]{}%é\u{80}\u{ff}\t\n~!".chars().collect::<Vec<_>>()),
        1 => any::<char>().prop_filter("NUL", |c| *c != '\0'),
    ];
    prop::collection::vec(chars, 0..10).prop_map(|c| c.into_iter().collect())
}

fn bytes(len: std::ops::Range<usize>) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), len)
}

/// A property list: names, numbers and strings, a list inside it.
fn a_list() -> impl Strategy<Value = Dictionary> {
    let value = prop_oneof![
        bytes(0..6).prop_map(Object::name),
        any::<i32>().prop_map(Object::from),
        number()
            .prop_filter("finite", |x| x.is_finite())
            .prop_map(Object::from),
        bytes(0..8).prop_map(crate::pdf::literal),
        Just(Object::from(dictionary! { "MCID" => 3 })),
    ];
    prop::collection::vec(
        (
            prop::sample::select(vec!["Type", "Subtype", "K", "E"]),
            value,
        ),
        0..4,
    )
    .prop_map(|entries| {
        let mut dict = Dictionary::new();
        for (key, value) in entries {
            dict.set(key, value);
        }
        dict
    })
}

/// A resource drawn: one written already, or objects, a stream among
/// them.
fn a_drawn() -> impl Strategy<Value = Drawn> {
    prop_oneof![
        (1u32..6).prop_map(written),
        (0i64..4).prop_map(|n| {
            Drawn::Objects(vec![
                Stream::new(
                    dictionary! { "Type" => Object::name("XObject"), "N" => n },
                    vec![n as u8; 3],
                )
                .into(),
            ])
        }),
    ]
}

fn a_call() -> impl Strategy<Value = Vec<Call>> {
    let one = |call: Call| vec![call];
    prop_oneof![
        prop::sample::select(vec![
            Call::Save,
            Call::Restore,
            Call::Close,
            Call::BeginText,
            Call::EndText,
            Call::NextLine,
            Call::EndMarked,
            Call::Clip(FillRule::NonZero),
            Call::Clip(FillRule::EvenOdd),
            Call::LineCap(LineCap::Butt),
            Call::LineCap(LineCap::Round),
            Call::LineCap(LineCap::Square),
            Call::LineJoin(LineJoin::Miter),
            Call::LineJoin(LineJoin::Round),
            Call::LineJoin(LineJoin::Bevel),
        ])
        .prop_map(one),
        affine().prop_map(Call::Transform).prop_map(one),
        affine().prop_map(Call::TextMatrix).prop_map(one),
        a_color().prop_map(Call::FillColor).prop_map(one),
        a_color().prop_map(Call::StrokeColor).prop_map(one),
        a_gstate().prop_map(Call::GState).prop_map(one),
        number().prop_map(Call::LineWidth).prop_map(one),
        number().prop_map(Call::HorizontalScale).prop_map(one),
        number().prop_map(Call::Leading).prop_map(one),
        (prop::collection::vec(number(), 0..4), number())
            .prop_map(|(dashes, phase)| Call::Dash(dashes, phase))
            .prop_map(one),
        numbers::<2>()
            .prop_map(|[x, y]| Call::MoveTo(x, y))
            .prop_map(one),
        numbers::<2>()
            .prop_map(|[x, y]| Call::LineTo(x, y))
            .prop_map(one),
        numbers::<2>()
            .prop_map(|[x, y]| Call::TextMove(x, y))
            .prop_map(one),
        numbers::<6>().prop_map(Call::CurveTo).prop_map(one),
        numbers::<4>().prop_map(Call::Rect).prop_map(one),
        prop::sample::select(PAINTS.to_vec())
            .prop_map(Call::Paint)
            .prop_map(one),
        prop::sample::select(TEXT_MODES.to_vec())
            .prop_map(Call::TextMode)
            .prop_map(one),
        (a_face(), number())
            .prop_map(|(face, size)| Call::Font(face, size))
            .prop_map(one),
        // Text in its font, set the line before, as sheaf's callers
        // write it.
        (a_face(), number(), prop::collection::vec(a_text(), 1..3)).prop_map(
            |(face, size, texts)| {
                let mut calls = vec![Call::Font(face, size)];
                calls.extend(texts.into_iter().map(|s| Call::Text(Some(face), s)));
                calls
            }
        ),
        a_drawn().prop_map(Call::Draw).prop_map(one),
        (a_drawn(), a_box())
            .prop_map(|(res, target)| Call::Image(res, target))
            .prop_map(one),
        // A form placed: the matrix sheaf's geometry makes of these is
        // what's written, so it's that which stays below 2^31.
        (a_drawn(), numbers::<4>(), affine(), a_box())
            .prop_filter(
                "a matrix past 32 bits",
                |(_, [x0, y0, x1, y1], matrix, target)| {
                    let bbox = Rect::new(*x0, *y0, *x1, *y1);
                    Affine::onto(matrix.bounds(bbox), *target).is_none_or(|onto| {
                        array(onto)
                            .iter()
                            .all(|&x| below_2_31(x) == x || x.is_nan())
                    })
                }
            )
            .prop_map(|(res, [x0, y0, x1, y1], matrix, target)| {
                Call::PlaceForm(res, Rect::new(x0, y0, x1, y1), matrix, target)
            })
            .prop_map(one),
        a_tag().prop_map(Call::BeginTagged).prop_map(one),
        (a_tag(), a_list())
            .prop_map(|(tag, list)| Call::BeginMarked(tag, list))
            .prop_map(one),
        (prop::collection::vec(bytes(0..6), 0..3), bytes(0..6))
            .prop_map(|(operands, op)| Call::Operation(operands, op))
            .prop_map(one),
    ]
}

fn calls() -> impl Strategy<Value = Vec<Call>> {
    prop::collection::vec(a_call(), 0..24).prop_map(|c| c.concat())
}

/// The bytes a number is written as by each writer, as `w`'s operand.
fn number_written(x: f32) -> (String, String) {
    let (ours, theirs) = both(&[Call::LineWidth(x)]);
    let operand = |named: Named| {
        let content = String::from_utf8(named.0).unwrap();
        content.strip_suffix(" w\n").unwrap().to_owned()
    };
    (operand(ours), operand(theirs))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Any calls, their operands anywhere a float below 2^31 can be, NaN
    /// and the infinities among them: the same bytes, the same needs.
    #[test]
    fn any_calls_write_the_same(calls in calls()) {
        let (ours, theirs) = both(&calls);
        prop_assert_eq!(
            String::from_utf8_lossy(&ours.0),
            String::from_utf8_lossy(&theirs.0)
        );
        prop_assert_eq!(ours, theirs);
    }

    /// Any float at all is written the same by both, but a finite one of
    /// 2^31 or more, or below -2^31: there the family's keeps a point,
    /// and that's the whole difference.
    #[test]
    fn a_number_differs_only_past_32_bits(x in any_number()) {
        let (ours, theirs) = number_written(x);
        let in_32_bits = !x.is_finite() || (-2_147_483_648.0..2_147_483_648.0).contains(&x);
        if in_32_bits {
            prop_assert_eq!(ours, theirs);
        } else {
            prop_assert_eq!(format!("{ours}."), theirs);
        }
    }
}

// ---- The differences taken, each exactly ----

/// A whole number outside the signed 32-bit range keeps its point in the
/// family's writer, where sheaf's wrote it bare (PDFium reads that as 0);
/// -2^31 is in the range and stays as it was.
#[test]
fn a_whole_number_past_32_bits_keeps_its_point() {
    let max = format!("{:.0}", f32::MAX);
    for (x, ours, theirs) in [
        (2_147_483_520.0, "2147483520", "2147483520".to_owned()),
        (-2_147_483_520.0, "-2147483520", "-2147483520".to_owned()),
        (-2_147_483_648.0, "-2147483648", "-2147483648".to_owned()),
        (2_147_483_648.0, "2147483648", "2147483648.".to_owned()),
        (-2_147_483_904.0, "-2147483904", "-2147483904.".to_owned()),
        (4_294_967_296.0, "4294967296", "4294967296.".to_owned()),
        (1e10, "10000000000", "10000000000.".to_owned()),
        (-1e10, "-10000000000", "-10000000000.".to_owned()),
        (f32::MAX, max.as_str(), format!("{max}.")),
    ] {
        assert_eq!(number_written(x), (ours.to_owned(), theirs), "{x}");
    }
    // Every operand is written so: a matrix's, a dash's.
    let calls = [
        Call::Transform(Affine {
            e: 3e9,
            f: -3e9,
            ..Affine::IDENTITY
        }),
        Call::Dash(vec![3e9, 1.0], 3e9),
    ];
    let (ours, theirs) = both(&calls);
    assert_eq!(
        String::from_utf8_lossy(&ours.0),
        "1 0 0 1 3000000000 -3000000000 cm\n[3000000000 1] 3000000000 d\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&theirs.0),
        "1 0 0 1 3000000000. -3000000000. cm\n[3000000000. 1] 3000000000. d\n"
    );
}

/// The number `key` is in the ExtGState a need is.
fn alpha_of(named: &Named, key: &[u8]) -> f64 {
    let [(kind, _, _, Body::Objects(objects))] = &named.1[..] else {
        panic!("{:?}", named.1)
    };
    assert_eq!(*kind, b"ExtGState");
    let dict = objects[0].as_dict().unwrap();
    match dict.get(key).unwrap().value() {
        Value::Real(r) => *r,
        other => panic!("{other:?}"),
    }
}

/// An alpha that isn't finite is 0 in the family's ExtGState, as its
/// numbers are. sheaf's held NaN for NaN, and clamped the infinities (1
/// and 0): so NaN and +infinity are another dictionary, with another
/// name; -infinity is the same.
#[test]
fn an_alpha_that_isnt_finite_is_written_0() {
    for (a, ours_is, differs) in [
        (f32::NAN, f64::NAN, true),
        (f32::INFINITY, 1.0, true),
        (f32::NEG_INFINITY, 0.0, false),
    ] {
        for (key, state) in [
            (
                &b"ca"[..],
                GState {
                    fill_alpha: Some(a),
                    ..GState::default()
                },
            ),
            (
                b"CA",
                GState {
                    stroke_alpha: Some(a),
                    ..GState::default()
                },
            ),
        ] {
            let (ours, theirs) = both(&[Call::GState(state)]);
            let was = alpha_of(&ours, key);
            assert!(
                was == ours_is || (was.is_nan() && ours_is.is_nan()),
                "{was}"
            );
            assert_eq!(alpha_of(&theirs, key), 0.0, "{a}");
            assert_eq!(ours.1[0].2 != theirs.1[0].2, differs, "{a}: the id");
            assert_eq!(ours.0 != theirs.0, differs, "{a}: the name");
            // The family's is the dictionary a finite 0 makes, by either.
            let zero = same(&[Call::GState(match key {
                b"ca" => GState {
                    fill_alpha: Some(0.0),
                    ..GState::default()
                },
                _ => GState {
                    stroke_alpha: Some(0.0),
                    ..GState::default()
                },
            })]);
            assert_eq!(theirs, zero, "{a}");
        }
    }
}

/// One resource drawn as a form and named as a property list: sheaf's
/// writer kept one need, in the category it was first drawn in, where the
/// family's keeps one in each. The name is the same, so the content is.
#[test]
fn a_resource_drawn_in_two_categories_is_a_need_in_each() {
    let dict = list("Header");
    let calls = [
        Call::Draw(Drawn::Objects(vec![dict.clone().into()])),
        Call::BeginMarked("Artifact".to_owned(), dict.clone()),
        Call::EndMarked,
    ];
    let (ours, theirs) = both(&calls);
    assert_eq!(ours.0, theirs.0, "the content");
    let [form] = &ours.1[..] else {
        panic!("{:?}", ours.1)
    };
    assert_eq!(form.0, b"XObject");
    let [their_form, their_list] = &theirs.1[..] else {
        panic!("{:?}", theirs.1)
    };
    assert_eq!(their_form, form);
    assert_eq!(their_list.0, b"Properties");
    assert_eq!(their_list.1[..], form.1[..], "one name, in each category");
    assert_eq!((their_list.2, &their_list.3), (form.2, &form.3));
    let name = String::from_utf8_lossy(&form.1);
    assert_eq!(
        String::from_utf8_lossy(&ours.0),
        format!("/{name} Do\n/Artifact /{name} BDC\nEMC\n")
    );
}

// ---- The digest a resource is named by ----

/// `objects`' digest by sheaf's and by the family's: the same, and the
/// same through each one's `Res`.
#[track_caller]
fn same_id(objects: &[Object]) -> u128 {
    let ours = ResId::of(objects).0;
    assert_eq!(ours, fam::ResId::of(objects).0, "{objects:?}");
    assert_eq!(Res::of(objects.to_vec()).id.0, ours);
    let theirs = fam::Res::of(objects.to_vec()).expect("held in memory");
    assert_eq!(theirs.id().0, ours);
    ours
}

fn image(pixels: Pixels) -> Object {
    let page = PageImage {
        width: 2,
        height: 2,
        dpi: (72.0, 72.0),
        rotate: 0,
        pixels,
    };
    crate::images::image_stream(&page).unwrap().into()
}

/// A form as sheaf holds one to draw: the form first, then what it
/// reaches, a reference `(i, 0)` to the `i`th.
fn form_objects() -> Vec<Object> {
    let form = Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"),
            "Subtype" => Object::name("Form"),
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), 612.into(), Object::from(791.5f32)],
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 0.into(), 0.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => ObjectId::new(1, 0) },
                "ExtGState" => dictionary! {
                    "G0" => dictionary! { "ca" => 0.5, "SMask" => Object::name("None") },
                },
                "XObject" => dictionary! { "Im0" => ObjectId::new(3, 0) },
                "ProcSet" => vec![Object::name("PDF"), Object::name("Text")],
            },
            "Group" => dictionary! { "S" => Object::name("Transparency"), "K" => true },
        },
        b"q\nBT\n/F1 12 Tf\n(Hi) Tj\nET\n/Im0 Do\nQ\n".to_vec(),
    );
    let font = dictionary! {
        "Type" => Object::name("Font"),
        "Subtype" => Object::name("TrueType"),
        "BaseFont" => Object::name("ABCDEF+Some#Font"),
        "FirstChar" => 32,
        "Widths" => vec![Object::from(250), Object::from(333.5f32), Object::null()],
        "FontDescriptor" => ObjectId::new(2, 0),
    };
    let descriptor = dictionary! {
        "Type" => Object::name("FontDescriptor"),
        "FontName" => Object::name("ABCDEF+Some#Font"),
        "Flags" => 32,
        "ItalicAngle" => -12.5f32,
        "CharSet" => crate::pdf::literal(b"/a/b".to_vec()),
        "Panose" => Object::new(Value::String(
            Str::bytes(vec![0u8, 1, 2, 255]),
            Some(StringFormat::Hex),
        )),
        "FontFile2" => ObjectId::new(4, 0),
    };
    let program = Stream::new(dictionary! { "Length1" => 5 }, vec![0, 1, 0, 0, 0]);
    vec![
        form.into(),
        font.into(),
        descriptor.into(),
        image(Pixels::Gray(vec![0, 85, 170, 255])),
        program.into(),
    ]
}

/// Every resource sheaf makes is the same id by the family's digest:
/// each standard font and Glyphless, ExtGStates, a property list, an
/// image stream of each kind, a form's objects, an object written.
#[test]
fn resources_sheaf_makes_digest_the_same() {
    let mut ids = BTreeSet::new();
    for face in (0..14).map(Face::Std).chain([Face::Glyphless]) {
        let res = face.font().resource();
        assert_eq!(same_id(&res.objects), res.id.0, "{face:?}");
        assert_eq!(face.family().id().0, res.id.0);
        assert_eq!(Res::from(res.clone()).id.0, res.id.0);
        ids.insert(res.id.0);
    }
    assert_eq!(ids.len(), 15, "each font its own");
    assert_eq!(GLYPHLESS.resource().objects.len(), 6);

    // ExtGStates, as each writer makes them, and by the other's digest.
    let alphas = [None, Some(0.0), Some(0.3), Some(0.5), Some(1.0), Some(2.0)];
    for fill_alpha in alphas {
        for stroke_alpha in alphas {
            for blend in [None, Some(Blend::Normal), Some(Blend::Multiply)] {
                let state = GState {
                    fill_alpha,
                    stroke_alpha,
                    blend,
                };
                let named = same(&[Call::GState(state)]);
                let (_, _, id, Body::Objects(objects)) = &named.1[0] else {
                    panic!("{named:?}")
                };
                assert_eq!(same_id(objects), *id);
            }
        }
    }
    for blend in BLENDS {
        let named = same(&[Call::GState(GState {
            blend: Some(blend),
            ..GState::default()
        })]);
        let (_, _, id, Body::Objects(objects)) = &named.1[0] else {
            panic!("{named:?}")
        };
        assert_eq!(same_id(objects), *id);
    }

    // Property lists: pagination's, and one of sheaf's own with a string
    // and a list inside.
    for subtype in ["Header", "Footer", "Watermark", "Background"] {
        let dict = list(subtype);
        let id = same_id(&[dict.clone().into()]);
        assert_eq!(Res::dict(dict.clone()).id.0, id);
        assert_eq!(fam::Res::dict(dict).unwrap().id().0, id);
    }
    let own = dictionary! {
        "Type" => Object::name("Pagination"),
        "Subtype" => Object::name("Header"),
        "SHEF_Gen" => crate::pdf::literal(b"0123456789abcdef".to_vec()),
        "SHEF_Block" => dictionary! { "L" => 3, "R" => 1.5f32, "K" => Object::name("Flat") },
        "MCID" => 0,
        "Empty" => Dictionary::new(),
        "None" => Object::null(),
        "Hex" => Object::new(Value::String(Str::bytes(vec![0xfe, 0xff, 0, 65]), Some(StringFormat::Hex))),
    };
    same_id(&[own.into()]);

    // An image stream of each kind sheaf writes.
    let mut images = BTreeSet::new();
    for pixels in [
        Pixels::Rgb((0..12).collect()),
        Pixels::Gray(vec![0, 85, 170, 255]),
        Pixels::Jpeg {
            bytes: b"\xff\xd8\xff\xe0not a picture\xff\xd9".to_vec(),
            components: 3,
        },
        Pixels::Jpeg {
            bytes: b"\xff\xd8\xff\xe0not a picture\xff\xd9".to_vec(),
            components: 1,
        },
        Pixels::G4(vec![0x26, 0xa0, 0x00, 0x10, 0x01]),
    ] {
        images.insert(same_id(&[image(pixels)]));
    }
    assert_eq!(images.len(), 5);

    // A form's objects, and each alone.
    let form = form_objects();
    same_id(&form);
    for object in &form {
        same_id(std::slice::from_ref(object));
    }

    // One written already is named by its number.
    for id in [
        ObjectId::new(1, 0),
        ObjectId::new(7, 0),
        ObjectId::new(u32::MAX, 65535),
    ] {
        assert_eq!(Res::written(id).id.0, fam::Res::written(id).id().0);
        assert_eq!(Res::written(id).id.0, same_id(&[Object::from(id)]));
    }
}

/// Any object: every kind of value, strings literal and hex, reals of
/// every kind, dictionaries, arrays and streams with their data in
/// memory, nested.
fn an_object() -> impl Strategy<Value = Object> {
    let real = prop_oneof![
        proptest::num::f64::ANY,
        prop::sample::select(vec![f64::NAN, f64::INFINITY, 0.0, -0.0, 0.1, 0.5]),
    ];
    let format = prop::sample::select(vec![
        None,
        Some(StringFormat::Literal),
        Some(StringFormat::Hex),
    ]);
    let leaf = prop_oneof![
        Just(Object::null()),
        any::<bool>().prop_map(Object::from),
        any::<i64>().prop_map(Object::from),
        real.prop_map(Object::from),
        proptest::num::f32::ANY.prop_map(Object::from),
        bytes(0..10).prop_map(Object::name),
        (bytes(0..24), format)
            .prop_map(|(b, format)| Object::new(Value::String(Str::bytes(b), format))),
        (any::<u32>(), any::<u16>()).prop_map(|(n, g)| Object::from(ObjectId::new(n, g))),
    ];
    leaf.prop_recursive(4, 40, 5, |inner| {
        let dict = prop::collection::vec((bytes(0..6), inner.clone()), 0..5)
            .prop_map(|entries| {
                let mut dict = Dictionary::new();
                for (key, value) in entries {
                    dict.set(key, value);
                }
                dict
            })
            .boxed();
        prop_oneof![
            prop::collection::vec(inner, 0..5).prop_map(Object::from),
            dict.clone().prop_map(Object::from),
            (dict, bytes(0..48)).prop_map(|(dict, data)| Object::from(Stream::new(dict, data))),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Any objects whose streams hold their data in memory digest the
    /// same by sheaf's and by the family's, and the family takes them.
    #[test]
    fn any_objects_digest_the_same(objects in prop::collection::vec(an_object(), 1..4)) {
        let ours = ResId::of(&objects).0;
        prop_assert_eq!(ours, fam::ResId::of(&objects).0);
        prop_assert_eq!(Res::of(objects.clone()).id.0, ours);
        let theirs = fam::Res::of(objects.clone());
        prop_assert!(theirs.is_ok(), "{:?}", theirs);
        prop_assert_eq!(theirs.unwrap().id().0, ours);
    }
}

// ---- The names a resources dictionary already binds ----

/// A name `Sh` and six base32 characters, counted up by one.
fn next(name: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let n = name[2..].iter().fold(0u32, |n, c| {
        n * 32 + ALPHABET.iter().position(|a| a == c).unwrap() as u32
    });
    let n = (n + 1) & ((1 << 30) - 1);
    let mut out = b"Sh".to_vec();
    out.extend((0..6).rev().map(|i| ALPHABET[(n >> (5 * i)) as usize & 31]));
    out
}

/// A document read as a sink reads it, for [`reserved_shallow`].
struct Reads<'a>(&'a Pdf);

impl Sink for Reads<'_> {
    fn dict(&mut self, id: ObjectId) -> Option<Dictionary> {
        self.0.get_dictionary(id).ok().cloned()
    }

    fn array(&mut self, id: ObjectId) -> Option<Vec<Object>> {
        (self.0.get_object(id).ok()?.as_array()).map(<[Object]>::to_vec)
    }

    fn resource(&mut self, _: &Res) -> ObjectId {
        unreachable!("nothing is written")
    }

    fn reserved(&mut self, _: Option<&Object>) -> Reserved {
        unreachable!("read by the test")
    }
}

/// The names `resources` binds, read through `pdf` by the family's
/// `Reserved`.
fn family_deep(pdf: &Pdf, resources: Option<&Object>) -> fam::Reserved {
    let objects = resolve_fn(|id| pdf.objects.get(&id));
    fam::Reserved::read(objects, resources, &fam::Limits::DEFAULT)
}

/// [`reserved_shallow`] over the family's `Reserved`: the dictionary and
/// its categories followed through the sink, and nothing a name binds
/// read.
fn family_shallow(pdf: &Pdf, resources: Option<&Object>) -> fam::Reserved {
    let mut sink = Reads(pdf);
    let mut dict = match resources.map(Object::value) {
        Some(Value::Ref(r)) => sink.dict(*r).unwrap_or_default(),
        Some(Value::Dict(d)) => d.clone(),
        _ => return fam::Reserved::default(),
    };
    for &kind in fam::ResKind::ALL {
        if let Some(r) = dict.get(kind.key()).and_then(Object::as_reference) {
            let category = sink.dict(r).unwrap_or_default();
            dict.set(kind.key(), category);
        }
    }
    let dict = Object::from(dict);
    let nothing = resolve_fn(|_: ObjectId| None::<&Object>);
    fam::Reserved::read(nothing, Some(&dict), &fam::Limits::DEFAULT)
}

fn old_names(reserved: &Reserved) -> BTreeSet<(&'static [u8], Vec<u8>)> {
    (reserved.names())
        .map(|(kind, name)| (kind.key(), name.to_vec()))
        .collect()
}

fn family_names(reserved: &fam::Reserved) -> BTreeSet<(&'static [u8], Vec<u8>)> {
    (reserved.names())
        .map(|(kind, name)| (kind.key(), name.to_vec()))
        .collect()
}

/// `calls` named against `resources` read both ways by both: the same
/// names kept, the same content and needs. What it's named with the
/// resources read through, and read shallow.
#[track_caller]
fn named_against(pdf: &Pdf, resources: &Object, calls: &[Call]) -> (Named, Named) {
    let (ours, _) = old(calls);
    let (theirs, _) = family(calls);

    let (deep, their_deep) = (
        Reserved::read(pdf, Some(resources)),
        family_deep(pdf, Some(resources)),
    );
    assert_eq!(old_names(&deep), family_names(&their_deep));
    let through = old_named(&ours, &deep);
    assert_eq!(through, family_named(&theirs, &their_deep), "read through");

    let (shallow, their_shallow) = (
        reserved_shallow(&mut Reads(pdf), Some(resources)),
        family_shallow(pdf, Some(resources)),
    );
    assert_eq!(old_names(&shallow), family_names(&their_shallow));
    let by_number = old_named(&ours, &shallow);
    assert_eq!(
        by_number,
        family_named(&theirs, &their_shallow),
        "read shallow"
    );
    (through, by_number)
}

/// What a name is bound to in the resources a case reads.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Bound {
    /// The resource itself: its object, or the reference a written one
    /// is.
    Same,
    /// Another dictionary.
    Other,
    /// A reference to an object equal to the resource's.
    SameByRef,
    /// A reference to another dictionary.
    OtherByRef,
    /// A reference to a reference to an object equal to the resource's.
    SameByTwoRefs,
    /// A reference to no object.
    Missing,
    /// A stream that isn't the resource.
    OtherStream,
    Null,
}

const BOUNDS: [Bound; 8] = [
    Bound::Same,
    Bound::Other,
    Bound::SameByRef,
    Bound::OtherByRef,
    Bound::SameByTwoRefs,
    Bound::Missing,
    Bound::OtherStream,
    Bound::Null,
];

impl Bound {
    /// The object a name is bound to, for a need whose resource is
    /// `body`, with what it refers to added to `pdf`.
    fn object(self, body: &Body, pdf: &mut Pdf) -> Object {
        let same = match body {
            Body::Objects(objects) => objects[0].clone(),
            Body::Written(id) => Object::from(*id),
        };
        match self {
            Bound::Same => same,
            Bound::Other => list("Other").into(),
            Bound::SameByRef => pdf.add_object(same).into(),
            Bound::OtherByRef => pdf.add_object(list("Other")).into(),
            Bound::SameByTwoRefs => {
                let id = pdf.add_object(same);
                pdf.add_object(Object::from(id)).into()
            }
            Bound::Missing => Object::from(ObjectId::new(9999, 0)),
            Bound::OtherStream => Stream::new(Dictionary::new(), b"other".to_vec()).into(),
            Bound::Null => Object::null(),
        }
    }
}

/// How a resources dictionary and its categories are held.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Held {
    Direct,
    ByReference,
    CategoriesByReference,
    BothByReference,
}

const HELD: [Held; 4] = [
    Held::Direct,
    Held::ByReference,
    Held::CategoriesByReference,
    Held::BothByReference,
];

/// `entries`, a name and what it's bound to in a category, as a resources
/// dictionary held as `held` says.
fn resources(pdf: &mut Pdf, held: Held, entries: &[(&[u8], Vec<u8>, Object)]) -> Object {
    let mut dict = Dictionary::new();
    for (key, name, bound) in entries {
        let mut category = match dict.get(key) {
            Some(c) => c.as_dict().unwrap().clone(),
            None => Dictionary::new(),
        };
        category.set(name.clone(), bound.clone());
        dict.set(*key, category);
    }
    if matches!(held, Held::CategoriesByReference | Held::BothByReference) {
        let keys: Vec<Vec<u8>> = dict.iter().map(|(key, _)| key.to_vec()).collect();
        for key in keys {
            let category = dict.get(&key).unwrap().clone();
            let id = pdf.add_object(category);
            dict.set(key, id);
        }
    }
    match held {
        Held::ByReference | Held::BothByReference => pdf.add_object(dict).into(),
        Held::Direct | Held::CategoriesByReference => dict.into(),
    }
}

/// Content that draws with a resource of every kind sheaf makes: two
/// property lists, a font of one object and one of six, an ExtGState, a
/// form written already, an image.
fn every_kind() -> Vec<Call> {
    vec![
        marked("Artifact", "Header"),
        Call::EndMarked,
        marked("Artifact", "Footer"),
        Call::EndMarked,
        Call::Font(HELVETICA, 9.0),
        text(HELVETICA, "Hi"),
        Call::GState(GState {
            fill_alpha: Some(0.5),
            ..GState::default()
        }),
        Call::Draw(written(7)),
        Call::Draw(Drawn::Objects(vec![image(Pixels::Gray(vec![
            0, 85, 170, 255,
        ]))])),
        Call::Font(Face::Glyphless, 10.0),
        text(Face::Glyphless, "Hi"),
    ]
}

/// Whether a name bound so is the need's own resource, and so used again:
/// as far as the resources are read through, and read shallow.
fn used_again(bound: Bound, body: &Body) -> (bool, bool) {
    // What refers to another object isn't told by what it is: Glyphless's
    // font dictionary, which names its descendant.
    let told = match body {
        Body::Objects(objects) => objects.len() == 1,
        Body::Written(_) => false,
    };
    let written = matches!(body, Body::Written(_));
    match bound {
        // A written resource's own reference is itself.
        Bound::Same => (told || written, told || written),
        // Read through, a reference to an equal object is the resource;
        // shallow, it's a number that isn't.
        Bound::SameByRef | Bound::SameByTwoRefs => (told, false),
        Bound::Other | Bound::OtherByRef | Bound::Missing | Bound::OtherStream | Bound::Null => {
            (false, false)
        }
    }
}

/// Resources that bind the names the writer would give: to the very
/// resource, to another, by reference, with the next name up bound too,
/// the dictionary and its categories direct and by reference. Both read
/// the same names and name the content the same, and a name is used again
/// or counted past exactly where it should be.
#[test]
fn names_already_bound_are_read_the_same() {
    let calls = every_kind();
    let natural = same(&calls);
    assert_eq!(natural.1.len(), 7);
    let (mut cases, mut bumped, mut kept) = (0, 0, 0);
    for (i, (key, name, _, body)) in natural.1.iter().enumerate() {
        for bound in BOUNDS {
            for after in [None, Some(Bound::Other), Some(Bound::Same)] {
                for held in HELD {
                    let mut pdf = Pdf::with_version("1.7");
                    let mut entries = vec![(*key, name.clone(), bound.object(body, &mut pdf))];
                    if let Some(after) = after {
                        entries.push((*key, next(name), after.object(body, &mut pdf)));
                    }
                    let resources = resources(&mut pdf, held, &entries);
                    let (through, shallow) = named_against(&pdf, &resources, &calls);

                    let what = format!("need {i}, {bound:?}, then {after:?}, {held:?}");
                    for (named, again) in [
                        (&through, used_again(bound, body).0),
                        (&shallow, used_again(bound, body).1),
                    ] {
                        let after_again = after.is_some_and(|a| used_again(a, body).0);
                        let want = match (again, after) {
                            (true, _) => name.clone(),
                            (false, None) => next(name),
                            (false, Some(_)) if after_again => next(name),
                            (false, Some(_)) => next(&next(name)),
                        };
                        assert_eq!(named.1[i].1, want, "{what}");
                        if again {
                            kept += 1;
                        } else {
                            bumped += 1;
                        }
                        // Every other need keeps its name, and the
                        // content names each where it's drawn.
                        for (j, (need, natural)) in named.1.iter().zip(&natural.1).enumerate() {
                            if j != i {
                                assert_eq!(need, natural, "{what}");
                            }
                        }
                        assert_eq!(named.0.len(), natural.0.len());
                    }
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 7 * 8 * 3 * 4);
    assert!(kept > 100 && bumped > 100, "{kept} kept, {bumped} bumped");
}

/// `names_are_bound_at_the_end`'s cases, by both.
#[test]
fn the_writer_tests_reserved_cases_name_the_same() {
    let calls = two_lists();
    let natural = same(&calls);
    let (header, footer) = (natural.1[0].1.clone(), natural.1[1].1.clone());
    let mut pdf = Pdf::with_version("1.7");
    let same_list = pdf.add_object(list("Header"));
    for (bound, bumped) in [
        (Object::from(list("Other")), true),
        (Object::from(same_list), false),
        (list("Header").into(), false),
        (Object::from(ObjectId::new(99, 0)), true),
    ] {
        let resources: Object = dictionary! {
            "Properties" => dictionary! { header.clone() => bound.clone() },
        }
        .into();
        let (through, _) = named_against(&pdf, &resources, &calls);
        let want = if bumped {
            next(&header)
        } else {
            header.clone()
        };
        assert_eq!(through.1[0].1, want, "{bound:?}");
        assert_eq!(through.1[1].1, footer);
    }
    // A name in another category doesn't count.
    let fonts: Object = dictionary! {
        "Font" => dictionary! { header.clone() => list("Other") },
    }
    .into();
    let (through, shallow) = named_against(&pdf, &fonts, &calls);
    assert_eq!(through, natural);
    assert_eq!(shallow, natural);
    // A name reserved is never made.
    let resources: Object = dictionary! {
        "Properties" => dictionary! {
            header.clone() => list("Other"),
            next(&header) => list("Another"),
        },
    }
    .into();
    let (through, shallow) = named_against(&pdf, &resources, &calls);
    assert_eq!(through.1[0].1, next(&next(&header)));
    assert_eq!(shallow.1[0].1, next(&next(&header)));
    // No resources, and resources that aren't a dictionary.
    for resources in [
        Object::null(),
        Object::from(3),
        Object::from(ObjectId::new(99, 0)),
    ] {
        let (through, shallow) = named_against(&pdf, &resources, &calls);
        assert_eq!(through, natural);
        assert_eq!(shallow, natural);
    }
    let (ours, _) = old(&calls);
    let (theirs, _) = family(&calls);
    assert_eq!(
        old_named(&ours, &Reserved::read(&pdf, None)),
        family_named(&theirs, &family_deep(&pdf, None))
    );
}

/// Names that aren't the writer's, bound to the writer's own resources or
/// to anything else, in every category: kept, and they move no name.
#[test]
fn foreign_names_move_nothing() {
    let calls = every_kind();
    let natural = same(&calls);
    let foreign: [&[u8]; 9] = [
        b"F1",
        b"GS0",
        b"Im1",
        b"Sh",
        b"ShAAAAA",
        b"ShAAAAAAA",
        b"shAAAAAA",
        b"ShAAAAA1",
        b"ShAAAA A",
    ];
    for bound in BOUNDS {
        for held in HELD {
            let mut pdf = Pdf::with_version("1.7");
            let mut entries = Vec::new();
            for (key, _, _, body) in &natural.1 {
                for name in foreign {
                    entries.push((*key, name.to_vec(), bound.object(body, &mut pdf)));
                }
            }
            for kind in ResKind::ALL {
                entries.push((kind.key(), b"Fm0".to_vec(), list("Other").into()));
            }
            let resources = resources(&mut pdf, held, &entries);
            let (through, shallow) = named_against(&pdf, &resources, &calls);
            assert_eq!(through, natural, "{bound:?}, {held:?}");
            assert_eq!(shallow, natural, "{bound:?}, {held:?}");
            let kept = family_names(&family_deep(&pdf, Some(&resources)));
            assert_eq!(kept.len(), 4 * foreign.len() + 7, "{bound:?}, {held:?}");
        }
    }
}

/// Names taken by content written before, and names forgotten: the same
/// by both.
#[test]
fn names_taken_and_forgotten_are_the_same() {
    let first = every_kind();
    let natural = same(&first);
    let (key, header) = (natural.1[0].0, natural.1[0].1.clone());
    assert_eq!(key, b"Properties");

    // The header's name bound to another list, so it's counted past.
    let mut pdf = Pdf::with_version("1.7");
    let entries = [(key, header.clone(), Object::from(list("Other")))];
    let resources = resources(&mut pdf, Held::Direct, &entries);
    let mut ours = Reserved::read(&pdf, Some(&resources));
    let mut theirs = family_deep(&pdf, Some(&resources));

    let (w, _) = old(&first);
    let (their_w, _) = family(&first);
    let (content, needs) = w.named(&ours);
    let (their_content, their_needs) = their_w.named(&theirs);
    assert_eq!(content, their_content);
    assert_eq!(old_needs(&needs), family_needs(&their_needs));
    assert_eq!(needs.0[0].name, next(&header));

    // What's written after goes with it: the same resources get the same
    // names again, and another list one of its own.
    ours.take(&needs);
    theirs.take(&their_needs);
    assert_eq!(old_names(&ours), family_names(&theirs));
    let mut second = first.clone();
    second.extend([marked("Artifact", "Watermark"), Call::EndMarked]);
    let (w, _) = old(&second);
    let (their_w, _) = family(&second);
    let again = old_named(&w, &ours);
    assert_eq!(again, family_named(&their_w, &theirs));
    assert_eq!(again.1[..7], old_needs(&needs)[..]);

    // The name the other list had, forgotten, is free for the header.
    ours.forget(ResKind::Properties, &header);
    theirs.forget(fam::ResKind::Properties, &header);
    assert_eq!(old_names(&ours), family_names(&theirs));
    let free = old_named(&w, &ours);
    assert_eq!(free, family_named(&their_w, &theirs));
    assert_eq!(free.1[0].1, header);
    // And the one the header took, forgotten too.
    ours.forget(ResKind::Properties, &next(&header));
    theirs.forget(fam::ResKind::Properties, &next(&header));
    assert_eq!(old_names(&ours), family_names(&theirs));
    assert_eq!(old_named(&w, &ours), family_named(&their_w, &theirs));
}
