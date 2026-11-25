//! A text field's appearance as sheaf makes it (pdf-forms' generator, M10
//! slice 6) against the one PDFium makes for the same value, by pixels.
//! Each case is a form built in code ([`common::fields`]): ours is the
//! field filled through the model and saved, PDFium's the field given its
//! `/V` and no appearance, drawn under PDFium's form-fill environment and
//! saved by PDFium; both are drawn appearances only at two pixels a point
//! and compared inside the widget's box, within a tolerance
//! ([`against_pdfium`]).
//!
//! The cases: left, centered and right; a comb, right-aligned and with
//! its dividers; multiline with a wrap and a hard break; an automatic
//! size, single line and multiline; each upright and on a `/Rotate 90`
//! page, with `/MK /R 90` and without. Each border style, backgrounds in
//! gray, RGB and CMYK, and the IRS navy. Every case has a twin, ours with
//! one thing wrong (its value, alignment, comb or size), which must be
//! past the tolerance, so that a case can fail. pdf-forms' stated
//! differences from PDFium (a missing font falls back by its name,
//! `/MaxLen` cuts only a comb's text, a dash array is drawn as given) are
//! each held as differences: past the tolerance against PDFium's drawing
//! of the same file, and within it against PDFium's drawing of the file
//! that says what PDFium does. These run on Linux alone: on Windows and
//! macOS PDFium draws the standard 14 in the system's faces (Arial,
//! Helvetica), whose heights set its own layout otherwise than the faces
//! it's built with, which pdf-forms was held to.
//!
//! On every system, a realized page draws a changed field as its saved
//! file does, appearances only and live; "é € — “ ”" in the state forms'
//! `/Helv` and `/Arial` reads back, by sheaf's interpreter over the made
//! form and by PDFium's text of the page it flattened (the `/Arial`'s "€"
//! as PDFium reads it on each system: [`pdfium_reads`]); and a name in the
//! field's font and the fallback font (M10 slice 7), in an IRS form of the
//! corpus, draws inside its box and reads back, composed.
//!
//! PDFium is sheaf's linked build (`scripts/fetch-pdfium.sh`). Needs
//! libpdfium, like the app itself.

mod common;

use futures::executor::block_on;
use sheaf_doc::forms::fixture::{STATE_TEXT, nj_helv, pa_arial};
use sheaf_doc::pdf::{Document as Pdf, Object};
use sheaf_doc::realize::{EngineKey, ViewInputs};
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::{PdfiumEngine, appearance_text};
use sheaf_render::{Budget, Job, PageIndex};

use common::fields::{
    Case, SIDE_PX, alignments, appearance, borders, combs, differing_in, draw, flat, ink, live_in,
    multiline, ours, shown_text,
};
use common::views;

/// Page 0 of `bytes` drawn live, as the page view draws it.
fn live(bytes: &[u8]) -> Raster {
    live_in(&PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap())
}

/// A changed field's page is realized (an engine of its group's, not its
/// base), and draws as its saved file does: appearances only, from the
/// realization's segments in an engine of their own, and live, through
/// the pool, as the screen draws it.
#[test]
fn a_realized_page_draws_a_changed_field_as_its_save_does() {
    let whole = [0, 0, SIDE_PX as usize, SIDE_PX as usize];
    let every: Vec<Case> = [alignments(), combs(), multiline(), borders()]
        .into_iter()
        .flatten()
        .map(|(case, _, _)| case)
        .collect();
    let picked = [
        "left (upright)",
        "comb divided (/Rotate 90 /R 90)",
        "multiline (/Rotate 90)",
        "beveled on RGB (upright)",
    ];
    let cases = picked.map(|label| (every.iter()).find(|c| c.label() == label).unwrap());
    for case in cases {
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

/// "é € — “ ”" in the NJ-style `/Helv` and the PA-40-style `/Arial`, each
/// `/DR`'s own font, reads back: the made form's strings decoded through
/// its font by sheaf's content interpreter, and PDFium's text of the page
/// it flattened, as a viewer of appearances draws it. The form draws in
/// `/DR`'s font itself, not a fallback.
#[test]
fn the_state_forms_text_reads_back() {
    for (name, font) in [("Helv", nj_helv()), ("Arial", pa_arial())] {
        let case = Case {
            da: format!("/{name} 10 Tf 0 g"),
            fonts: vec![(name, font)],
            ..Case::new(&format!("state {name}"), STATE_TEXT)
        };
        let saved = ours(&case).saved;
        let made = appearance(&saved).unwrap();
        assert_eq!(
            shown_text(&saved, &made).as_deref(),
            Some(STATE_TEXT),
            "{name}"
        );
        let pdf = Pdf::load_mem(&saved).unwrap();
        let resources = made.dict.get(b"Resources").unwrap();
        let fonts: Vec<_> = (resources.as_dict().and_then(|r| r.get(b"Font")))
            .and_then(Object::as_dict)
            .unwrap()
            .iter()
            .map(|(_, f)| f.as_reference())
            .collect();
        let dr = (pdf.catalog().unwrap().get(b"AcroForm"))
            .and_then(Object::as_reference)
            .and_then(|form| pdf.get_dictionary(form).ok())
            .and_then(|form| form.get(b"DR"))
            .and_then(Object::as_dict)
            .and_then(|dr| dr.get(b"Font"))
            .and_then(Object::as_dict)
            .and_then(|fonts| fonts.get(name.as_bytes()))
            .and_then(Object::as_reference);
        assert_eq!(fonts, [dr], "{name}: the form's font is /DR's");
        let text = appearance_text(&saved, None, PageIndex(0)).unwrap();
        println!("{name}\tPDFium reads {text:?}");
        let reads = pdfium_reads(name);
        assert!(
            text.contains(reads),
            "{name}: {text:?}, not {reads:?} (is an Arial installed?)"
        );
    }
}

/// What PDFium's text of the flattened page reads for [`STATE_TEXT`] in the
/// state form's font `name`. The `/Arial` isn't embedded, and PDFium's
/// reading of it turns on the system's fonts: on Windows and macOS, which
/// have an Arial, it reads code 219 by MacRoman's own table, `¤`, where the
/// font's `/Differences` names it `/Euro` (CI's windows-latest and
/// macos-latest alike); on Linux, with none, it reads the `/Differences`.
/// sheaf's interpreter reads the `/Differences` on every system.
fn pdfium_reads(name: &str) -> &'static str {
    if name == "Arial" && cfg!(any(target_os = "windows", target_os = "macos")) {
        "é ¤ — “ ”"
    } else {
        STATE_TEXT
    }
}

/// The corpus's f1040, its first name field (`f1_14`, the left of the
/// name row, at `[36 684 251.25 698]`), and that box on page 0 drawn two
/// pixels a point: left, top, right, bottom.
const F1040: &str = "forms/irs/f1040.pdf";
const FIRST_NAME: &str = "topmostSubform[0].Page1[0].f1_14[0]";
const FIRST_NAME_PX: [usize; 4] = [72, 188, 503, 216];

/// "Łukasz Nguyễn Ölçer" filled into the f1040's first name field, whose
/// font has no `Ł` or `ễ`, is drawn in it and the fallback font sheaf
/// bundles (M10 slice 7): saved, the subset's every code is a glyph of
/// its cut program with a text, the page drawn appearances only by PDFium
/// changes inside the field's box alone, with ink there, and PDFium's text
/// of the page gives the name back. Given decomposed (NFD), it's drawn
/// composed: the same appearance, and PDFium reads it back composed (NFC).
/// Where the corpus is.
#[test]
fn a_name_in_two_fonts_reads_back_from_an_irs_form() {
    use sheaf_doc::content::fixture::{FALLBACK_TEXT, fallback_subsets, subsets_hold};
    let Some(bytes) = common::corpus_form(F1040) else {
        return;
    };
    let width = 2 * 612;
    let drawn = |bytes: &[u8]| {
        let engine = PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
        common::fields::flat_at(&engine, PageIndex(0), width).unwrap()
    };
    let before = drawn(&bytes);
    // Its NFD: `ễ` as `e` and two marks, `Ö` and `ç` as a letter and one
    // (`Ł` has no decomposition).
    let decomposed = "\u{141}ukasz Nguye\u{302}\u{303}n O\u{308}lc\u{327}er".to_string();
    assert_ne!(decomposed, FALLBACK_TEXT);
    let mut made = Vec::new();
    for value in [FALLBACK_TEXT, &decomposed] {
        let mut doc = common::opened(bytes.clone(), "f1040.pdf", None);
        sheaf_doc::tx::fixture::fill(&mut doc, FIRST_NAME, value);
        let saved = sheaf_doc::save::rewrite(&doc).unwrap();
        assert_eq!(fallback_subsets(&saved).len(), 1, "one subset");
        assert_eq!(subsets_hold(&saved), 2, "Ł and ễ");
        let after = drawn(&saved);
        let whole = [0, 0, after.width as usize, after.height as usize];
        let changed = differing_in(&after, &before, whole);
        let inside = differing_in(&after, &before, FIRST_NAME_PX);
        println!("{value:?}\t{changed} px changed, {inside} inside its box");
        assert!(inside > 300, "{inside} px drawn in the box");
        assert_eq!(changed, inside, "nothing drawn outside the field's box");
        let text = appearance_text(&saved, None, PageIndex(0)).unwrap();
        assert!(text.contains(FALLBACK_TEXT), "PDFium reads {text:?}");
        assert!(!text.contains(&decomposed));
        made.push(after);
    }
    assert!(made[0].data == made[1].data, "NFD drawn as NFC");
}

/// Ours against PDFium's own appearance, case by case. Linux alone: see
/// the file's header.
#[cfg(target_os = "linux")]
mod against_pdfium {
    use sheaf_render::pdfium::PdfiumEngine;

    use super::common::fields::{
        COMB, Case, MULTILINE, Place, SIDE_PX, Stated, Twinned, alignments, appearance, auto_sizes,
        borders, combs, dash_array, differing_in, draw, ink, keep, keep_drawn, keep_saved, live_in,
        max_len, missing_fonts, multiline, navy, ours, theirs,
    };

    /// The pixels inside a widget's box ours may differ from PDFium's by.
    /// Measured: none, in every case. pdf-forms writes PDFium's numbers
    /// (its layout was held to PDFium's own appearances), so the two draw
    /// the same glyphs at the same places, and anti-aliasing at a glyph's
    /// edge has nothing to blend otherwise; the smallest twin, one glyph
    /// of 10-point text other, is past it by more than a hundred. That
    /// holds where PDFium draws the standard 14 in the faces it's built
    /// with: its font info maps Helvetica to an installed Arial, on Linux
    /// too, whose heights would move PDFium's layout.
    const TOLERANCE: usize = 0;

    /// What a count past [`TOLERANCE`] may say of the system.
    const FACES: &str = "is an Arial installed? PDFium draws Helvetica in it";

    /// Whether a count of differing pixels is past [`TOLERANCE`].
    fn past(count: usize) -> bool {
        count > TOLERANCE
    }

    /// Ours of `ours_case` against PDFium's of `theirs_case`: the pixels
    /// inside the box that differ. Neither side draws outside the box, and
    /// PDFium's draws inside it. Under `SHEAF_FIELDS_OUT`, each file and
    /// picture is kept, named for `theirs_case` and `what`.
    fn against(ours_case: &Case, theirs_case: &Case, what: &str) -> usize {
        let px = theirs_case.pixels();
        assert_eq!(px, ours_case.pixels(), "{}", ours_case.label());
        let copy = theirs(theirs_case);
        let saved = ours(ours_case).saved;
        keep(theirs_case, "file.pdf", &theirs_case.file(true));
        keep_saved(theirs_case, "pdfium", &copy);
        keep_saved(theirs_case, what, &saved);
        let [pdfium, drawn] = [&copy, &saved].map(|b| draw(b));
        keep_drawn(theirs_case, "pdfium", &pdfium);
        keep_drawn(theirs_case, what, &drawn);
        let label = theirs_case.label();
        assert!(
            ink(&pdfium, px, true) > 0,
            "{label}: PDFium's draws nothing"
        );
        for (side, r) in [("PDFium's", &pdfium), (what, &drawn)] {
            assert_eq!(
                ink(r, px, false),
                0,
                "{label}: {side} draws outside the box"
            );
        }
        differing_in(&drawn, &pdfium, px)
    }

    /// A case's counts: the pixels its box differs by between ours and
    /// PDFium's, and between its twin (ours with one thing wrong) and
    /// PDFium's.
    struct Counted {
        label: String,
        count: usize,
        twin: usize,
        wrong: &'static str,
    }

    /// `case` by sheaf and by PDFium, and `twin`, `case` with one thing
    /// (`wrong`) other, by sheaf, each against PDFium's `case`.
    fn compared(case: &Case, twin: &Case, wrong: &'static str) -> Counted {
        Counted {
            label: case.label(),
            count: against(case, case, "sheaf"),
            twin: against(twin, case, "twin"),
            wrong,
        }
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

    /// A stated difference held: ours of its case is past [`TOLERANCE`] of
    /// PDFium's drawing of the same file, and what says the difference is
    /// within it.
    fn stated(s: &Stated) {
        let differ = against(&s.case, &s.case, "sheaf");
        let same = against(&s.ours_of, &s.theirs_of, "stated");
        let (label, what) = (s.case.label(), &s.what);
        println!("{label}\tagainst PDFium's {differ} px\t{what} {same} px");
        assert!(past(differ), "{label}: drawn as PDFium does ({differ} px)");
        assert!(!past(same), "{label}: {what}, {same} px ({FACES})");
    }

    /// How PDFium's own appearance is had. The file has none; PDFium's
    /// copy saved before the page's widgets are drawn has none either;
    /// drawing them (`FPDF_FFLDraw`) makes it, and the copy saved after
    /// holds it. That copy, flattened and drawn appearances only, is pixel
    /// for pixel the page view's live drawing of the file (`FFLDraw` of the
    /// appearance it made), so the checks compare ours with the copy,
    /// through the one path both are drawn by.
    #[test]
    fn pdfium_s_appearance_is_the_one_its_widgets_drew() {
        let cases = [
            Case::new("left", "Hello world"),
            Case {
                flags: COMB,
                max_len: Some(10),
                bs: Some((1, "S", Vec::new())),
                bc: Some(vec![0.0, 0.0, 1.0]),
                ..Case::new("comb", "1234567")
            }
            .at(Place::TurnedR90),
            Case {
                flags: MULTILINE,
                size: [200.0, 80.0],
                da: "/Helv 0 Tf 0 g".into(),
                ..Case::new(
                    "auto multiline",
                    "Words enough to wrap in a box of this size",
                )
            }
            .at(Place::Turned),
        ];
        for case in cases {
            let file = case.file(true);
            assert!(appearance(&file).is_none());
            let engine = PdfiumEngine::open_bytes(file.into(), None).unwrap();
            let before = appearance(&engine.save_copy().unwrap());
            assert!(before.is_none(), "{}: made before drawing", case.label());
            let live = live_in(&engine);
            let copy = engine.save_copy().unwrap();
            assert!(appearance(&copy).is_some(), "{}: none made", case.label());
            let drawn = draw(&copy);
            assert!(ink(&drawn, case.pixels(), true) > 0, "{}", case.label());
            let count = differing_in(&live, &drawn, [0, 0, SIDE_PX as usize, SIDE_PX as usize]);
            println!("{}\tlive against the copy {count} px", case.label());
            assert_eq!(count, 0, "{}", case.label());
        }
    }

    /// Each case of a group against PDFium's, and its twin.
    fn group(cases: &[Twinned]) {
        let rows: Vec<Counted> = (cases.iter())
            .map(|(case, twin, wrong)| compared(case, twin, wrong))
            .collect();
        held(&rows);
    }

    /// Left, centered and right, at each place: the twin aligned
    /// otherwise.
    #[test]
    fn each_alignment_is_pdfium_s() {
        group(&alignments());
    }

    /// A comb, left, right-aligned, and with a border that draws its
    /// dividers, at each place: the twin no comb, or aligned left.
    #[test]
    fn a_comb_is_pdfium_s() {
        group(&combs());
    }

    /// A multiline field with a line that wraps and a hard break, at each
    /// place: the twin's value one character other.
    #[test]
    fn multiline_is_pdfium_s() {
        group(&multiline());
    }

    /// An automatic size, single line and multiline, at each place: the
    /// twin at a size of its own.
    #[test]
    fn an_automatic_size_is_pdfium_s() {
        group(&auto_sizes());
    }

    /// Each border style, and backgrounds in gray, RGB and CMYK: the
    /// twin's value one character other.
    #[test]
    fn each_border_and_background_is_pdfium_s() {
        group(&borders());
    }

    /// The IRS's navy, `0 0 0.502 rg`, is the text's color: in the made
    /// form's content, and in its pixels, the ones the text covers whole
    /// `0 0 128`, and none of the others but navy's blend with white. The
    /// twin is black.
    #[test]
    fn the_irs_navy_is_pdfium_s() {
        let (case, twin, wrong) = navy();
        held(&[compared(&case, &twin, wrong)]);
        let saved = ours(&case).saved;
        let made = appearance(&saved).unwrap();
        let content = String::from_utf8_lossy(&made.content);
        assert!(content.contains("\n0 0 0.502 rg\n"), "{content}");
        let drawn = draw(&saved);
        let [l, t, r, b] = case.pixels();
        let inked: Vec<&[u8]> = (t..b)
            .flat_map(|y| (l..r).map(move |x| (x, y)))
            .map(|(x, y)| &drawn.data[(y * drawn.width as usize + x) * 4..][..3])
            .filter(|p| *p != [255, 255, 255])
            .collect();
        assert!(inked.contains(&&[0, 0, 128][..]), "no pixel is navy whole");
        let blends = |p: &&[u8]| p[0] == p[1] && p[2] >= p[0];
        assert!(inked.iter().all(blends), "a pixel that isn't navy's");
    }

    /// A `/DA` font `/DR` lacks falls back to the standard 14 family its
    /// name gives (pdf-forms'): drawn so, it differs from PDFium's, which
    /// draws every such font in Helvetica, and is PDFium's drawing of the
    /// same file with that face in `/DR`.
    #[test]
    fn a_missing_font_falls_back_by_its_name_where_pdfium_draws_helvetica() {
        missing_fonts().iter().for_each(stated);
    }

    /// `/MaxLen` cuts only a comb's text (pdf-forms': it draws the display
    /// text it's given): a field that isn't a comb, past it, differs from
    /// PDFium's, which cuts every field's text, and is PDFium's drawing of
    /// the same field with no `/MaxLen`.
    #[test]
    fn max_len_cuts_only_a_comb_s_text_where_pdfium_cuts_every_field_s() {
        stated(&max_len());
    }

    /// A dashed border is drawn with its `/BS /D` (pdf-forms', as ISO
    /// 32000-2 has it): `[6 2]` differs from PDFium's, which draws every
    /// dashed border `[3 3]`; and ours of `[3 3]` is PDFium's of `[6 2]`.
    #[test]
    fn a_dash_array_is_drawn_as_given_where_pdfium_draws_3_3() {
        stated(&dash_array());
    }
}
