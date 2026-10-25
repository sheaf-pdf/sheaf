//! The content interpreter against PDFium (M11 slice 3): each glyph the
//! interpreter places on pages built in code (`content::fixture`), against
//! the character PDFium's text page makes of it, and each image against
//! PDFium's image object; and the same over the corpus's real files.
//!
//! PDFium bounds a character loosely (`cpdf_textpage.cpp`'s
//! `GetLooseBounds`) by its metric box, the glyph's width from the font's
//! descent to its ascent through the character's matrix, united with its
//! outline's box (its tight box, `FPDFText_GetCharBox`), which only the
//! font's program says. The interpreter unites them where it reads the
//! program, so each glyph's origin is its character's
//! (`FPDFText_GetCharOrigin`), within [`EPSILON`], and:
//! - a glyph whose outline its font's program says, nothing it's placed
//!   by guessed ([`exact`]), has its box its character's loose box, each
//!   side within [`EPSILON`];
//! - any other's box is the metric box (a Type3 glyph's `d1` box isn't
//!   united), which lies within the character's loose box, give or take
//!   [`EPSILON`]; and where the character's tight box lies within it,
//!   give or take [`EPSILON`], the two are equal within it, the outline
//!   having added nothing.
//!
//! A font whose heights PDFium takes from the face it draws in, as for
//! the standard 14 without a descriptor, has them guessed
//! ([`FontApprox::heights`]): PDFium's face is its own where the system
//! hasn't the font, else the system's (Arial on Windows, Helvetica on
//! macOS), each taller than Adobe's metrics, the guess. Its glyphs are
//! held as others whose outline isn't known, but to equality along the
//! line only.
//!
//! In vertical writing PDFium bounds a character by the em-wide column its
//! advance spans, centred on the pen, an em of the font's size in
//! PageSpace whatever the text space's scale, united with its outline's
//! box; the interpreter's quad is that column, so united, through the
//! text space, so where that isn't scaled, as here, the same hold. Each
//! glyph has its character, and each character its glyph: none is left
//! over on either side.
//!
//! A `TJ` is laid out as PDFium lays one out, a text object, its glyphs
//! where PDFium puts them however far along the line; and each standard 14
//! glyph is as wide as PDFium's own face draws it, but those whose width
//! is guessed (the last ignored, as only where PDFium draws in its own
//! faces).
//!
//! An image is the unit square through the CTM, PDFium's image object's
//! rotated bounds (`FPDFPageObj_GetRotatedBounds`), each corner within
//! [`EPSILON`]; PDFium places an image in a form in the form's space, so
//! its corners are taken through each form's matrix.
//!
//! Content written again from what it draws (`interp::encode`), its text
//! moved to each line by a `Tm` the interpreter's state says, draws as it
//! did: each fixture's page and forms, so written into an update after
//! it, render the same, pixel for pixel, 1,024 pixels wide.
//!
//! The pages draw in fonts PDFium lays out the same on every OS: Type3,
//! fonts over an embedded program, and the standard 14 given `/Widths`
//! ([`fixture::standard_14`]), since without them PDFium takes their
//! widths, as their heights, from the face it draws in, the system's
//! where the system has the font. PDFium's stock Helvetica can't be given
//! `/Widths`, so its glyphs, each first on its line, are held to their
//! origins only. It binds PDFium itself, behind one lock, as it reads the
//! text page and the page objects, which the engine doesn't show
//! (`scripts/fetch-pdfium.sh`).
//!
//! # Over the corpus
//!
//! Three tiers read every form of the corpus, the pool's sample
//! (`bench/pool-sample.txt`), every scan (`scans/`, other tools'
//! recognized text among them) and the conformance suites' files that
//! claim PDF/UA (`conformance/`, tagged content), each file on a thread,
//! PDFium's calls behind its lock, a panic caught as the file's and
//! counted:
//!
//! - **Interpreted:** every page, its forms drawn, each glyph laid out,
//!   within [`Limits::DEFAULT`] and with no panic. Pages past a cap, and
//!   those whose content, or a form's, can't be lexed or drawn, are
//!   listed, and none is, nor is a glyph whose box isn't finite; those
//!   whose content can't be read are counted; forms refused are counted
//!   by why; and the most a page came to of each measure a cap bounds is
//!   printed, where it is, beside the cap.
//! - **Glyph boxes:** each glyph matched to a character PDFium makes (none
//!   it generates), by origin, code point, size and box, in passes that
//!   take ever more ([`Boxes::compare`]), and held to the rule above, by
//!   [`Class`], within the class's tolerance ([`Class::tolerance`], a
//!   thousandth of a point for each): the distances' 50th and 99th
//!   percentiles and most are printed by class, and a glyph past them
//!   fails. In text turned but by quarter turns, the interpreter unites
//!   the two boxes in text space and PDFium their bounds in PageSpace, so
//!   a glyph held on every side has its box hold its character's loose
//!   box and lie within the bounds, through the text space, of that box's
//!   bounds in it. In vertical writing the column is the one the font's
//!   metrics give PDFium's way, and where the text space is scaled, which
//!   the quad's column is but PDFium's isn't, a glyph is held as one
//!   whose outline isn't known. One file's glyphs, whose program PDFium
//!   can't load and sheaf reads ([`PROGRAM_DECODED_OTHERWISE`]), may be
//!   past their tolerance, as many as it says. A glyph PDFium bounds by a
//!   box with no area, or
//!   by its outline alone, its font's ascent being its descent, is held
//!   to its origin; one past its tolerance where another glyph at its
//!   origin could be its character as well, drawn over it in another font
//!   or read otherwise by PDFium, is counted as matched ambiguously, and
//!   listed. Glyphs PDFium makes no character of (under an `/ActualText`,
//!   which it reads in their place, a duplicate, a space its line passes
//!   over), and characters no glyph is (an `/ActualText`'s, where a glyph
//!   under it is), are counted by why; those it can't say why for are
//!   listed, and held to each file's count in [`UNEXPLAINED`], so a glyph
//!   the interpreter misses, or puts far from its character, fails. A
//!   page not compared fails too. Glyphs whose codes or widths are
//!   guessed, or moved by a width guessed before them on their line, are
//!   counted, not held to anything, their fonts listed; and glyphs and
//!   characters [`FAR`] off the page are counted, not compared.
//! - **Written again:** each file's pages and the forms they draw written
//!   again ([`written_again`]), into an update after it, encrypted as the
//!   file is, render as the file's at 1,024 px, its annotations' and its
//!   form's appearances drawn: pixel for pixel, or within
//!   [`ANTI_ALIASING_PX`] pixels; or, where it differs past that, as
//!   PDFium draws the file's own page when it loads the file again, its
//!   caches then warm for both, pixel for pixel or within the allowance
//!   (PDFium draws some pages otherwise the second time a process loads
//!   them). Each page that differs at all is printed, with both counts.
//!   A page not written again fails, saying why; a file PDFium refuses, or
//!   sheaf doesn't open, or that no update can go after, is counted and
//!   printed, never left out unsaid.
//!
//! Ignored by default: in release, the first takes ten seconds; the second
//! about four minutes, a text page a page through one lock; the third
//! about fifty minutes, every page rendered twice through it. The
//! orchestrator runs them in full before a merge (`just premerge`), or
//! alone:
//!
//! ```sh
//! just tier-interp
//! ```
//!
//! They skip when there's no corpus (see `sheaf_doc::forms::corpus`). To
//! look at a few files by hand, name them in [`ONLY`]; the tiers' own runs
//! leave it unset. The standard 14 widths test, ignored too, isn't a tier:
//! it holds only where PDFium draws in its own faces, on a system with no
//! font named Helvetica, Arial or Times New Roman, so it's run by hand
//! there (`cargo test -p sheaf-render --test content_interp -- --ignored
//! --exact standard_14_widths_are_pdfium_s_faces_but_where_guessed`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, btree_map};
use std::ops::ControlFlow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use lopdf::{Dictionary, Object, ObjectId, Stream, dictionary};
use pdfium_render::prelude::*;
use sheaf_doc::content::fixture::{self, Fixture};
use sheaf_doc::content::interp::{
    EncodeError, Glyph, InterpError, Kind, Opts, Refused, encode, interpret,
};
use sheaf_doc::content::{FontApprox, FontKind, FontProgram, Fonts, Foreign, PageContent, Std14};
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::geom::{Affine, Rect};
use sheaf_doc::materialize::ObjKey;
use sheaf_doc::serialize::append::{self, Base};
use sheaf_doc::source::{Limits, PageFacts};
use sheaf_doc::store::{BlobKind, Src, Store};

/// How far, in points, a glyph's origin and box may be from PDFium's: a
/// thousandth of a point, where both compute in `f32`.
const EPSILON: f32 = 1e-3;

fn pdfium() -> MutexGuard<'static, Pdfium> {
    static PDFIUM: OnceLock<Mutex<Pdfium>> = OnceLock::new();
    let bound = PDFIUM.get_or_init(|| {
        let dir = std::env::var_os("PDFIUM_LIB_DIR").map_or_else(
            || {
                Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                    "../../vendor/pdfium/{}-{}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
            },
            PathBuf::from,
        );
        Mutex::new(Pdfium::new(
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir))
                .expect("libpdfium (scripts/fetch-pdfium.sh)"),
        ))
    });
    bound.lock().unwrap_or_else(|e| e.into_inner())
}

/// A glyph the interpreter placed.
#[derive(Debug, Clone)]
struct Placed {
    code: u32,
    origin: (f32, f32),
    bounds: Rect,
    /// What of its font's metrics is guessed.
    approx: FontApprox,
    /// It's in PDFium's stock Helvetica, as wide as the face PDFium draws
    /// it in.
    stock: bool,
    /// Its box is held to PDFium's loose box on every side ([`exact`]).
    exact: bool,
}

/// A character PDFium's text page made.
#[derive(Debug, Clone)]
struct Char {
    /// Its code point, as `FPDFText_GetUnicode` gives it.
    unicode: u32,
    /// Its font's size, `Tf`'s.
    size: f32,
    origin: (f32, f32),
    loose: Rect,
    /// None where PDFium gives none.
    tight: Option<Rect>,
}

fn placed(f: &Fixture) -> Vec<Placed> {
    let content = f.content();
    let mut glyphs = Vec::new();
    interpret(
        &f.pdf,
        content.concat(),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
        Opts::default(),
        |event| {
            if let Kind::Text(show) = event.kind {
                let approx = show.font.map(|f| f.approx()).unwrap_or_default();
                glyphs.extend(show.glyphs().map(|g| Placed {
                    code: g.code,
                    origin: (g.origin.x, g.origin.y),
                    bounds: g.quad.bounds(),
                    approx,
                    stock: show.stock,
                    exact: show.font.is_some_and(|f| exact(f, g.approx)),
                }));
            }
            ControlFlow::Continue(())
        },
    )
    .expect("the fixture interprets");
    glyphs
}

fn rect(r: PdfRect) -> Rect {
    Rect::new(
        r.left().value,
        r.bottom().value,
        r.right().value,
        r.top().value,
    )
}

/// The characters PDFium's text page makes of `page`, but those it
/// generates (spaces and line ends between words), in its order: None
/// where it makes no text page. Right-to-left text makes a character it
/// generates a piece (`CPDF_TextPage::AddCharInfo`), which
/// `FPDFText_IsGenerated` doesn't tell, but it's still of no text object,
/// or a space where its text object has it, at a point.
fn page_chars(page: &PdfPage<'_>) -> Option<Vec<Char>> {
    let text = page.text().ok()?;
    let chars = text.chars();
    let chars = (chars.iter())
        .filter(|c| !c.is_generated().unwrap_or(true) && c.text_object().is_ok())
        .map(|c| Char {
            unicode: c.unicode_value(),
            size: c.unscaled_font_size().value,
            origin: c
                .origin()
                .map_or((f32::NAN, f32::NAN), |(x, y)| (x.value, y.value)),
            loose: c.loose_bounds().map_or(Rect::default(), rect),
            tight: c.tight_bounds().ok().map(rect),
        })
        .filter(|c| {
            let point = c.loose.x0 == c.loose.x1 && c.loose.y0 == c.loose.y1;
            !(c.unicode == 0x20 && point)
        })
        .collect();
    Some(chars)
}

/// [`page_chars`] of the page `bytes` holds.
fn chars(bytes: &[u8]) -> Vec<Char> {
    let pdfium = pdfium();
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium opens it");
    let page = doc.pages().get(0).expect("its page");
    page_chars(&page).expect("its text page")
}

/// Whether `inner` lies within `outer`, give or take `by`.
fn within_by(inner: Rect, outer: Rect, by: f32) -> bool {
    inner.x0 >= outer.x0 - by
        && inner.y0 >= outer.y0 - by
        && inner.x1 <= outer.x1 + by
        && inner.y1 <= outer.y1 + by
}

/// Whether `inner` lies within `outer`, give or take [`EPSILON`].
fn within(inner: Rect, outer: Rect) -> bool {
    within_by(inner, outer, EPSILON)
}

fn near(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() <= EPSILON && (a.1 - b.1).abs() <= EPSILON
}

/// Whether a glyph of `font`'s, what's guessed of it `approx`, has its box
/// held to PDFium's loose box on every side: its outline's box is its
/// program's, united with its metrics' as PDFium unites them, and nothing
/// it's placed by is guessed. A Type3 glyph's `d1` box isn't united.
fn exact(font: &Foreign, approx: FontApprox) -> bool {
    font.kind() != FontKind::Type3 && !approx.any()
}

/// What's wrong with `fixture`'s glyphs, against PDFium's characters.
fn compare(name: &str, mut fixture: Fixture) -> Vec<String> {
    let glyphs = placed(&fixture);
    let mut chars = chars(&fixture.bytes());
    let mut wrong = Vec::new();
    for (i, glyph) in glyphs.iter().enumerate() {
        let at = format!(
            "{name} glyph {i} (code {:#x} at {:?})",
            glyph.code, glyph.origin
        );
        let approx = glyph.approx;
        if approx.codes || approx.widths {
            wrong.push(format!("{at}: its font's codes or widths are guessed"));
        }
        let Some(found) = chars.iter().position(|c| near(c.origin, glyph.origin)) else {
            wrong.push(format!("{at}: no character there"));
            continue;
        };
        let c = chars.remove(found);
        if glyph.stock {
            continue;
        }
        if !within(glyph.bounds, c.loose) {
            wrong.push(format!(
                "{at}: box {:?} past PDFium's {:?}",
                glyph.bounds, c.loose
            ));
        }
        let (b, l) = (glyph.bounds, c.loose);
        let across = [(b.x0, l.x0), (b.x1, l.x1)];
        let up = [(b.y0, l.y0), (b.y1, l.y1)];
        let sides = match approx.heights {
            true => &across[..],
            false => &[across, up].concat(),
        };
        let equal = sides.iter().all(|(a, b)| (a - b).abs() <= EPSILON);
        if glyph.exact && !equal {
            wrong.push(format!(
                "{at}: box {:?}, PDFium's {:?}, its outline {:?}",
                glyph.bounds, c.loose, c.tight
            ));
        } else if c.tight.is_some_and(|t| within(t, glyph.bounds)) && !equal {
            wrong.push(format!(
                "{at}: box {:?}, PDFium's {:?}, its outline {:?} within",
                glyph.bounds, c.loose, c.tight
            ));
        }
    }
    for c in chars {
        wrong.push(format!(
            "{name}: PDFium's {:?} at {:?} is no glyph",
            char::from_u32(c.unicode),
            c.origin
        ));
    }
    wrong
}

/// Each fixture's glyphs are PDFium's characters: the operators of the
/// text state, each kind of font, fonts over predefined CMaps, whose CIDs
/// are Adobe's as PDFium's own CMaps have them, fonts embedding a program
/// of each format, their heights the program's where the descriptor lacks
/// them, forms in forms, text in clipping modes,
/// a form that finds its font in the page's resources, and forms nested
/// as deep as PDFium draws them and one deeper, which neither draws. (Not marked content's: PDFium's text page makes a
/// range's `/ActualText` its characters, in place of its glyphs.) A glyph
/// in the stock Helvetica is held to its origin alone; one whose font's
/// program says its outline, to PDFium's loose box on every side
/// ([`exact`]).
#[test]
fn glyphs_are_where_pdfium_puts_characters() {
    let depth = Limits::DEFAULT.content.form_depth;
    let fixtures = [
        ("operators", fixture::operators()),
        ("fonts", fixture::fonts()),
        ("predefined CMaps", fixture::predefined()),
        ("font programs", fixture::programs()),
        ("forms", fixture::forms()),
        ("clips", fixture::clips()),
        ("inherited", fixture::inherited()),
        ("chain at form_depth", fixture::chain(depth)),
        ("chain past form_depth", fixture::chain(depth + 1)),
    ];
    let mut wrong = Vec::new();
    let mut glyphs = 0;
    let mut exact = Vec::new();
    for (name, f) in fixtures {
        let placed = placed(&f);
        glyphs += placed.len();
        exact.push(placed.iter().filter(|g| g.exact).count());
        wrong.extend(compare(name, f));
    }
    for line in &wrong {
        eprintln!("{line}");
    }
    assert!(
        wrong.is_empty(),
        "{} of {glyphs} glyphs aren't PDFium's",
        wrong.len()
    );
    assert_eq!(glyphs, 37 + 24 + 13 + 26 + 3 + 4 + 1 + 1);
    // Those over a program, the fonts' TrueType and CIDFonts among them,
    // held to PDFium's box on every side.
    assert_eq!(exact, [0, 15, 13, 26, 0, 0, 0, 0, 0]);
}

/// A quad's corners: bottom-left, bottom-right, top-right, top-left.
type Corners = [(f32, f32); 4];

/// Each image PDFium's page holds, in order: its object's rotated bounds,
/// through the matrix of each form it's drawn in.
fn image_objects(bytes: &[u8]) -> Vec<Corners> {
    fn collect(object: &PdfPageObject<'_>, within: &[PdfMatrix], quads: &mut Vec<Corners>) {
        match object {
            PdfPageObject::Image(image) => {
                let b = image.bounds().expect("an image's bounds");
                let mut corners = [
                    (b.x1(), b.y1()),
                    (b.x2(), b.y2()),
                    (b.x3(), b.y3()),
                    (b.x4(), b.y4()),
                ];
                for m in within.iter().rev() {
                    corners = corners.map(|(x, y)| m.apply_to_points(x, y));
                }
                quads.push(corners.map(|(x, y)| (x.value, y.value)));
            }
            PdfPageObject::XObjectForm(form) => {
                let mut within = within.to_vec();
                within.push(form.matrix().expect("a form's matrix"));
                for inner in form.iter() {
                    collect(&inner, &within, quads);
                }
            }
            _ => {}
        }
    }
    let pdfium = pdfium();
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium opens it");
    let page = doc.pages().get(0).expect("its page");
    let mut quads = Vec::new();
    for object in page.objects().iter() {
        collect(&object, &[], &mut quads);
    }
    quads
}

/// Each image the interpreter draws is PDFium's image object, in order,
/// and none is left over on either side: inline images by each rule
/// that finds their data, a stencil turned a quarter, image XObjects,
/// and one in a form, skewed.
#[test]
fn images_are_where_pdfium_puts_them() {
    let mut f = fixture::images();
    let content = f.content();
    let mut ours = Vec::new();
    interpret(
        &f.pdf,
        content.concat(),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
        Opts::default(),
        |event| {
            if let Kind::Image(image) = event.kind {
                ours.push(image.quad.0.map(|p| (p.x, p.y)));
            }
            ControlFlow::Continue(())
        },
    )
    .expect("the fixture interprets");
    let theirs = image_objects(&f.bytes());
    assert_eq!(
        ours.len(),
        theirs.len(),
        "ours {ours:?}, PDFium's {theirs:?}"
    );
    for (i, (ours, theirs)) in ours.iter().zip(&theirs).enumerate() {
        assert!(
            ours.iter().zip(theirs).all(|(&a, &b)| near(a, b)),
            "image {i}: {ours:?}, PDFium's {theirs:?}"
        );
    }
    assert_eq!(ours.len(), 13);
}

/// A file of one page, 5,000 points wide, `content` drawn with `fonts`.
fn one_page(fonts: Dictionary, content: String) -> Vec<u8> {
    let mut pdf = lopdf::Document::with_version("1.7");
    let stream = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 5000.into(), 900.into()],
        "Contents" => stream,
        "Resources" => dictionary! { "Font" => fonts },
    });
    let kids = dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 };
    pdf.objects.insert(pages, Object::Dictionary(kids));
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).expect("it saves");
    bytes
}

/// A `TJ` of many strings, each moved on by an adjustment of its own, in
/// `Tz` and `Tc`, lays its glyphs out where PDFium does, to a thousandth
/// of a point however far along the line: one text object, each string
/// on from where the one before it ends, in text space before `Tz`, as
/// `CPDF_TextObject` lays it out, not from a pen moved string by string,
/// whose sums, large far along a line, round otherwise. Its Helvetica has
/// `/Widths` ([`fixture::standard_14`]), so PDFium lays it out the same
/// whatever face it draws in.
#[test]
fn a_long_tj_is_laid_out_as_pdfium_lays_it_out() {
    let mut content = String::from("BT /F 9.7 Tf 87 Tz 0.13 Tc 1 0 0 1 60 500 Tm 3.1 0 Td [");
    for i in 0..400u32 {
        let letter = char::from(b'b' + (i % 20) as u8);
        content += &format!("(a{letter}) -{}.{:03} ", i % 7, (i * 37) % 1000);
    }
    content += "] TJ ET";
    let helvetica = fixture::standard_14("Type1", "Helvetica");
    let bytes = one_page(dictionary! { "F" => helvetica }, content);
    let src = source(&bytes);
    let page = src.page(0).expect("its page");
    let read = PageContent::read(src.pdf(), &page.contents, src.limits()).expect("its content");
    let mut ours = Vec::new();
    interpret(
        src.pdf(),
        read.concat(),
        page.resources.as_deref(),
        &Limits::DEFAULT,
        &Fonts::default(),
        Opts::default(),
        |event| {
            if let Kind::Text(show) = event.kind {
                ours.extend(show.glyphs().map(|g| (g.origin.x, g.origin.y)));
            }
            ControlFlow::Continue(())
        },
    )
    .expect("it interprets");
    let theirs: Vec<(f32, f32)> = chars(&bytes).iter().map(|c| c.origin).collect();
    assert_eq!(ours.len(), 800);
    assert_eq!(ours.len(), theirs.len());
    for (i, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        assert!(near(*a, *b), "glyph {i}: {a:?}, PDFium's {b:?}");
    }
}

/// Every glyph of each standard 14 font a file draws without a program or
/// `/Widths` is as wide as PDFium's own face draws it, where its width
/// isn't guessed ([`Foreign::width_guessed`]): each named by
/// `/Differences` in turn, shown between two `A`s, the second where their
/// widths put it.
///
/// Ignored, as it holds where PDFium draws in its own faces (Foxit's), as
/// it does where the system hasn't the fonts, as here; on a system with
/// Arial, Helvetica or Times New Roman, PDFium draws in those, which may
/// be otherwise wide, as the guessed glyphs say.
#[test]
#[ignore = "PDFium's own faces: cargo test -p sheaf-render --test content_interp -- --ignored --exact standard_14_widths_are_pdfium_s_faces_but_where_guessed"]
fn standard_14_widths_are_pdfium_s_faces_but_where_guessed() {
    let afms = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sheaf-doc/assets/afm");
    let (mut checked, mut guessed, mut wrong) = (0, 0, Vec::new());
    for std14 in Std14::ALL {
        let afm = std::fs::read_to_string(afms.join(format!("{}.afm", std14.name())));
        let afm = afm.expect("its AFM");
        let names: Vec<&str> = (afm.lines().filter(|l| l.starts_with("C ")))
            .filter_map(|l| l.split(';').find_map(|f| f.trim().strip_prefix("N ")))
            .map(str::trim)
            .collect();
        for chunk in names.chunks(128) {
            let differences: Vec<Object> = [128.into()]
                .into_iter()
                .chain(chunk.iter().map(|&n| Object::Name(n.into())))
                .collect();
            let font = dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => std14.name(),
                "Encoding" => dictionary! { "Differences" => differences },
            };
            let none = lopdf::Document::new();
            let foreign = Foreign::read(&none, &font, &Limits::DEFAULT, &mut 0).expect("no stream");
            // Lines far enough apart that PDFium takes no `A` for one of
            // the line's before it (`CPDF_TextPage::ProcessTextObjectItems`).
            let y = |code: u32| (20 * (code - 127)) as f32;
            let line = |code| {
                format!(
                    "BT /F 10 Tf 1 0 0 1 10 {} Tm <41{code:02x}41> Tj ET\n",
                    y(code)
                )
            };
            let content: String = (128..128 + chunk.len() as u32).map(line).collect();
            let chars = chars(&one_page(dictionary! { "F" => font }, content));
            for (code, name) in (128u32..).zip(chunk) {
                let xs = (chars.iter())
                    .filter(|c| (c.origin.1 - y(code)).abs() < EPSILON)
                    .map(|c| c.origin.0);
                let (first, last) = xs.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
                    (lo.min(x), hi.max(x))
                });
                let theirs = (last - first) * 100.0 - foreign.width(0x41);
                if foreign.width_guessed(code) {
                    guessed += 1;
                    continue;
                }
                checked += 1;
                if !theirs.is_finite() || (theirs - foreign.width(code)).abs() > 0.01 {
                    wrong.push(format!(
                        "{} {name}: {}, PDFium's {theirs}",
                        std14.name(),
                        foreign.width(code)
                    ));
                }
            }
        }
    }
    eprintln!("{checked} standard 14 glyphs as PDFium's faces draw them, {guessed} guessed");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `bytes`, read as a source.
fn source(bytes: &[u8]) -> Src {
    let kind = BlobKind::file(Path::new("fixture.pdf"), None);
    (Store::new().open(Arc::from(bytes), kind)).expect("it opens")
}

/// Why a page's content wasn't written again.
#[derive(Debug, Clone)]
enum Unwritten {
    /// It can't be read: there's no content to interpret.
    Unread(String),
    /// It can't be interpreted ([`InterpError`]).
    Interp(String),
    /// What it does can't be written so it reads back the same
    /// ([`EncodeError::Unwritable`]).
    Unwritable(String),
}

impl Unwritten {
    fn kind(&self) -> &'static str {
        match self {
            Unwritten::Unread(_) => "its content can't be read",
            Unwritten::Interp(_) => "it can't be interpreted",
            Unwritten::Unwritable(_) => "it can't be written so it reads back the same",
        }
    }

    fn why(&self) -> &str {
        match self {
            Unwritten::Unread(why) | Unwritten::Interp(why) | Unwritten::Unwritable(why) => why,
        }
    }
}

/// A file with its pages written again, in an update after it.
struct Written {
    bytes: Vec<u8>,
    /// Each page's content as written, or why it wasn't.
    pages: Vec<Result<Vec<u8>, Unwritten>>,
}

/// `src` with every page's content and the forms it draws written again
/// (`interp::encode`), in one update after it, encrypted as it is: each
/// page's `/Contents` one new stream, and each form's stream its content,
/// unfiltered, as the first page that draws it writes it. A page whose
/// content can't be read or written again is left as it is, and says
/// why. None where no update can go after it ([`Base::over`]).
fn written_again(src: &Src) -> anyhow::Result<Option<Written>> {
    let Some(base) = Base::over(src) else {
        return Ok(None);
    };
    let (pdf, lim) = (src.pdf(), src.limits());
    let fonts = Fonts::default();
    let key = |obj| ObjKey::Base {
        blob: src.id(),
        obj,
        copy: 0,
    };
    let mut objects = BTreeMap::new();
    let mut next = base.size;
    let mut done: HashMap<ObjectId, Result<Vec<u8>, Unwritten>> = HashMap::new();
    let mut pages = Vec::new();
    for page in src.pages() {
        if let Some(written) = done.get(&page.id) {
            pages.push(written.clone());
            continue;
        }
        let dict = pdf.get_object(page.id).and_then(Object::as_dict);
        let encoded = (PageContent::read(pdf, &page.contents, lim))
            .map_err(|fault| Unwritten::Unread(fault.to_string()))
            .and_then(|content| {
                let resources = page.resources.as_deref();
                encode(pdf, content.concat(), resources, lim, &fonts).map_err(|e| match e {
                    EncodeError::Interp(e) => Unwritten::Interp(e.to_string()),
                    e @ EncodeError::Unwritable { .. } => Unwritten::Unwritable(e.to_string()),
                })
            })
            .and_then(|encoded| match dict {
                Ok(dict) => Ok((encoded, dict.clone())),
                Err(e) => Err(Unwritten::Unread(format!("its dictionary: {e}"))),
            });
        let written = encoded.map(|(encoded, mut dict)| {
            let stream = (next, 0);
            next += 1;
            let content = Stream::new(dictionary! {}, encoded.content.clone());
            objects.insert(stream, (key(stream), Object::Stream(content)));
            dict.set("Contents", stream);
            objects.insert(page.id, (key(page.id), Object::Dictionary(dict)));
            for (id, data) in encoded.forms {
                if let btree_map::Entry::Vacant(slot) = objects.entry(id) {
                    let form = pdf.get_object(id).and_then(Object::as_stream);
                    let mut form = form.expect("a form it drew").clone();
                    form.set_plain_content(data);
                    slot.insert((key(id), Object::Stream(form)));
                }
            }
            encoded.content
        });
        done.insert(page.id, written.clone());
        pages.push(written);
    }
    let crypt = src.crypt().map(|c| &**c);
    let bytes = match append::section(&base, &objects, next, crypt)? {
        Some(update) => [&src.bytes()[..], &update.bytes].concat(),
        None => src.bytes().to_vec(),
    };
    Ok(Some(Written { bytes, pages }))
}

/// How wide pages are rendered, and how tall at most: a page taller than
/// eight times its width is rendered narrower.
const WIDTH: i32 = 1024;
const TALLEST: i32 = 8 * WIDTH;

/// Page `index` of `doc` rendered [`WIDTH`] pixels wide, RGBA, as PDFium
/// draws a page to show it: its annotations' and its form's appearances
/// drawn.
fn drawn(doc: &PdfDocument<'_>, index: i32) -> Result<Vec<u8>, PdfiumError> {
    let page = doc.pages().get(index)?;
    let config = PdfRenderConfig::new()
        .set_target_width(WIDTH)
        .set_maximum_height(TALLEST);
    Ok(page.render_with_config(&config)?.as_raw_bytes())
}

/// The first page of `bytes`, [`drawn`].
fn rendered(bytes: &[u8]) -> Vec<u8> {
    let pdfium = pdfium();
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .expect("PDFium opens it");
    drawn(&doc, 0).expect("it renders")
}

/// Each fixture's page, its content and the forms it draws written again
/// from what the interpreter reads of them, draws as it did, pixel for
/// pixel: text moved to each line by the `Tm` the interpreter says, `'`
/// and `"` among them, and the rest as written. The copy reads as the
/// content written, so it's that which PDFium draws.
#[test]
fn fixtures_written_again_draw_the_same() {
    for (name, mut f) in fixture::every() {
        let bytes = f.bytes();
        let written = written_again(&source(&bytes)).expect("the update");
        let written = written.expect("its catalog");
        let content = written.pages[0].clone().expect("its content is written");
        let reread = source(&written.bytes);
        let page = reread.page(0).expect("its page");
        let read = PageContent::read(reread.pdf(), &page.contents, reread.limits());
        assert_eq!(read.expect("its content").into_bytes(), content, "{name}");
        assert!(
            rendered(&bytes) == rendered(&written.bytes),
            "{name} draws otherwise"
        );
    }
}

/// Corpus paths, comma-separated, to run the corpus tiers on alone
/// (`SHEAF_INTERP_ONLY=forms/irs/f1040.pdf,…`): for looking at a few
/// files by hand. Unset, a tier reads every file it picks.
const ONLY: &str = "SHEAF_INTERP_ONLY";

/// The pixels a page written again may differ by from the file's and
/// pass: realize_corpus's allowance, PDFium's anti-aliasing at a
/// picture's or a shape's edge, which changes with what the process drew
/// before (PDFium's caches). Each page within it is still printed and
/// counted.
const ANTI_ALIASING_PX: usize = 16;

/// Each file whose glyphs or characters the glyph boxes tier can't say why
/// it matched none for, and how many of each, at most: a line each,
/// `path`, glyphs and characters, tab apart, after the lines of `#` that
/// say why there are any. A file it doesn't name has none. The tier fails
/// where a file has more, and says so where it has fewer; with
/// [`WRITE_UNEXPLAINED`] set, over every file, it writes the list again.
const UNEXPLAINED: &str = "tests/content_interp_unexplained.tsv";

/// Set, the glyph boxes tier writes [`UNEXPLAINED`] from what it found.
const WRITE_UNEXPLAINED: &str = "SHEAF_INTERP_WRITE_UNEXPLAINED";

/// The file whose glyphs held to PDFium's box on every side can't all be,
/// the program theirs are drawn from, and how many are past their
/// tolerance, exactly: its TrueType program `482 0 R`'s stream is corrupt,
/// and lopdf's inflate reads it to its end where zlib's stops ("invalid
/// distance too far back"), so sheaf reads the program, with what the bad
/// stretch decodes to, while PDFium, short of it, can't load it and draws
/// in a substitute face (`NWODGU+TimesNewRoman`). The file's other glyphs
/// are held as every file's. Greg accepted the allowance by this exact
/// count (2026-10-24); which reading is right is backlog F27's research,
/// decided on merit, not by PDFium's recovery.
const PROGRAM_DECODED_OTHERWISE: (&str, ObjectId, usize) =
    ("forms/safedocs/5208359.pdf", (482, 0), 38);

/// How far from the page, in points, a glyph is compared: frames.rs's
/// `FAR`, 2^17, where an `f32` still holds 1/64 pt. Farther, a
/// comparison would measure `f32`, not where the interpreter puts it.
const FAR: f32 = 131_072.0;

/// A file the corpus tiers read, and the set that picks it.
struct Pick {
    path: String,
    set: &'static str,
}

/// Every file the corpus tiers read: every form, the pool's sample, every
/// scan, and the conformance suites' files that claim PDF/UA, each once,
/// narrowed by [`ONLY`].
fn picks(corpus: &Corpus) -> Vec<Pick> {
    let list = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/pool-sample.txt");
    let pool = std::fs::read_to_string(list).expect("bench/pool-sample.txt");
    let pool = pool.lines().map(str::trim).filter(|l| !l.is_empty());
    let forms = (corpus.forms().unwrap().into_iter()).map(|e| (e.path, "forms"));
    let pool = pool.map(|path| (path.to_string(), "pool sample"));
    let scans = (corpus.scans().unwrap().into_iter()).map(|e| (e.path, "scans"));
    let ua = (corpus.conformance().unwrap().into_iter())
        .filter(|e| e.column("claims").contains("PDF/UA"))
        .map(|e| (e.path, "PDF/UA"));
    let all = forms.chain(pool).chain(scans).chain(ua);
    let mut seen = HashSet::new();
    let mut picks: Vec<Pick> = all
        .filter(|(path, _)| seen.insert(path.clone()))
        .map(|(path, set)| Pick { path, set })
        .collect();
    let Ok(only) = std::env::var(ONLY) else {
        return picks;
    };
    let named: BTreeSet<&str> = (only.split(',').map(str::trim))
        .filter(|p| !p.is_empty())
        .collect();
    assert!(!named.is_empty(), "{ONLY} is set but names no file");
    picks.retain(|pick| named.contains(pick.path.as_str()));
    assert_eq!(
        picks.len(),
        named.len(),
        "{ONLY} names a file the tiers don't read: {only}"
    );
    eprintln!("narrowed by {ONLY} to {} files", picks.len());
    picks
}

/// What became of a file a tier read: what `read` made of it, or why
/// it couldn't.
enum Outcome<T> {
    Read(T),
    /// sheaf doesn't open it.
    Unopened(String),
    /// `read` panicked, saying this.
    Panicked(String),
}

/// `read` of each of `picks`' files, its bytes and the source sheaf opens
/// on them, on a thread a core, a panic caught.
fn each_file<T: Send>(
    corpus: &Corpus,
    picks: &[Pick],
    read: impl Fn(&[u8], &Src) -> T + Sync,
) -> Vec<Outcome<T>> {
    let entries: Vec<Entry> = (picks.iter())
        .map(|pick| Entry {
            path: pick.path.clone(),
            ..Entry::default()
        })
        .collect();
    each_entry(&entries, |entry| {
        let bytes: Arc<[u8]> = match std::fs::read(corpus.path(entry)) {
            Ok(bytes) => bytes.into(),
            Err(e) => return Outcome::Unopened(e.to_string()),
        };
        let kind = BlobKind::file(Path::new(&entry.path), None);
        let src = match Store::new().open(bytes.clone(), kind) {
            Ok(src) => src,
            Err(e) => return Outcome::Unopened(format!("{e:#}")),
        };
        match catch_unwind(AssertUnwindSafe(|| read(&bytes, &src))) {
            Ok(read) => Outcome::Read(read),
            Err(panic) => {
                let said = (panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .or_else(|| panic.downcast_ref::<String>().cloned());
                Outcome::Panicked(said.unwrap_or_else(|| "a panic".into()))
            }
        }
    })
}

/// A tier over the corpus: every file it reads ([`picks`]) read by
/// `read`, those that don't open printed, a panic failing it; what each
/// file came to, with its set and path. None when there's no corpus.
fn tier<T: Send>(
    name: &str,
    read: impl Fn(&[u8], &Src) -> T + Sync,
) -> Option<Vec<(&'static str, String, T)>> {
    let corpus = Corpus::or_skip()?;
    let picks = picks(&corpus);
    let started = Instant::now();
    let read = each_file(&corpus, &picks, read);
    let mut files: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut read_ok, mut panics) = (Vec::new(), Vec::new());
    for (pick, outcome) in picks.into_iter().zip(read) {
        *files.entry(pick.set).or_default() += 1;
        match outcome {
            Outcome::Read(t) => read_ok.push((pick.set, pick.path, t)),
            Outcome::Unopened(e) => eprintln!("{name}: doesn't open: {} ({e})", pick.path),
            Outcome::Panicked(e) => panics.push(format!("{}: {e}", pick.path)),
        }
    }
    eprintln!(
        "{name}: {files:?} files, {} read, in {:.0} s",
        read_ok.len(),
        started.elapsed().as_secs_f64()
    );
    assert!(panics.is_empty(), "{name} panics:\n{}", panics.join("\n"));
    Some(read_ok)
}

/// The measures of a page's interpretation a cap bounds, and the cap:
/// its name in [`ContentLimits`](sheaf_doc::source::ContentLimits), and
/// its value. A form's use is a `Do`, an operator.
type Cap = (&'static str, &'static str, fn(&Limits) -> usize);
const MEASURES: [Cap; 9] = [
    ("operators", "operators", |l| l.content.operators),
    ("glyphs", "glyphs", |l| l.content.glyphs),
    ("bytes interpreted", "interpreted", |l| {
        l.content.interpreted
    }),
    ("bytes decompressed", "decompressed", |l| {
        l.content.decompressed
    }),
    ("path segments", "segments", |l| l.content.segments),
    ("form uses", "operators", |l| l.content.operators),
    ("form depth", "form_depth", |l| l.content.form_depth),
    ("saved states", "states", |l| l.content.states),
    ("marked nesting", "depth", |l| l.content.depth),
];

/// What interpreting a file's pages came to.
#[derive(Default)]
struct Interpreted {
    pages: usize,
    /// Pages whose content can't be read, which there's nothing of to
    /// interpret.
    unread: Vec<String>,
    /// Pages past a cap ([`InterpError::Limit`]).
    limited: Vec<String>,
    /// Pages whose content, or a form's, doesn't lex, or that draw an
    /// XObject that can't be read.
    failed: Vec<String>,
    /// Pages whose glyphs laid out aren't as many as the interpretation
    /// counts.
    miscounted: Vec<String>,
    /// Glyphs whose box isn't finite.
    unbounded: usize,
    /// Form uses refused: within themselves, and too deep.
    cycles: usize,
    deep: usize,
    /// The fonts its text is shown in, by what sheaf read of their
    /// programs: none, read, malformed ([`FontProgram`]); and those
    /// malformed, a line each. A font by number counts once a file, one
    /// written as a dictionary, which each page reads anew, once a page.
    programs: [usize; 3],
    malformed: Vec<String>,
    /// Glyphs whose outline the font's program says, and whose it doesn't
    /// ([`FontApprox::boxes`]).
    outlines: [usize; 2],
    /// The most of each of [`MEASURES`] a page came to, and which.
    most: [(usize, String); 9],
}

fn interpreted(src: &Src) -> Interpreted {
    let mut out = Interpreted::default();
    let lim = &Limits::DEFAULT;
    let fonts = Fonts::default();
    let mut seen = HashSet::new();
    for (i, page) in src.pages().iter().enumerate() {
        out.pages += 1;
        let at = format!("page {}", i + 1);
        let content = match PageContent::read(src.pdf(), &page.contents, lim) {
            Ok(content) => content,
            Err(fault) => {
                out.unread.push(format!("{at}: {fault}"));
                continue;
            }
        };
        let (mut laid, mut unbounded, mut cycles, mut deep) = (0, 0, 0, 0);
        let resources = page.resources.as_deref();
        let read = interpret(
            src.pdf(),
            content.concat(),
            resources,
            lim,
            &fonts,
            Opts::default(),
            |event| {
                match event.kind {
                    Kind::Text(show) => {
                        // A dictionary font lives as long as its page's
                        // interpretation, so its address is told apart
                        // only within it.
                        let key = |font: &Foreign| match font.id() {
                            Some(id) => (Some(id), 0, 0),
                            None => (None, i, std::ptr::from_ref(font) as usize),
                        };
                        if let Some(font) = show.font
                            && seen.insert(key(font))
                        {
                            let program = font.program();
                            out.programs[program as usize] += 1;
                            if program == FontProgram::Malformed {
                                out.malformed
                                    .push(format!("{at}: {}", font_name(src, font)));
                            }
                        }
                        for g in show.glyphs() {
                            laid += 1;
                            out.outlines[usize::from(g.approx.boxes)] += 1;
                            let b = g.quad.bounds();
                            let finite = [b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite());
                            unbounded += usize::from(!finite);
                        }
                    }
                    Kind::FormRefused { why, .. } => match why {
                        Refused::Cycle => cycles += 1,
                        Refused::Depth => deep += 1,
                    },
                    _ => {}
                }
                ControlFlow::Continue(())
            },
        );
        (out.cycles, out.deep, out.unbounded) = (
            out.cycles + cycles,
            out.deep + deep,
            out.unbounded + unbounded,
        );
        let summary = match read {
            Ok(summary) => summary,
            Err(InterpError::Limit(cap)) => {
                out.limited.push(format!("{at}: past the cap on {cap}"));
                continue;
            }
            Err(e) => {
                out.failed.push(format!("{at}: {e}"));
                continue;
            }
        };
        if laid != summary.glyphs {
            out.miscounted
                .push(format!("{at}: {laid} glyphs laid out, {}", summary.glyphs));
        }
        let measured = [
            summary.operators,
            summary.glyphs,
            summary.interpreted,
            summary.decompressed,
            summary.segments,
            summary.uses,
            summary.deepest_form,
            summary.deepest_q,
            summary.deepest_marked,
        ];
        for (most, n) in out.most.iter_mut().zip(measured) {
            if n > most.0 {
                *most = (n, at.clone());
            }
        }
    }
    out
}

/// Every page of every file the tiers read, its forms drawn and its
/// glyphs laid out, within the caps, with every box finite and no panic;
/// the most a page comes to of each measure a cap bounds printed beside
/// it.
#[test]
#[ignore = "corpus tier: just tier-interp"]
fn every_page_is_interpreted_within_the_caps() {
    let name = "content_interp interpreted";
    let Some(files) = tier(name, |_, src| interpreted(src)) else {
        return;
    };
    let lim = &Limits::DEFAULT;
    let mut most: [(usize, String); 9] = Default::default();
    let mut pages: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut unread, mut unbounded) = (0, 0);
    let (mut cycles, mut deep) = (0, 0);
    let (mut programs, mut outlines) = ([0; 3], [0; 2]);
    let (mut limited, mut miscounted, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    for (set, path, o) in &files {
        *pages.entry(set).or_default() += o.pages;
        for (n, of_file) in programs.iter_mut().zip(o.programs) {
            *n += of_file;
        }
        for (n, of_file) in outlines.iter_mut().zip(o.outlines) {
            *n += of_file;
        }
        for line in &o.malformed {
            eprintln!("{name}: a program that doesn't read: {path} {line}");
        }
        unread += o.unread.len();
        (unbounded, cycles, deep) = (unbounded + o.unbounded, cycles + o.cycles, deep + o.deep);
        limited.extend(o.limited.iter().map(|l| format!("{path} {l}")));
        miscounted.extend(o.miscounted.iter().map(|l| format!("{path} {l}")));
        failed.extend(o.failed.iter().map(|l| format!("{path} {l}")));
        for (most, (n, at)) in most.iter_mut().zip(&o.most) {
            if *n > most.0 {
                *most = (*n, format!("{path} {at}"));
            }
        }
    }
    for line in &failed {
        eprintln!("{name}: not interpreted: {line}");
    }
    eprintln!(
        "{name}: pages {pages:?}: {unread} whose content can't be read, {} that can't be \
         interpreted (content or a form's that doesn't lex, an XObject that can't be read), \
         {} past a cap; form uses refused: {cycles} within themselves, {deep} too deep; \
         {unbounded} glyphs whose box isn't finite",
        failed.len(),
        limited.len()
    );
    eprintln!(
        "{name}: fonts shown in, by number once a file and as a dictionary once a page: {} \
         embedding no program, {} whose program is read, {} whose program doesn't read; glyphs: {} whose outline the program says, {} \
         whose it doesn't",
        programs[0], programs[1], programs[2], outlines[0], outlines[1]
    );
    for ((measure, cap, value), (n, at)) in MEASURES.iter().zip(&most) {
        eprintln!(
            "{name}: most {measure} on a page: {n} ({at}); its cap, {cap}, {}",
            value(lim)
        );
    }
    for line in limited.iter().chain(&miscounted) {
        eprintln!("{name}: {line}");
    }
    assert!(limited.is_empty(), "{} pages past a cap", limited.len());
    assert!(failed.is_empty(), "{} pages not interpreted", failed.len());
    assert_eq!(unbounded, 0, "glyphs whose box isn't finite");
    assert!(
        miscounted.is_empty(),
        "{} pages miscount their glyphs",
        miscounted.len()
    );
}

/// What a glyph's box is held to: by its font and how it's drawn. A
/// glyph is in the first class that takes it, from the last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// A simple font's (Type1, MMType1, TrueType), upright.
    Simple,
    /// A Type0 font's, written across, upright.
    CidHorizontal,
    /// A Type0 font's, written down.
    CidVertical,
    /// A Type3 font's.
    Type3,
    /// Text turned or skewed: its text rendering matrix isn't upright.
    Rotated,
    /// A glyph whose codes or widths are guessed, or that a width guessed
    /// before it on its line moves (its `approx`, a [`FontApprox`]):
    /// counted, not held to anything.
    Approx,
}

impl Class {
    const ALL: [Class; 6] = [
        Class::Simple,
        Class::CidHorizontal,
        Class::CidVertical,
        Class::Type3,
        Class::Rotated,
        Class::Approx,
    ];

    /// A glyph's class, what's guessed of it `approx`, in `font` and drawn
    /// through `trm`.
    fn of(approx: FontApprox, font: &Foreign, trm: Affine) -> Class {
        match font.kind() {
            _ if approx.codes || approx.widths => Class::Approx,
            _ if font.is_vertical() => Class::CidVertical,
            FontKind::Type3 => Class::Type3,
            _ if trm.b != 0.0 || trm.c != 0.0 => Class::Rotated,
            FontKind::Type0(_) => Class::CidHorizontal,
            _ => Class::Simple,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Class::Simple => "simple",
            Class::CidHorizontal => "CID horizontal",
            Class::CidVertical => "CID vertical",
            Class::Type3 => "Type3",
            Class::Rotated => "rotated or skewed",
            Class::Approx => "codes or widths guessed",
        }
    }

    /// How far, in points, a glyph's origin and box may be from PDFium's;
    /// None where it isn't held to anything.
    fn tolerance(self) -> Option<f32> {
        match self {
            Class::Approx => None,
            _ => Some(EPSILON),
        }
    }
}

/// Distances, in points, as a histogram, ten buckets a decade from
/// 1e-9 pt: their percentiles to within a bucket, and the most.
#[derive(Clone)]
struct Spread {
    n: u64,
    zero: u64,
    buckets: [u64; BUCKETS],
    max: f32,
}

/// From 1e-9 pt to 1e4.
const BUCKETS: usize = 130;

impl Default for Spread {
    fn default() -> Spread {
        Spread {
            n: 0,
            zero: 0,
            buckets: [0; BUCKETS],
            max: 0.0,
        }
    }
}

impl Spread {
    fn add(&mut self, d: f32) {
        self.n += 1;
        self.max = self.max.max(d);
        if d <= 0.0 {
            self.zero += 1;
            return;
        }
        let k = ((d.log10() + 9.0) * 10.0).floor();
        self.buckets[k.clamp(0.0, (BUCKETS - 1) as f32) as usize] += 1;
    }

    fn merge(&mut self, other: &Spread) {
        self.n += other.n;
        self.zero += other.zero;
        for (a, b) in self.buckets.iter_mut().zip(&other.buckets) {
            *a += b;
        }
        self.max = self.max.max(other.max);
    }

    /// The least bucket's top that a share `p` of the distances are
    /// within, or the most.
    fn within(&self, p: f64) -> f32 {
        let want = (p * self.n as f64).ceil() as u64;
        let mut seen = self.zero;
        if seen >= want {
            return 0.0;
        }
        for (k, &n) in self.buckets.iter().enumerate() {
            seen += n;
            if seen >= want {
                return 10f32.powf((k + 1) as f32 / 10.0 - 9.0).min(self.max);
            }
        }
        self.max
    }
}

impl std::fmt::Display for Spread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "p50 {:.1e}, p99 {:.1e}, max {:.1e}",
            self.within(0.5),
            self.within(0.99),
            self.max
        )
    }
}

/// A class's glyphs compared, and how far they came from PDFium's.
#[derive(Default, Clone)]
struct Compared {
    glyphs: usize,
    /// Of them, in a font whose heights are guessed, which are held only
    /// along their line; and those whose outline no program says
    /// ([`FontApprox::boxes`]).
    heights: usize,
    boxes: usize,
    /// Of those held on every side, those turned but by quarter turns
    /// ([`Shown::turned`]).
    turned: usize,
    /// How far each origin is from its character's.
    origin: Spread,
    /// How far each box reaches past its character's loose box.
    outside: Spread,
    /// Where the character's outline lies within the box, how far a side
    /// is from its loose box's.
    unequal: Spread,
    /// Of them, held to the loose box on every side ([`Shown::exact`]):
    /// how far a side is from it, or turned, how far each box reaches
    /// past the other's bound.
    exact: Spread,
    /// Of them, matched to a character PDFium bounds by a box with no
    /// area, its outline's where it has none and no width (`\u{200B}`),
    /// or a matrix flattens it, or by its outline alone, where its font's
    /// ascent is its descent: held to its origin only.
    flat: usize,
    /// Past the class's tolerance at an origin another glyph shares that
    /// could be the character's as well, of its size and code point, or
    /// with a box more like its: glyphs drawn one over another in other
    /// fonts, or whose texts PDFium reads otherwise, matched ambiguously.
    ambiguous: usize,
    /// Past the class's tolerance, and of them, how far by decade, the
    /// worst distance of each: to 1e-2 pt, 1e-1, 1, 10, 100, and past it.
    wrong: usize,
    wrong_by: [usize; 6],
}

impl Compared {
    fn merge(&mut self, other: &Compared) {
        self.glyphs += other.glyphs;
        self.heights += other.heights;
        self.boxes += other.boxes;
        self.turned += other.turned;
        self.flat += other.flat;
        self.ambiguous += other.ambiguous;
        self.origin.merge(&other.origin);
        self.outside.merge(&other.outside);
        self.unequal.merge(&other.unequal);
        self.exact.merge(&other.exact);
        self.wrong += other.wrong;
        for (a, b) in self.wrong_by.iter_mut().zip(other.wrong_by) {
            *a += b;
        }
    }
}

/// A glyph the interpreter placed on a corpus page, as comparing it
/// needs it.
struct Shown {
    origin: (f32, f32),
    /// Its box: its quad's bounds, or in vertical writing, the column
    /// PDFium bounds it by ([`vertical_cell`]).
    bounds: Rect,
    /// In vertical writing in a text space that isn't scaled, where that
    /// column is the quad's, how far the quad's bounds are from it.
    off_cell: f32,
    class: Class,
    /// Its heights are guessed ([`FontApprox::heights`]), and its outline
    /// ([`FontApprox::boxes`]).
    heights: bool,
    boxes: bool,
    /// Its font's ascent is its descent, so PDFium bounds it by its
    /// outline alone (`GetLooseBounds`), and it's held to its origin.
    outline: bool,
    /// The sides of its box held to its character's loose box's: x0, y0,
    /// x1 and y1. Along its line only, where its heights are guessed;
    /// none where it's turned.
    sides: [bool; 4],
    /// Its box is held to its character's loose box on every side, its
    /// outline's box its font's program's and nothing of it guessed
    /// ([`exact`]); in vertical writing, where its text space isn't
    /// scaled, the quad then PDFium's column.
    exact: bool,
    /// Where it's held so and its text space is turned but by quarter
    /// turns, that space: its box, united with its outline's there, holds
    /// PDFium's, which unites the two boxes' bounds in PageSpace, and lies
    /// within the bounds through it of the bounds of PDFium's in it.
    turned: Option<Affine>,
    text: Option<String>,
    /// The stream of its font's program, where it's a reference.
    program: Option<ObjectId>,
    /// Its font, as the interpretation holds it, its size and its code:
    /// how PDFium tells a duplicate.
    font: usize,
    size: f32,
    code: u32,
    /// An em at its size in PageSpace, along its line, and the distance
    /// within which PDFium takes a glyph of its code in its font for a
    /// duplicate.
    em: f32,
    dup: f32,
    /// It's under an `/ActualText` that PDFium reads in place of its
    /// glyphs.
    actual: bool,
    /// The operator that shows it, and the content it's in, a number
    /// each.
    op: usize,
    stream: usize,
}

/// What a page shows, as the interpreter reads it.
#[derive(Default)]
struct Showing {
    glyphs: Vec<Shown>,
    /// The code points of each `/ActualText` its glyphs are under, and
    /// where those glyphs are: each one's box, half an em wider all round.
    actual: HashSet<u32>,
    actual_boxes: Vec<Rect>,
    /// Each operator's string under an `/ActualText` PDFium reads, where
    /// its first glyph is, and the text.
    spans: Vec<((f32, f32), String)>,
    /// It refuses a form drawn within itself, which PDFium draws to its
    /// depth.
    cycles: bool,
    /// Glyphs whose codes or widths are guessed, or that a width guessed
    /// before them moves, by their font's name and what's guessed.
    approx: BTreeMap<String, usize>,
}

/// `font`'s `/BaseFont` and kind, for a line.
/// The stream `font`'s program is in, where the font is an object: its
/// descriptor's, or its descendant's, first of `/FontFile`, `/FontFile2`
/// and `/FontFile3` that's a reference.
fn program_of(src: &Src, font: &Foreign) -> Option<ObjectId> {
    fn dict<'a>(pdf: &'a lopdf::LazyPdf, o: &'a Object) -> Option<&'a Dictionary> {
        pdf.dereference(o).ok()?.1.as_dict().ok()
    }
    let pdf = src.pdf();
    let mut font = pdf.get_object(font.id()?).ok()?.as_dict().ok()?;
    if let Ok(descendants) = font.get(b"DescendantFonts") {
        let first = pdf
            .dereference(descendants)
            .ok()?
            .1
            .as_array()
            .ok()?
            .first()?;
        font = dict(pdf, first)?;
    }
    let descriptor = dict(pdf, font.get(b"FontDescriptor").ok()?)?;
    [&b"FontFile"[..], b"FontFile2", b"FontFile3"]
        .into_iter()
        .find_map(|key| descriptor.get(key).ok()?.as_reference().ok())
}

fn font_name(src: &Src, font: &Foreign) -> String {
    let named = font.id().and_then(|id| {
        let dict = src.pdf().get_object(id).and_then(Object::as_dict).ok()?;
        let name = dict.get(b"BaseFont").and_then(Object::as_name).ok()?;
        Some(String::from_utf8_lossy(name).into_owned())
    });
    format!(
        "{} ({:?})",
        named.as_deref().unwrap_or("no name"),
        font.kind()
    )
}

/// `font`'s name and what's guessed of a glyph of its, `approx`, for a
/// list: its codes or widths, or only where the glyph is, a width before
/// it on its line guessed ([`Foreign::width_guessed`]).
fn font_label(src: &Src, font: &Foreign, approx: FontApprox) -> String {
    let of_font = font.approx();
    let guessed = match (approx.codes, of_font.widths) {
        (true, true) => "codes and widths",
        (true, false) => "codes",
        (false, true) => "widths",
        (false, false) => "a width on the line",
    };
    format!("{}, {guessed} guessed", font_name(src, font))
}

/// Where `g`, a glyph of `font`'s at `size`, writes vertically, the
/// column PDFium bounds it by (`GetLooseBounds`): an em wide in
/// PageSpace whatever the text space's scale, from its vertical origin
/// down by its advance, from the origin `g` has; its quad is that column
/// through the text space, so the two are one where that isn't scaled.
fn vertical_cell(font: &Foreign, g: &Glyph<'_>, size: f32) -> Option<Rect> {
    let v = font.vertical(g.code)?;
    let left = g.origin.x + (v.vx - 500.0) * size / 1000.0;
    let top = g.origin.y + v.vy * size / 1000.0;
    Some(Rect::new(
        left,
        top + v.w1 * size / 1000.0,
        left + size,
        top,
    ))
}

/// How far apart `a`'s sides are from `b`'s, the farthest.
fn distance(a: Rect, b: Rect) -> f32 {
    [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1]
        .iter()
        .fold(0.0, |far, d| far.max(d.abs()))
}

/// How far `a` reaches past `b`, on the side it reaches farthest; 0 where
/// it lies within.
fn reach_past(a: Rect, b: Rect) -> f32 {
    [b.x0 - a.x0, b.y0 - a.y0, a.x1 - b.x1, a.y1 - b.y1]
        .iter()
        .fold(0.0, |far, d| far.max(*d))
}

/// How unlike `a` is `b`: how far apart their sides are, all told.
fn unlike(a: Rect, b: Rect) -> f32 {
    [a.x0 - b.x0, a.y0 - b.y0, a.x1 - b.x1, a.y1 - b.y1]
        .iter()
        .map(|d| d.abs())
        .sum()
}

/// What page `page` of `src` shows: every glyph, its forms drawn.
fn showing(src: &Src, page: &PageFacts, fonts: &Fonts) -> Result<Showing, String> {
    let lim = &Limits::DEFAULT;
    let content = PageContent::read(src.pdf(), &page.contents, lim)
        .map_err(|fault| format!("its content can't be read: {fault}"))?;
    let mut out = Showing::default();
    let mut labels: HashMap<(usize, bool, bool), String> = HashMap::new();
    let mut streams = HashMap::new();
    let mut op = 0;
    let mut spanned = None;
    let resources = page.resources.as_deref();
    interpret(
        src.pdf(),
        content.concat(),
        resources,
        lim,
        fonts,
        Opts::default(),
        |event| {
            match event.kind {
                Kind::Op => op += 1,
                Kind::Text(show) => {
                    let Some(font) = show.font else {
                        return ControlFlow::Continue(());
                    };
                    let trm = show.trm();
                    let metrics = font.metrics();
                    let id = std::ptr::from_ref(font) as usize;
                    let actual = (event.marks.iter())
                        .filter(|m| m.stream.as_ref() == event.at.stream)
                        .find_map(|m| m.actual_text.as_deref());
                    out.actual
                        .extend(actual.into_iter().flat_map(|t| t.chars().map(u32::from)));
                    let read = actual.filter(|t| !t.is_empty());
                    if let Some(text) = read
                        && spanned != Some(op)
                        && let Some(first) = show.glyphs().next()
                    {
                        spanned = Some(op);
                        out.spans
                            .push(((first.origin.x, first.origin.y), text.to_string()));
                    }
                    let actual = read.is_some();
                    let next = streams.len();
                    let stream = *streams.entry(event.at.stream.cloned()).or_insert(next);
                    let along = trm.a.hypot(trm.b);
                    let s = show.start;
                    let unscaled = (s.a, s.b, s.c, s.d) == (1.0, 0.0, 0.0, 1.0);
                    let program = program_of(src, font);
                    for g in show.glyphs() {
                        let class = Class::of(g.approx, font, trm);
                        let heights = g.approx.heights && class != Class::CidVertical;
                        let sides = match heights {
                            false => [true; 4],
                            true if trm.b == 0.0 && trm.c == 0.0 => [true, false, true, false],
                            true if trm.a == 0.0 && trm.d == 0.0 => [false, true, false, true],
                            true => [false; 4],
                        };
                        if class == Class::Approx {
                            let label = (labels.entry((id, g.approx.codes, g.approx.widths)))
                                .or_insert_with(|| font_label(src, font, g.approx));
                            *out.approx.entry(label.clone()).or_default() += 1;
                        }
                        let quad = g.quad.bounds();
                        let exact = class != Class::Approx && exact(font, g.approx);
                        let (bounds, off_cell, exact) = match vertical_cell(font, &g, show.size) {
                            Some(_) if unscaled && exact => (quad, 0.0, true),
                            Some(cell) if unscaled => (cell, distance(quad, cell), false),
                            Some(cell) => (cell, 0.0, false),
                            None => (quad, 0.0, exact),
                        };
                        let quarter = (s.b == 0.0 && s.c == 0.0) || (s.a == 0.0 && s.d == 0.0);
                        let em = along.max(trm.c.hypot(trm.d));
                        if actual {
                            let half = em / 2.0;
                            out.actual_boxes.push(Rect {
                                x0: bounds.x0 - half,
                                y0: bounds.y0 - half,
                                x1: bounds.x1 + half,
                                y1: bounds.y1 + half,
                            });
                        }
                        out.glyphs.push(Shown {
                            origin: (g.origin.x, g.origin.y),
                            bounds,
                            off_cell,
                            class,
                            heights,
                            boxes: g.approx.boxes,
                            outline: !font.is_vertical() && metrics.ascent == metrics.descent,
                            sides,
                            exact,
                            turned: (exact && !quarter).then_some(s),
                            text: g.text().map(|t| t.into_owned()),
                            program,
                            font: id,
                            size: show.size,
                            code: g.code,
                            em,
                            dup: 0.07 * along,
                            actual,
                            op,
                            stream,
                        });
                    }
                }
                Kind::FormRefused {
                    why: Refused::Cycle,
                    ..
                } => out.cycles = true,
                _ => {}
            }
            ControlFlow::Continue(())
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Whether `p` is a place within [`FAR`] of the page.
fn near_page(p: (f32, f32)) -> bool {
    p.0.abs() < FAR && p.1.abs() < FAR
}

/// A pass of matching glyphs to characters: how far from a glyph's origin
/// it looks, whether it takes a character of any code point, and whether
/// only for a glyph whose text isn't known.
type Pass = (fn(&Shown) -> f32, bool, bool);

/// A grid cell's side, in points.
const CELL: f32 = 8.0;

/// How near a glyph's origin, in points, a character of its code point is
/// taken before one farther off, and one of any for a glyph whose text
/// isn't known: well past any class's tolerance, so a glyph a little past
/// it is matched, and fails.
const AT_ORIGIN: f32 = 16.0 * EPSILON;

fn cell_of(p: (f32, f32)) -> (i32, i32) {
    ((p.0 / CELL).floor() as i32, (p.1 / CELL).floor() as i32)
}

/// PDFium's characters on a page, found by where their origins are, each
/// taken once.
struct Grid {
    chars: Vec<Char>,
    taken: Vec<bool>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl Grid {
    /// `chars`, those without an origin [`near_page`] taken already: how
    /// many those are.
    fn new(chars: Vec<Char>) -> (Grid, usize) {
        let taken: Vec<bool> = chars.iter().map(|c| !near_page(c.origin)).collect();
        let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, c) in chars.iter().enumerate().filter(|(i, _)| !taken[*i]) {
            cells.entry(cell_of(c.origin)).or_default().push(i);
        }
        let far = taken.iter().filter(|t| **t).count();
        (
            Grid {
                chars,
                taken,
                cells,
            },
            far,
        )
    }

    /// The characters not yet taken within `radius` of `at`, of a code
    /// point `text` has, or where `any`, of any: how far each is, and
    /// which.
    fn near(
        &self,
        at: (f32, f32),
        radius: f32,
        text: Option<&str>,
        any: bool,
    ) -> Vec<(f32, usize)> {
        let has = |c: &Char| text.is_some_and(|t| t.chars().any(|ch| u32::from(ch) == c.unicode));
        let (x0, y0) = cell_of((at.0 - radius, at.1 - radius));
        let (x1, y1) = cell_of((at.0 + radius, at.1 + radius));
        let mut near = Vec::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                for &i in self.cells.get(&(x, y)).into_iter().flatten() {
                    let c = &self.chars[i];
                    let d = (c.origin.0 - at.0).abs().max((c.origin.1 - at.1).abs());
                    if !self.taken[i] && d <= radius && (any || has(c)) {
                        near.push((d, i));
                    }
                }
            }
        }
        near
    }

    /// Takes the character not yet taken nearest `at`, within `radius`,
    /// of a code point `text` has.
    fn take(&mut self, at: (f32, f32), radius: f32, text: Option<&str>) -> Option<usize> {
        let near = self.near(at, radius, text, false);
        let (_, i) = near
            .into_iter()
            .min_by(|a, b| a.partial_cmp(b).expect("finite"))?;
        self.taken[i] = true;
        Some(i)
    }

    /// Takes the characters that are the same glyph's as `i`, which
    /// PDFium makes at its origin with its box: one for each other code
    /// point of its `text`, and for each piece of a ligature it breaks up
    /// (U+FB00 to U+FB06).
    fn take_rest(&mut self, i: usize, text: Option<&str>) {
        let text = text.unwrap_or("");
        let ligature = text.chars().any(|c| ('\u{FB00}'..='\u{FB06}').contains(&c));
        let mut rest = match ligature {
            true => 3,
            false => text.chars().count().saturating_sub(1),
        };
        let first = self.chars[i].clone();
        let mut same: Vec<usize> = (self.cells.get(&cell_of(first.origin)).into_iter().flatten())
            .copied()
            .filter(|&j| {
                let c = &self.chars[j];
                let piece = ligature || text.chars().any(|ch| u32::from(ch) == c.unicode);
                !self.taken[j] && c.origin == first.origin && c.loose == first.loose && piece
            })
            .collect();
        same.sort_unstable();
        for j in same {
            if rest == 0 {
                break;
            }
            self.taken[j] = true;
            rest -= 1;
        }
    }

    /// Takes the characters PDFium makes of an `/ActualText` its glyphs
    /// are under (`ProcessMarkedContent`), at where their text object
    /// starts, `at`: one each of its code points, a control one a space,
    /// and none of U+FFFD or past; a hyphen that ends a line its U+0002
    /// (`CPDF_TextPage::ProcessGenerateCharacter`). How many it took.
    fn take_actual(&mut self, at: (f32, f32), actual: &str) -> usize {
        let mut took = 0;
        for c in actual.chars().filter(|&c| u32::from(c) < 0xFFFD) {
            let c = match c <= '\u{80}' && !(' '..'\u{7F}').contains(&c) {
                true => ' ',
                false => c,
            };
            let text = match c {
                '-' | '\u{AD}' => format!("{c}\u{2}"),
                _ => c.to_string(),
            };
            took += usize::from(self.take(at, EPSILON, Some(&text)).is_some());
        }
        took
    }
}

/// What comparing a file's glyphs to PDFium's characters came to.
#[derive(Default)]
struct Boxes {
    /// Why it wasn't compared: PDFium refuses it, or its pages aren't
    /// sheaf's.
    skipped: Option<String>,
    pages: usize,
    /// Pages not compared: PDFium makes no text page of it, or sheaf
    /// can't interpret it.
    uncompared: Vec<String>,
    classes: BTreeMap<Class, Compared>,
    /// Glyphs, and PDFium's characters, [`FAR`] off the page.
    far: usize,
    far_chars: usize,
    /// Glyphs PDFium makes no character of, and characters no glyph is,
    /// by why.
    unmatched: BTreeMap<&'static str, usize>,
    leftover: BTreeMap<&'static str, usize>,
    /// Glyphs matched to a character whose code point their text hasn't.
    recoded: usize,
    approx: BTreeMap<String, usize>,
    /// Of the glyphs past their class's tolerance, those drawn from
    /// [`PROGRAM_DECODED_OTHERWISE`]'s program.
    otherwise: usize,
    /// Glyphs past their class's tolerance, those so matched ambiguously
    /// ([`Compared::ambiguous`]), and those unmatched, and characters left
    /// over, it can't say why for: a line each.
    wrong: Vec<String>,
    ambiguous: Vec<String>,
    unexplained: Vec<String>,
}

/// How many lines of each kind a file prints at most.
const LINES: usize = 20;

impl Boxes {
    fn note(lines: &mut Vec<String>, line: impl FnOnce() -> String) {
        if lines.len() < LINES {
            lines.push(line());
        }
    }

    /// `g`, matched to `c`, held to its class's rule; where it's past its
    /// tolerance and `shared` (another glyph at its origin could be `c`'s
    /// as well), counted as matched ambiguously.
    fn check(&mut self, at: &str, g: &Shown, c: &Char, shared: impl FnOnce() -> bool) {
        let compared = self.classes.entry(g.class).or_default();
        compared.glyphs += 1;
        compared.heights += usize::from(g.heights);
        compared.boxes += usize::from(g.boxes);
        let (b, l) = (g.bounds, c.loose);
        let origin = (g.origin.0 - c.origin.0)
            .abs()
            .max((g.origin.1 - c.origin.1).abs());
        let past = [l.x0 - b.x0, l.y0 - b.y0, b.x1 - l.x1, b.y1 - l.y1];
        let diff = [b.x0 - l.x0, b.y0 - l.y0, b.x1 - l.x1, b.y1 - l.y1];
        let flat = l.x0 >= l.x1 || l.y0 >= l.y1 || g.outline;
        compared.flat += usize::from(flat);
        let sides = match flat {
            true => [false; 4],
            false => g.sides,
        };
        let held = |d: [f32; 4]| -> f32 {
            (d.iter().zip(sides))
                .filter(|(_, held)| *held)
                .map(|(d, _)| *d)
                .fold(0.0, f32::max)
        };
        let tolerance = g.class.tolerance();
        compared.origin.add(origin);
        // Held on every side: equal, or turned, each within the other's
        // bound; else within the loose box, and equal to it where the
        // outline lies within.
        let (outside, unequal) = match (g.exact && !flat, g.turned) {
            (true, None) => {
                let off = distance(b, l);
                compared.exact.add(off);
                (0.0, Some(off))
            }
            (true, Some(start)) => {
                compared.turned += 1;
                let bound = start.bounds(start.inverse().bounds(l));
                let off = reach_past(l, b).max(reach_past(b, bound));
                compared.exact.add(off);
                (0.0, Some(off))
            }
            (false, _) => {
                let outside = held(past).max(g.off_cell);
                compared.outside.add(outside);
                let outline = c
                    .tight
                    .filter(|t| within_by(*t, b, tolerance.unwrap_or(EPSILON)));
                let unequal = outline.map(|_| held(diff.map(f32::abs)));
                if let Some(unequal) = unequal {
                    compared.unequal.add(unequal);
                }
                (outside, unequal)
            }
        };
        if g.text
            .as_deref()
            .is_some_and(|t| !t.chars().any(|ch| u32::from(ch) == c.unicode))
        {
            self.recoded += 1;
        }
        let Some(tolerance) = tolerance else {
            return;
        };
        let worst = origin.max(outside).max(unequal.unwrap_or(0.0));
        if worst <= tolerance {
            return;
        }
        let line = || {
            format!(
                "{at}: {} glyph {:#x} {:?} at {:?}, box {b:?}{}: PDFium's {:?} at {:?}, loose \
                 {l:?}, tight {:?}: origin {origin:.1e} off, {outside:.1e} past, unequal by {}",
                g.class.name(),
                g.code,
                g.text,
                g.origin,
                if g.exact { " held on every side" } else { "" },
                char::from_u32(c.unicode),
                c.origin,
                c.tight,
                unequal.map_or("nothing".into(), |u| format!("{u:.1e}"))
            )
        };
        if shared() {
            compared.ambiguous += 1;
            Boxes::note(&mut self.ambiguous, line);
        } else {
            compared.wrong += 1;
            self.otherwise += usize::from(g.program == Some(PROGRAM_DECODED_OTHERWISE.1));
            let decade = (worst.log10() + 2.0).ceil().clamp(0.0, 5.0) as usize;
            compared.wrong_by[decade] += 1;
            Boxes::note(&mut self.wrong, line);
        }
    }

    /// The glyphs `showing` has against PDFium's characters `theirs`.
    fn compare(&mut self, at: &str, showing: Showing, theirs: Vec<Char>) {
        for (label, n) in showing.approx {
            *self.approx.entry(label).or_default() += n;
        }
        let (mut grid, far) = Grid::new(theirs);
        self.far_chars += far;
        let glyphs = &showing.glyphs;
        // The characters an `/ActualText` makes, which are no glyph's; then
        // each glyph's, in passes that take ever more: at its origin, of
        // any code point; within `AT_ORIGIN` of one its text has; within
        // half an em of one it has; and within `AT_ORIGIN` of any, where
        // its text isn't known. So a glyph PDFium makes no character of
        // can't take its neighbour's first.
        let actual: usize = (showing.spans.iter())
            .map(|(at, text)| grid.take_actual(*at, text))
            .sum();
        *self.leftover.entry("an /ActualText's").or_default() += actual;
        let mut matched: Vec<Option<usize>> = vec![None; glyphs.len()];
        for (g, m) in glyphs.iter().zip(&mut matched) {
            let b = g.bounds;
            if !(near_page(g.origin) && near_page((b.x0, b.y0)) && near_page((b.x1, b.y1))) {
                self.far += 1;
            } else if g.actual {
                *self.unmatched.entry("under an /ActualText").or_default() += 1;
            } else {
                continue;
            }
            *m = Some(usize::MAX);
        }
        let em = |g: &Shown| (g.em / 2.0).clamp(0.5, 32.0);
        let passes: [Pass; 4] = [
            (|_| EPSILON, true, false),
            (|_| AT_ORIGIN, false, false),
            (em, false, false),
            (|_| AT_ORIGIN, true, true),
        ];
        // Within a pass, a glyph and a character the more alike first: at
        // its origin, one of its code point and size, then the one whose
        // box is more like its own, so that of strings drawn one over
        // another, in other fonts or sizes, each takes its own; and farther
        // off, the nearer.
        for (radius, any, unknown) in passes {
            let mut pairs = Vec::new();
            for (i, g) in glyphs.iter().enumerate() {
                if matched[i].is_some() || (unknown && g.text.is_some()) {
                    continue;
                }
                let at = radius(g) == EPSILON;
                let has = |c: &Char| {
                    g.text
                        .as_deref()
                        .is_some_and(|t| t.chars().any(|ch| u32::from(ch) == c.unicode))
                };
                for (d, c) in grid.near(g.origin, radius(g), g.text.as_deref(), any) {
                    let char = &grid.chars[c];
                    let unlike = unlike(g.bounds, char.loose);
                    let other =
                        f32::from(u8::from(!has(char))) + f32::from(u8::from(char.size != g.size));
                    pairs.push(match at {
                        true => (other, unlike, d, i, c),
                        false => (d, other, unlike, i, c),
                    });
                }
            }
            pairs.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
            for (_, _, _, i, c) in pairs {
                if matched[i].is_none() && !grid.taken[c] {
                    grid.taken[c] = true;
                    grid.take_rest(c, glyphs[i].text.as_deref());
                    matched[i] = Some(c);
                }
            }
        }
        let dup_strings = duplicate_strings(glyphs);
        for (i, (g, m)) in glyphs.iter().zip(&matched).enumerate() {
            match *m {
                Some(usize::MAX) => continue,
                Some(c) => {
                    let c = &grid.chars[c];
                    let like = unlike(g.bounds, c.loose);
                    let has = |o: &Shown| {
                        (o.text.as_deref())
                            .is_some_and(|t| t.chars().any(|ch| u32::from(ch) == c.unicode))
                    };
                    // Another glyph at its origin as like the character:
                    // of its size and code point, or with a box more like
                    // its own.
                    let shared = || {
                        glyphs.iter().any(|o| {
                            let alike = o.size == c.size && (has(o) || !has(g));
                            let likelier = unlike(o.bounds, c.loose) < like;
                            !std::ptr::eq(o, g) && near(o.origin, g.origin) && (alike || likelier)
                        })
                    };
                    self.check(at, g, c, shared);
                    continue;
                }
                None => {}
            }
            let earlier = &glyphs[i.saturating_sub(7)..i];
            let dup = dup_strings.contains(&g.op)
                || earlier.iter().any(|e| {
                    e.font == g.font
                        && e.code == g.code
                        && (e.origin.0 - g.origin.0).abs() < g.dup
                        && (e.origin.1 - g.origin.1).abs() < g.dup
                });
            let why = match () {
                _ if dup => "a duplicate, which PDFium passes over",
                _ if g.text.as_deref().is_some_and(|t| t.trim().is_empty()) => {
                    "a space, which PDFium's line passes over"
                }
                _ => "unexplained",
            };
            *self.unmatched.entry(why).or_default() += 1;
            if why == "unexplained" {
                Boxes::note(&mut self.unexplained, || {
                    format!(
                        "{at}: no character for {} glyph {:#x} {:?} at {:?}",
                        g.class.name(),
                        g.code,
                        g.text,
                        g.origin
                    )
                });
            }
        }
        // A character an `/ActualText` makes is near a glyph under it.
        let under_actual = |c: &Char| {
            let l = c.loose;
            (showing.actual_boxes.iter())
                .any(|b| l.x0 <= b.x1 && b.x0 <= l.x1 && l.y0 <= b.y1 && b.y0 <= l.y1)
        };
        for (c, _) in (grid.chars.iter().zip(&grid.taken)).filter(|(_, taken)| !**taken) {
            let why = match () {
                _ if (showing.actual.contains(&c.unicode) || c.unicode == 0x20)
                    && under_actual(c) =>
                {
                    "an /ActualText's"
                }
                _ if showing.cycles => "in a form PDFium draws within itself",
                _ => "unexplained",
            };
            *self.leftover.entry(why).or_default() += 1;
            if why == "unexplained" {
                Boxes::note(&mut self.unexplained, || {
                    format!(
                        "{at}: no glyph for PDFium's {:?} ({:#x}) at {:?}",
                        char::from_u32(c.unicode),
                        c.unicode,
                        c.origin
                    )
                });
            }
        }
    }
}

/// The operators of `glyphs` whose string PDFium takes for a duplicate
/// of one of the five shown before it in the same content
/// (`IsSameTextObject`): the same codes at the same size, starting
/// within nine tenths of the last glyph's width across and an eighth of
/// an em up.
fn duplicate_strings(glyphs: &[Shown]) -> HashSet<usize> {
    let mut strings: Vec<&[Shown]> = Vec::new();
    let mut rest = glyphs;
    while let Some(first) = rest.first() {
        let n = rest.iter().take_while(|g| g.op == first.op).count();
        strings.push(&rest[..n]);
        rest = &rest[n..];
    }
    let mut dups = HashSet::new();
    for (i, s) in strings.iter().enumerate() {
        let before = (strings[..i].iter().rev())
            .filter(|p| p[0].stream == s[0].stream)
            .take(5);
        let (first, last) = (&s[0], &s[s.len() - 1]);
        let width = last.bounds.x1 - last.bounds.x0;
        for p in before {
            let same = p.len() == s.len()
                && p.iter().zip(s.iter()).all(|(a, b)| a.code == b.code)
                && p[0].size == first.size
                && (p[0].origin.0 - first.origin.0).abs() <= 0.9 * width
                && (p[0].origin.1 - first.origin.1).abs() <= first.em / 8.0;
            if same {
                dups.insert(first.op);
                break;
            }
        }
    }
    dups
}

/// `src`'s glyphs against PDFium's characters, page by page.
fn boxes(bytes: &[u8], src: &Src) -> Boxes {
    let mut out = Boxes::default();
    let theirs: Vec<Option<Vec<Char>>> = {
        let pdfium = pdfium();
        let Ok(doc) = pdfium.load_pdf_from_byte_slice(bytes, None) else {
            out.skipped = Some("PDFium refuses it".into());
            return out;
        };
        doc.pages().iter().map(|page| page_chars(&page)).collect()
    };
    if theirs.len() != src.pages().len() {
        out.skipped = Some(format!(
            "PDFium has {} pages, sheaf {}",
            theirs.len(),
            src.pages().len()
        ));
        return out;
    }
    let fonts = Fonts::default();
    for (i, (page, theirs)) in src.pages().iter().zip(theirs).enumerate() {
        out.pages += 1;
        let at = format!("page {}", i + 1);
        let Some(theirs) = theirs else {
            out.uncompared
                .push(format!("{at}: PDFium makes no text page"));
            continue;
        };
        match showing(src, page, &fonts) {
            Ok(showing) => out.compare(&at, showing, theirs),
            Err(e) => out.uncompared.push(format!("{at}: {e}")),
        }
    }
    out
}

/// [`UNEXPLAINED`]'s counts, by file.
fn unexplained_list() -> BTreeMap<String, [usize; 2]> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(UNEXPLAINED);
    let list = std::fs::read_to_string(path).expect("the list of what's unexplained");
    (list.lines())
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [path, glyphs, chars] = fields[..] else {
                panic!("{UNEXPLAINED}: {line:?} isn't a path and two counts");
            };
            let count = |n: &str| n.parse().expect("a count");
            (path.to_string(), [count(glyphs), count(chars)])
        })
        .collect()
}

/// [`UNEXPLAINED`] written again: its lines of `#`, and `found`'s files
/// that have any.
fn write_unexplained(found: &BTreeMap<String, [usize; 2]>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(UNEXPLAINED);
    let list = std::fs::read_to_string(&path).expect("the list of what's unexplained");
    let mut out: String = (list.lines().take_while(|l| l.starts_with('#')))
        .map(|l| format!("{l}\n"))
        .collect();
    for (file, [glyphs, chars]) in found.iter().filter(|(_, n)| **n != [0, 0]) {
        out += &format!("{file}\t{glyphs}\t{chars}\n");
    }
    std::fs::write(path, out).expect("writing the list");
}

/// Each glyph the interpreter places on every page of every file the
/// tiers read is where PDFium puts its character, within its class's
/// tolerance; what PDFium makes no character of, or no glyph is, counted
/// by why, and what it can't say why for no more than [`UNEXPLAINED`]
/// says for each file.
#[test]
#[ignore = "corpus tier: just tier-interp"]
fn glyph_boxes_are_pdfium_characters_on_every_page() {
    let name = "content_interp glyph boxes";
    let Some(files) = tier(name, boxes) else {
        return;
    };
    let mut classes: BTreeMap<Class, Compared> = BTreeMap::new();
    let mut unmatched: BTreeMap<&str, usize> = BTreeMap::new();
    let mut leftover: BTreeMap<&str, usize> = BTreeMap::new();
    let mut approx: BTreeMap<String, usize> = BTreeMap::new();
    let (mut pages, mut uncompared, mut far, mut far_chars, mut recoded) = (0, 0, 0, 0, 0);
    let (mut past, mut decoded_otherwise, mut unheld) = (0, 0, None);
    let mut wrong = Vec::new();
    let mut found: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    for (_, path, o) in &files {
        let unexplained = |by: &BTreeMap<&str, usize>| by.get("unexplained").copied();
        let counts = [unexplained(&o.unmatched), unexplained(&o.leftover)];
        found.insert(path.clone(), counts.map(|n| n.unwrap_or(0)));
        if let Some(why) = &o.skipped {
            eprintln!("{name}: not compared: {path}: {why}");
            continue;
        }
        pages += o.pages;
        uncompared += o.uncompared.len();
        for line in &o.uncompared {
            eprintln!("{name}: page not compared: {path} {line}");
        }
        for (class, c) in &o.classes {
            classes.entry(*class).or_default().merge(c);
        }
        for (why, n) in &o.unmatched {
            *unmatched.entry(why).or_default() += n;
        }
        for (why, n) in &o.leftover {
            *leftover.entry(why).or_default() += n;
        }
        for (font, n) in &o.approx {
            *approx.entry(format!("{path}: {font}")).or_default() += n;
        }
        (far, far_chars, recoded) = (far + o.far, far_chars + o.far_chars, recoded + o.recoded);
        for line in o.unexplained.iter().chain(&o.ambiguous) {
            eprintln!("{name}: {path} {line}");
        }
        let of_file: usize = o.classes.values().map(|c| c.wrong).sum();
        let (otherwise, _, allowed) = PROGRAM_DECODED_OTHERWISE;
        let lines = o.wrong.iter().map(|l| format!("{path} {l}"));
        if *path == otherwise && o.otherwise != allowed {
            unheld = Some(o.otherwise);
        }
        if *path == otherwise && o.otherwise == allowed && of_file == allowed {
            decoded_otherwise += of_file;
            lines.for_each(|l| eprintln!("{name}: its program decoded otherwise: {l}"));
        } else {
            past += of_file;
            wrong.extend(lines);
        }
        let by_class: Vec<String> = (o.classes.iter())
            .filter(|(_, c)| c.wrong > 0)
            .map(|(class, c)| format!("{} {}", c.wrong, class.name()))
            .collect();
        if !by_class.is_empty() {
            eprintln!(
                "{name}: {path}: past the tolerance: {}",
                by_class.join(", ")
            );
        }
    }
    for (font, n) in &approx {
        eprintln!("{name}: guessed font: {font}: {n} glyphs");
    }
    let glyphs: usize = classes.values().map(|c| c.glyphs).sum();
    eprintln!(
        "{name}: {pages} pages, {uncompared} not compared; {glyphs} glyphs matched, {recoded} \
         of them to a character of another code point; {far} glyphs and {far_chars} characters \
         far off the page, not compared; glyphs no character is: {unmatched:?}; characters no \
         glyph is: {leftover:?}"
    );
    for class in Class::ALL {
        let c = classes.get(&class).cloned().unwrap_or_default();
        let tolerance = class
            .tolerance()
            .map_or("none".into(), |t| format!("{t:e} pt"));
        eprintln!(
            "{name}: {}: {} glyphs ({} of them with heights guessed, {} with outlines guessed, {} \
             boxed by PDFium in a box with no area), tolerance {tolerance}, {} past it matched \
             ambiguously at an origin another glyph shares, \
             {} past it (by decade from 1e-2 pt: {:?}); origin {}; held to the loose box on \
             every side ({} glyphs, {} of them turned) {}; of the rest, past the loose box {}; \
             unequal where the outline is within ({} glyphs) {}",
            class.name(),
            c.glyphs,
            c.heights,
            c.boxes,
            c.flat,
            c.ambiguous,
            c.wrong,
            c.wrong_by,
            c.origin,
            c.exact.n,
            c.turned,
            c.exact,
            c.outside,
            c.unequal.n,
            c.unequal
        );
    }
    for line in &wrong {
        eprintln!("{name}: {line}");
    }
    eprintln!(
        "{name}: {past} glyphs past their class's tolerance, and {decoded_otherwise} in {}, \
         whose program PDFium can't load and sheaf reads",
        PROGRAM_DECODED_OTHERWISE.0
    );
    if std::env::var_os(WRITE_UNEXPLAINED).is_some() {
        assert!(
            std::env::var_os(ONLY).is_none(),
            "{WRITE_UNEXPLAINED} with {ONLY}"
        );
        write_unexplained(&found);
    }
    let listed = unexplained_list();
    let mut more = Vec::new();
    for (file, counts) in &found {
        let allowed = listed.get(file).copied().unwrap_or_default();
        if counts.iter().zip(allowed).any(|(n, most)| *n > most) {
            more.push(format!("{file}: {counts:?} unexplained, past {allowed:?}"));
        } else if *counts != allowed {
            eprintln!("{name}: {file}: {counts:?} unexplained, fewer than {allowed:?}");
        }
    }
    for line in &more {
        eprintln!("{name}: {line}");
    }
    assert!(past == 0, "{past} glyphs past their class's tolerance");
    let (otherwise, program, allowed) = PROGRAM_DECODED_OTHERWISE;
    assert!(
        unheld.is_none(),
        "{otherwise}: {unheld:?} glyphs of {} {} R's past their tolerance, not {allowed}",
        program.0,
        program.1
    );
    assert_eq!(uncompared, 0, "pages not compared");
    assert!(
        more.is_empty(),
        "{} files with more unexplained than {UNEXPLAINED} says",
        more.len()
    );
}

/// What writing a file's pages again and rendering them came to.
#[derive(Default)]
struct Redrawn {
    /// Why it wasn't compared: no update can go after it, PDFium refuses
    /// it, or nothing was written.
    skipped: Option<String>,
    /// Why writing it again or rendering the copy failed.
    failed: Option<String>,
    pages: usize,
    /// Pages not written again, by why, a line each.
    unwritten: Vec<(&'static str, String)>,
    /// Pages PDFium doesn't render from the file, which aren't compared.
    unrendered: Vec<String>,
    /// Pages that differ past [`ANTI_ALIASING_PX`], and within it, a line
    /// each.
    differing: Vec<String>,
    within: Vec<String>,
    /// Pages that differ past it from the file's first render, but are its
    /// render once PDFium loads the file again, pixel for pixel or within
    /// the allowance: PDFium's caches, warm for both, not the copy.
    noisy: Vec<String>,
}

/// `src`'s pages and forms [`written_again`], each page of the copy
/// rendered against the file's.
fn redrawn(bytes: &[u8], src: &Src) -> Redrawn {
    let mut out = Redrawn::default();
    let written = match written_again(src) {
        Ok(Some(written)) => written,
        Ok(None) => {
            out.skipped = Some("no update can go after it: its trailer names no catalog".into());
            return out;
        }
        Err(e) => {
            out.failed = Some(format!("its update: {e:#}"));
            return out;
        }
    };
    for (i, page) in written.pages.iter().enumerate() {
        if let Err(why) = page {
            (out.unwritten).push((why.kind(), format!("page {}: {}", i + 1, why.why())));
        }
    }
    if written.bytes.len() == bytes.len() {
        out.skipped = Some("no page is written again".into());
        return out;
    }
    let pdfium = pdfium();
    let Ok(doc) = pdfium.load_pdf_from_byte_slice(bytes, None) else {
        out.skipped = Some("PDFium refuses it".into());
        return out;
    };
    let copy = match pdfium.load_pdf_from_byte_slice(&written.bytes, None) {
        Ok(copy) => copy,
        Err(e) => {
            out.failed = Some(format!("PDFium refuses the copy: {e:?}"));
            return out;
        }
    };
    let n = doc.pages().len();
    if copy.pages().len() != n {
        out.failed = Some(format!("{n} pages, the copy {}", copy.pages().len()));
        return out;
    }
    let differ = |a: &[u8], b: &[u8]| match a.len() == b.len() {
        true => (a.chunks(4).zip(b.chunks(4)))
            .filter(|(a, b)| a != b)
            .count(),
        false => usize::MAX,
    };
    // The file loaded again, where a page differs past the allowance: its
    // page drawn as the copy's is, PDFium's caches warm for both.
    let mut again = None;
    for i in 0..n {
        let at = format!("page {}", i + 1);
        let Ok(theirs) = drawn(&doc, i) else {
            out.unrendered.push(at);
            continue;
        };
        out.pages += 1;
        let Ok(ours) = drawn(&copy, i) else {
            out.differing
                .push(format!("{at}: the copy's doesn't render"));
            continue;
        };
        match differ(&theirs, &ours) {
            0 => {}
            px if px <= ANTI_ALIASING_PX => out.within.push(format!("{at}: {px} px")),
            px => {
                let again =
                    again.get_or_insert_with(|| pdfium.load_pdf_from_byte_slice(bytes, None));
                let warm = again.as_ref().ok().and_then(|a| drawn(a, i).ok());
                let warm = warm.map_or(usize::MAX, |warm| differ(&warm, &ours));
                let line = format!("{at}: {px} px, from the file loaded again {warm} px");
                match warm <= ANTI_ALIASING_PX {
                    true => out.noisy.push(line),
                    false => out.differing.push(line),
                }
            }
        }
    }
    out
}

/// Every file the tiers read, its pages and the forms they draw written
/// again from what the interpreter reads of them, renders as it did,
/// each page within [`ANTI_ALIASING_PX`] pixels.
#[test]
#[ignore = "corpus tier: just tier-interp"]
fn pages_written_again_render_the_same() {
    let name = "content_interp written again";
    let Some(files) = tier(name, redrawn) else {
        return;
    };
    // Per set: files compared, pages compared, differing past the
    // allowance, within it, and drawn as PDFium draws the file loaded
    // again, pages not written, not rendered.
    let mut sets: BTreeMap<&str, [usize; 7]> = BTreeMap::new();
    let mut unwritten: BTreeMap<&str, usize> = BTreeMap::new();
    let mut skipped: BTreeMap<&str, usize> = BTreeMap::new();
    let mut problems = Vec::new();
    for (set, path, o) in &files {
        for (kind, line) in &o.unwritten {
            *unwritten.entry(kind).or_default() += 1;
            problems.push(format!("{path} not written again: {line}"));
        }
        if let Some(why) = &o.skipped {
            *skipped.entry(why.as_str()).or_default() += 1;
            eprintln!("{name}: not compared: {path}: {why}");
            continue;
        }
        if let Some(e) = &o.failed {
            problems.push(format!("{path}: {e}"));
            continue;
        }
        for line in &o.unrendered {
            eprintln!("{name}: PDFium doesn't render {path} {line}");
        }
        for line in &o.within {
            eprintln!("{name}: {path} {line}, within the anti-aliasing allowance");
        }
        for line in &o.noisy {
            eprintln!("{name}: {path} {line}, as PDFium draws the file loaded again");
        }
        problems.extend(o.differing.iter().map(|l| format!("{path} {l}")));
        let counts = sets.entry(set).or_default();
        counts[0] += 1;
        counts[1] += o.pages;
        counts[2] += o.differing.len();
        counts[3] += o.within.len();
        counts[4] += o.noisy.len();
        counts[5] += o.unwritten.len();
        counts[6] += o.unrendered.len();
    }
    eprintln!("{name}: files not compared: {skipped:?}; pages not written again: {unwritten:?}");
    for (
        set,
        [
            files,
            pages,
            differing,
            within,
            noisy,
            unwritten,
            unrendered,
        ],
    ) in &sets
    {
        eprintln!(
            "{name}: {set}: {files} files, {} of {pages} pages differ, {within} of them within \
             the anti-aliasing allowance ({ANTI_ALIASING_PX} px), {noisy} drawn as PDFium draws \
             the file loaded again; {unwritten} pages not written again, {unrendered} PDFium \
             doesn't render",
            differing + within + noisy
        );
    }
    for line in &problems {
        eprintln!("{name}: {line}");
    }
    assert!(problems.is_empty(), "{} problems", problems.len());
}
