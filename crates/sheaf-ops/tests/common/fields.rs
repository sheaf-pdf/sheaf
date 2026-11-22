//! Text fields drawn by sheaf and by PDFium, for the pixel checks
//! (`fields.rs`) and the other renderers (`interop.rs`): each [`Case`] a
//! form built in code, one text field on a page of its own, a square
//! [`SIDE`] points on a side, upright or turned (`/Rotate 90`), its widget
//! turned by `/MK /R 90` or not ([`Place`]).
//!
//! Ours ([`ours`]) is the case's file with no `/V` and no appearance, the
//! field filled through the model (`tx::fixture::fill`, which no op does
//! until M10 slice 9) and saved as a Rewrite: the widget's appearance is
//! the save's own, made by pdf-forms. PDFium's ([`theirs`]) is the same
//! file with the value as `/V` and no appearance, in an engine whose
//! widgets are drawn once (`FPDF_FFLDraw`, under the form-fill environment
//! every load sets up), which makes the appearance each widget lacks, and
//! then saved by PDFium (`FPDF_SaveAsCopy`): the copy's appearance is
//! PDFium's own. Both are drawn appearances only ([`draw`]) at [`SCALE`]
//! pixels a point, and compared inside the widget's box ([`Case::pixels`]).

use std::ops::ControlFlow;

use sheaf_doc::Document;
use sheaf_doc::content::interp::{Forms, Kind, Opts, Wants, interpret};
use sheaf_doc::content::{Concat, Fonts};
use sheaf_doc::forms::fixture::{Form, Rect};
use sheaf_doc::pdf::{Dictionary, Document as Pdf, Object, StreamExt, dictionary, literal};
use sheaf_doc::tx::fixture as fx;
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Draw, Job, PageIndex};

use super::opened;

/// A page's side, in points.
pub const SIDE: f32 = 300.0;
/// Pixels a point.
pub const SCALE: u32 = 2;
/// A page's side, in pixels.
pub const SIDE_PX: u32 = SIDE as u32 * SCALE;
/// Where a widget's box starts on its page, in points from the left and
/// the bottom.
const ORIGIN: f32 = 50.0;
/// The field's name.
pub const NAME: &str = "field";

/// `/Ff` bits.
pub const MULTILINE: i64 = 1 << 12;
pub const COMB: i64 = 1 << 24;

/// Where a case's widget is: on an upright page or a `/Rotate 90` one,
/// turned by `/MK /R 90` or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Upright,
    /// Upright, the widget turned by `/MK /R 90`: its text runs up the
    /// page.
    UprightR90,
    /// On a `/Rotate 90` page: its text runs down the page as shown.
    Turned,
    /// On a `/Rotate 90` page, the widget turned by `/MK /R 90`: its text
    /// upright as the page is shown, as a viewer turns a field on a turned
    /// page.
    TurnedR90,
}

impl Place {
    pub const ALL: [Place; 4] = [
        Place::Upright,
        Place::UprightR90,
        Place::Turned,
        Place::TurnedR90,
    ];

    fn page_rotate(self) -> i64 {
        match self {
            Place::Turned | Place::TurnedR90 => 90,
            Place::Upright | Place::UprightR90 => 0,
        }
    }

    fn mk_r(self) -> Option<i64> {
        matches!(self, Place::UprightR90 | Place::TurnedR90).then_some(90)
    }

    pub fn label(self) -> &'static str {
        match self {
            Place::Upright => "upright",
            Place::UprightR90 => "upright /R 90",
            Place::Turned => "/Rotate 90",
            Place::TurnedR90 => "/Rotate 90 /R 90",
        }
    }
}

/// One text field: its value, `/DA`, `/Q`, `/Ff`, `/MaxLen`, box, `/MK`
/// colors, `/BS`, and the fonts its form's `/DR` has besides the
/// fixture's Helvetica (`/Helv`), where it is.
#[derive(Clone)]
pub struct Case {
    pub name: String,
    pub value: String,
    pub da: String,
    pub q: Option<i64>,
    pub flags: i64,
    pub max_len: Option<i64>,
    /// The box's width and height as its text runs: the widget's `/Rect`
    /// is turned with it where `/MK /R` is 90.
    pub size: [f32; 2],
    pub place: Place,
    pub bg: Option<Vec<f32>>,
    pub bc: Option<Vec<f32>>,
    /// `/BS`: its width, its style, and its dashes (`/D`) where any.
    pub bs: Option<(i64, &'static str, Vec<f32>)>,
    /// `/DR /Font` entries the form gains, by name (in place of the
    /// fixture's `/Helv`, where that's the name).
    pub fonts: Vec<(&'static str, Dictionary)>,
}

impl Case {
    /// A single-line field showing `value` upright, 200 by 30, in `/DR`'s
    /// Helvetica at 12 points, black, left-aligned, with no `/MK` or `/BS`.
    pub fn new(name: &str, value: &str) -> Case {
        Case {
            name: name.into(),
            value: value.into(),
            da: "/Helv 12 Tf 0 g".into(),
            q: None,
            flags: 0,
            max_len: None,
            size: [200.0, 30.0],
            place: Place::Upright,
            bg: None,
            bc: None,
            bs: None,
            fonts: Vec::new(),
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
    pub fn rect(&self) -> Rect {
        let [w, h] = match self.place.mk_r() {
            Some(_) => [self.size[1], self.size[0]],
            None => self.size,
        };
        [ORIGIN, ORIGIN, ORIGIN + w, ORIGIN + h]
    }

    /// The widget's box in the page drawn at [`SCALE`]: its left, top,
    /// right and bottom pixels, the last two past it. A `/Rotate 90` page
    /// is shown turned a quarter clockwise, so its user space's y runs
    /// right and its x down.
    pub fn pixels(&self) -> [usize; 4] {
        let [x0, y0, x1, y1] = self.rect();
        let shown = match self.place.page_rotate() {
            0 => [x0, SIDE - y1, x1, SIDE - y0],
            _ => [y0, x0, y1, x1],
        };
        shown.map(|v| (v * SCALE as f32).round() as usize)
    }

    /// The case's file, with its value as `/V` where `with_value`, and no
    /// appearance.
    pub fn file(&self, with_value: bool) -> Vec<u8> {
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
        for (name, font) in &self.fonts {
            f.dr_font(name, font.clone());
        }
        let id = f.text(NAME, 0, self.rect());
        let numbers = |v: &[f32]| v.iter().map(|&n| Object::from(n)).collect::<Vec<_>>();
        let d = f.dict(id);
        d.set("DA", literal(self.da.as_str()));
        if let Some(q) = self.q {
            d.set("Q", q);
        }
        if self.flags != 0 {
            d.set("Ff", self.flags);
        }
        if let Some(n) = self.max_len {
            d.set("MaxLen", n);
        }
        let mut mk = Dictionary::new();
        if let Some(bg) = &self.bg {
            mk.set("BG", numbers(bg));
        }
        if let Some(bc) = &self.bc {
            mk.set("BC", numbers(bc));
        }
        if let Some(r) = self.place.mk_r() {
            mk.set("R", r);
        }
        if !mk.is_empty() {
            d.set("MK", mk);
        }
        if let Some((w, style, dash)) = &self.bs {
            let mut bs = dictionary! { "W" => *w, "S" => Object::name(*style) };
            if !dash.is_empty() {
                bs.set("D", numbers(dash));
            }
            d.set("BS", bs);
        }
        if with_value {
            d.set("V", Object::text(&self.value));
        }
        f.bytes()
    }
}

/// A case, its twin (ours with one thing wrong, which must draw past the
/// tolerance of PDFium's case), and what the twin has wrong.
pub type Twinned = (Case, Case, &'static str);

/// Left, centered and right, at each place: the twin aligned otherwise.
pub fn alignments() -> Vec<Twinned> {
    let mut cases = Vec::new();
    for place in Place::ALL {
        for (name, q, other) in [("left", 0, 1), ("centred", 1, 2), ("right", 2, 0)] {
            let case = Case {
                q: Some(q),
                ..Case::new(name, "Hello world")
            }
            .at(place);
            let twin = Case {
                q: Some(other),
                ..case.clone()
            };
            cases.push((case, twin, "aligned otherwise"));
        }
    }
    cases
}

/// A comb, left, right-aligned, and with a border that draws its
/// dividers, at each place: the twin no comb, or aligned left.
pub fn combs() -> Vec<Twinned> {
    let mut cases = Vec::new();
    for place in Place::ALL {
        let comb = Case {
            flags: COMB,
            max_len: Some(10),
            ..Case::new("comb", "1234567")
        }
        .at(place);
        let right = Case {
            name: "comb right".into(),
            q: Some(2),
            ..comb.clone()
        };
        let divided = Case {
            name: "comb divided".into(),
            bs: Some((1, "S", Vec::new())),
            bc: Some(vec![0.0, 0.0, 1.0]),
            ..comb.clone()
        };
        let no_comb = |c: &Case| Case {
            flags: 0,
            ..c.clone()
        };
        let left = Case {
            q: Some(0),
            ..right.clone()
        };
        cases.push((comb.clone(), no_comb(&comb), "no comb"));
        cases.push((right, left, "aligned left"));
        cases.push((divided.clone(), no_comb(&divided), "no comb"));
    }
    cases
}

/// A multiline field with a line that wraps and a hard break, at each
/// place: the twin's value one character other.
pub fn multiline() -> Vec<Twinned> {
    (Place::ALL.into_iter())
        .map(|place| {
            let case = Case {
                flags: MULTILINE,
                size: [200.0, 80.0],
                da: "/Helv 10 Tf 0 g".into(),
                ..Case::new(
                    "multiline",
                    "A first line long enough to wrap past the edge of its box\n\
                     then a hard break and an end",
                )
            }
            .at(place);
            let twin = Case {
                value: one_other(&case.value),
                ..case.clone()
            };
            (case, twin, "a character other")
        })
        .collect()
}

/// An automatic size, single line and multiline, at each place: the twin
/// at a size of its own.
pub fn auto_sizes() -> Vec<Twinned> {
    let mut cases = Vec::new();
    for place in Place::ALL {
        let single = Case {
            da: "/Helv 0 Tf 0 g".into(),
            ..Case::new("auto", "Hello world")
        }
        .at(place);
        let multiline = Case {
            name: "auto multiline".into(),
            flags: MULTILINE,
            size: [200.0, 80.0],
            value: "Words enough to wrap in a box of this size at its own size".into(),
            ..single.clone()
        };
        for case in [single, multiline] {
            let twin = Case {
                da: "/Helv 7 Tf 0 g".into(),
                ..case.clone()
            };
            cases.push((case, twin, "at 7 points"));
        }
    }
    cases
}

/// Each border style in RGB, and backgrounds in gray, RGB (beveled over
/// it, whose bands are its shades) and CMYK (a CMYK border round it),
/// upright: the twin's value one character other.
pub fn borders() -> Vec<Twinned> {
    let blue = Some(vec![0.0, 0.0, 1.0]);
    let mut cases = Vec::new();
    for (name, w, style) in [
        ("solid", 1, "S"),
        ("dashed", 1, "D"),
        ("beveled", 2, "B"),
        ("inset", 2, "I"),
        ("underline", 1, "U"),
    ] {
        cases.push(Case {
            bs: Some((w, style, Vec::new())),
            bc: blue.clone(),
            ..Case::new(name, "Hello world")
        });
    }
    cases.push(Case {
        bg: Some(vec![0.8]),
        ..Case::new("background gray", "Hello world")
    });
    cases.push(Case {
        bs: Some((2, "B", Vec::new())),
        bg: Some(vec![1.0, 1.0, 0.6]),
        bc: blue,
        ..Case::new("beveled on RGB", "Hello world")
    });
    cases.push(Case {
        bs: Some((1, "S", Vec::new())),
        bg: Some(vec![0.1, 0.2, 0.3, 0.0]),
        bc: Some(vec![0.0, 0.0, 0.0, 1.0]),
        ..Case::new("background CMYK", "Hello world")
    });
    (cases.into_iter())
        .map(|case| {
            let twin = Case {
                value: one_other(&case.value),
                ..case.clone()
            };
            (case, twin, "a character other")
        })
        .collect()
}

/// The IRS's navy, `0 0 0.502 rg`: the twin black.
pub fn navy() -> Twinned {
    let case = Case {
        da: "/Helv 12 Tf 0 0 0.502 rg".into(),
        ..Case::new("navy", "Hello world")
    };
    let twin = Case {
        da: "/Helv 12 Tf 0 g".into(),
        ..case.clone()
    };
    (case, twin, "black")
}

/// One of pdf-forms' stated differences from PDFium: ours of `case`
/// differs from PDFium's of it, and ours of `ours_of` is PDFium's of
/// `theirs_of`, which says the difference is that alone.
pub struct Stated {
    pub case: Case,
    pub ours_of: Case,
    pub theirs_of: Case,
    pub what: String,
}

/// A `/DA` font `/DR` lacks falls back to the standard 14 family its
/// name gives, where PDFium draws every such font in Helvetica: ours is
/// PDFium's with that face in `/DR`.
pub fn missing_fonts() -> Vec<Stated> {
    let faces = [
        ("TiRo", "Times-Roman"),
        ("Cour", "Courier"),
        ("HelveticaLTStd-Bold", "Helvetica-Bold"),
    ];
    (faces.into_iter())
        .map(|(name, face)| {
            let missing = Case {
                da: format!("/{name} 12 Tf 0 g"),
                ..Case::new(&format!("missing {name}"), "Hello world")
            };
            let font = dictionary! {
                "Type" => Object::name("Font"), "Subtype" => Object::name("Type1"),
                "BaseFont" => Object::name(face), "Encoding" => Object::name("WinAnsiEncoding"),
            };
            let present = Case {
                name: format!("{name} in /DR"),
                fonts: vec![(name, font)],
                ..missing.clone()
            };
            Stated {
                case: missing.clone(),
                ours_of: missing,
                theirs_of: present,
                what: format!("against PDFium's with /DR's {face}"),
            }
        })
        .collect()
}

/// `/MaxLen` cuts only a comb's text, where PDFium cuts every field's:
/// ours is PDFium's of the field with no `/MaxLen`.
pub fn max_len() -> Stated {
    let cut = Case {
        max_len: Some(5),
        ..Case::new("max len", "Hello world")
    };
    let uncut = Case {
        name: "no max len".into(),
        max_len: None,
        ..cut.clone()
    };
    Stated {
        case: cut.clone(),
        ours_of: cut,
        theirs_of: uncut,
        what: "against PDFium's with no /MaxLen".into(),
    }
}

/// A dashed border is drawn with its `/BS /D`, `[6 2]`, where PDFium draws
/// every dashed border `[3 3]`: ours of `[3 3]` is PDFium's of `[6 2]`.
pub fn dash_array() -> Stated {
    let given = Case {
        bs: Some((1, "D", vec![6.0, 2.0])),
        bc: Some(vec![0.0, 0.0, 1.0]),
        ..Case::new("dash array", "Hello world")
    };
    let pdfium_s = Case {
        bs: Some((1, "D", vec![3.0, 3.0])),
        ..given.clone()
    };
    Stated {
        case: given.clone(),
        ours_of: pdfium_s,
        theirs_of: given,
        what: "ours of [3 3] against PDFium's".into(),
    }
}

/// Every pair the pixel checks hold within the tolerance: ours of the
/// first against PDFium's of the second. Each case against itself, and
/// each stated difference's stand-ins.
pub fn held_pairs() -> Vec<(Case, Case)> {
    let twinned = [alignments(), combs(), multiline(), auto_sizes(), borders()];
    let mut pairs: Vec<(Case, Case)> = (twinned.into_iter().flatten())
        .chain([navy()])
        .map(|(case, _, _)| (case.clone(), case))
        .collect();
    let stated = missing_fonts().into_iter().chain([max_len(), dash_array()]);
    pairs.extend(stated.map(|s| (s.ours_of, s.theirs_of)));
    pairs
}

/// `value` with its last character another that Helvetica draws as wide
/// (a digit, or one of `abdeghnopqu`, each 556 units): one glyph other
/// and every other where it was.
pub fn one_other(value: &str) -> String {
    const WIDE: &str = "abdeghnopqu";
    let mut chars: Vec<char> = value.chars().collect();
    let last = chars.last_mut().expect("a value");
    *last = match *last {
        '9' => '0',
        c @ '0'..='8' => (c as u8 + 1) as char,
        c => {
            let at = (WIDE.find(c)).unwrap_or_else(|| panic!("{value:?}: {c:?} has no twin"));
            WIDE.as_bytes()[(at + 1) % WIDE.len()] as char
        }
    };
    chars.into_iter().collect()
}

/// A widget's normal appearance: its `/AP`'s keys, and its form's
/// dictionary and decoded content.
pub struct Made {
    pub keys: Vec<String>,
    pub dict: Dictionary,
    pub content: Vec<u8>,
}

/// The normal appearance of the one widget on page 0 of `bytes`, or none
/// where it has no `/AP /N` stream.
pub fn appearance(bytes: &[u8]) -> Option<Made> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let page = pdf.page_iter().next().expect("a page");
    let annots = pdf.get_page_annotations(page).unwrap();
    let [widget] = annots.as_slice() else {
        panic!("{} annotations on the page, not one", annots.len());
    };
    let resolve = |o: &Object| match o.as_reference() {
        Some(id) => pdf.get_object(id).ok().cloned(),
        None => Some(o.clone()),
    };
    let ap = resolve(widget.get(b"AP")?)?;
    let ap = ap.as_dict()?;
    let stream = resolve(ap.get(b"N")?)?;
    let stream = stream.as_stream()?;
    Some(Made {
        keys: (ap.iter())
            .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
            .collect(),
        dict: stream.dict.clone(),
        content: stream.decompressed_content().unwrap(),
    })
}

/// What sheaf saves of a case, and the document it saved.
pub struct Ours {
    pub doc: Document,
    pub saved: Vec<u8>,
}

/// `case`'s file with no value, opened, the field filled with the case's
/// value through the model, and saved as a Rewrite: its widget's
/// appearance the one the save made (the file has none), `/N` alone, in
/// `/Tx BMC` .. `EMC`.
pub fn ours(case: &Case) -> Ours {
    let file = case.file(false);
    assert!(appearance(&file).is_none(), "{}", case.label());
    let mut doc = opened(file, "field.pdf", None);
    fx::fill(&mut doc, NAME, &case.value);
    let saved = sheaf_doc::save::rewrite(&doc).unwrap();
    let made = (appearance(&saved))
        .unwrap_or_else(|| panic!("{}: the save made no appearance", case.label()));
    assert_eq!(made.keys, ["N"], "{}", case.label());
    let content = String::from_utf8_lossy(&made.content);
    assert!(
        content.contains("/Tx BMC") && content.contains("EMC"),
        "{}: {content}",
        case.label()
    );
    Ours { doc, saved }
}

/// What PDFium makes of `case`: its file with the value as `/V` and no
/// appearance, its widgets drawn once (`FPDF_FFLDraw`, which makes the
/// appearance each lacks), and saved by PDFium (`FPDF_SaveAsCopy`), the
/// copy's widget holding the appearance PDFium made.
pub fn theirs(case: &Case) -> Vec<u8> {
    let file = case.file(true);
    assert!(appearance(&file).is_none(), "{}", case.label());
    let engine = PdfiumEngine::open_bytes(file.into(), None).unwrap();
    live_in(&engine);
    let copy = engine.save_copy().unwrap();
    assert!(
        appearance(&copy).is_some(),
        "{}: PDFium made no appearance",
        case.label()
    );
    copy
}

/// Page 0 of `engine` drawn as the page view draws it, its widgets by
/// `FPDF_FFLDraw` and nothing else beside its content.
pub fn live_in(engine: &PdfiumEngine) -> Raster {
    live_at(engine, PageIndex(0), SIDE_PX).unwrap()
}

/// Page `page` of `engine` drawn [`live_in`]'s way, `width` pixels wide:
/// drawing its widgets makes the appearance each lacks.
pub fn live_at(engine: &PdfiumEngine, page: PageIndex, width: u32) -> anyhow::Result<Raster> {
    let job = Job::Page {
        width,
        rotation: Rotation::R0,
    };
    let draw = Draw {
        annots: false,
        forms: true,
    };
    engine.render_with(page, job, PixelFormat::Rgba, draw)
}

/// Page 0 of `bytes`, appearances only, at [`SCALE`]: flattened by
/// [`PdfiumEngine::render_appearances`] in an engine of its own, then its
/// content drawn [`SIDE_PX`] wide.
pub fn draw(bytes: &[u8]) -> Raster {
    let engine = PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
    flat(&engine, PageIndex(0))
}

/// Page `page` of `engine`, appearances only, at [`SCALE`]. The engine's
/// own [`render_appearances`](PdfiumEngine::render_appearances) flattens
/// the page in its document, as a viewer of appearances only draws it,
/// and draws it at a pixel a point whatever width it's asked for (its
/// matrix is pdfium-render's fixed size, unscaled), so the flattened
/// page's content is drawn again, no annotation beside it, at [`SCALE`].
pub fn flat(engine: &PdfiumEngine, page: PageIndex) -> Raster {
    flat_at(engine, page, SIDE_PX).unwrap()
}

/// Page `page` of `engine` drawn [`flat`]'s way, `width` pixels wide. The
/// flatten's own drawing is a pixel wide: it's thrown away.
pub fn flat_at(engine: &PdfiumEngine, page: PageIndex, width: u32) -> anyhow::Result<Raster> {
    engine.render_appearances(page, 1)?;
    let job = Job::Page {
        width,
        rotation: Rotation::R0,
    };
    let draw = Draw {
        annots: false,
        forms: false,
    };
    engine.render_with(page, job, PixelFormat::Rgba, draw)
}

/// How many pixels inside `px` (left, top, right, bottom) differ between
/// two renders of a page.
pub fn differing_in(a: &Raster, b: &Raster, px: [usize; 4]) -> usize {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let w = a.width as usize;
    let at = |x: usize, y: usize| (y * w + x) * 4..(y * w + x) * 4 + 4;
    let [l, t, r, bottom] = px;
    (t..bottom)
        .flat_map(|y| (l..r).map(move |x| (x, y)))
        .filter(|&(x, y)| a.data[at(x, y)] != b.data[at(x, y)])
        .count()
}

/// How many of a render's pixels aren't white, inside `px` or outside it.
pub fn ink(r: &Raster, px: [usize; 4], inside: bool) -> usize {
    let [l, t, right, b] = px;
    (r.data.chunks(4).enumerate())
        .filter(|(i, p)| {
            let (x, y) = (i % r.width as usize, i / r.width as usize);
            let within = (l..right).contains(&x) && (t..b).contains(&y);
            within == inside && p[..3] != [255, 255, 255]
        })
        .count()
}

/// The text the strings of `made`'s content show, decoded through the
/// fonts its `/Resources` name in `bytes` by sheaf's content interpreter
/// (pdf-gfx, the family's reading of each font), in the order shown; none
/// where a string's codes have no text in its font.
pub fn shown_text(bytes: &[u8], made: &Made) -> Option<String> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let resources = made.dict.get(b"Resources");
    let opts = Opts {
        forms: Forms::Skip,
        wants: Wants {
            text: true,
            ..Wants::NONE
        },
    };
    let (mut text, mut decoded) = (String::new(), true);
    let lim = sheaf_doc::source::Limits::DEFAULT;
    let content = Concat::new(&[&made.content]);
    interpret(
        &pdf,
        &content,
        resources,
        &lim,
        &Fonts::for_text(),
        opts,
        |event| {
            if let Kind::Text(show) = event.kind {
                let font = show.font.filter(|_| !show.stock);
                match font.and_then(|f| f.text(show.string)) {
                    Some(shown) => text.push_str(&shown),
                    None => decoded = false,
                }
            }
            ControlFlow::Continue(())
        },
    )
    .unwrap();
    decoded.then_some(text)
}

/// The directory `SHEAF_FIELDS_OUT` names, where the checks keep what
/// they compare for other tools to read; none where it's unset.
fn out() -> Option<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(std::env::var_os("SHEAF_FIELDS_OUT")?);
    std::fs::create_dir_all(&dir).unwrap();
    Some(dir)
}

/// `bytes` kept as `<case>.<what>`, where [`out`] says.
pub fn keep(case: &Case, what: &str, bytes: &[u8]) {
    if let Some(dir) = out() {
        let path = dir.join(format!("{}.{what}", case.file_name()));
        std::fs::write(path, bytes).unwrap();
    }
}

/// `saved` kept as `<what>.pdf`, and its widget's appearance's decoded
/// content as `<what>.ap`, where [`out`] says.
pub fn keep_saved(case: &Case, what: &str, saved: &[u8]) {
    if out().is_none() {
        return;
    }
    keep(case, &format!("{what}.pdf"), saved);
    if let Some(made) = appearance(saved) {
        keep(case, &format!("{what}.ap"), &made.content);
    }
}

/// `r` kept as `<what>.ppm`, where [`out`] says.
pub fn keep_drawn(case: &Case, what: &str, r: &Raster) {
    if out().is_none() {
        return;
    }
    let mut ppm = format!("P6 {} {} 255\n", r.width, r.height).into_bytes();
    ppm.extend(r.data.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
    keep(case, &format!("{what}.ppm"), &ppm);
}

/// The box of a render's pixels inside `px` that aren't white: left, top,
/// right and bottom, the last two past it; none where none is inked.
pub fn ink_box(r: &Raster, px: [usize; 4]) -> Option<[usize; 4]> {
    let [l, t, right, b] = px;
    let w = r.width as usize;
    let inked = (t..b)
        .flat_map(|y| (l..right).map(move |x| (x, y)))
        .filter(|&(x, y)| r.data[(y * w + x) * 4..][..3] != [255, 255, 255]);
    inked.fold(None, |found, (x, y)| {
        let [x0, y0, x1, y1] = found.unwrap_or([x, y, x + 1, y + 1]);
        Some([x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)])
    })
}
