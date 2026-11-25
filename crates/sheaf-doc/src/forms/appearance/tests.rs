//! A text field's appearance from the model's plain data: the effective
//! `/DA` and `/Q`, the font and its fallbacks, the state forms' encodings,
//! hostile plain data, which widgets are stale, and a golden of what's
//! made, on forms built in code; and the corpus's IRS, NJ- and PA-40-style
//! fonts, where the corpus is.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;

use super::*;
use crate::content::Fallback;
use crate::content::fixture::FALLBACK_TEXT;
use crate::forms::corpus::Corpus;
use crate::forms::field::flags;
use crate::forms::fixture::{Form, Rect, STATE_TEXT, nj_helv, pa_arial, row};
use crate::materialize::tests::{add, insert, open};
use crate::model::Document;
use crate::pdf::content::read_back;
use crate::pdf::gfx::UnencodableChar;
use crate::pdf::{Object, dictionary, literal};
use crate::tx::fixture as fx;

/// A kid widget of field `parent` at `rect` on page 0, last in its
/// `/Annots`.
fn kid(f: &mut Form, parent: ObjectId, rect: Rect) -> ObjectId {
    let r: Vec<Object> = rect.iter().map(|&v| Object::from(v)).collect();
    let id = f.pdf.add_object(dictionary! {
        "Type" => Object::name("Annot"), "Subtype" => Object::name("Widget"),
        "Rect" => r, "F" => 4, "P" => f.pages[0], "Parent" => parent,
    });
    let mut annots = f.annots(0);
    annots.push(id);
    f.set_annots(0, &annots);
    id
}

/// Widget `n` of the field named `name` in `d`.
fn widget_of(d: &Document, name: &str, n: usize) -> AnnotId {
    let st = d.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    let term = st
        .field(id)
        .and_then(|f| f.term.clone())
        .expect("a terminal");
    term.widgets[n]
}

/// The look of widget `n` of the field named `name`.
fn look_of(d: &Document, name: &str, n: usize) -> TextLook {
    let aid = widget_of(d, name, n);
    match look(d.state(), aid) {
        Ok(Look::Text(look)) => look,
        other => panic!("{name}: a text widget: {other:?}"),
    }
}

/// `look`'s appearance in the font its `/DA` names in `src`, named as
/// sheaf names it (copy 0); a font named by an entry bound to no object,
/// which changes none of the content's bytes.
fn made_in(src: &Source, look: &TextLook) -> Result<Appearance, Unmade> {
    made_in_two(src, look).map(normal)
}

/// [`made_in`]'s appearance, and the fallback's glyphs it draws.
fn made_in_two(src: &Source, look: &TextLook) -> Result<Made, Unmade> {
    let name = look.frame.font_name();
    let font = font(src, name.as_deref()).map_err(Unmade::from)?;
    let written = |at: FontAt<'_>| match at {
        FontAt::Object(id) => id,
        FontAt::Entry(_) => ObjectId::new(0, 0),
    };
    let res = font_res(&font, src.id(), 0, &name.unwrap_or_default(), written);
    make_text(look, &font, res)
}

/// `look` made in `font`, named by `res`.
fn make_text(look: &TextLook, font: &FieldFont, res: Res) -> Result<Made, Unmade> {
    make(&Look::Text(look.clone()), DaFont { font, res })
}

/// The one form a text widget's look makes: `/N`'s, and no other.
fn normal(made: Made) -> Appearance {
    let mut forms = made.forms.into_iter();
    let (slot, form) = forms.next().expect("a form");
    let next = forms.next().map(|(slot, _)| slot);
    assert_eq!((slot, next), (Slot::Normal, None), "a text's /N alone");
    form
}

/// The bytes the content's one `Tj` shows.
pub(crate) fn shown(content: &[u8]) -> Vec<u8> {
    let (ops, _) = read_back::operations(content, None).expect("content that reads");
    let mut shows = ops.iter().filter(|(_, op)| *op == b"Tj");
    let (operands, _) = shows.next().expect("a Tj");
    assert!(shows.next().is_none(), "one Tj");
    string_bytes(operands[0])
}

/// A string operand's bytes, literal or hex, as written.
fn string_bytes(s: &[u8]) -> Vec<u8> {
    if let Some(hex) = s.strip_prefix(b"<").and_then(|s| s.strip_suffix(b">")) {
        let hex: Vec<u8> = hex.iter().copied().filter(u8::is_ascii_hexdigit).collect();
        return (hex.chunks(2))
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect();
    }
    let inner = (s.strip_prefix(b"(").and_then(|s| s.strip_suffix(b")"))).expect("a string");
    let mut out = Vec::new();
    let mut bytes = inner.iter();
    while let Some(&b) = bytes.next() {
        match b {
            b'\\' => out.push(*bytes.next().expect("an escaped byte")),
            b => out.push(b),
        }
    }
    out
}

/// A form whose `/DA` and `/Q` come from each level: `own`, its own
/// widget, sets both; `plain` sets neither, so the form's are its (`/Helv
/// 0 Tf 0 g`, centered); under the parent `p` (`/Cour 9 Tf 0 g`, right),
/// `p.k` has two kid widgets, the first with its own (`/TiRo 7 Tf 0 0 1
/// rg`, centered) and the second with none, and `p.k2` sets its own
/// (`/HeBo 8 Tf 0 g`, left) for its one kid widget.
fn levels() -> Vec<u8> {
    let mut f = Form::new(1);
    let acroform = f.acroform;
    f.dict(acroform).set("Q", 1);
    let own = f.text("own", 0, row(0));
    f.dict(own).set("DA", literal("/Helv 12 Tf 1 0 0 rg"));
    f.dict(own).set("Q", 2);
    let plain = f.text("plain", 0, row(1));
    f.dict(plain).remove(b"DA");
    let p = f.pdf.add_object(dictionary! {
        "FT" => Object::name("Tx"), "T" => literal("p"),
        "DA" => literal("/Cour 9 Tf 0 g"), "Q" => 2,
    });
    let k = f
        .pdf
        .add_object(dictionary! { "T" => literal("k"), "Parent" => p });
    let k2 = f.pdf.add_object(dictionary! {
        "T" => literal("k2"), "Parent" => p, "DA" => literal("/HeBo 8 Tf 0 g"), "Q" => 0,
    });
    let w1 = kid(&mut f, k, row(2));
    f.dict(w1).set("DA", literal("/TiRo 7 Tf 0 0 1 rg"));
    f.dict(w1).set("Q", 1);
    let w2 = kid(&mut f, k, row(3));
    let w3 = kid(&mut f, k2, row(4));
    f.dict(k).set("Kids", vec![Object::from(w1), w2.into()]);
    f.dict(k2).set("Kids", vec![Object::from(w3)]);
    f.dict(p).set("Kids", vec![Object::from(k), k2.into()]);
    let mut fields = f.fields();
    fields.push(p);
    f.set_fields(&fields);
    f.bytes()
}

/// A form of one page whose own `/DA` and `/Q` are other than [`levels`]':
/// `theirs`, which sets neither, is drawn in Times at 11 points in red,
/// right-justified.
fn theirs() -> Vec<u8> {
    let mut f = Form::new(1);
    let acroform = f.acroform;
    f.dict(acroform).set("DA", literal("/TiRo 11 Tf 1 0 0 rg"));
    f.dict(acroform).set("Q", 2);
    let theirs = f.text("theirs", 0, row(0));
    f.dict(theirs).remove(b"DA");
    f.bytes()
}

/// The effective `/DA` and `/Q` are a widget's own, else its field's (its
/// own or a parent's), else its instance's form's: the document's for the
/// file it opened, and an inserted file's own for its fields, which the
/// document's doesn't change.
#[test]
fn the_da_and_q_are_the_widget_s_then_the_field_s_then_the_form_s() {
    let mut d = open(&levels(), None);
    let said = |d: &Document, name: &str, n: usize| {
        let look = look_of(d, name, n);
        (String::from_utf8(look.frame.da).unwrap(), look.align)
    };
    let cases = [
        ("own", 0, "/Helv 12 Tf 1 0 0 rg", 2),
        ("plain", 0, "/Helv 0 Tf 0 g", 1),
        ("p.k", 0, "/TiRo 7 Tf 0 0 1 rg", 1),
        ("p.k", 1, "/Cour 9 Tf 0 g", 2),
        ("p.k2", 0, "/HeBo 8 Tf 0 g", 0),
    ];
    for (name, n, da, q) in cases {
        assert_eq!(said(&d, name, n), (da.to_string(), q), "{name} {n}");
    }

    // An inserted file's field draws by its own form's, and the opened
    // file's by the document's still.
    let other = add(&d, &theirs(), "theirs.pdf");
    insert(&mut d, &other, 0);
    assert_eq!(said(&d, "theirs", 0), ("/TiRo 11 Tf 1 0 0 rg".into(), 2));
    assert_eq!(said(&d, "plain", 0), ("/Helv 0 Tf 0 g".into(), 1));
    let st = d.state();
    let theirs = TextLook {
        text: "Sample 123".into(),
        ..look_of(&d, "theirs", 0)
    };
    assert_eq!(st.form().da.as_deref(), Some("/Helv 0 Tf 0 g"));
    assert_ne!(st.opened_file(), Some(other.src.id()));
    // Drawn in its own source's Times, by name: its /DR has none.
    let font = font(&other.src, theirs.frame.font_name().as_deref()).unwrap();
    assert!(font.is_fallback());
    let made = made_in(&other.src, &theirs).unwrap();
    assert_eq!(made.font_size, 11.0);
    let content = String::from_utf8_lossy(&made.content);
    assert!(content.contains("\n1 0 0 rg\n"), "{content}");
}

/// What the look holds is the model's: the box, `/MK`, the border and its
/// dashes, the flags and `/MaxLen`; a widget with no border is width 1,
/// solid; a check box's widget, or another annotation, has none, refused
/// as not a text field's.
#[test]
fn the_look_is_the_model_s_plain_data() {
    let mut f = Form::new(1);
    let a = f.text("a", 0, [10.0, 700.0, 110.0, 720.0]);
    f.dict(a).set("Ff", (1 << 24) | (1 << 12));
    f.dict(a).set("MaxLen", 7);
    f.dict(a).set(
        "MK",
        dictionary! {
            "BG" => vec![Object::from(1)],
            "BC" => vec![Object::from(0), 0.into(), 1.into()],
            "R" => 90,
        },
    );
    f.dict(a).set(
        "BS",
        dictionary! {
            "W" => 2, "S" => Object::name("D"), "D" => vec![Object::from(4), 2.into()],
        },
    );
    let b = f.text("b", 0, [10.0, 600.0, 60.0, 615.0]);
    f.fill(b, "Trenton");
    f.check_box("box", 0, [10.0, 500.0, 20.0, 510.0], true);
    let d = open(&f.bytes(), None);
    let look = look_of(&d, "a", 0);
    let frame = &look.frame;
    assert_eq!((frame.width, frame.height), (100.0, 20.0));
    assert_eq!(look.flags, flags::COMB | flags::MULTILINE);
    assert_eq!(look.max_len, Some(7));
    assert_eq!(frame.background, Some(field::Color::Gray(1.0)));
    assert_eq!(frame.border_color, Some(field::Color::Rgb([0.0, 0.0, 1.0])));
    assert_eq!(frame.rotation, 90);
    let dashed = Border {
        width: 2.0,
        style: BorderStyle::Dashed,
        dash: vec![4.0, 2.0],
    };
    assert_eq!(frame.border, dashed);
    assert_eq!(&*look.text, "");
    let b = look_of(&d, "b", 0);
    assert_eq!(&*b.text, "Trenton");
    assert_eq!(b.frame.border, Border::solid(1.0));
    // Never the text in its Debug.
    let debug = format!("{b:?}");
    assert!(
        !debug.contains("Trenton") && debug.contains("text_len: 7"),
        "{debug}"
    );
    let as_look = format!("{:?}", Look::Text(b.clone()));
    assert!(!as_look.contains("Trenton"), "{as_look}");
    assert_eq!(
        super::look(d.state(), widget_of(&d, "box", 0)),
        Err(Unmade::NotText)
    );
}

/// The IRS's navy, `0 0 0.502 rg`, is the text's color: a field of a
/// fixture with its `/DA`, and the corpus's f1040's, where the corpus is.
#[test]
fn an_irs_field_is_drawn_in_its_navy() {
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.dict(a).set("DA", literal("/Helv 8 Tf 0 0 0.502 rg"));
    let mut d = open(&f.bytes(), None);
    fx::fill(&mut d, "a", "123-45-6789");
    let look = look_of(&d, "a", 0);
    let made = made_in(d.primary(), &look).unwrap();
    let content = String::from_utf8_lossy(&made.content);
    assert!(content.contains("\n0 0 0.502 rg\nBT\n"), "{content}");

    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let bytes = std::fs::read(corpus.root.join("forms/irs/f1040.pdf")).unwrap();
    let d = open(&bytes, None);
    let st = d.state();
    let mut navy = 0;
    for (aid, _) in st.annots() {
        let Ok(Look::Text(mut look)) = super::look(st, aid) else {
            continue;
        };
        if !look.frame.da.ends_with(b" 0.502 rg") {
            continue;
        }
        look.text = "Sample 123".into();
        let made = made_in(d.primary(), &look).unwrap();
        let content = String::from_utf8_lossy(&made.content);
        assert!(content.contains("\n0 0 0.502 rg\nBT\n"), "{content}");
        navy += 1;
    }
    assert!(navy > 100, "the f1040's navy fields: {navy}");
}

/// "é € — “ ”" in each state font, one code a character:
/// PDFDocEncoding's in the `/Helv`, MacRoman's (and the `/Differences`'
/// Euro) in the `/Arial`.
const HELV_CODES: &[u8] = b"\xe9 \xa0 \x84 \x8d \x8e";
const ARIAL_CODES: &[u8] = b"\x8e \xdb \xd1 \xd2 \xd3";

/// The look of a field drawn in `/DR`'s font `name`, showing the state
/// forms' text.
fn state_look(name: &str) -> TextLook {
    TextLook {
        text: STATE_TEXT.into(),
        frame: Frame {
            da: format!("/{name} 10 Tf 0 g").into_bytes(),
            ..frame()
        },
        ..plain()
    }
}

/// "é € — “ ”" is one code a character in the NJ-style `/Helv` and the
/// PA-40-style `/Arial`: fonts built so, and the corpus's own, where the
/// corpus is.
#[test]
fn the_state_forms_text_is_a_code_a_character() {
    let mut f = Form::new(1);
    f.dr_font("Helv", nj_helv());
    f.dr_font("Arial", pa_arial());
    let src = f.source();
    for (name, codes) in [("Helv", HELV_CODES), ("Arial", ARIAL_CODES)] {
        let made = made_in(&src, &state_look(name)).unwrap();
        assert_eq!(shown(&made.content), codes, "{name}");
    }

    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let files = [
        ("forms/ca-ftb/2025-540.pdf", "Helv", HELV_CODES),
        ("forms/safedocs/0007134.pdf", "Arial", ARIAL_CODES),
    ];
    for (path, name, codes) in files {
        let bytes = std::fs::read(corpus.root.join(path)).unwrap();
        let src = crate::forms::fixture::source(bytes);
        let font = src.field_font(name.as_bytes()).unwrap();
        assert!(!font.is_fallback(), "{path}: /DR has its {name}");
        let made = made_in(&src, &state_look(name)).unwrap();
        assert_eq!(shown(&made.content), codes, "{path}");
    }
}

/// A `/DA` font `/DR` lacks falls back by its name, and says so; a `/DA`
/// with no font draws in Helvetica. Each is read once a name.
#[test]
fn a_missing_font_falls_back_by_name() {
    let src = Form::new(1).source();
    let base = |font: &FieldFont| match font.res().as_ref().map(Res::body) {
        Some(ResBody::Objects(objects)) => (objects[0].as_dict())
            .and_then(|d| d.get(b"BaseFont"))
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n).into_owned()),
        _ => None,
    };
    let cases = [
        ("ArialMT", "Helvetica"),
        ("Arial,Bold", "Helvetica-Bold"),
        ("HelveticaLTStd-Bold", "Helvetica-Bold"),
        ("TiRo", "Times-Roman"),
        ("CourierNewPSMT", "Courier"),
        ("Gone", "Helvetica"),
    ];
    for (name, face) in cases {
        let font = src.field_font(name.as_bytes()).unwrap();
        assert!(font.is_fallback(), "{name}");
        assert_eq!(base(&font).as_deref(), Some(face), "{name}");
        let again = src.field_font(name.as_bytes()).unwrap();
        assert!(Arc::ptr_eq(&font, &again), "{name}: read once");
    }
    // /DR's own Helvetica, by reference: no fallback.
    let helv = src.field_font(b"Helv").unwrap();
    assert!(!helv.is_fallback());
    let by_reference = helv.res().as_ref().map(Res::body).cloned();
    assert!(matches!(by_reference, Some(ResBody::Written(_))));
    // No font named: Helvetica.
    let look = TextLook {
        frame: Frame {
            da: b"0 g".to_vec(),
            ..frame()
        },
        ..plain()
    };
    assert_eq!(look.frame.font_name(), None);
    let font = font(&src, None).unwrap();
    assert_eq!(base(&font).as_deref(), Some("Helvetica"));
    let made = made_in(&src, &look).unwrap();
    assert!(
        made.font_size >= 4.0,
        "an automatic size: {}",
        made.font_size
    );
}

/// A plain look: a box of 200 by 20, the text `Golden 123` left-justified
/// in Helvetica at 12 points, black, a black border of width 1.
fn plain() -> TextLook {
    TextLook {
        text: "Golden 123".into(),
        flags: 0,
        max_len: None,
        align: 0,
        frame: frame(),
    }
}

/// A plain look's frame ([`plain`]).
fn frame() -> Frame {
    Frame {
        width: 200.0,
        height: 20.0,
        background: None,
        border_color: Some(field::Color::Gray(0.0)),
        rotation: 0,
        border: Border::solid(1.0),
        da: b"/Helv 12 Tf 0 g".to_vec(),
    }
}

/// Helvetica, as a field whose `/DR` lacks its font draws in it, and its
/// own resource.
fn helvetica() -> (FieldFont, Res) {
    let font = FieldFont::standard(Std14::Helvetica);
    let res = font.res().expect("a fallback's dictionary");
    (font, res)
}

/// Each kind of reason is named by hand, in sheaf's words: none is a
/// type's `Debug` text, which a rename in the PDF layer would move under
/// a warning, the fields dump and the corpus tier's counts.
#[test]
fn every_kind_is_named_by_hand() {
    use crate::pdf::forms::Cap;
    use crate::pdf::{Unreadable, Why};
    let unreadable = |why| Unreadable::new(ObjectId::new(7, 0), why, "no");
    let whys = [
        (Why::Damaged, "damaged"),
        (Why::Limit, "limit"),
        (Why::Capped, "capped"),
        (Why::Locked, "locked"),
        (Why::Source, "source"),
        (Why::Filter, "filter"),
        (Why::Detached, "detached"),
    ];
    for (why, name) in whys {
        let want = format!("unreadable:{name}");
        assert_eq!(unmade_kind(&Unmade::Unreadable(unreadable(why))), want);
        assert_eq!(error_kind(&Error::Unreadable(unreadable(why))), want);
    }
    let caps = [
        (Cap::Fields, "fields"),
        (Cap::Items, "items"),
        (Cap::Text, "text"),
        (Cap::CombCells, "comb-cells"),
        (Cap::FontBudget, "font-budget"),
        (
            Cap::Annots(AnnotsCap::AnnotsPerPage),
            "annots:annots-per-page",
        ),
        (Cap::Annots(AnnotsCap::ActionChain), "annots:action-chain"),
        (Cap::Annots(AnnotsCap::Quads), "annots:quads"),
        (Cap::Annots(AnnotsCap::Points), "annots:points"),
        (Cap::Annots(AnnotsCap::DestTree), "annots:dest-tree"),
    ];
    for (cap, name) in caps {
        let want = format!("limit:{name}");
        assert_eq!(unmade_kind(&Unmade::Limit(cap)), want);
        assert_eq!(error_kind(&Error::Limit(cap)), want);
    }
    let plain = [
        (Unmade::NoBox, "no-box"),
        (Unmade::NotFinite, "not-finite"),
        (Unmade::NotText, "not-text"),
        (Unmade::FontNotBindable, "font-not-bindable"),
    ];
    for (why, name) in plain {
        assert_eq!(unmade_kind(&why), name);
    }
}

/// A value is drawn in the field's font, and what that lacks in the
/// fallback ([`Fallback::SANS`], named by its marker): a WinAnsi value
/// names Helvetica alone and draws no glyph of the fallback; a name with
/// `Ł` and `ễ` draws those two in it, each glyph with its text, and
/// nothing else; decomposed, it draws as composed, byte for byte. A
/// cluster neither font draws, a CJK ideograph or an emoji, leaves it
/// unmade, `unencodable`, at its byte offset in the text.
#[test]
fn a_value_is_drawn_in_the_field_s_font_then_the_fallback() {
    let (font, res) = helvetica();
    let made = |text: &str| {
        let look = TextLook {
            text: text.into(),
            ..plain()
        };
        make_text(&look, &font, res.clone())
    };
    let ids =
        |a: &Appearance| -> BTreeSet<ResId> { a.needs.0.iter().map(|n| n.res.id()).collect() };
    let marker = Fallback::SANS.res().id();

    let winansi = made("Camden \u{d6}l\u{e7}er \u{20ac}").unwrap();
    assert!(winansi.glyphs.is_empty());
    assert_eq!(ids(&normal(winansi)), BTreeSet::from([res.id()]));

    let two = made(FALLBACK_TEXT).unwrap();
    assert_eq!(
        ids(&normal(two.clone())),
        BTreeSet::from([res.id(), marker])
    );
    let program = Fallback::SANS.program();
    let texts: Vec<(u16, &str)> = two.glyphs.iter().collect();
    let mut want: Vec<(u16, &str)> = ["\u{141}", "\u{1ec5}"]
        .map(|c| (program.glyph(c.chars().next().unwrap()).unwrap(), c))
        .into_iter()
        .collect();
    want.sort();
    assert_eq!(texts, want, "Ł and ễ, each with its text");
    // What's made says how many glyphs, never which.
    let debug = format!("{two:?}");
    assert!(
        debug.contains("glyphs: 2") && !debug.contains('\u{141}'),
        "{debug}"
    );
    let decomposed: String = FALLBACK_TEXT.nfd().collect();
    assert_ne!(decomposed, FALLBACK_TEXT);
    assert_eq!(made(&decomposed), Ok(two), "drawn composed");

    for (text, at, character) in [
        ("Camden \u{738b}", 7, '\u{738b}'),
        ("\u{1f600}", 0, '\u{1f600}'),
        ("\u{141}ukasz \u{1f600}!", 8, '\u{1f600}'),
    ] {
        let want = Unmade::Unencodable(UnencodableChar { at, character });
        assert_eq!(made(text), Err(want), "{at}");
        assert_eq!(unmade_kind(&made(text).unwrap_err()), "unencodable");
    }
}

/// Plain data no file should give, but a model edited by hand might: a
/// typed refusal or an appearance, never a panic.
#[test]
fn hostile_plain_data_is_refused_by_kind_or_made() {
    use crate::pdf::forms::Cap;
    let (font, res) = helvetica();
    let made = |look: TextLook| make_text(&look, &font, res.clone()).map(normal);
    let comb = flags::COMB;
    let p = plain;
    let framed = |frame: Frame| TextLook { frame, ..p() };
    let f = frame;
    assert_eq!(
        made(framed(Frame { width: 0.0, ..f() })),
        Err(Unmade::NoBox)
    );
    assert_eq!(
        made(framed(Frame { height: 0.0, ..f() })),
        Err(Unmade::NoBox)
    );
    assert!(
        made(framed(Frame {
            width: f32::NAN,
            ..f()
        }))
        .is_err()
    );
    assert!(
        made(framed(Frame {
            height: f32::INFINITY,
            ..f()
        }))
        .is_err()
    );
    let cells = TextLook {
        flags: comb,
        max_len: Some(u32::MAX),
        ..p()
    };
    assert_eq!(made(cells), Err(Unmade::Limit(Cap::CombCells)));
    let long = "x".repeat(crate::source::Limits::DEFAULT.forms.text + 1);
    let long = TextLook {
        text: long.into(),
        ..p()
    };
    assert_eq!(made(long), Err(Unmade::Limit(Cap::Text)));
    let unencodable = TextLook {
        text: "中".into(),
        ..p()
    };
    assert!(matches!(made(unencodable), Err(Unmade::Unencodable(_))));
    // An empty or unreadable /DA draws in Helvetica at an automatic size.
    for da in [&b""[..], b"/Helv x Tf ((( rg", b"Tf Tf Tf", b"\xff\x00/"] {
        let look = framed(Frame {
            da: da.to_vec(),
            ..f()
        });
        assert!(made(look).is_ok(), "{da:?}");
    }
    // A border wider than its box, dashes past reason, any rotation the
    // model holds, more text than a comb's cells, a box at the model's
    // reach: each made or refused, by kind.
    let wide = framed(Frame {
        border: Border {
            width: 100.0,
            style: BorderStyle::Beveled,
            dash: Vec::new(),
        },
        width: 10.0,
        height: 10.0,
        ..f()
    });
    let _ = made(wide);
    let dashes = framed(Frame {
        border: Border {
            width: 1.0,
            style: BorderStyle::Dashed,
            dash: vec![1e30, 0.0, f32::MAX],
        },
        ..f()
    });
    let _ = made(dashes);
    for rotation in [0, 90, 180, 270] {
        assert!(
            made(framed(Frame { rotation, ..f() })).is_ok(),
            "{rotation}"
        );
    }
    let many = TextLook {
        flags: comb | flags::MULTILINE | flags::PASSWORD,
        max_len: Some(1000),
        text: "9".repeat(5000).into(),
        ..p()
    };
    assert!(made(many).unwrap().overflowed);
    let far = framed(Frame {
        width: 2_000_000.0,
        height: 2_000_000.0,
        da: b"/Helv 0 Tf 0 g".to_vec(),
        ..f()
    });
    let _ = made(far);
}

/// Whether a widget needs its appearance made: not as read; when its text
/// moved, cleared too; not when it's put back, or a field without one is
/// given an empty one; never for a check box.
#[test]
fn a_widget_is_stale_while_its_text_isn_t_the_file_s() {
    let mut f = Form::new(1);
    let city = f.text("city", 0, row(0));
    f.fill(city, "Trenton");
    f.text("empty", 0, row(1));
    f.check_box("box", 0, [72.0, 600.0, 82.0, 610.0], true);
    let mut d = open(&f.bytes(), None);
    let (city, empty, check) = (
        widget_of(&d, "city", 0),
        widget_of(&d, "empty", 0),
        widget_of(&d, "box", 0),
    );
    let stale_ones = |d: &Document| [city, empty, check].map(|aid| stale(d.state(), aid));
    assert_eq!(stale_ones(&d), [false, false, false], "as read");
    let steps = [
        ("city", "Camden", [true, false, false]),
        ("city", "", [true, false, false]),
        ("city", "Trenton", [false, false, false]),
        ("empty", "", [false, false, false]),
        ("empty", "Newark", [false, true, false]),
    ];
    for (name, value, want) in steps {
        fx::fill(&mut d, name, value);
        assert_eq!(stale_ones(&d), want, "{name} = {value:?}");
    }
}

/// Every case of [`golden`]: its name and its look, and whether it's drawn
/// in `/DR`'s font by reference (else Helvetica, by its dictionary).
fn golden_cases() -> Vec<(&'static str, TextLook, bool)> {
    let p = plain;
    let f = frame;
    let framed = |frame: Frame| TextLook { frame, ..p() };
    let bordered = |width: f32, style: BorderStyle, dash: &[f32]| {
        framed(Frame {
            border: Border {
                width,
                style,
                dash: dash.to_vec(),
            },
            background: Some(field::Color::Gray(0.75)),
            ..f()
        })
    };
    let multiline = "First line\nand a second line long enough to wrap twice over in the box";
    let comb = |align: u8| TextLook {
        flags: flags::COMB,
        max_len: Some(10),
        text: "1234567".into(),
        align,
        ..p()
    };
    let lines = |da: &[u8]| TextLook {
        flags: flags::MULTILINE,
        text: multiline.into(),
        frame: Frame {
            height: 60.0,
            da: da.to_vec(),
            ..f()
        },
        ..p()
    };
    let auto = b"/Helv 0 Tf 0 g";
    vec![
        ("left", p(), false),
        ("center", TextLook { align: 1, ..p() }, false),
        ("right", TextLook { align: 2, ..p() }, false),
        ("comb", comb(0), false),
        ("comb right", comb(2), false),
        ("multiline", lines(b"/Helv 10 Tf 0 g"), false),
        (
            "auto size",
            framed(Frame {
                da: auto.to_vec(),
                ..f()
            }),
            false,
        ),
        ("auto size multiline", lines(auto), false),
        ("solid 2", bordered(2.0, BorderStyle::Solid, &[]), false),
        (
            "dashed",
            bordered(1.0, BorderStyle::Dashed, &[3.0, 2.0]),
            false,
        ),
        (
            "dashed by default",
            bordered(1.0, BorderStyle::Dashed, &[]),
            false,
        ),
        ("beveled", bordered(2.0, BorderStyle::Beveled, &[]), false),
        ("inset", bordered(2.0, BorderStyle::Inset, &[]), false),
        (
            "underline",
            bordered(1.0, BorderStyle::Underline, &[]),
            false,
        ),
        (
            "rotated 90",
            framed(Frame {
                rotation: 90,
                ..f()
            }),
            false,
        ),
        (
            "rotated 180",
            framed(Frame {
                rotation: 180,
                ..f()
            }),
            false,
        ),
        (
            "rotated 270",
            framed(Frame {
                rotation: 270,
                ..f()
            }),
            false,
        ),
        (
            "password",
            TextLook {
                flags: flags::PASSWORD,
                ..p()
            },
            false,
        ),
        (
            "empty",
            TextLook {
                text: "".into(),
                ..p()
            },
            false,
        ),
        (
            "navy on blue",
            framed(Frame {
                background: Some(field::Color::Rgb([0.9, 0.9, 1.0])),
                border_color: Some(field::Color::Rgb([0.0, 0.0, 0.502])),
                da: b"/Helv 9 Tf 0 0 0.502 rg".to_vec(),
                ..f()
            }),
            false,
        ),
        (
            "cmyk",
            framed(Frame {
                border_color: Some(field::Color::Cmyk([0.0, 0.0, 0.0, 1.0])),
                da: b"/Helv 9 Tf 0 0 0 1 k".to_vec(),
                ..f()
            }),
            false,
        ),
        ("by reference", p(), true),
        // In two fonts (M10 slice 7): what Helvetica lacks drawn in the
        // fallback, named by its marker.
        ("two fonts", two(p()), false),
        ("two fonts by reference", two(p()), true),
        (
            "two fonts decomposed",
            TextLook {
                text: FALLBACK_TEXT.nfd().collect::<String>().into(),
                ..p()
            },
            false,
        ),
        ("two fonts right", two(TextLook { align: 2, ..p() }), false),
        (
            "two fonts multiline",
            TextLook {
                text: format!("{FALLBACK_TEXT}\n{multiline}").into(),
                ..lines(b"/Helv 10 Tf 0 g")
            },
            false,
        ),
        (
            "two fonts comb",
            TextLook {
                text: "\u{141}\u{f3}d\u{17a} 12".into(),
                ..comb(0)
            },
            false,
        ),
        (
            "two fonts auto size",
            two(framed(Frame {
                da: auto.to_vec(),
                ..f()
            })),
            false,
        ),
        // A password's `*` is Helvetica's: nothing in the fallback.
        (
            "two fonts password",
            two(TextLook {
                flags: flags::PASSWORD,
                ..p()
            }),
            false,
        ),
    ]
}

/// `look` showing [`FALLBACK_TEXT`], which Helvetica draws but for `Ł`
/// and `ễ`.
fn two(look: TextLook) -> TextLook {
    TextLook {
        text: FALLBACK_TEXT.into(),
        ..look
    }
}

/// A made appearance as a line of the golden: its box, matrix, size,
/// whether it overflowed, what it draws with (each need's name and the
/// resource's id: the fallback by its marker's, as the emitter names it),
/// and its content's length and checksum, then, where it draws in the
/// fallback, how many glyphs (`fallback=`); or why it wasn't made.
fn golden_line(case: &str, made: &Result<Made, Unmade>) -> String {
    let (a, glyphs) = match made {
        Ok(made) => (&normal(made.clone()), &made.glyphs),
        Err(why) => return format!("{case}\tunmade:{why}"),
    };
    let (b, m) = (&a.bbox, &a.matrix);
    let needs: Vec<String> = (a.needs.0.iter())
        .map(|n| format!("{}:{:032x}", String::from_utf8_lossy(&n.name), n.res.id().0))
        .collect();
    let fallback = match glyphs.len() {
        0 => String::new(),
        n => format!("\tfallback={n}"),
    };
    format!(
        "{case}\tbbox={} {} {} {}\tmatrix={} {} {} {} {} {}\tsize={}\tovf={}\tneeds={}\t{}:{}{fallback}",
        b.x0,
        b.y0,
        b.x1,
        b.y1,
        m.a,
        m.b,
        m.c,
        m.d,
        m.e,
        m.f,
        a.font_size,
        u8::from(a.overflowed),
        needs.join("+"),
        a.content.len(),
        sheaf_util::checksum_hex(&a.content),
    )
}

/// What sheaf makes of each case, a line each.
fn golden() -> String {
    let (helv, helv_res) = helvetica();
    // /DR's Helvetica by reference, named as sheaf names it, from a blob,
    // copy and object fixed here: what the content names doesn't move
    // with the fixture's bytes.
    let src = Form::new(1).source();
    let by_ref = src.field_font(b"Helv").unwrap();
    let blob = BlobId {
        hash: 0x5eaf,
        len: 6,
    };
    let named = font_res(&by_ref, blob, 0, b"Helv", |_| ObjectId::new(7, 0));
    let mut out = String::new();
    for (case, look, reference) in golden_cases() {
        let made = match reference {
            true => make_text(&look, &by_ref, named.clone()),
            false => make_text(&look, &helv, helv_res.clone()),
        };
        let _ = writeln!(out, "{}", golden_line(case, &made));
    }
    out
}

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/forms/appearance_golden.txt"
);

/// What sheaf makes of each alignment, comb, multiline, an automatic size,
/// each border style, `/MK /R` 90, 180 and 270, a password field and an
/// empty value is what the golden holds, line for line: a move of
/// pdf-forms' pin that changes a byte is seen here. `SHEAF_GOLDEN_UPDATE=1`
/// writes it.
#[test]
fn what_sheaf_makes_is_what_the_golden_holds() {
    let text = golden();
    assert_eq!(text, golden(), "the same twice");
    if std::env::var_os("SHEAF_GOLDEN_UPDATE").is_some_and(|v| v == "1") {
        std::fs::write(GOLDEN, &text).expect("the golden written");
        return;
    }
    let want = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("{GOLDEN}: {e}; SHEAF_GOLDEN_UPDATE=1 writes it"));
    let mut differs = String::new();
    for (i, (got, held)) in text.lines().zip(want.lines()).enumerate() {
        if got != held {
            let _ = writeln!(differs, "line {}:\n  held {held}\n  got  {got}", i + 1);
        }
    }
    let (got, held) = (text.lines().count(), want.lines().count());
    if got != held {
        let _ = writeln!(differs, "{got} lines made, {held} held");
    }
    assert!(
        differs.is_empty(),
        "what sheaf makes differs from {GOLDEN}: a byte of a made appearance moved, to \
         find (read each line, then SHEAF_GOLDEN_UPDATE=1 writes it):\n{differs}"
    );
    // A case a line, none refused.
    assert_eq!(got, golden_cases().len());
    assert!(!text.contains("\tunmade:"), "{text}");
}

/// The name a `/DR` font is given is its source's, copy's and place's
/// alone: the same run to run, and another for each of them.
#[test]
fn a_dr_font_is_named_by_its_source_copy_and_place() {
    let blob = BlobId { hash: 1, len: 2 };
    let at = FontAt::Object(ObjectId::new(7, 0));
    let id = font_res_id(blob, 0, at);
    assert_eq!(id, font_res_id(blob, 0, at));
    let others = [
        font_res_id(BlobId { hash: 2, len: 2 }, 0, at),
        font_res_id(BlobId { hash: 1, len: 3 }, 0, at),
        font_res_id(blob, 1, at),
        font_res_id(blob, 0, FontAt::Object(ObjectId::new(8, 0))),
        font_res_id(blob, 0, FontAt::Object(ObjectId::new(7, 1))),
        font_res_id(blob, 0, FontAt::Entry(b"Helv")),
    ];
    for (i, other) in others.iter().enumerate() {
        assert_ne!(*other, id, "{i}");
    }
    // A fallback is named by its dictionary, whatever its source.
    let (helv, res) = helvetica();
    let named = font_res(&helv, blob, 3, b"Gone", |_| unreachable!());
    assert_eq!(named, res);
}

/// The caps are pdf-forms' with each sheaf has one of in its place, and
/// the read layer's tree caps the family's.
#[test]
fn the_caps_are_the_family_s_with_sheaf_s() {
    let ours = crate::source::Limits::DEFAULT;
    let family = Limits::DEFAULT;
    let caps = limits();
    assert_eq!(caps.hops, ours.reader.max_depth);
    assert_eq!(caps.annots, crate::source::annots::limits());
    assert_eq!(caps.text, ours.forms.text);
    assert_eq!(caps.comb_cells, crate::forms::read::MAX_LAYOUT_LEN as usize);
    assert_eq!(caps.fonts, ours.fonts());
    assert_eq!(caps.font_budget, ours.content.decompressed);
    assert_eq!(
        (caps.fields, caps.depth, caps.items),
        (family.fields, family.depth, family.items)
    );
    // Each of sheaf's is its own, not the family's.
    assert_ne!(caps.text, family.text);
    assert_ne!(caps.hops, family.hops);
    assert_ne!(caps.font_budget, family.font_budget);
}
