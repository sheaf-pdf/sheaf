//! One-page PDFs built in code for the content interpreter, here and in
//! other crates' tests (the `fixtures` feature): each operator of the
//! text state, each kind of font, forms in forms, forms drawn within
//! themselves, images of each kind, paths, clips and shadings, and marked
//! content. Each says what it draws, worked by hand, in PageSpace:
//! origins, and boxes as `x0..x1 × y0..y1`. Text that PDFium is to place
//! the same is in fonts it lays out the same on every OS: Type3, fonts
//! over an embedded program, the glyphless TrueType OCR text is written
//! in ([`Glyphless`]), and the standard 14 given `/Widths`, Adobe's
//! ([`standard_14`]), as PDFium takes a standard 14 font's widths, as its
//! heights, from the face it draws in where it has none, which is the
//! system's where the system has the font. Only PDFium's stock Helvetica,
//! which no `/Widths` can be given, is drawn without. [`every`] names
//! each, and [`fuzz_page`] is the page the `interp` fuzz target draws on.
//!
//! Most draw in [`SQUARES`]: a Type3 font whose `a` is a square an em
//! wide and `b` half one, from 0.2 em below the baseline to 0.8 above.

use std::io::Write;

use image::{Rgb, RgbImage};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};

use super::interp::Encoded;
use super::{Foreign, Glyphless, PageContent, contents};
use crate::source::Limits;

/// A one-page PDF.
pub struct Fixture {
    pub pdf: Pdf,
}

impl Fixture {
    /// Its page's dictionary.
    pub fn page(&self) -> &Dictionary {
        let id = self.pdf.page_iter().next().expect("one page");
        self.pdf.get_dictionary(id).expect("its page")
    }

    /// Its page's content, decoded.
    pub fn content(&self) -> PageContent {
        PageContent::read(&self.pdf, &contents(self.page()), &Limits::DEFAULT)
            .expect("the fixture's content")
    }

    /// Its page's `/Resources`, as written.
    pub fn resources(&self) -> Option<&Object> {
        self.page().get(b"Resources").ok()
    }

    /// It, as a file.
    pub fn bytes(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        self.pdf.save_to(&mut out).expect("writing the fixture");
        out
    }

    /// It with its page's content and its forms written again as
    /// `encoded` has them ([`encode`](super::interp::encode)): its page's
    /// `/Contents` one stream, and each form's stream its content,
    /// unfiltered.
    pub fn encoded(&self, encoded: &Encoded) -> Fixture {
        let mut pdf = self.pdf.clone();
        let content = pdf.add_object(Stream::new(dictionary! {}, encoded.content.clone()));
        let page = pdf.page_iter().next().expect("one page");
        (pdf.get_dictionary_mut(page).expect("its page")).set("Contents", content);
        for (id, data) in &encoded.forms {
            let form = pdf.get_object_mut(*id).and_then(Object::as_stream_mut);
            form.expect("a form").set_plain_content(data.clone());
        }
        Fixture { pdf }
    }
}

/// The Type3 font most fixtures draw in, named `/T3`, in glyph space a
/// thousandth of text space: `a` a square 1000 wide, `b` 500, and the
/// space 250 and empty; all from -200 to 800, its `/FontBBox`, which are
/// its descent and ascent, as it has no `A` or `g`.
pub const SQUARES: &[u8] = b"T3";

/// Each operator of the text state, and `cm`, `q` and `Q`, in
/// [`SQUARES`] at 10 unless it's said: `a` 10 wide and `b` 5, 2 below
/// the baseline to 8 above. A `Q` with nothing saved comes first, then a
/// string shown before any `Tf`, which draws nothing; each case after it
/// is in a `q` of its own. Its 37 glyphs, origin then box:
///
/// | case | glyph | origin | box |
/// |---|---|---|---|
/// | `Td` | a, b | 100,700; 110,700 | 100..110 × 698..708; 110..115 |
/// | `2 Tc` | a, b | 100,680; 112,680 | 100..110; 112..117 × 678..688 |
/// | `5 Tw`, `(a a)` | a, space, a | 100; 110; 117.5 at 660 | 100..110; 110..112.5; 117.5..127.5 |
/// | `50 Tz`, `[(ab) -1000 (a)] TJ` | a, b, a | 100; 105; 112.5 at 640 | 100..105; 105..107.5; 112.5..117.5 |
/// | `12 TL`, `T*` | a, b | 100,620; 100,608 | 100..110 × 618..628; 100..105 × 606..616 |
/// | `3 Ts` | a | 100,593 | 100..110 × 591..601 |
/// | `0 1 -1 0 300 500 Tm` | a, b | 300,500; 300,510 | 292..302 × 500..510; 292..302 × 510..515 |
/// | `100 560 TD`, `0 -14 TD`, `T*` | a, b, a | 100,560; 100,546; 100,532 | at 100 |
/// | `10 TL`, `'` | a | 100,510 | 100..110 × 508..518 |
/// | `10 TL`, `4 1 (a a) "` | a, space, a | 100; 111; 118.5 at 490 | 100..110; 111..113.5; 118.5..128.5 |
/// | `[(a) -500 (b) 200 (a)] TJ` | a, b, a | 100; 115; 118 at 470 | 100..110; 115..120; 118..128 |
/// | `2 0 0 2 50 50 cm`, `10 200 Td` | a | 70,450 | 70..90 × 446..466 |
/// | `q /T3 20 Tf 3 Tc 50 50 Td Q` | a | 100,420 | 100..110 × 418..428 |
/// | `gs /Font [/T3 20]` | b | 100,400 | 100..110 × 396..416 |
/// | `gs /Font [T3 by reference, 10]` | a, stock | 100,380 | 100..105.56 |
/// | `/Nope 12 Tf` | a, stock | 100,360 | 100..106.672 |
/// | `[(a) -1000 ()] TJ (b) Tj` | a, b | 100,340; 110,340 | 100..110; 110..115 |
/// | `[(a) () -500 (b) -1000 ()] TJ (b) Tj` | a, b, b | 100; 115; 120 at 320 | 100..110; 115..120; 120..125 |
/// | `[(a) -1000] TJ (b) Tj` | a, b | 100,300; 120,300 | 100..110; 120..125 |
///
/// The stock two are in PDFium's stock Helvetica: an ExtGState's font by
/// reference names no font in the resources, as PDFium reads it, nor
/// does `/Nope`. Helvetica's `a` is 556 wide. A `TJ` that passes over an
/// empty string loses its last adjustment, as PDFium reads one.
pub fn operators() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let by_name = pdf.add_object(dictionary! { "Type" => "ExtGState", "Font" => vec![
        Object::Name(SQUARES.to_vec()), 20.into(),
    ] });
    let by_ref = pdf.add_object(dictionary! { "Type" => "ExtGState", "Font" => vec![
        squares.into(), 10.into(),
    ] });
    let resources = dictionary! {
        "Font" => dictionary! { "T3" => squares },
        "ExtGState" => dictionary! { "GS1" => by_name, "GS2" => by_ref },
    };
    let content = b"Q\n\
        BT 100 760 Td (a) Tj ET\n\
        q BT /T3 10 Tf 100 700 Td (ab) Tj ET Q\n\
        q BT /T3 10 Tf 2 Tc 100 680 Td (ab) Tj ET Q\n\
        q BT /T3 10 Tf 5 Tw 100 660 Td (a a) Tj ET Q\n\
        q BT /T3 10 Tf 50 Tz 100 640 Td [(ab) -1000 (a)] TJ ET Q\n\
        q BT /T3 10 Tf 12 TL 100 620 Td (a) Tj T* (b) Tj ET Q\n\
        q BT /T3 10 Tf 3 Ts 100 590 Td (a) Tj ET Q\n\
        q BT /T3 10 Tf 0 1 -1 0 300 500 Tm (ab) Tj ET Q\n\
        q BT /T3 10 Tf 100 560 TD (a) Tj 0 -14 TD (b) Tj T* (a) Tj ET Q\n\
        q BT /T3 10 Tf 10 TL 100 520 Td (a) ' ET Q\n\
        q BT /T3 10 Tf 10 TL 100 500 Td 4 1 (a a) \" ET Q\n\
        q BT /T3 10 Tf 100 470 Td [(a) -500 (b) 200 (a)] TJ ET Q\n\
        q 2 0 0 2 50 50 cm BT /T3 10 Tf 10 200 Td (a) Tj ET Q\n\
        q BT /T3 10 Tf 100 420 Td q /T3 20 Tf 3 Tc 50 50 Td Q (a) Tj ET Q\n\
        q /GS1 gs BT 100 400 Td (b) Tj ET Q\n\
        q BT /T3 10 Tf /GS2 gs 100 380 Td (a) Tj ET Q\n\
        q BT /Nope 12 Tf 100 360 Td (a) Tj ET Q\n\
        q BT /T3 10 Tf 100 340 Td [(a) -1000 ()] TJ (b) Tj ET Q\n\
        q BT /T3 10 Tf 100 320 Td [(a) () -500 (b) -1000 ()] TJ (b) Tj ET Q\n\
        q BT /T3 10 Tf 100 300 Td [(a) -1000] TJ (b) Tj ET Q\n";
    one_page(pdf, content, resources)
}

/// Text in each kind of font, at 10, each line a font, top down:
///
/// | font | strings | glyphs: origin x, box x | box y |
/// |---|---|---|---|
/// | `/HV` Helvetica, standard 14 ([`standard_14`]) | `(Hi)` | H 100, 100..107.22; i 107.22..109.44 | Helvetica's, at 700 |
/// | `/HB` Helvetica-Bold as MMType1, `3 Tr` | `(Hi)` | H 100..107.22; i 107.22..110 | at 680, invisible |
/// | `/CO` Courier | `(ab)` | 100..106; 106..112 | at 660 |
/// | `/TT` TrueType, `/Widths` | `(AB)` | 100..105; 105..111 | 639..649 |
/// | `/I2` Type0, Identity-H, CIDFontType2, `/W` | `<0001 0002 000A>` | 100..104; 104..109; 109..116 | 618..628 |
/// | `/I0` Type0, Identity-H, CIDFontType0 | `<0001>` | 100..104.5 | 598..608 |
/// | `/CM` Type0, its CMap embedded, `6 Tw` | `(A A)` | 100..108; space 108..111; 117..125 | 578..588 |
/// | `/T3B` Type3, a hundredth `/FontMatrix` | `(a)` | 100..110 | 558.01..567.99 |
///
/// And `/V`, Type0 Identity-V, writing down a column from 300,600:
/// `<0001 0002>`, CID 1 by `/W2` (`w1` -1000, `v` 250,880), CID 2 by
/// `/DW2` and half its width (600): origins 297.5,591.2 and 297,581.2,
/// boxes 295..305 × 590..600 and 295..305 × 580..590.
///
/// Then `/I2` again, a code cut off at a string's end: `<00010A>` is
/// codes 1 and 0x0A00, as PDFium reads it, a 0 for the byte it hasn't,
/// so `/DW`'s 1000 wide, 104..114, and `/T3B`'s `a` after it at 114,540;
/// and with `6 Tw`, `<000120>` is 1 and 0x2000, which isn't the space,
/// so the `a` after it is at 114,520 too. The `/I2` glyphs are 538..548,
/// or 518..528, high, the `a`s 114..124 × 538.01..547.99 and
/// 518.01..527.99. (PDFium's text page drops a string in the glyphless
/// font shown just after another, so `/T3B` follows.)
///
/// `/T3B`'s heights are its `/FontBBox`'s, -20 and 80, a hundredth, in
/// thousandths cut to whole ones as PDFium cuts them: -199 and 799, as
/// an `f32` has them -199.99998 and 799.99994.
///
/// The fonts over a program draw it embedded: the glyphless TrueType.
pub fn fonts() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let resources = dictionary! { "Font" => font_kinds(&mut pdf) };
    let content = b"BT /HV 10 Tf 100 700 Td (Hi) Tj ET\n\
        BT /HB 10 Tf 3 Tr 100 680 Td (Hi) Tj 0 Tr ET\n\
        BT /CO 10 Tf 100 660 Td (ab) Tj ET\n\
        BT /TT 10 Tf 100 640 Td (AB) Tj ET\n\
        BT /I2 10 Tf 100 620 Td <00010002000A> Tj ET\n\
        BT /I0 10 Tf 100 600 Td <0001> Tj ET\n\
        BT /CM 10 Tf 6 Tw 100 580 Td (A A) Tj 0 Tw ET\n\
        BT /T3B 10 Tf 100 560 Td (a) Tj ET\n\
        BT /V 10 Tf 300 600 Td <00010002> Tj ET\n\
        BT /I2 10 Tf 100 540 Td <00010A> Tj /T3B 10 Tf (a) Tj ET\n\
        BT /I2 10 Tf 6 Tw 100 520 Td <000120> Tj /T3B 10 Tf (a) Tj 0 Tw ET\n";
    one_page(pdf, content, resources)
}

/// Standard 14 font `name`, as `subtype`, in WinAnsiEncoding, its
/// `/Widths` from codes 32 to 255 Adobe's, as sheaf reads them without:
/// laid out the same wherever PDFium draws it.
pub fn standard_14(subtype: &str, name: &str) -> Dictionary {
    let mut font = dictionary! {
        "Type" => "Font", "Subtype" => subtype, "BaseFont" => name,
        "Encoding" => "WinAnsiEncoding",
    };
    let read = Foreign::read(&Pdf::new(), &font, &Limits::DEFAULT, &mut 0);
    let afm = read.expect("a font by name alone has no stream to decompress");
    let widths = (32..=255).map(|code| Object::Integer(afm.width(code) as i64));
    font.set("FirstChar", 32);
    font.set("LastChar", 255);
    font.set("Widths", widths.collect::<Vec<_>>());
    font
}

/// [`fonts`]' fonts, by the names it gives them.
fn font_kinds(pdf: &mut Pdf) -> Dictionary {
    let std14 = |pdf: &mut Pdf, subtype: &str, name: &str| -> Object {
        pdf.add_object(standard_14(subtype, name)).into()
    };
    let hv = std14(pdf, "Type1", "Helvetica");
    let hb = std14(pdf, "MMType1", "Helvetica-Bold");
    let co = std14(pdf, "Type1", "Courier");
    let program = embedded(pdf);
    let descriptor = |pdf: &mut Pdf, ascent: i64, descent: i64| -> Object {
        pdf.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => "GlyphLessFont", "Flags" => 32,
            "FontBBox" => vec![0.into(), descent.into(), 1000.into(), ascent.into()],
            "Ascent" => ascent, "Descent" => descent, "CapHeight" => ascent,
            "ItalicAngle" => 0, "StemV" => 80, "FontFile2" => program.clone(),
        })
        .into()
    };
    let tt_descriptor = descriptor(pdf, 900, -100);
    let tt = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "GlyphLessFont",
        "FirstChar" => 65, "LastChar" => 66, "Widths" => vec![500.into(), 600.into()],
        "Encoding" => "WinAnsiEncoding", "FontDescriptor" => tt_descriptor,
    });
    let cid_descriptor = descriptor(pdf, 800, -200);
    let w = |entries: Vec<Object>| Object::Array(entries);
    let type0 = |pdf: &mut Pdf, encoding: Object, cid: Dictionary| -> Object {
        let mut cid_font = dictionary! {
            "Type" => "Font", "BaseFont" => "GlyphLessFont", "FontDescriptor" => cid_descriptor.clone(),
            "CIDSystemInfo" => dictionary! {
                "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"), "Supplement" => 0,
            },
        };
        cid_font.extend(&cid);
        let descendant = pdf.add_object(cid_font);
        pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "GlyphLessFont",
            "Encoding" => encoding, "DescendantFonts" => vec![descendant.into()],
        })
        .into()
    };
    let i2 = type0(
        pdf,
        "Identity-H".into(),
        dictionary! {
            "Subtype" => "CIDFontType2",
            "W" => w(vec![1.into(), w(vec![400.into(), 500.into()]), 10.into(), 20.into(), 700.into()]),
        },
    );
    let i0 = type0(
        pdf,
        "Identity-H".into(),
        dictionary! { "Subtype" => "CIDFontType0", "W" => w(vec![1.into(), w(vec![450.into()])]) },
    );
    let cmap = pdf.add_object(Stream::new(
        dictionary! { "Type" => "CMap", "CMapName" => "OneByte" },
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
          /CMapName /OneByte def\n\
          1 begincodespacerange <00> <FF> endcodespacerange\n\
          1 begincidrange <20> <7E> 1 endcidrange\n\
          endcmap CMapName currentdict /CMap defineresource pop end end"
            .to_vec(),
    ));
    let cm = type0(
        pdf,
        cmap.into(),
        dictionary! {
            "Subtype" => "CIDFontType2",
            "W" => w(vec![1.into(), w(vec![300.into()]), 34.into(), w(vec![800.into()])]),
        },
    );
    let v = type0(
        pdf,
        "Identity-V".into(),
        dictionary! {
            "Subtype" => "CIDFontType2",
            "W" => w(vec![1.into(), w(vec![500.into(), 600.into()])]),
            "W2" => w(vec![1.into(), w(vec![(-1000).into(), 250.into(), 880.into()])]),
            "DW2" => w(vec![880.into(), (-1000).into()]),
        },
    );
    let t3b = squares(pdf, 100.0);
    dictionary! {
        "HV" => hv, "HB" => hb, "CO" => co, "TT" => tt, "I2" => i2, "I0" => i0,
        "CM" => cm, "V" => v, "T3B" => t3b,
    }
}

/// A form in a form, the inner one drawn twice. The page draws `/Fm1`
/// at 100,300 in [`SQUARES`] at 10 with `50 Tz`; a form starts with the
/// font, its size, `Tc`, `Tw` and `Tr` it's drawn with, but `Tz`, `TL`
/// and `Ts` as before any operator, as PDFium starts it.
/// - `/Fm1`: `/Matrix [1 0 0 1 10 20]`, `/BBox [0 0 200 100]`, its own
///   resources; it shows `a` at 5,5, then draws `/Fm2` as it is and moved
///   50 along. `a`: origin 115,325, box 115..125 × 323..333.
/// - `/Fm2`: `/Matrix [2 0 0 2 0 0]`, `/BBox [0 0 50 50]`, no resources
///   of its own, so `/Fm1`'s; it shows `b` at 1,1. `b`s: origins 112,322
///   and 162,322, boxes 112..122 and 162..172 × 318..338.
pub fn forms() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let resources = dictionary! {
        "Font" => dictionary! { "T3" => squares },
        "XObject" => dictionary! { "Fm1" => nested(&mut pdf, squares) },
    };
    one_page(
        pdf,
        b"q 1 0 0 1 100 300 cm 50 Tz /T3 10 Tf /Fm1 Do Q",
        resources,
    )
}

/// [`forms`]' `/Fm1`, its font `squares`.
fn nested(pdf: &mut Pdf, squares: ObjectId) -> ObjectId {
    let inner = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Matrix" => numbers(&[2.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
            "BBox" => numbers(&[0.0, 0.0, 50.0, 50.0]),
        },
        b"BT 1 1 Td (b) Tj ET".to_vec(),
    ));
    pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Matrix" => numbers(&[1.0, 0.0, 0.0, 1.0, 10.0, 20.0]),
            "BBox" => numbers(&[0.0, 0.0, 200.0, 100.0]),
            "Resources" => dictionary! {
                "Font" => dictionary! { "T3" => squares },
                "XObject" => dictionary! { "Fm2" => inner },
            },
        },
        b"BT 5 5 Td (a) Tj ET q /Fm2 Do Q q 1 0 0 1 50 0 cm /Fm2 Do Q".to_vec(),
    ))
}

/// Forms drawn within themselves: `/Self` shows `a` at 100,700 and draws
/// itself; `/A` draws `/B`, which shows `b` at 100,650 and draws `/A`.
/// After both the page shows `a` at 100,600. Each form's one use is
/// drawn and its drawing itself refused: three glyphs.
pub fn cycles() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let own = pdf.new_object_id();
    let a = pdf.new_object_id();
    let b = pdf.new_object_id();
    let draws = [
        (
            own,
            &b"BT /T3 10 Tf 100 700 Td (a) Tj ET /Self Do"[..],
            "Self",
            own,
        ),
        (a, b"/B Do", "B", b),
        (b, b"BT /T3 10 Tf 100 650 Td (b) Tj ET /A Do", "A", a),
    ];
    for (id, content, name, draws) in draws {
        let xobjects = dictionary! { name => draws };
        pdf.objects
            .insert(id, form(squares, content, xobjects).into());
    }
    let resources = dictionary! {
        "Font" => dictionary! { "T3" => squares },
        "XObject" => dictionary! { "Self" => own, "A" => a },
    };
    let content = b"/Self Do /A Do BT /T3 10 Tf 100 600 Td (a) Tj ET";
    one_page(pdf, content, resources)
}

/// `n` forms, each drawing the next, from the page's; the last shows `a`
/// at 100,700.
pub fn chain(n: usize) -> Fixture {
    fan_out(n, 1)
}

/// `levels` forms, each drawing the next `uses` times, from the page's;
/// the last shows `a` at 100,700. Drawn in full, it's `uses` to the
/// `levels` uses.
pub fn fan_out(levels: usize, uses: usize) -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let mut next = None;
    for _ in 0..levels {
        let drawn = match next {
            None => form(
                squares,
                b"BT /T3 10 Tf 100 700 Td (a) Tj ET",
                dictionary! {},
            ),
            Some(id) => form(squares, &b"/F Do ".repeat(uses), dictionary! { "F" => id }),
        };
        next = Some(pdf.add_object(drawn));
    }
    let xobjects = next.map_or_else(Dictionary::new, |id| dictionary! { "F" => id });
    one_page(pdf, b"/F Do", dictionary! { "XObject" => xobjects })
}

/// Images of each kind, each the unit square through its CTM, in order:
///
/// | image | how | quad: bottom-left, bottom-right, top-right, top-left |
/// |---|---|---|
/// | inline, unfiltered | `20 0 0 10 100 700 cm` | 100,700; 120,700; 120,710; 100,710 |
/// | inline, `/AHx` | at 130,700 | 130..150 × 700..710 |
/// | inline, `/A85` | at 160,700 | 160..180 |
/// | inline, `/RL` | at 190,700 | 190..210 |
/// | inline, `/DCT`, a JPEG | at 220,700 | 220..240 |
/// | inline, `/Fl` with `/L` | at 250,700 | 250..270 |
/// | inline, `/Fl`, its end found by `EI` | at 280,700 | 280..300 |
/// | inline, `/IM true`, a stencil | `0 10 -20 0 400 700 cm` | 400,700; 400,710; 380,710; 380,700 |
/// | `/Im`, its `/SMask` and `/OC` | `50 0 0 50 100 500 cm` | 100..150 × 500..550 |
/// | `/St`, `/ImageMask true` | at 200,500 | 200..250 |
/// | `/Mk`, a stencil `/Mask` | at 300,500 | 300..350 |
/// | `/Ck`, a color key `/Mask` | at 400,500 | 400..450 |
/// | `/Im` in `/Fm` | `40 0 10 40 0 0 cm` in `/Fm` at 310,400 | 310,400; 350,400; 360,440; 320,440 |
///
/// Then `/PS Do`, a PostScript XObject, which PDFium doesn't draw. The
/// inline images are 2 × 2, gray but the JPEG, and every one's data is
/// whole, so PDFium reads each.
pub fn images() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let gray = |w: i64| {
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => 2,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
        }
    };
    let stencil = || {
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 2,
            "ImageMask" => true,
        }
    };
    let samples = vec![0, 255, 0, 255];
    let smask = pdf.add_object(Stream::new(gray(2), vec![255, 128, 64, 0]));
    let ocg = pdf.add_object(dictionary! {
        "Type" => "OCG", "Name" => Object::string_literal("Layer"),
    });
    let mut im = gray(2);
    im.set("SMask", smask);
    im.set("OC", ocg);
    let im = pdf.add_object(Stream::new(im, samples.clone()));
    let st = pdf.add_object(Stream::new(stencil(), vec![0x40, 0x80]));
    let mask = pdf.add_object(Stream::new(stencil(), vec![0x40, 0x80]));
    let mut mk = gray(2);
    mk.set("Mask", mask);
    let mk = pdf.add_object(Stream::new(mk, samples.clone()));
    let mut ck = gray(2);
    ck.set("Mask", vec![0.into(), 0.into()]);
    let ck = pdf.add_object(Stream::new(ck, samples.clone()));
    let fm = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Matrix" => numbers(&[1.0, 0.0, 0.0, 1.0, 10.0, 0.0]),
            "BBox" => numbers(&[0.0, 0.0, 100.0, 100.0]),
        },
        b"q 40 0 10 40 0 0 cm /Im Do Q".to_vec(),
    ));
    let ps = pdf.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "PS" },
        b"% PostScript, which PDFium doesn't draw".to_vec(),
    ));
    let flate = {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&samples).expect("compressing in memory");
        z.finish().expect("compressing in memory")
    };
    let jpeg = sheaf_image::encode::jpeg(&RgbImage::from_pixel(2, 2, Rgb([200, 50, 50])), 90, 72)
        .expect("a JPEG of 2 × 2");
    let dict = |more: &str| format!("/W 2 /H 2 /BPC 8 {more}");
    let inline: [(String, Vec<u8>); 7] = [
        (dict("/CS /G"), samples.clone()),
        (dict("/CS /G /F /AHx"), b"00FF00FF>".to_vec()),
        (dict("/CS /G /F /A85"), b"z~>".to_vec()),
        (dict("/CS /G /F /RL"), b"\x03\x00\xff\x00\xff\x80".to_vec()),
        (dict("/CS /RGB /F /DCT"), jpeg),
        (
            dict(&format!("/CS /G /F /Fl /L {}", flate.len())),
            flate.clone(),
        ),
        (dict("/CS /G /F /Fl"), flate),
    ];
    let mut content = Vec::new();
    for (i, (dict, data)) in inline.into_iter().enumerate() {
        let x = 100 + 30 * i;
        content.extend(format!("q 20 0 0 10 {x} 700 cm BI {dict} ID ").into_bytes());
        content.extend(data);
        content.extend(b" EI Q\n");
    }
    content.extend(b"q 0 10 -20 0 400 700 cm BI /IM true /W 2 /H 2 ID \x40\x80 EI Q\n");
    content.extend(
        b"q 50 0 0 50 100 500 cm /Im Do Q\n\
          q 50 0 0 50 200 500 cm /St Do Q\n\
          q 50 0 0 50 300 500 cm /Mk Do Q\n\
          q 50 0 0 50 400 500 cm /Ck Do Q\n\
          q 1 0 0 1 300 400 cm /Fm Do Q\n\
          /PS Do\n",
    );
    let resources = dictionary! { "XObject" => dictionary! {
        "Im" => im, "St" => st, "Mk" => mk, "Ck" => ck, "Fm" => fm, "PS" => ps,
    } };
    one_page(pdf, &content, resources)
}

/// Paths painted, clips and shadings, in order. Paths, their paint and
/// bounds:
///
/// | path | paint | bounds |
/// |---|---|---|
/// | `2 w 100 700 m 200 700 l` | `S` | 1 past: 99..201 × 699..701 |
/// | `100 600 50 50 re` | `f` | 100..150 × 600..650 |
/// | `4 w 2 J 300 700 m 300 750 l`, square caps | `S` | 2√2 past: 297.172..302.828 × 697.172..752.828 |
/// | `2 w 400 600 m 450 650 l 500 600 l`, a miter join | `S` | 10 past: 390..510 × 590..660 |
/// | `1 J 1 j 5 w 50 50 m h`, a round dot | `S` | 2.5 past: 47.5..52.5 × 47.5..52.5 |
/// | `100 100 m 100 200 200 200 200 100 c`, a curve | `S` | 5 past: 95..205 × 95..205 |
///
/// A stroke reaches half its width past its points, all round; √2 times
/// that at a square cap; and the miter limit, 10, times it at a miter
/// join, which a curve has.
///
/// Then the clips, each tried by a line `0 0 m 612 792 l S` drawn in it,
/// its clip's bounds:
/// - `100 400 200 100 re W n`, a path clip: 100..300 × 400..500; in it
///   `/Sh sh`, an axial shading whose `/BBox` is 0..150 × 0..450, painting
///   100..150 × 400..450; and within it the triangle `150 420 m 250 420 l
///   200 480 l h W* n`, an even-odd clip: 150..250 × 420..480.
/// - `Helvetica 10 Tf 7 Tr 100 300 Td (ab) Tj ET`, a text clip, by
///   Helvetica's widths ([`standard_14`]) and heights: 100..111.12 ×
///   297.93..307.18.
/// - The same in [`SQUARES`] clips nothing: PDFium only fills Type3 text.
/// - `10 10 m W n`, a path of one point, clips everything away: 0..0 ×
///   0..0; `/Sh2 sh` in it, a shading with no `/BBox`, paints there.
/// - `/Fm`, `/Matrix [2 0 0 2 0 0]` and `/BBox [0 0 100 50]`, drawn at
///   400,100, clips to 400..600 × 100..200.
///
/// Last, `/Sh2 sh` with no clip paints all the page shows: no bounds.
pub fn clips() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let helvetica = pdf.add_object(standard_14("Type1", "Helvetica"));
    let function = dictionary! {
        "FunctionType" => 2, "Domain" => numbers(&[0.0, 1.0]),
        "C0" => numbers(&[0.0]), "C1" => numbers(&[1.0]), "N" => 1,
    };
    let axial = |bbox: Option<[f64; 4]>| {
        let mut shading = dictionary! {
            "ShadingType" => 2, "ColorSpace" => "DeviceGray",
            "Coords" => numbers(&[100.0, 400.0, 300.0, 400.0]), "Function" => function.clone(),
        };
        if let Some(bbox) = bbox {
            shading.set("BBox", numbers(&bbox));
        }
        shading
    };
    let sh = pdf.add_object(axial(Some([0.0, 0.0, 150.0, 450.0])));
    let sh2 = pdf.add_object(axial(None));
    let fm = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Matrix" => numbers(&[2.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
            "BBox" => numbers(&[0.0, 0.0, 100.0, 50.0]),
        },
        b"0 0 m 612 792 l S".to_vec(),
    ));
    let resources = dictionary! {
        "Font" => dictionary! { "T3" => squares, "HV" => helvetica },
        "Shading" => dictionary! { "Sh" => sh, "Sh2" => sh2 },
        "XObject" => dictionary! { "Fm" => fm },
    };
    let content = b"q 2 w 100 700 m 200 700 l S Q\n\
        q 100 600 50 50 re f Q\n\
        q 4 w 2 J 300 700 m 300 750 l S Q\n\
        q 2 w 400 600 m 450 650 l 500 600 l S Q\n\
        q 1 J 1 j 5 w 50 50 m h S Q\n\
        q 100 100 m 100 200 200 200 200 100 c S Q\n\
        q 100 400 200 100 re W n /Sh sh\n\
          q 150 420 m 250 420 l 200 480 l h W* n 0 0 m 612 792 l S Q\n\
          0 0 m 612 792 l S\n\
        Q\n\
        q BT /HV 10 Tf 7 Tr 100 300 Td (ab) Tj ET 0 0 m 612 792 l S Q\n\
        q BT /T3 10 Tf 7 Tr 100 250 Td (ab) Tj ET 0 0 m 612 792 l S Q\n\
        q 10 10 m W n /Sh2 sh Q\n\
        q 1 0 0 1 400 100 cm /Fm Do Q\n\
        /Sh2 sh\n";
    one_page(pdf, content, resources)
}

/// Marked content, its ranges as PDFium reads them, each opened and
/// ended in order, and what each glyph, in [`SQUARES`], is marked by,
/// the innermost first:
///
/// | content | events |
/// |---|---|
/// | `EMC` | none: it ends nothing |
/// | `/P << /MCID 0 >> BDC` | P opens, MCID 0 |
/// | `(a)` at 100,700 | a, in P |
/// | `/Span << /ActualText <FEFF0041> >> BDC` | Span opens, its text `A` |
/// | `(b)` at 100,680, `EMC EMC` | b, in Span and P; Span ends, then P |
/// | `/OC /MC0 BDC` | OC opens, the `/Properties` entry `/MC0` by reference |
/// | `(a)` at 100,660 | a, in OC |
/// | `/Fm Do`, its `/OC` | the form, in OC: |
/// | … `EMC` | none: it can't end the range its `Do` is in |
/// | … `/Span << /ActualText (inside) /MCID 7 >> BDC (a) EMC` | Span opens in the form, MCID 7; a at 100,200, in Span and OC; Span ends |
/// | … `/Open BMC` | Open opens, and ends where the form's content does |
/// | `EMC` | OC ends |
/// | `/OC /Missing BDC` | none: its name isn't in `/Properties` |
/// | `/Artifact BMC (b) EMC EMC` | Artifact opens; b at 100,640, in it; Artifact ends; the last `EMC` ends nothing |
/// | `/Tag MP`, `/Tag /P1 DP` | two points, the second MCID 5 by `/P1` |
/// | `/Open BMC (a)` | Open opens; a at 100,600, in it; Open ends where the content does |
pub fn marked() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let ocg = |pdf: &mut Pdf, name: &str| {
        pdf.add_object(dictionary! { "Type" => "OCG", "Name" => Object::string_literal(name) })
    };
    let layer = ocg(&mut pdf, "Layer");
    let form_layer = ocg(&mut pdf, "Form layer");
    let fm = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "OC" => form_layer,
            "BBox" => numbers(&[0.0, 0.0, 612.0, 792.0]),
        },
        b"EMC /Span << /ActualText (inside) /MCID 7 >> BDC \
          BT /T3 10 Tf 100 200 Td (a) Tj ET EMC /Open BMC"
            .to_vec(),
    ));
    let resources = dictionary! {
        "Font" => dictionary! { "T3" => squares },
        "XObject" => dictionary! { "Fm" => fm },
        "Properties" => dictionary! { "MC0" => layer, "P1" => dictionary! { "MCID" => 5 } },
    };
    let content = b"EMC\n\
        /P << /MCID 0 >> BDC BT /T3 10 Tf 100 700 Td (a) Tj ET\n\
          /Span << /ActualText <FEFF0041> >> BDC BT /T3 10 Tf 100 680 Td (b) Tj ET EMC\n\
        EMC\n\
        /OC /MC0 BDC BT /T3 10 Tf 100 660 Td (a) Tj ET /Fm Do EMC\n\
        /OC /Missing BDC /Artifact BMC BT /T3 10 Tf 100 640 Td (b) Tj ET EMC EMC\n\
        /Tag MP /Tag /P1 DP\n\
        /Open BMC BT /T3 10 Tf 100 600 Td (a) Tj ET\n";
    one_page(pdf, content, resources)
}

/// A form whose own resources have only `/XObject`, so what else it
/// draws with is the page's, as PDFium finds it (`FindResourceHolder`):
/// `/Fm`, drawn as it is, opens a range by the page's `/Properties` entry
/// `/P0` (MCID 3), shows `a` in the page's [`SQUARES`] at 100,700, box
/// 100..110 × 698..708, and draws an inline image in the page's color
/// space `/CS3`, DeviceRGB, at 100,600, 20 by 10: its 12 bytes of data
/// are found by their length, which the color space gives, as they hold
/// ` EI ` where a reader that doesn't know it would end them.
pub fn inherited() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let mut content = b"/Span /P0 BDC BT /T3 10 Tf 100 700 Td (a) Tj ET EMC\n\
        q 20 0 0 10 100 600 cm BI /W 2 /H 2 /BPC 8 /CS /CS3 ID "
        .to_vec();
    content.extend(b"\0 EI \0\0\0\0\0\0\0");
    content.extend(b" EI Q");
    let fm = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => numbers(&[0.0, 0.0, 612.0, 792.0]),
            "Resources" => dictionary! { "XObject" => dictionary! {} },
        },
        content,
    ));
    let resources = dictionary! {
        "Font" => dictionary! { "T3" => squares },
        "ColorSpace" => dictionary! { "CS3" => "DeviceRGB" },
        "Properties" => dictionary! { "P0" => dictionary! { "MCID" => 3 } },
        "XObject" => dictionary! { "Fm" => fm },
    };
    one_page(pdf, b"/Fm Do", resources)
}

/// Each fixture, by name, as the content round trip and the fuzz seeds
/// take them, with forms nested `form_depth` deep, one deeper, and
/// fanned out.
pub fn every() -> Vec<(&'static str, Fixture)> {
    let depth = Limits::DEFAULT.content.form_depth;
    vec![
        ("operators", operators()),
        ("fonts", fonts()),
        ("forms", forms()),
        ("cycles", cycles()),
        ("chain at form_depth", chain(depth)),
        ("chain past form_depth", chain(depth + 1)),
        ("fan out", fan_out(3, 3)),
        ("images", images()),
        ("clips", clips()),
        ("marked", marked()),
        ("inherited", inherited()),
    ]
}

/// A page that draws nothing, whose resources name what the `interp`
/// fuzz target's content draws with: a font of each kind, [`fonts`]'
/// and [`SQUARES`], and `/F1`, Helvetica; `/CS1`, `/CS3` and `/CS4`, a
/// color space of each size; `/P0`, a property list with `/ActualText`
/// and `/MCID`, and `/OC0`, optional content; `/GS1`, an ExtGState with a
/// `/Font`; and forms: `/X0`, which draws itself, `/X1`, which draws
/// `/X2`, and [`forms`]' `/Fm1`, which draws `/Fm2` twice.
pub fn fuzz_page() -> Fixture {
    let mut pdf = Pdf::with_version("1.7");
    let squares = squares(&mut pdf, 1000.0);
    let mut fonts = font_kinds(&mut pdf);
    fonts.set("T3", squares);
    let helvetica = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    fonts.set("F1", helvetica);
    let own = pdf.new_object_id();
    let draws_itself = b"BT /T3 10 Tf 100 700 Td (a) Tj ET /X0 Do";
    let own_form = form(squares, draws_itself, dictionary! { "X0" => own });
    pdf.objects.insert(own, own_form.into());
    let last = form(
        squares,
        b"BT /T3 10 Tf 100 650 Td (b) Tj ET",
        dictionary! {},
    );
    let last = pdf.add_object(last);
    let chain = form(
        squares,
        b"q 1 0 0 1 0 -50 cm /X2 Do Q",
        dictionary! { "X2" => last },
    );
    let chain = pdf.add_object(chain);
    let layer = pdf.add_object(dictionary! {
        "Type" => "OCG", "Name" => Object::string_literal("Layer"),
    });
    let resources = dictionary! {
        "Font" => fonts,
        "ColorSpace" => dictionary! {
            "CS1" => "DeviceGray", "CS3" => "DeviceRGB", "CS4" => "DeviceCMYK",
        },
        "Properties" => dictionary! {
            "P0" => dictionary! { "ActualText" => Object::string_literal("a"), "MCID" => 0 },
            "OC0" => layer,
        },
        "ExtGState" => dictionary! { "GS1" => dictionary! {
            "Type" => "ExtGState", "LW" => 2, "Font" => vec![Object::Name(SQUARES.to_vec()), 20.into()],
        } },
        "XObject" => dictionary! { "X0" => own, "X1" => chain, "Fm1" => nested(&mut pdf, squares) },
    };
    one_page(pdf, b"", resources)
}

/// A form the size of the page drawing `content` in [`SQUARES`], its
/// font `squares`, and `xobjects`.
fn form(squares: ObjectId, content: &[u8], xobjects: Dictionary) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => numbers(&[0.0, 0.0, 612.0, 792.0]),
            "Resources" => dictionary! {
                "Font" => dictionary! { "T3" => squares }, "XObject" => xobjects,
            },
        },
        content.to_vec(),
    )
}

/// The Type3 font [`SQUARES`] names, its glyph space `units` an em: its
/// glyphs filled boxes, each its `d1`'s, but the space, `d0` and empty.
fn squares(pdf: &mut Pdf, units: f64) -> ObjectId {
    let u = |n: f64| n * units / 1000.0;
    let glyph = |pdf: &mut Pdf, width: f64| -> Object {
        let (w, top, bottom) = (u(width), u(800.0), u(-200.0));
        let content = format!("{w} 0 0 {bottom} {w} {top} d1 0 0 {w} {top} re f");
        pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()))
            .into()
    };
    let a = glyph(pdf, 1000.0);
    let b = glyph(pdf, 500.0);
    let space = pdf.add_object(Stream::new(
        dictionary! {},
        format!("{} 0 d0", u(250.0)).into_bytes(),
    ));
    // Codes 32 to 98: the space, nothing, `a` and `b`.
    let mut widths = vec![Object::Real(u(250.0)); 1];
    widths.extend(std::iter::repeat_n(Object::Integer(0), 97 - 33));
    widths.extend([Object::Real(u(1000.0)), Object::Real(u(500.0))]);
    pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type3",
        "FontMatrix" => numbers(&[1.0 / units, 0.0, 0.0, 1.0 / units, 0.0, 0.0]),
        "FontBBox" => numbers(&[0.0, u(-200.0), u(1000.0), u(800.0)]),
        "FirstChar" => 32, "LastChar" => 98, "Widths" => widths,
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "Differences" => vec![32.into(), "space".into(), 97.into(), "a".into(), "b".into()],
        },
        "CharProcs" => dictionary! { "space" => space, "a" => a, "b" => b },
        "Resources" => dictionary! {},
    })
}

/// The glyphless TrueType, as a font file.
fn embedded(pdf: &mut Pdf) -> Object {
    let program = Glyphless::PROGRAM;
    pdf.add_object(Stream::new(
        dictionary! { "Length1" => program.len() as i64 },
        program.to_vec(),
    ))
    .into()
}

fn numbers(n: &[f64]) -> Object {
    Object::Array(n.iter().map(|&n| Object::Real(n)).collect())
}

/// `pdf` with one Letter page drawing `content` with `resources`.
fn one_page(mut pdf: Pdf, content: &[u8], resources: Dictionary) -> Fixture {
    let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content, "Resources" => resources,
    });
    pdf.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    Fixture { pdf }
}
