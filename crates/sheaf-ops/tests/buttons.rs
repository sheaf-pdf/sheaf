//! A check box's, a radio button's, a combo box's, a list box's and a push
//! button's appearances as sheaf makes them (pdf-forms' makers, M10 slice
//! 8) against the ones PDFium makes for the same state, value or captions,
//! by pixels. Each case is a form built in code ([`common::buttons`]):
//! ours is the field changed through the model and saved, PDFium's the
//! field given the state, value or captions and no appearance, drawn
//! under PDFium's form-fill environment and saved by PDFium; both are
//! drawn appearances only at two pixels a point and compared inside the
//! widget's box ([`against_pdfium`]).
//!
//! The frames and the text are held to the text cases' tolerance: a check
//! box's and a radio button's `Off` (the radio's frame round) in each
//! border style and colour space, and with no frame; a combo box's label
//! and its own text, with the drop button; a list box with one option
//! selected, several, and scrolled by `/TI`, with the highlight; a push
//! button's caption; each upright and turned by `/MK /R 90`. A mark (a
//! check, a cross, a star, a circle, the default; a check box's and a radio
//! button's) is a ZapfDingbats glyph in ours and a path of PDFium's own in
//! PDFium's, so it's held by where its ink reaches, as pdf-forms holds it:
//! the box of the pixels a widget's on state draws otherwise than its
//! `Off`, its centre and its extent against PDFium's within the tolerance
//! [`common::buttons::mark_apart`] states and derives (pdf-forms' own
//! against PDFium's shape, and a pixel of raster an edge). The down and
//! rollover appearances are drawn
//! by a copy whose `/N` is that entry, on each side alike. Every case has a
//! twin, ours with one thing other (on against off, another mark, another
//! value, another caption), which must be past the tolerance, so that a
//! case can fail. These run on Linux alone, as the text cases do: on
//! Windows and macOS PDFium draws the standard 14 in the system's faces.
//!
//! On every system: a realized page draws each changed kind as its saved
//! file does, appearances only and live; a combo box's, a list box's and a
//! push button's text reads back by sheaf's interpreter; and a check box
//! group whose widgets have on states of their own, so that they act as
//! radio buttons, pressed through the model, shows the pressed one on and
//! the others off, where its file draws its states and where it draws
//! none.
//!
//! PDFium is sheaf's linked build (`scripts/fetch-pdfium.sh`). Needs
//! libpdfium, like the app itself.

mod common;

use futures::executor::block_on;
use sheaf_doc::forms::fixture::{Button, Form, Rect};
use sheaf_doc::pdf::{Object, literal};
use sheaf_doc::realize::{EngineKey, ViewInputs};
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Budget, Job, PageIndex};

use common::buttons::{Case, PLACES, Shows, choices, normal, ours, pushes, rebound};
use common::fields::{SIDE_PX, differing_in, draw, flat, flat_at, ink, live_in, shown_text};
use common::views;

/// Page 0 of `bytes` drawn live, as the page view draws it.
fn live(bytes: &[u8]) -> Raster {
    live_in(&PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap())
}

/// A changed widget's page is realized (an engine of its group's, not its
/// base), and draws as its saved file does: appearances only, from the
/// realization's segments in an engine of their own, and live, through the
/// pool, as the screen draws it. A check box and a radio button turned on,
/// framed, upright and turned; a combo box, a list box and a push button
/// with its three appearances.
#[test]
fn a_realized_page_draws_each_changed_kind_as_its_save_does() {
    let whole = [0, 0, SIDE_PX as usize, SIDE_PX as usize];
    let framed = |name: &str, radio| Case {
        bs: Some((2, "B")),
        bg: Some(vec![1.0, 1.0, 0.6]),
        bc: Some(vec![0.0, 0.0, 1.0]),
        ..Case::toggle(name, radio, None, true)
    };
    let mut cases = Vec::new();
    for place in PLACES {
        cases.push(framed("check box on", false).at(place));
        cases.push(framed("radio on", true).at(place));
    }
    let picked = ["combo label", "list several", "push /H /P"];
    let others = (choices().into_iter().chain(pushes()))
        .map(|(case, _, _)| case)
        .filter(|c| picked.contains(&c.name.as_str()) && c.place == PLACES[0]);
    cases.extend(others);
    for case in &cases {
        let label = case.label();
        let ours = ours(case);
        let (d, saved) = (&ours.doc, &ours.saved);
        let views = views();
        let inputs = ViewInputs::screen();
        let st = d.state();
        let page = d.page(0).unwrap();
        let view = views.view(st, page, &inputs).unwrap();
        assert!(
            matches!(view.engine, EngineKey::Group(..)),
            "{label}: {view:?}"
        );
        let r = views.realizer().open(st, view.engine, &inputs).unwrap();
        let engine = PdfiumEngine::open_segments(r.segments, r.password.as_deref()).unwrap();
        let realized = flat(&engine, PageIndex(view.index));
        let appearances = differing_in(&realized, &draw(saved), whole);
        let job = Job::Page {
            width: SIDE_PX,
            rotation: Rotation::R0,
        };
        let rx = views.render(st, page, &inputs, job, PixelFormat::Rgba, 0, Budget::Screen);
        let pooled = block_on(rx).unwrap().unwrap().raster;
        let live = differing_in(&pooled, &live(saved), whole);
        println!("{label}\tappearances {appearances} px\tlive {live} px");
        assert_eq!((appearances, live), (0, 0), "{label}");
        assert!(ink(&realized, case.pixels(), true) > 0, "{label}");
    }
}

/// A combo box's, a list box's and a push button's made forms read back:
/// the strings each shows, decoded through its fonts by sheaf's content
/// interpreter, are the label of the option a combo box's value is, or
/// its own text; a list box's options from its top, one a line; a push
/// button's normal caption, and with `/H /P` its rollover and down ones,
/// each form drawn by a copy whose `/N` is it.
#[test]
fn a_choice_s_and_a_push_button_s_text_reads_back() {
    let cases = (choices().into_iter().chain(pushes())).map(|(case, _, _)| case);
    let mut read = 0;
    for case in cases.filter(|c| c.place == PLACES[0]) {
        let label = case.label();
        let saved = ours(&case).saved;
        let mut want = vec![("N", saved.clone(), String::new())];
        match &case.shows {
            Shows::Combo { options, value, .. } => {
                let shown = (options.iter()).find(|(export, _)| export == value);
                want[0].2 = shown.map_or(*value, |(_, label)| *label).into();
            }
            Shows::List { options, top, .. } => {
                let top = top.unwrap_or(0) as usize;
                want[0].2 = options[top..].concat();
            }
            Shows::Push {
                caption,
                rollover,
                down,
                pressed,
            } => {
                want[0].2 = caption.to_string();
                if *pressed {
                    for (entry, text) in [("R", rollover), ("D", down)] {
                        let text = text.unwrap_or(*caption);
                        want.push((entry, rebound(&saved, entry), text.into()));
                    }
                }
            }
            Shows::Toggle { .. } => unreachable!("{label}"),
        }
        for (entry, bytes, text) in want {
            let made = normal(&bytes).unwrap_or_else(|| panic!("{label}: no /{entry}"));
            let shown = shown_text(&bytes, &made);
            println!("{label}\t/{entry} reads {shown:?}");
            assert_eq!(shown.as_deref(), Some(text.as_str()), "{label}: /{entry}");
            read += 1;
        }
    }
    assert_eq!(read, 9, "five choices, two buttons, one with three forms");
}

/// Three check boxes of one field 20 points a side, a row down a Letter
/// page from its top.
fn group_rects() -> [Rect; 3] {
    [0, 1, 2].map(|i| {
        let top = 700.0 - 40.0 * i as f32;
        [72.0, top - 20.0, 92.0, top]
    })
}

/// `rect` on a Letter page drawn two pixels a point.
fn letter_pixels(rect: Rect) -> [usize; 4] {
    let [x0, y0, x1, y1] = rect.map(|v| (2.0 * v).round() as usize);
    [x0, 2 * 792 - y1, x1, 2 * 792 - y0]
}

/// Page 0 of `bytes`, a Letter page, appearances only, two pixels a point.
fn drawn_letter(bytes: &[u8]) -> Raster {
    let engine = PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
    flat_at(&engine, PageIndex(0), 2 * 612).unwrap()
}

/// A check box group whose widgets have distinct on states acts as a radio
/// group (as NY IT-201's filing status does): its first widget on, its
/// third pressed through the model, saved, and drawn, the third shows on
/// and the first and second off. Where the file draws each widget's states
/// (`/a`, `/b`, `/c`, each a filled square and nothing), the page is the
/// one the file draws with `/AS` set so by hand, and not the file's own;
/// where it draws none, its states named by their places under `/Opt`,
/// the pressed widget's mark is made and the others draw nothing.
#[test]
fn a_radio_like_check_box_group_shows_the_pressed_one_on_and_the_others_off() {
    let rects = group_rects();
    let page = |states: [Option<&str>; 3], shows: [&str; 3], opt: bool| {
        let mut f = Form::new(1);
        let buttons: Vec<Button> = (0..3)
            .map(|i| Button {
                page: 0,
                rect: rects[i],
                on_state: states[i],
                shows: Some(shows[i]),
            })
            .collect();
        // A page of no content, so only the widgets draw.
        let first = f.pages[0];
        f.dict(first).remove(b"Contents");
        let (field, _) = f.buttons("filing", 0, &buttons);
        if opt {
            let opt: Vec<Object> = ["single", "joint", "separate"].map(literal).into();
            f.dict(field).set("Opt", opt);
        }
        f.bytes()
    };
    let boxes = rects.map(letter_pixels);
    let inked = |r: &Raster| boxes.map(|px| ink(r, px, true) > 0);
    for (what, states, before, after, opt) in [
        (
            "its own states",
            [Some("a"), Some("b"), Some("c")],
            ["a", "Off", "Off"],
            ["Off", "Off", "c"],
            false,
        ),
        (
            "states made",
            [None, None, None],
            ["0", "Off", "Off"],
            ["Off", "Off", "2"],
            true,
        ),
    ] {
        let file = page(states, before, opt);
        let mut doc = common::opened(file.clone(), "filing.pdf", None);
        let pressed = widgets_named(&doc, "filing")[2];
        sheaf_doc::tx::fixture::press(&mut doc, pressed);
        let saved = sheaf_doc::save::rewrite(&doc).unwrap();
        let shown: Vec<Option<String>> = common::form_in(&saved)
            .widgets
            .values()
            .flatten()
            .map(|w| w.state.clone())
            .collect();
        println!("{what}: /AS {shown:?}");
        let want: Vec<Option<String>> = after.iter().map(|s| Some(s.to_string())).collect();
        assert_eq!(shown, want, "{what}");
        let drawn = drawn_letter(&saved);
        assert_eq!(inked(&drawn), [false, false, true], "{what}");
        let whole = [0, 0, drawn.width as usize, drawn.height as usize];
        assert_eq!(ink(&drawn, whole, true), ink(&drawn, boxes[2], true));
        if !opt {
            assert_eq!(inked(&drawn_letter(&file)), [true, false, false]);
            let by_hand = drawn_letter(&page(states, after, opt));
            assert_eq!(differing_in(&drawn, &by_hand, whole), 0, "{what}");
            assert!(differing_in(&drawn, &drawn_letter(&file), whole) > 0);
        }
    }
}

/// The widgets of the field named `name` in `doc`.
fn widgets_named(doc: &sheaf_doc::Document, name: &str) -> Vec<sheaf_doc::AnnotId> {
    let st = doc.state();
    let id = st.field_named(name).expect("the field");
    let term = st
        .field(id)
        .and_then(|f| f.term.clone())
        .expect("a terminal");
    term.widgets.iter().copied().collect()
}

/// Ours against PDFium's own appearances, case by case. Linux alone: see
/// the file's header.
#[cfg(target_os = "linux")]
mod against_pdfium {
    use sheaf_render::pdfium::PdfiumEngine;

    use super::common::buttons::{
        Case, Shows, Twinned, ap_of, choices, differing_box, mark_apart, marks, ours, pushes,
        rebound, showing, theirs, toggle_frames,
    };
    use super::common::fields::{
        SIDE_PX, TOLERANCE, differing_in, draw, ink, keep_drawn_as, live_in,
    };
    use super::common::form_in;
    use sheaf_doc::forms::Value;

    /// What a count past [`TOLERANCE`] may say of the system.
    const FACES: &str = "is an Arial installed? PDFium draws Helvetica in it";

    /// Whether a count of differing pixels is past [`TOLERANCE`].
    fn past(count: usize) -> bool {
        count > TOLERANCE
    }

    /// Ours of `ours_case` against PDFium's of `theirs_case`, both drawn by
    /// `drawn` from the saved file (its normal appearance, or a copy with
    /// another slot or state): the pixels inside the box that differ.
    /// Neither side draws outside the box, and PDFium's draws inside it
    /// where the case paints.
    fn against(
        ours_case: &Case,
        theirs_case: &Case,
        drawn: impl Fn(&[u8]) -> Vec<u8>,
        what: &str,
    ) -> usize {
        let px = theirs_case.pixels();
        assert_eq!(px, ours_case.pixels(), "{}", ours_case.label());
        let [pdfium, sheaf] =
            [theirs(theirs_case), ours(ours_case).saved].map(|bytes| draw(&drawn(&bytes)));
        let label = theirs_case.label();
        keep_drawn_as(&theirs_case.file_name(), &format!("{what}.pdfium"), &pdfium);
        keep_drawn_as(&theirs_case.file_name(), what, &sheaf);
        if theirs_case.paints() {
            assert!(
                ink(&pdfium, px, true) > 0,
                "{label}: PDFium's draws nothing"
            );
        }
        for (side, r) in [("PDFium's", &pdfium), (what, &sheaf)] {
            assert_eq!(
                ink(r, px, false),
                0,
                "{label}: {side} draws outside the box"
            );
        }
        differing_in(&sheaf, &pdfium, px)
    }

    /// The file as it is.
    fn as_is(bytes: &[u8]) -> Vec<u8> {
        bytes.to_vec()
    }

    /// A case's counts: the pixels its box differs by between ours and
    /// PDFium's, and between its twin (ours with one thing other) and
    /// PDFium's.
    struct Counted {
        label: String,
        count: usize,
        twin: usize,
        wrong: &'static str,
    }

    /// Each row printed, then each held: ours within [`TOLERANCE`] of
    /// PDFium's, its twin past it.
    fn held(rows: &[Counted]) {
        for r in rows {
            let (label, count, wrong, twin) = (&r.label, r.count, r.wrong, r.twin);
            println!("{label}\tours {count} px\ttwin ({wrong}) {twin} px");
        }
        for r in rows {
            assert!(!past(r.count), "{}: {} px ({FACES})", r.label, r.count);
            assert!(past(r.twin), "{}: its twin, {} px", r.label, r.twin);
        }
    }

    /// Each case of `cases` against PDFium's, and its twin, drawn by
    /// `drawn`, named `what`.
    fn group(cases: &[Twinned], drawn: impl Fn(&[u8]) -> Vec<u8> + Copy, what: &str) {
        let rows: Vec<Counted> = (cases.iter())
            .map(|(case, twin, wrong)| Counted {
                label: format!("{} {what}", case.label()),
                count: against(case, case, drawn, what),
                twin: against(twin, case, drawn, &format!("{what} twin")),
                wrong,
            })
            .collect();
        held(&rows);
    }

    /// How PDFium's own appearances are had, for a button as for a text
    /// field: the file has none, PDFium's copy saved before the page's
    /// widgets are drawn has none either, and the copy saved after has
    /// every slot (a toggle's `/N` and `/D` state dictionaries, a push
    /// button's `/N`, `/R` and `/D`), which its load under the form-fill
    /// environment made. That copy, flattened and drawn appearances only, is
    /// pixel for pixel the page view's live drawing of the file. And ours
    /// of a check box turned on, its `/AS` and its field's `/V` the model's
    /// (M10 slices 8 and 9), draws live as it draws appearances only, its
    /// mark shown.
    #[test]
    fn pdfium_makes_every_appearance_a_widget_lacks_and_draws_by_its_state() {
        let (frame, _, _) = toggle_frames().remove(2);
        let (mark, _, _) = marks().remove(0);
        let (combo, _, _) = choices().remove(0);
        let (list, _, _) = choices().remove(3);
        let (push, _, _) = pushes().remove(1);
        let whole = [0, 0, SIDE_PX as usize, SIDE_PX as usize];
        for (case, slots) in [
            (&frame, &["N", "D"][..]),
            (&mark, &["N", "D"][..]),
            (&combo, &["N"][..]),
            (&list, &["N"][..]),
            (&push, &["N", "R", "D"][..]),
        ] {
            let label = case.label();
            let file = case.file(true);
            assert!(ap_of(&file).is_none(), "{label}");
            let engine = PdfiumEngine::open_bytes(file.into(), None).unwrap();
            let before = ap_of(&engine.save_copy().unwrap());
            assert!(before.is_none(), "{label}: made before drawing");
            let live = live_in(&engine);
            let copy = engine.save_copy().unwrap();
            let ap = ap_of(&copy).unwrap_or_else(|| panic!("{label}: none made"));
            let keys: Vec<String> = (ap.iter())
                .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
                .collect();
            for slot in slots {
                assert!(keys.iter().any(|k| k == slot), "{label}: {keys:?}");
            }
            let drawn = draw(&copy);
            if case.paints() {
                assert!(ink(&drawn, case.pixels(), true) > 0, "{label}");
            }
            let count = differing_in(&live, &drawn, whole);
            println!("{label}\t/AP {keys:?}\tlive against the copy {count} px");
            assert_eq!(count, 0, "{label}");
        }
        let saved = ours(&mark).saved;
        let form = form_in(&saved);
        let value = (form.fields.iter()).find_map(|f| f.value.clone());
        let shown: Vec<_> = form.widgets.values().flatten().map(|w| &w.state).collect();
        assert_eq!(value, Some(Value::State("Yes".into())), "/V the model's");
        assert_eq!(shown, [&Some("Yes".to_string())], "/AS the model's");
        let live = live_in(&PdfiumEngine::open_bytes(saved.clone().into(), None).unwrap());
        let count = differing_in(&live, &draw(&saved), whole);
        println!("{}: ours live against ours drawn {count} px", mark.label());
        assert_eq!(count, 0, "a live drawing of ours follows /AS");
        assert!(ink(&live, mark.pixels(), true) > 0);
    }

    /// A check box's and a radio button's `Off` in each border style and
    /// colour space, the radio's frame round, and with no frame, which
    /// paints nothing, at each place: the twin on.
    #[test]
    fn each_toggle_s_frame_is_pdfium_s() {
        group(&toggle_frames(), as_is, "normal");
    }

    /// The same frames down (`/D`), as a press draws them: the background
    /// less a quarter (gray where there's none), a beveled border's shades
    /// exchanged, an inset's black and white: the twin on.
    #[test]
    fn each_toggle_s_frame_down_is_pdfium_s() {
        group(&toggle_frames(), |b| rebound(b, "D"), "down");
    }

    /// A combo box's label and own text with its drop button, a list box's
    /// selection, several, and from `/TI`, each with its highlight, at each
    /// place: the twin's value another.
    #[test]
    fn each_choice_is_pdfium_s() {
        group(&choices(), as_is, "normal");
    }

    /// A push button's caption, at each place: the twin's one glyph other.
    /// With `/H /P`, its rollover and down appearances too, each drawn by a
    /// copy whose `/N` is it: the twin's normal caption other, and its
    /// rollover and down ones, each one glyph other.
    #[test]
    fn each_push_button_s_appearances_are_pdfium_s() {
        let cases = pushes();
        group(&cases, as_is, "normal");
        let pressed: Vec<Twinned> = (cases.into_iter())
            .filter(|(case, _, _)| matches!(case.shows, Shows::Push { pressed: true, .. }))
            .map(|(case, mut twin, _)| {
                if let Shows::Push { rollover, down, .. } = &mut twin.shows {
                    for text in [rollover, down] {
                        let other = super::common::fields::one_other(text.unwrap());
                        *text = Some(Box::leak(other.into_boxed_str()));
                    }
                }
                (case, twin, "captions other")
            })
            .collect();
        assert_eq!(pressed.len(), 2);
        group(&pressed, |b| rebound(b, "R"), "rollover");
        group(&pressed, |b| rebound(b, "D"), "down");
    }

    /// Where a mark's ink reaches in `bytes`, drawn by `drawn`: the box of
    /// the pixels inside the case's box its on state draws otherwise than
    /// its `Off`, each a copy whose `/AS` names it.
    fn mark_box(case: &Case, bytes: &[u8], drawn: impl Fn(&[u8]) -> Vec<u8>) -> [usize; 4] {
        let px = case.pixels();
        let [on, off] = ["Yes", "Off"].map(|state| draw(&drawn(&showing(bytes, state))));
        for r in [&on, &off] {
            assert_eq!(ink(r, px, false), 0, "{}: outside the box", case.label());
        }
        differing_box(&on, &off, px).unwrap_or_else(|| panic!("{}: no mark", case.label()))
    }

    /// Each mark of [`marks`] against PDFium's, drawn by `drawn`: within the
    /// tolerance, and its twin's mark past it.
    fn marks_held(drawn: impl Fn(&[u8]) -> Vec<u8> + Copy, what: &str) {
        let mut failed = Vec::new();
        for (case, twin, _) in marks() {
            let label = format!("{} {what}", case.label());
            let pdfium = mark_box(&case, &theirs(&case), drawn);
            let sheaf = mark_box(&case, &ours(&case).saved, drawn);
            let other = mark_box(&twin, &ours(&twin).saved, drawn);
            let [held, twin_held] = [sheaf, other].map(|b| {
                let apart = mark_apart(&case, b, pdfium, 0.0);
                (apart.iter().all(|(d, allowed)| d <= allowed), apart)
            });
            println!(
                "{label}\tPDFium {pdfium:?} ours {sheaf:?} twin {other:?}\tours apart {:?}\t\
                 twin apart {:?}",
                held.1, twin_held.1
            );
            if !held.0 {
                failed.push(format!("{label}: ours {:?}", held.1));
            }
            if twin_held.0 {
                failed.push(format!("{label}: its twin within, {:?}", twin_held.1));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
    }

    /// A check box's and a radio button's mark, on, normal: where its ink
    /// reaches is PDFium's, within the tolerance, and its twin's, another
    /// mark, past it.
    #[test]
    fn each_mark_reaches_where_pdfium_s_does() {
        marks_held(<[u8]>::to_vec, "normal");
    }

    /// The same marks down (`/D`), on the pressed frame.
    #[test]
    fn each_mark_down_reaches_where_pdfium_s_does() {
        marks_held(|b| rebound(b, "D"), "down");
    }
}
