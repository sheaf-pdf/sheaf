//! Buttons and choices drawn by sheaf and by PDFium (M10 slice 8), for the
//! pixel checks (`buttons.rs`) and the other renderers (`interop.rs`), as
//! [`super::fields`] draws text fields, whose page, scale, places and
//! drawing this shares: each [`Case`] a form built in code, one widget on
//! a page of its own, [`SIDE`](super::fields::SIDE) points on a side,
//! upright, the widget turned by `/MK /R 90` or not ([`Place`]).
//!
//! Ours ([`ours`]) is the case's file as it was before the change, opened,
//! changed and saved as a Rewrite: a check box's or radio button's state
//! given (`tx::fixture::set_buttons`) and a choice's value (`choose`), by
//! `SetFieldValues` as the user sets them, and a push button's captions
//! (`caption`), a put no op makes. Its appearances are the save's own,
//! made by pdf-forms; `/AS` and the field's `/V` are the model's (M10
//! slices 8 and 9). PDFium's ([`theirs`]) is the file with the state,
//! value or captions and no appearance, its widgets drawn once under
//! PDFium's form-fill environment (`FPDF_FFLDraw`), whose load makes every
//! appearance a widget lacks, every slot and state, as the family's oracle
//! got them (`FORM_OnAfterLoadPage`); then saved by PDFium. Both are drawn
//! appearances only ([`fields::draw`]) and compared inside the widget's
//! box. A down or rollover appearance is drawn by a copy whose `/N` is
//! that entry ([`rebound`]), a check box's or radio button's other state
//! by a copy whose `/AS` names it ([`showing`]), on each side alike.

use sheaf_doc::Document;
use sheaf_doc::forms::fixture::{Button, COMBO, Form, RADIO};
use sheaf_doc::pdf::{Dictionary, Document as Pdf, Object, ObjectId, StreamExt, dictionary};
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{AnnotId, DocState};
use sheaf_image::raster::Raster;
use sheaf_render::pdfium::PdfiumEngine;

use super::fields::{self, Made, NAME, Place, SCALE, SIDE, live_in, pixels_of, rect_at};
use super::opened;

/// `/Ff` bits a combo box and a list box take.
pub const EDIT: i64 = 1 << 18;
pub const MULTI_SELECT: i64 = 1 << 21;

/// What a case's widget is, and what it shows once changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Shows {
    /// A check box or (`radio`) a radio button, its mark named by its
    /// `/MK /CA` `caption`, in its on state (`/Yes`, as the group's rules
    /// name a widget's that has no appearance and no `/Opt`) or `/Off`.
    Toggle {
        radio: bool,
        caption: Option<&'static str>,
        on: bool,
    },
    /// A combo box of `options` (export value, label), editable where
    /// `edit`, its value `value`.
    Combo {
        options: Vec<(&'static str, &'static str)>,
        value: &'static str,
        edit: bool,
    },
    /// A list box of `options`, several selectable where `multi`, its `/TI`
    /// `top` where given, its value's texts `values`.
    List {
        options: Vec<&'static str>,
        values: Vec<&'static str>,
        multi: bool,
        top: Option<i64>,
    },
    /// A push button's captions, `/MK /CA`, `/RC` and `/AC`, and whether
    /// its highlighting is a push (`/H /P`), which gives it rollover and
    /// down appearances.
    Push {
        caption: &'static str,
        rollover: Option<&'static str>,
        down: Option<&'static str>,
        pressed: bool,
    },
}

/// One button or choice field: what it shows, its `/DA`, box, `/MK`
/// colors and `/BS`.
#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub shows: Shows,
    pub da: String,
    /// The box's width and height as its text runs: the widget's `/Rect`
    /// is turned with it where `/MK /R` is 90.
    pub size: [f32; 2],
    pub place: Place,
    pub bg: Option<Vec<f32>>,
    pub bc: Option<Vec<f32>>,
    /// `/BS`: its width and its style.
    pub bs: Option<(i64, &'static str)>,
}

impl Case {
    /// A check box or (`radio`) a radio button 30 by 20, upright, on or
    /// off, its mark named by `caption`, in `/ZaDb` at an automatic size,
    /// with no `/MK` colors or `/BS`.
    pub fn toggle(name: &str, radio: bool, caption: Option<&'static str>, on: bool) -> Case {
        Case {
            name: name.into(),
            shows: Shows::Toggle { radio, caption, on },
            da: "/ZaDb 0 Tf 0 g".into(),
            size: [30.0, 20.0],
            place: Place::Upright,
            bg: None,
            bc: None,
            bs: None,
        }
    }

    /// A widget 150 by 24 showing `shows` in `/DR`'s Helvetica at 10
    /// points, black, with a solid black border of 1.
    pub fn of(name: &str, shows: Shows) -> Case {
        Case {
            name: name.into(),
            shows,
            da: "/Helv 10 Tf 0 g".into(),
            size: [150.0, 24.0],
            place: Place::Upright,
            bg: None,
            bc: Some(vec![0.0]),
            bs: Some((1, "S")),
        }
    }

    /// The case at `place`.
    pub fn at(mut self, place: Place) -> Case {
        self.place = place;
        self
    }

    /// Its name and place, for a message or a file's name.
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.place.label())
    }

    /// [`label`](Self::label) as a file's name.
    pub fn file_name(&self) -> String {
        let label = self.label().replace(['/', '(', ')'], "");
        label.split_whitespace().collect::<Vec<_>>().join("-")
    }

    /// The widget's `/Rect`.
    pub fn rect(&self) -> [f32; 4] {
        rect_at(self.size, self.place)
    }

    /// The widget's box in the page drawn at [`SCALE`].
    pub fn pixels(&self) -> [usize; 4] {
        pixels_of(self.rect(), self.place)
    }

    /// Whether its normal appearance paints anything: all but an `Off`
    /// with no background and no border color.
    pub fn paints(&self) -> bool {
        match self.shows {
            Shows::Toggle { on: false, .. } => self.bg.is_some() || self.bc.is_some(),
            _ => true,
        }
    }

    /// The case with a toggle's state the other.
    pub fn flipped(&self) -> Case {
        let mut case = self.clone();
        if let Shows::Toggle { on, .. } = &mut case.shows {
            *on = !*on;
        }
        case
    }

    /// The side, in pixels at [`SCALE`], of the centre square of the box
    /// less its border's reach (its width, twice that for a beveled or
    /// inset border, `1` where there's no `/BS`), as pdf-forms and PDFium
    /// place a mark in it.
    pub fn centre_side_px(&self) -> f32 {
        let (w, style) = self.bs.unwrap_or((1, "S"));
        let reach = match style {
            "B" | "I" => 2.0 * w as f32,
            _ => w as f32,
        };
        let [width, height] = self.size;
        (width.min(height) - 2.0 * reach) * SCALE as f32
    }

    /// The case's file with no appearance: as it was before the change
    /// (`theirs` false: a toggle's other state, a choice with no value, a
    /// push button with no captions), or with what the change gives
    /// (`theirs`: the state as `/V` and `/AS`, the value as `/V`, the
    /// captions in `/MK`), as PDFium is given it.
    pub fn file(&self, theirs: bool) -> Vec<u8> {
        let mut f = Form::new(1);
        let page = f.pages[0];
        let side = || vec![0.into(), 0.into(), SIDE.into(), SIDE.into()];
        let p = f.dict(page);
        p.set("MediaBox", side());
        // A page of no content, so only the widget draws.
        p.remove(b"Contents");
        if self.place.page_rotate() != 0 {
            p.set("Rotate", self.place.page_rotate());
        }
        let rect = self.rect();
        let numbers = |v: &[f32]| v.iter().map(|&n| Object::from(n)).collect::<Vec<_>>();
        let texts = |v: &[&str]| v.iter().map(|&t| Object::text(t)).collect::<Vec<_>>();
        let mut mk = Dictionary::new();
        let widget = match &self.shows {
            Shows::Toggle { radio, caption, on } => {
                let shown = if *on == theirs { "Yes" } else { "Off" };
                let button = Button {
                    page: 0,
                    rect,
                    on_state: None,
                    shows: Some(shown),
                };
                let ff = if *radio { RADIO } else { 0 };
                let (_, kids) = f.buttons(NAME, ff, &[button]);
                if let Some(caption) = caption {
                    mk.set("CA", Object::text(caption));
                }
                kids[0]
            }
            Shows::Combo {
                options,
                value,
                edit,
            } => {
                let id = f.choice(NAME, 0, rect, &[], true);
                let pairs: Vec<Object> = (options.iter())
                    .map(|&(export, label)| Object::from(texts(&[export, label])))
                    .collect();
                let d = f.dict(id);
                d.set("Opt", pairs);
                if *edit {
                    d.set("Ff", COMBO | EDIT);
                }
                if theirs {
                    d.set("V", Object::text(value));
                }
                id
            }
            Shows::List {
                options,
                values,
                multi,
                top,
            } => {
                let id = f.choice(NAME, 0, rect, options, false);
                let d = f.dict(id);
                if *multi {
                    d.set("Ff", MULTI_SELECT);
                }
                if let Some(top) = top {
                    d.set("TI", *top);
                }
                if theirs {
                    match values.as_slice() {
                        [one] => d.set("V", Object::text(one)),
                        many => d.set("V", texts(many)),
                    }
                }
                id
            }
            Shows::Push {
                caption,
                rollover,
                down,
                pressed,
            } => {
                let id = f.push_button(NAME, 0, rect, caption);
                let d = f.dict(id);
                d.remove(b"AP");
                d.remove(b"MK");
                if *pressed {
                    d.set("H", Object::name("P"));
                }
                if theirs {
                    mk.set("CA", Object::text(caption));
                    for (key, text) in [("RC", rollover), ("AC", down)] {
                        if let Some(text) = text {
                            mk.set(key, Object::text(text));
                        }
                    }
                }
                id
            }
        };
        if let Some(bg) = &self.bg {
            mk.set("BG", numbers(bg));
        }
        if let Some(bc) = &self.bc {
            mk.set("BC", numbers(bc));
        }
        if let Some(r) = self.place.mk_r() {
            mk.set("R", r);
        }
        let d = f.dict(widget);
        d.set("DA", sheaf_doc::pdf::literal(self.da.as_str()));
        if !mk.is_empty() {
            d.set("MK", mk);
        }
        if let Some((w, style)) = self.bs {
            d.set("BS", dictionary! { "W" => w, "S" => Object::name(style) });
        }
        f.bytes()
    }
}

/// A case, its twin (ours with one thing wrong, which must be past the
/// tolerance of PDFium's case), and what the twin has wrong.
pub type Twinned = (Case, Case, &'static str);

/// The places each case is drawn at: upright, and turned by `/MK /R 90`.
pub const PLACES: [Place; 2] = [Place::Upright, Place::UprightR90];

/// Each border style in RGB, and backgrounds in gray, RGB (beveled over
/// it, whose bands are its shades) and CMYK (a CMYK border round it), as
/// the text cases have them: `(name, /BS, /MK /BG, /MK /BC)`.
#[allow(clippy::type_complexity)]
pub fn frames() -> Vec<(
    &'static str,
    Option<(i64, &'static str)>,
    Option<Vec<f32>>,
    Option<Vec<f32>>,
)> {
    let blue = Some(vec![0.0, 0.0, 1.0]);
    vec![
        ("solid", Some((1, "S")), None, blue.clone()),
        ("dashed", Some((1, "D")), None, blue.clone()),
        ("beveled", Some((2, "B")), None, blue.clone()),
        ("inset", Some((2, "I")), None, blue.clone()),
        ("underline", Some((1, "U")), None, blue.clone()),
        ("background gray", None, Some(vec![0.8]), None),
        (
            "beveled on RGB",
            Some((2, "B")),
            Some(vec![1.0, 1.0, 0.6]),
            blue,
        ),
        (
            "background CMYK",
            Some((1, "S")),
            Some(vec![0.1, 0.2, 0.3, 0.0]),
            Some(vec![0.0, 0.0, 0.0, 1.0]),
        ),
    ]
}

/// A check box's and a radio button's `Off` (the radio's frame round, its
/// default mark being the circle) in each of [`frames`], and one with no
/// frame at all, which paints nothing, at each place: the twin on.
pub fn toggle_frames() -> Vec<Twinned> {
    let mut cases = Vec::new();
    for place in PLACES {
        for radio in [false, true] {
            let kind = if radio { "radio" } else { "check box" };
            let bare = Case::toggle(&format!("{kind} off, no frame"), radio, None, false);
            let framed = frames().into_iter().map(|(name, bs, bg, bc)| Case {
                name: format!("{kind} off, {name}"),
                bs,
                bg,
                bc,
                ..bare.clone()
            });
            for case in [bare.clone()].into_iter().chain(framed) {
                let case = case.at(place);
                let twin = case.flipped();
                cases.push((case, twin, "on"));
            }
        }
    }
    cases
}

/// A check box's and a radio button's mark, on, framed by a solid black
/// border: a check (`4`), a cross (`8`), a star (`H`), a circle (`l`) and
/// the default (none: a check box's check, a radio button's circle), at
/// each place. The twin shows another mark whose glyph pdf-forms fits a
/// share of the centre square another by a sixth of it or more (a check
/// `0.49`, a cross `1`, a star `0.70`, a radio's circle `0.5`, any other
/// two thirds), so the twin's ink reaches past the tolerance.
pub fn marks() -> Vec<Twinned> {
    let mut cases = Vec::new();
    for place in PLACES {
        for radio in [false, true] {
            let kind = if radio { "radio" } else { "check box" };
            let marks: [(&str, Option<&'static str>, &'static str); 5] = [
                ("check", Some("4"), "8"),
                ("cross", Some("8"), "4"),
                ("star", Some("H"), "4"),
                ("circle", Some("l"), if radio { "8" } else { "4" }),
                ("default", None, "8"),
            ];
            for (name, caption, other) in marks {
                let case = Case {
                    bs: Some((1, "S")),
                    bc: Some(vec![0.0]),
                    ..Case::toggle(&format!("{kind} {name}"), radio, caption, true)
                }
                .at(place);
                let mut twin = case.clone();
                if let Shows::Toggle { caption, .. } = &mut twin.shows {
                    *caption = Some(other);
                }
                cases.push((case, twin, "another mark"));
            }
        }
    }
    cases
}

/// The options of [`choices`]' combo boxes, by export value and label.
const COMBO_OPTIONS: [(&str, &str); 3] = [
    ("e1", "Alpha one"),
    ("e2", "Bravo two"),
    ("e3", "Charlie three"),
];

/// The options of [`choices`]' list boxes.
pub const LIST_OPTIONS: [&str; 6] = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];

/// A combo box showing the label of the option its value is, with its
/// drop button, on a background of RGB; an editable one showing its own
/// text; a list box with one option selected, one with several, and one
/// scrolled by `/TI`, each with its highlight, one beveled on gray; at
/// each place. The twin's value is another.
pub fn choices() -> Vec<Twinned> {
    let combo = |value, edit| Shows::Combo {
        options: COMBO_OPTIONS.to_vec(),
        value,
        edit,
    };
    let list = |values: Vec<&'static str>, multi, top| Shows::List {
        options: LIST_OPTIONS.to_vec(),
        values,
        multi,
        top,
    };
    let tall = [120.0, 80.0];
    let mut cases = Vec::new();
    for place in PLACES {
        let label = Case {
            bg: Some(vec![0.9, 0.95, 1.0]),
            ..Case::of("combo label", combo("e2", false))
        };
        let own = Case::of("combo own text", combo("Zulu 9", true));
        let one = Case {
            size: tall,
            ..Case::of("list one", list(vec!["bravo"], false, None))
        };
        let several = Case {
            size: tall,
            bs: Some((2, "B")),
            bg: Some(vec![0.75]),
            ..Case::of(
                "list several",
                list(vec!["bravo", "delta", "echo"], true, None),
            )
        };
        let scrolled = Case {
            size: tall,
            ..Case::of("list from /TI 2", list(vec!["delta"], false, Some(2)))
        };
        let twins = [
            (label, combo("e3", false)),
            (own, combo("Zulu 0", true)),
            (one, list(vec!["charlie"], false, None)),
            (several, list(vec!["bravo", "delta", "foxtrot"], true, None)),
            (scrolled, list(vec!["echo"], false, Some(2))),
        ];
        for (case, other) in twins {
            let case = case.at(place);
            let twin = Case {
                shows: other,
                ..case.clone()
            };
            cases.push((case, twin, "another value"));
        }
    }
    cases
}

/// A push button with a caption, beveled on gray, PDFium's usual push
/// button; and one whose highlighting is a push (`/H /P`), with a
/// rollover caption and a down one, so it has all three appearances; at
/// each place. The twin's normal caption is one glyph other.
pub fn pushes() -> Vec<Twinned> {
    let mut cases = Vec::new();
    for place in PLACES {
        let plain = Case {
            bs: Some((2, "B")),
            bg: Some(vec![0.75]),
            ..Case::of("push", push("Submit page", None, None, false))
        };
        let pressed = Case::of(
            "push /H /P",
            push("Submit page", Some("Hover here"), Some("Pressed"), true),
        );
        for case in [plain, pressed] {
            let case = case.at(place);
            let mut twin = case.clone();
            if let Shows::Push { caption, .. } = &mut twin.shows {
                *caption = leak(fields::one_other(caption));
            }
            cases.push((case, twin, "a caption other"));
        }
    }
    cases
}

/// A push button's [`Shows`].
pub fn push(
    caption: &'static str,
    rollover: Option<&'static str>,
    down: Option<&'static str>,
    pressed: bool,
) -> Shows {
    Shows::Push {
        caption,
        rollover,
        down,
        pressed,
    }
}

/// `text` for the life of the test, as a case's captions are.
fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// What sheaf saves of a case, and the document it saved.
pub struct Ours {
    pub doc: Document,
    pub saved: Vec<u8>,
}

/// The widgets of the field [`NAME`] in `st`.
pub fn widgets(st: &DocState) -> Vec<AnnotId> {
    let id = (st.field_named(NAME)).expect("the case's field");
    let term = st.field(id).and_then(|f| f.term.clone());
    term.expect("a terminal").widgets.iter().copied().collect()
}

/// `case`'s file as it was before the change, opened, changed through the
/// model, and saved as a Rewrite: its widget's appearances are the ones the
/// save made (the file has none).
pub fn ours(case: &Case) -> Ours {
    let file = case.file(false);
    assert!(ap_of(&file).is_none(), "{}", case.label());
    let mut doc = opened(file, "button.pdf", None);
    changed(&mut doc, &case.shows);
    let saved = sheaf_doc::save::rewrite(&doc).unwrap();
    assert!(
        ap_of(&saved).is_some(),
        "{}: the save made no appearance",
        case.label()
    );
    Ours { doc, saved }
}

/// `doc`'s field [`NAME`] given what `shows` shows, as one version.
pub fn changed(doc: &mut Document, shows: &Shows) {
    match shows {
        Shows::Toggle { on, .. } => fx::set_buttons(doc, NAME, if *on { "Yes" } else { "Off" }),
        Shows::Combo { value, .. } => fx::choose(doc, NAME, &[*value]),
        Shows::List { values, .. } => fx::choose(doc, NAME, values),
        Shows::Push {
            caption,
            rollover,
            down,
            ..
        } => {
            let [widget] = widgets(doc.state())[..] else {
                panic!("one widget");
            };
            fx::caption(doc, widget, Some(*caption), *rollover, *down);
        }
    }
}

/// What PDFium makes of `case`: its file with the state, value or captions
/// and no appearance, its widgets drawn once (which makes, under the
/// form-fill environment, every appearance each lacks), and saved by
/// PDFium (`FPDF_SaveAsCopy`).
pub fn theirs(case: &Case) -> Vec<u8> {
    let file = case.file(true);
    assert!(ap_of(&file).is_none(), "{}", case.label());
    let engine = PdfiumEngine::open_bytes(file.into(), None).unwrap();
    live_in(&engine);
    let copy = engine.save_copy().unwrap();
    assert!(
        ap_of(&copy).is_some(),
        "{}: PDFium made no appearance",
        case.label()
    );
    copy
}

/// The first widget's `/AP` on page 0 of `bytes`, references followed;
/// none where it has none.
pub fn ap_of(bytes: &[u8]) -> Option<Dictionary> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let &widget = widget_ids(&pdf).first()?;
    let ap = pdf.get_dictionary(widget).ok()?.get(b"AP")?;
    Some(pdf.dereference(ap).ok()?.1.as_dict()?.clone())
}

/// Page 0's annotations in `pdf`, by object.
fn widget_ids(pdf: &Pdf) -> Vec<ObjectId> {
    let page = pdf.page_iter().next().expect("a page");
    let Some(annots) = pdf.get_dictionary(page).unwrap().get(b"Annots") else {
        return Vec::new();
    };
    let annots = pdf.dereference(annots).unwrap().1;
    let annots = annots.as_array().expect("an array of annotations");
    annots.iter().filter_map(Object::as_reference).collect()
}

/// `bytes` with `change` made to each widget of page 0, written again.
fn edited(bytes: &[u8], change: impl Fn(&mut Pdf, ObjectId)) -> Vec<u8> {
    let mut pdf = Pdf::load_mem(bytes).unwrap();
    for widget in widget_ids(&pdf) {
        change(&mut pdf, widget);
    }
    let mut out = Vec::new();
    pdf.save_to(&mut out).unwrap();
    out
}

/// `bytes` with each widget of page 0 showing the state `state` (`/AS`).
pub fn showing(bytes: &[u8], state: &str) -> Vec<u8> {
    edited(bytes, |pdf, widget| {
        let d = pdf.get_dictionary_mut(widget).unwrap();
        d.set("AS", Object::name(state.as_bytes()));
    })
}

/// `bytes` with each widget of page 0's `/AP /N` its `/AP`'s `entry` (`D`
/// or `R`), so a viewer of normal appearances draws that one. Panics where
/// a widget has no such entry.
pub fn rebound(bytes: &[u8], entry: &str) -> Vec<u8> {
    edited(bytes, |pdf, widget| {
        let ap = pdf.get_dictionary(widget).unwrap().get(b"AP").cloned();
        let ap = ap.unwrap_or_else(|| panic!("{widget:?}: no /AP"));
        let dict = match ap.as_reference() {
            Some(id) => pdf.get_dictionary_mut(id).unwrap(),
            None => (pdf.get_dictionary_mut(widget).unwrap().get_mut(b"AP"))
                .and_then(Object::as_dict_mut)
                .unwrap(),
        };
        let other = (dict.get(entry.as_bytes()).cloned())
            .unwrap_or_else(|| panic!("{widget:?}: no /AP /{entry}"));
        dict.set("N", other);
    })
}

/// The box of the pixels inside `px` in which two drawings of a page
/// differ: left, top, right and bottom, the last two past it; none where
/// none does.
pub fn differing_box(a: &Raster, b: &Raster, px: [usize; 4]) -> Option<[usize; 4]> {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let [l, t, right, bottom] = px;
    let w = a.width as usize;
    let at = |x: usize, y: usize| (y * w + x) * 4..(y * w + x) * 4 + 4;
    let differ = (t..bottom)
        .flat_map(|y| (l..right).map(move |x| (x, y)))
        .filter(|&(x, y)| a.data[at(x, y)] != b.data[at(x, y)]);
    differ.fold(None, |found, (x, y)| {
        let [x0, y0, x1, y1] = found.unwrap_or([x, y, x + 1, y + 1]);
        Some([x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)])
    })
}

/// The normal appearance `/AP /N` names on the one widget of page 0 of
/// `bytes`, as [`fields::shown_text`] reads one: a stream, or the state
/// `/AS` names of a state dictionary.
pub fn normal(bytes: &[u8]) -> Option<Made> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let &widget = widget_ids(&pdf).first()?;
    let d = pdf.get_dictionary(widget).ok()?;
    let ap = pdf.dereference(d.get(b"AP")?).ok()?.1.as_dict()?.clone();
    let mut n = pdf.dereference(ap.get(b"N")?).ok()?.1;
    if n.as_stream().is_none() {
        let shows = d.get(b"AS")?.as_name()?;
        n = pdf.dereference(n.as_dict()?.get(shows)?).ok()?.1;
    }
    let stream = n.as_stream()?;
    Some(Made {
        keys: (ap.iter())
            .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
            .collect(),
        dict: stream.dict.clone(),
        content: stream.decompressed_content().unwrap(),
    })
}

/// How far a mark's box may lie from PDFium's, as a share of the side of
/// the centre square of the box less its border's reach
/// ([`Case::centre_side_px`]): pdf-forms' own hold of its glyph against
/// PDFium's shape (its PDFium cases' `mark` rule), the centre on each axis
/// and the width and height within [`MARK`], a cross's width within
/// [`CROSS_WIDTH`], its glyph narrower than PDFium's square cross.
pub const MARK: f32 = 0.04;
pub const CROSS_WIDTH: f32 = 0.16;

/// What drawing adds, in pixels: a box of inked pixels has each edge at
/// the pixel the shape's edge crosses, which anti-aliasing inks by its
/// coverage, so within a pixel of the edge; set against another drawing of
/// the same scale, its centre moves by up to [`RASTER`] on each axis and
/// its width and height by up to twice that.
pub const RASTER: f32 = 1.0;

/// How far a mark's box `ours` lies from PDFium's `theirs`, against the
/// tolerance for `case` ([`MARK`], [`CROSS_WIDTH`], [`RASTER`]) and
/// `slack` pixels more on each edge: each of the centre's two axes, the
/// width and the height as the page is drawn, as `(apart, allowed)`. A
/// cross's glyph is narrower across than PDFium's: across the page where
/// the widget is upright, down it where `/MK /R 90` turns it.
pub fn mark_apart(
    case: &Case,
    ours: [usize; 4],
    theirs: [usize; 4],
    slack: f32,
) -> [(f32, f32); 4] {
    let side = case.centre_side_px();
    let cross = matches!(
        case.shows,
        Shows::Toggle {
            caption: Some("8"),
            ..
        }
    );
    let wide = if cross { CROSS_WIDTH } else { MARK };
    let turned = case.place.mk_r().is_some() != (case.place.page_rotate() != 0);
    let [across, down] = if turned { [MARK, wide] } else { [wide, MARK] };
    let [a, b] = [ours, theirs].map(|p| p.map(|v| v as f32));
    let centre = |p: [f32; 4], i: usize| (p[i] + p[i + 2]) / 2.0;
    let size = |p: [f32; 4], i: usize| p[i + 2] - p[i];
    let (edge, extent) = (RASTER + slack, 2.0 * (RASTER + slack));
    [
        ((centre(a, 0) - centre(b, 0)).abs(), MARK * side + edge),
        ((centre(a, 1) - centre(b, 1)).abs(), MARK * side + edge),
        ((size(a, 0) - size(b, 0)).abs(), across * side + extent),
        ((size(a, 1) - size(b, 1)).abs(), down * side + extent),
    ]
}
