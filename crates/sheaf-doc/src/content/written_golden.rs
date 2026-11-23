//! What sheaf writes as content, held by digest (M18 slice 19): every
//! place that makes a [`ContentWriter`], driven through its own entry
//! with fixed inputs and ids, and whole files saved after edits, each a
//! line of `written_golden.txt`: the case, the xxh3-128 of the content,
//! and each resource it draws with as `key/name/id`. A file's page is a
//! line too: each content stream's digest, the names its resources bind,
//! what each `Sh…` name binds, and each annotation's appearance.
//!
//! The file was written once, by the writer as it was before content
//! went through pdf-content-streams, and is what its bytes are held to
//! since: a line that differs is a byte that changed, to find, not a
//! golden to write again. Its lines moved once since, where pdf-annots
//! took over drawing a mark (M18 slice 24), each line that moved read
//! and its drawing rendered both ways. `SHEAF_GOLDEN_UPDATE=1` writes
//! it, and `SHEAF_GOLDEN_SHOW=<part of a case's name>` prints what cases
//! wrote.
//!
//! Each case's content is read back too: its tokens through
//! pdf-content-streams, and each operation written again by
//! [`ContentWriter::operation`], give the same bytes.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sheaf_edit::{
    BatesSpec, FlattenSet, FontChoice, GridPos, HeaderFooterSpec, Margins, OcrWord, Op, Rotation, Z,
};

use super::sink::{self, Sink};
use super::{
    BlockId, Concat, ContentWriter, DrawText, Fonts, GenId, Glyphless, Need, PaginationKind, Res,
    Reserved, ResourceNeeds, Std14, fixture, interp,
};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::export::Export;
use crate::forms::fixture as forms;
use crate::generate::{Art, layout, layout_digest};
use crate::geom::{Affine, Geom, Rect};
use crate::materialize::{How, Private};
use crate::model::{
    Document, Frozen, GenSpec, Generator, ImageRef, PageCtx, PageForm, PageId, Scope,
    WatermarkContent, WatermarkGen,
};
use crate::pdf::content::read_back::{self, Drawn};
use crate::pdf::{
    Dictionary, Document as Pdf, Object, ObjectId, Stream, StreamExt, Value, dictionary,
};
use crate::serialize::built::{Built, StampForm};
use crate::source::Limits;
use crate::text_layer::{self, TextLayer};
use crate::tx::fixture as fx;
use crate::tx::{Actor, CommitOpts};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/content/written_golden.txt"
);

/// What seeds the ids an op draws, before each file's edits.
const SEED: u64 = 0x5eaf_0019;

const ROTATIONS: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

/// A case: its line of the golden, and what it wrote, shown when the
/// line differs.
struct Case {
    line: String,
    shown: String,
}

/// The cases so far, in order.
#[derive(Default)]
struct Cases(Vec<Case>);

fn hex(digest: u128) -> String {
    format!("{digest:032x}")
}

fn digest(bytes: &[u8]) -> String {
    hex(sheaf_util::checksum(bytes))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Each need as `key/name/id`.
fn needs_of(needs: &ResourceNeeds) -> Vec<String> {
    (needs.0.iter())
        .map(|n| {
            format!(
                "{}/{}/{}",
                text(n.kind.key()),
                text(&n.name),
                hex(n.res.id().0)
            )
        })
        .collect()
}

/// `content` read back through pdf-content-streams' tokens and written
/// again an operation at a time: the same bytes.
fn read_back(case: &str, content: &[u8], with: Drawn<'_>) {
    let (operations, loose) = read_back::operations(content, with)
        .unwrap_or_else(|e| panic!("{case}: {e}:\n{}", text(content)));
    assert_eq!(loose, 0, "{case}: operands no operator takes");
    let mut w = ContentWriter::new();
    for (operands, op) in &operations {
        w.operation(operands.iter().copied(), op);
    }
    let (again, needs) = w.finish(&Reserved::default());
    assert!(needs.0.is_empty(), "{case}");
    assert!(
        again == content,
        "{case}: read back and written again, it differs:\n--- written\n{}--- again\n{}",
        text(content),
        text(&again)
    );
}

/// `content`'s tokens all read: content sheaf didn't write whole, which
/// isn't an operation a line.
fn reads(case: &str, content: &[u8], with: Drawn<'_>) {
    read_back::operations(content, with)
        .unwrap_or_else(|e| panic!("{case}: {e}:\n{}", text(content)));
}

impl Cases {
    /// A case's line: its content's digest, then `fields`.
    fn push(&mut self, case: &str, content: &[u8], fields: Vec<String>) {
        self.push_with(case, content, fields, None);
    }

    /// [`push`](Self::push), for content drawn with the resources `with`.
    fn push_with(&mut self, case: &str, content: &[u8], fields: Vec<String>, with: Drawn<'_>) {
        read_back(case, content, with);
        let mut line = format!("{case}\t{}", digest(content));
        for field in fields {
            let _ = write!(line, "\t{field}");
        }
        self.0.push(Case {
            line,
            shown: text(content),
        });
    }

    /// What a writer gave: its content and its needs.
    fn written(&mut self, case: &str, content: &[u8], needs: &ResourceNeeds) {
        self.push(case, content, needs_of(needs));
    }
}

// ----- marks' appearances ---------------------------------------------------

/// A sink that writes nothing: which resources it was asked for, in
/// order, each given the next object number.
#[derive(Default)]
struct Asked(Vec<u128>);

impl Sink for Asked {
    fn dict(&mut self, _: ObjectId) -> Option<Dictionary> {
        None
    }

    fn array(&mut self, _: ObjectId) -> Option<Vec<Object>> {
        None
    }

    fn resource(&mut self, res: &Res) -> ObjectId {
        self.0.push(res.id().0);
        ObjectId::new(self.0.len() as u32, 0)
    }

    fn reserved(&mut self, _: Option<&Object>) -> Reserved {
        Reserved::default()
    }
}

/// A quarter turn counter-clockwise about the origin, then moved: a
/// stamp's page turned.
const TURNED: Affine = Affine {
    a: 0.0,
    b: 1.0,
    c: -1.0,
    d: 0.0,
    e: 50.0,
    f: 0.0,
};

/// The form a page stamp draws, object 7 of a file being written, its
/// `/Matrix` `matrix`.
fn stamp_form(matrix: Affine) -> StampForm {
    StampForm::written(
        ObjectId::new(7, 0),
        Rect::new(0.0, 0.0, 100.0, 50.0),
        matrix,
    )
}

/// Mark `a`'s appearance ([`crate::annot::appearance`]), a page stamp
/// drawing `stamp`: its content, its needs as its form's resources bind
/// them, and its box.
fn appearance(cases: &mut Cases, case: &str, a: &Annot, stamp: Option<StampForm>) {
    let drawn = crate::annot::appearance(a, stamp);
    let mut asked = Asked::default();
    let form = sink::form_dict(&mut asked, drawn.bbox, drawn.matrix, &drawn.needs);
    let resources = form.get(b"Resources").and_then(Object::as_dict);
    let mut fields = Vec::new();
    for (i, id) in asked.0.iter().enumerate() {
        let at = ObjectId::new(i as u32 + 1, 0);
        let mut bound = None;
        for (key, category) in resources.into_iter().flat_map(Dictionary::iter) {
            for (name, object) in category.as_dict().into_iter().flat_map(Dictionary::iter) {
                if object.as_reference() == Some(at) {
                    bound = Some(format!("{}/{}", text(key), text(name)));
                }
            }
        }
        let bound = bound.unwrap_or_else(|| panic!("{case}: need {i} isn't bound"));
        fields.push(format!("{bound}/{}", hex(*id)));
    }
    let b = Rect::from(drawn.bbox);
    fields.push(format!("bbox={},{},{},{}", b.x0, b.y0, b.x1, b.y1));
    cases.push(case, &drawn.content, fields);
}

/// A kind's name, as its `Debug` starts.
fn kind_name(kind: &AnnotKind) -> String {
    let debug = format!("{kind:?}");
    debug
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Every kind of mark, as [`forms::every_mark_kind`] has them (opacity
/// 0.8, the square dashed, the ellipse and the polygon cloudy, the tape
/// turned in its box, the page stamp's form turned), upright and on a
/// page turned a quarter; then the ones the fixture doesn't draw.
fn appearances(cases: &mut Cases) {
    let marks = forms::every_mark_kind();
    for a in &marks {
        let stamp = matches!(a.kind, AnnotKind::PageStamp { .. }).then(|| stamp_form(TURNED));
        let name = format!("appearance:{:02}-{}", a.id.0, kind_name(&a.kind));
        appearance(cases, &name, a, stamp);
        let turned = Annot {
            turn: Rotation::R90,
            ..a.clone()
        };
        appearance(cases, &format!("{name}:turned"), &turned, stamp);
    }
    let of = |kind: fn(&AnnotKind) -> bool| -> Annot {
        (marks.iter().find(|a| kind(&a.kind)))
            .expect("a mark of every kind")
            .clone()
    };
    let square = of(|k| matches!(k, AnnotKind::Square { .. }));
    let opaque = Annot {
        opacity: 1.0,
        dashed: false,
        fill: None,
        ..square.clone()
    };
    appearance(cases, "appearance:Square:opaque-solid", &opaque, None);
    let clear = Annot {
        opacity: 0.0,
        ..square.clone()
    };
    appearance(cases, "appearance:Square:clear", &clear, None);
    let cloudy = Annot {
        cloudy: true,
        dashed: false,
        ..square
    };
    appearance(cases, "appearance:Square:cloudy", &cloudy, None);
    for (what, kind) in [
        ("Line", of(|k| matches!(k, AnnotKind::Line { .. }))),
        ("Polygon", of(|k| matches!(k, AnnotKind::Polygon { .. }))),
        ("Polyline", of(|k| matches!(k, AnnotKind::Polyline { .. }))),
        ("Ink", of(|k| matches!(k, AnnotKind::Ink { .. }))),
    ] {
        let dashed = Annot {
            dashed: true,
            cloudy: false,
            ..kind
        };
        appearance(cases, &format!("appearance:{what}:dashed"), &dashed, None);
    }
    // Text: a character Helvetica hasn't (`?`), ones WinAnsi has past
    // ASCII, what a literal string escapes, and lines wrapped.
    let free = of(|k| matches!(k, AnnotKind::FreeText { .. }));
    let odd = Annot {
        contents: "Café “x” – € 漢 (a\\b) and enough words after it to wrap to a second line"
            .into(),
        ..free
    };
    appearance(cases, "appearance:FreeText:unencoded", &odd, None);
    let callout = of(|k| matches!(k, AnnotKind::Callout { .. }));
    let odd = Annot {
        contents: "naïve (x) 漢".into(),
        turn: Rotation::R270,
        ..callout
    };
    appearance(cases, "appearance:Callout:unencoded-turned", &odd, None);
    // A label with control characters, which no standard face has (`?`:
    // only Glyphless writes a string in hex), and one a literal escapes.
    let connector = of(|k| matches!(k, AnnotKind::Connector { .. }));
    for (what, label) in [("control", "A\u{7f}B\tC"), ("escaped", "(12)\\")] {
        let AnnotKind::Connector { rect, partner, .. } = connector.kind.clone() else {
            unreachable!()
        };
        let labelled = Annot {
            kind: AnnotKind::Connector {
                rect,
                label: label.into(),
                partner,
            },
            ..connector.clone()
        };
        let name = format!("appearance:Connector:{what}");
        appearance(cases, &name, &labelled, None);
    }
    // A tape in three faces: its numbers, and the symbols WinAnsi lacks.
    let tape = of(|k| matches!(k, AnnotKind::Tape { .. }));
    for (what, rotation) in [("symbols", 0), ("symbols-turned", 270)] {
        let AnnotKind::Tape { rect, size, .. } = tape.kind.clone() else {
            unreachable!()
        };
        let symbols = Annot {
            kind: AnnotKind::Tape {
                rect,
                size,
                tape: crate::tape::Tape::from_text("5.00 +\n2.00 -\n3.00 ◊\n4.00 *\n12.50 T"),
                look: crate::tape::TapeLook {
                    rotation,
                    ..Default::default()
                },
            },
            ..tape.clone()
        };
        appearance(cases, &format!("appearance:Tape:{what}"), &symbols, None);
    }
    let stamp = of(|k| matches!(k, AnnotKind::Stamp { .. }));
    for (what, glyph) in [("cross", "✗"), ("text", "OK"), ("unencoded", "漢")] {
        let AnnotKind::Stamp { rect, .. } = stamp.kind else {
            unreachable!()
        };
        let other = Annot {
            kind: AnnotKind::Stamp {
                rect,
                glyph: glyph.into(),
            },
            ..stamp.clone()
        };
        appearance(cases, &format!("appearance:Stamp:{what}"), &other, None);
    }
    let attachment = of(|k| matches!(k, AnnotKind::FileAttachment { .. }));
    for icon in crate::annot::ATTACHMENT_ICONS {
        let AnnotKind::FileAttachment {
            pos,
            file,
            name,
            size,
            ..
        } = attachment.kind.clone()
        else {
            unreachable!()
        };
        let other = Annot {
            kind: AnnotKind::FileAttachment {
                pos,
                icon: icon.into(),
                file,
                name,
                size,
            },
            ..attachment.clone()
        };
        let name = format!("appearance:FileAttachment:{icon}");
        appearance(cases, &name, &other, None);
    }
    // A page stamp with no page to draw, one whose form isn't turned,
    // and one on a turned page with dynamic text the face lacks.
    let page_stamp = of(|k| matches!(k, AnnotKind::PageStamp { .. }));
    appearance(cases, "appearance:PageStamp:no-page", &page_stamp, None);
    let upright = Some(stamp_form(Affine::IDENTITY));
    appearance(
        cases,
        "appearance:PageStamp:upright-form",
        &page_stamp,
        upright,
    );
    let half = Annot {
        turn: Rotation::R180,
        ..page_stamp
    };
    let form = Some(stamp_form(TURNED));
    appearance(cases, "appearance:PageStamp:half-turn", &half, form);
    let text_stamp = of(|k| matches!(k, AnnotKind::TextStamp { .. }));
    let AnnotKind::TextStamp { rect, status, .. } = text_stamp.kind else {
        unreachable!()
    };
    let odd = Annot {
        kind: AnnotKind::TextStamp {
            rect,
            text: "REVISED (2)\nGE 漢 10/16/2026".into(),
            status,
        },
        ..text_stamp
    };
    appearance(cases, "appearance:TextStamp:unencoded", &odd, None);
    // Every kind as a save writes it: each mark's appearance and its
    // companions' forms in one file, their resources shared.
    let held = forms::holding(&marks);
    let mut f = forms::Form::new(1);
    let mut page = f.dict(f.pages[0]).clone();
    forms::write_marks(&mut f.pdf, &mut page, &held);
    *f.dict(f.pages[0]) = page;
    cases.0.extend(pages("marks:every", &f.bytes()));
    // A tape's picture: a page of its own.
    for (what, rotation) in [("upright", 0), ("turned", 90)] {
        let AnnotKind::Tape {
            rect,
            size,
            tape: rows,
            ..
        } = tape.kind.clone()
        else {
            unreachable!()
        };
        let a = Annot {
            kind: AnnotKind::Tape {
                rect,
                size,
                tape: rows,
                look: crate::tape::TapeLook {
                    rotation,
                    ..Default::default()
                },
            },
            ..tape.clone()
        };
        let pdf = crate::annot::tape_pdf(&a).expect("a tape's picture");
        cases.0.extend(pages(&format!("tape_pdf:{what}"), &pdf));
    }
}

// ----- generators -----------------------------------------------------------

/// A page whose media box is off the origin, cropped, turned `r`.
fn page_of(r: Rotation) -> Geom {
    let mut geom = Geom::read(Some(Rect::new(-20.0, 10.0, 592.0, 802.0)), None, 0);
    geom.crop = Rect::new(30.0, 60.0, 542.0, 702.0);
    geom.rotate = r;
    geom
}

/// What a layout reads of a page of `geom`: the third of twelve, labelled
/// `iii`, its Bates number 41.
fn ctx_of(geom: &Geom) -> PageCtx {
    PageCtx {
        geom: *geom,
        position: 2,
        count: 12,
        label: "iii".into(),
        bates: Some(41),
    }
}

/// Watermark `content` at `position`, generator 0x2a's, drawn at `z`.
fn watermark(content: WatermarkContent, position: GridPos, z: Z) -> Generator {
    Generator {
        id: GenId(0x2a),
        spec: GenSpec::Watermark(Arc::new(WatermarkGen { content, position })),
        z,
        scope: Scope::All,
        frozen: Frozen::default(),
        replaces: BTreeSet::from([WatermarkGen::kind_at(z)]),
    }
}

/// A watermark of `text`, turned `angle` degrees.
fn words(text: &str, font: FontChoice, angle: f32) -> WatermarkContent {
    WatermarkContent::Text {
        text: text.into(),
        font,
        size: 72.0,
        color: [200, 30, 30],
        opacity: 0.3,
        angle,
    }
}

/// A header and footer in `font`, every slot filled, its date and file
/// fixed, with Bates numbers: generator 0x2b's.
fn header(font: FontChoice) -> Generator {
    let spec = HeaderFooterSpec {
        texts: [
            "Café “x” – € (a\\b)".into(),
            "Page <<1>> of <<N>>".into(),
            "<<bates>>".into(),
            "<<file>>".into(),
            "<<label>> 漢".into(),
            "<<date>>".into(),
        ],
        font,
        size: 9.5,
        color: [10, 20, 130],
        margins: Margins {
            top: 36.0,
            bottom: 30.0,
            left: 72.0,
            right: 54.0,
        },
        first_number: 3,
        bates: Some(BatesSpec {
            prefix: "AB".into(),
            suffix: "-Z".into(),
            digits: 6,
            start: 1,
        }),
    };
    Generator {
        id: GenId(0x2b),
        spec: GenSpec::HeaderFooter(Arc::new(spec)),
        z: Z::Over,
        scope: Scope::All,
        frozen: Frozen {
            date: "2026-10-17".into(),
            file: "report (final).pdf".into(),
            bates: None,
        },
        replaces: BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]),
    }
}

/// Generator `g` laid out on the page `r` turns, drawing `art`, named
/// clear of `reserved`: its content, its needs and its layout's digest.
fn laid_out(
    cases: &mut Cases,
    case: &str,
    (g, art): (&Generator, Option<&Art>),
    r: Rotation,
    reserved: &Reserved,
    private: Private,
) -> ResourceNeeds {
    let ctx = ctx_of(&page_of(r));
    let drawn = layout(g, &ctx, art, reserved, private);
    // The digest alone is of the layout with its private data kept.
    if private == Private::Keep {
        assert_eq!(drawn.layout_digest, layout_digest(g, &ctx, art), "{case}");
    }
    assert!(!drawn.ops.is_empty(), "{case} draws");
    let mut fields = needs_of(&drawn.res);
    fields.push(format!("L={}", hex(drawn.layout_digest)));
    cases.push(case, &drawn.ops, fields);
    drawn.res
}

fn generators(cases: &mut Cases) {
    let free = Reserved::default();
    let faces = [
        ("helvetica", FontChoice::Helvetica),
        ("times-italic", FontChoice::TimesItalic),
    ];
    for (face, font) in faces {
        let g = header(font);
        for r in ROTATIONS {
            let case = format!("header:{face}:{r:?}");
            laid_out(cases, &case, (&g, None), r, &free, Private::Keep);
        }
        let case = format!("header:{face}:stripped");
        laid_out(
            cases,
            &case,
            (&g, None),
            Rotation::R0,
            &free,
            Private::Strip,
        );
    }
    let draft = watermark(
        words("DRAFT (2) – é 漢", FontChoice::HelveticaBold, 45.0),
        GridPos::Center,
        Z::Over,
    );
    let mut needs = ResourceNeeds::default();
    for r in ROTATIONS {
        let case = format!("watermark:text:{r:?}");
        needs = laid_out(cases, &case, (&draft, None), r, &free, Private::Keep);
    }
    laid_out(
        cases,
        "watermark:text:stripped",
        (&draft, None),
        Rotation::R90,
        &free,
        Private::Strip,
    );
    // Its names bound to something else already: each counted up.
    let other = Res::dict(dictionary! { "Type" => Object::name("Other") }).unwrap();
    let mut taken = Reserved::default();
    taken.take(&ResourceNeeds(
        (needs.0.iter())
            .map(|n| Need {
                kind: n.kind,
                name: n.name.clone(),
                res: other.clone(),
            })
            .collect(),
    ));
    laid_out(
        cases,
        "watermark:text:names-taken",
        (&draft, None),
        Rotation::R270,
        &taken,
        Private::Keep,
    );
    let under = watermark(
        words("Copy", FontChoice::CourierBoldOblique, -30.0),
        GridPos::BottomRight,
        Z::Under,
    );
    laid_out(
        cases,
        "watermark:text:background",
        (&under, None),
        Rotation::R90,
        &free,
        Private::Keep,
    );
    // An image and a page, each a resource the file being written has.
    let d = crate::tx::tests::doc_of(1, &[]);
    let at = crate::source::ObjRef {
        src: d.primary().clone(),
        id: ObjectId::new(1, 0),
    };
    let placed = crate::model::Placed {
        at,
        res: super::ResId(1),
        turns: 0,
    };
    let image = watermark(
        WatermarkContent::Image {
            image: ImageRef::Placed(placed.clone()),
            scale: 0.5,
            opacity: 0.6,
        },
        GridPos::TopLeft,
        Z::Over,
    );
    let pixels = Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"), "Subtype" => Object::name("Image"),
            "Width" => 2, "Height" => 1, "BitsPerComponent" => 8,
            "ColorSpace" => Object::name("DeviceGray"),
        },
        vec![0x40, 0xc0],
    );
    let res = Res::of([Object::from(pixels)]).unwrap();
    for turns in 0..4 {
        let art = Art::Image {
            res: res.clone(),
            width: 40,
            height: 20,
            turns,
        };
        let case = format!("watermark:image:turns{turns}");
        laid_out(
            cases,
            &case,
            (&image, Some(&art)),
            ROTATIONS[turns as usize],
            &free,
            Private::Keep,
        );
    }
    let page = watermark(
        WatermarkContent::Page {
            page: PageForm::Placed(placed),
            scale: 0.75,
            opacity: 1.0,
        },
        GridPos::Center,
        Z::Under,
    );
    for (what, matrix) in [("upright", Affine::IDENTITY), ("turned", TURNED)] {
        let art = Art::Form {
            res: Res::written(ObjectId::new(9, 0)),
            bbox: Rect::new(10.0, 20.0, 210.0, 100.0),
            matrix,
        };
        for r in [Rotation::R0, Rotation::R270] {
            let case = format!("watermark:page:{what}:{r:?}");
            laid_out(cases, &case, (&page, Some(&art)), r, &free, Private::Keep);
        }
    }
}

// ----- recognized text ------------------------------------------------------

/// Two lines as an engine saw them on a Letter page turned `upright`:
/// words Glyphless writes in hex, one whose codes are a literal string's
/// (`AA`), one whose codes a literal string escapes (`()`), and one past
/// the BMP.
fn recognized(upright: Rotation) -> TextLayer {
    let word = |text: &str, x: f32, y: f32, w: f32, h: f32| OcrWord {
        text: text.into(),
        rect: Box2 { x, y, w, h },
        confidence: 0.9,
    };
    let words = vec![
        word("World", 130.0, 100.0, 50.0, 10.0),
        word("Hello", 72.0, 100.0, 50.0, 10.0),
        word("\u{4141}", 190.0, 100.0, 12.0, 10.0),
        word("second", 72.0, 120.0, 60.0, 10.0),
        word("\u{2829}\u{5c5c}", 140.0, 121.0, 30.0, 9.0),
        word("😀 naïve", 180.0, 120.0, 55.5, 10.5),
    ];
    let geom = Geom::read(None, None, i64::from(upright.degrees()));
    text_layer::from_engine(words, &geom, "test")
}

fn text_layers(cases: &mut Cases) {
    let free = Reserved::default();
    let id = BlockId(0x2a);
    for r in ROTATIONS {
        let (content, needs) = text_layer::block(&recognized(r), id, &free, Private::Keep);
        cases.written(&format!("text_layer:{r:?}"), &content, &needs);
    }
    let layer = recognized(Rotation::R0);
    let (content, needs) = text_layer::block(&layer, id, &free, Private::Strip);
    cases.written("text_layer:stripped", &content, &needs);
    let (content, needs) =
        text_layer::block(&text_layer::fixture::layer(), id, &free, Private::Keep);
    cases.written("text_layer:fixture", &content, &needs);
}

// ----- a PDF built in memory ------------------------------------------------

/// Pages of text built in memory, as `serialize::built`'s own test
/// builds them: each font once.
fn built(cases: &mut Cases) {
    let mut built = Built::new();
    let faces: [&'static Std14; 3] = [&Std14::Helvetica, &Std14::TimesBold, &Std14::Helvetica];
    let lines = ["one (1)", "two – 漢", "three"];
    for (n, (line, face)) in lines.into_iter().zip(faces).enumerate() {
        let mut w = ContentWriter::new();
        w.begin_text();
        w.set_font(face, 12.0);
        w.text_move(72.0, 700.0);
        w.show_text(face, line);
        w.next_line();
        w.set_font(&Glyphless, 8.0);
        w.show_text(&Glyphless, line);
        w.end_text();
        let (content, needs) = w.finish(&Reserved::default());
        cases.written(&format!("built:page{n}"), &content, &needs);
        built.page(Rect::new(0.0, 0.0, 612.0, 792.0), content, &needs);
    }
    let bytes = built.finish().expect("a built PDF");
    cases.0.extend(pages("built:file", &bytes));
}

// ----- content written again ------------------------------------------------

/// Each of [`fixture::every`]'s pages written again
/// ([`interp::encode`]): its content, and each form's.
fn encoded(cases: &mut Cases) {
    for (name, f) in fixture::every() {
        let content = f.content();
        let made = interp::encode(
            &f.pdf,
            content.concat(),
            f.resources(),
            &Limits::DEFAULT,
            &Fonts::default(),
        );
        let case = format!("encode:{name}");
        match made {
            Ok(encoded) => {
                let mut fields = Vec::new();
                for (id, form) in &encoded.forms {
                    // A form whose own resources name no color space draws
                    // in its page's, as PDFium finds them.
                    let own = (f.pdf.get_object(*id).ok())
                        .and_then(Object::as_stream)
                        .and_then(|s| s.dict.get(b"Resources"))
                        .filter(|r| {
                            let own = direct(&f.pdf, r).as_dict();
                            own.is_some_and(|r| r.get(b"ColorSpace").is_some())
                        });
                    let with = Some((&f.pdf, own.or(f.resources())));
                    read_back(&format!("{case}: form {}", id.number), form, with);
                    fields.push(format!("form{}={}", id.number, digest(form)));
                }
                let with = Some((&f.pdf, f.resources()));
                cases.push_with(&case, &encoded.content, fields, with);
            }
            Err(e) => cases.0.push(Case {
                line: format!("{case}\trefused: {e}"),
                shown: String::new(),
            }),
        }
    }
    // Operators as a file writes them, numbers in every form among them.
    let f = fixture::operators();
    let content: &[u8] = b"q 1 0 0 1 +.5 -0.0 cm 1e2 0.000001 m 12345678901 -.0 l S Q \
        BT /F1 12 Tf 10 20 Td (a\\)b) Tj <4142> Tj [(a) -20 (b)] TJ T* (c) ' ET \
        /P <</MCID 0>> BDC EMC % a comment\n/Span BMC EMC";
    let made = interp::encode(
        &f.pdf,
        &Concat::new(&[content]),
        f.resources(),
        &Limits::DEFAULT,
        &Fonts::default(),
    );
    let encoded = made.expect("content written again");
    let with = Some((&f.pdf, f.resources()));
    cases.push_with("encode:as-written", &encoded.content, Vec::new(), with);
}

// ----- files ----------------------------------------------------------------

/// `object` of `pdf` and all it reaches, in a form its numbers don't
/// change: a reference is what it names (or how far back, where it
/// names something it's in), a dictionary's entries in key order, a
/// stream decoded and without its `/Length` and filters. `/Parent` isn't
/// followed.
fn deep(pdf: &Pdf, object: &Object, out: &mut Vec<u8>, within: &mut Vec<ObjectId>) {
    let run = |out: &mut Vec<u8>, bytes: &[u8]| {
        out.extend((bytes.len() as u64).to_le_bytes());
        out.extend(bytes);
    };
    let entries = |out: &mut Vec<u8>, dict: &Dictionary, within: &mut Vec<ObjectId>| {
        let skipped: [&[u8]; 4] = [b"Parent", b"Length", b"Filter", b"DecodeParms"];
        let mut entries: Vec<_> = (dict.iter())
            .filter(|(key, _)| !skipped.iter().any(|s| s[..] == key[..]))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        out.extend((entries.len() as u64).to_le_bytes());
        for (key, value) in entries {
            run(out, key);
            deep(pdf, value, out, within);
        }
    };
    match object.value() {
        Value::Null => out.push(b'n'),
        Value::Bool(b) => out.extend([b'b', u8::from(*b)]),
        Value::Integer(i) => {
            out.push(b'i');
            out.extend(i.to_le_bytes());
        }
        Value::Real(r) => {
            out.push(b'r');
            out.extend(r.to_bits().to_le_bytes());
        }
        Value::Name(name) => {
            out.push(b'/');
            run(out, name.as_bytes());
        }
        Value::String(s, _) => {
            out.push(b'(');
            run(out, s.as_bytes().unwrap_or(b""));
        }
        Value::Array(items) => {
            out.push(b'[');
            out.extend((items.len() as u64).to_le_bytes());
            for item in items {
                deep(pdf, item, out, within);
            }
        }
        Value::Dict(d) => {
            out.push(b'<');
            entries(out, d, within);
        }
        Value::Stream(s) => {
            out.push(b's');
            entries(out, &s.dict, within);
            run(out, &decoded(s));
        }
        Value::Ref(id) => match within.iter().rposition(|at| at == id) {
            Some(at) => {
                out.push(b'^');
                out.extend(((within.len() - at) as u64).to_le_bytes());
            }
            None => match pdf.get_object(*id) {
                Ok(named) => {
                    within.push(*id);
                    deep(pdf, named, out, within);
                    within.pop();
                }
                Err(_) => out.push(b'?'),
            },
        },
    }
}

/// [`deep`]'s digest of `object`.
fn deep_digest(pdf: &Pdf, object: &Object) -> String {
    let mut bytes = Vec::new();
    deep(pdf, object, &mut bytes, &mut Vec::new());
    digest(&bytes)
}

/// A stream's data, decoded where its filters are ones sheaf decodes.
fn decoded(s: &Stream) -> Vec<u8> {
    s.decompressed_content()
        .unwrap_or_else(|_| s.raw_bytes().to_vec())
}

/// `object`, a reference followed.
fn direct<'a>(pdf: &'a Pdf, object: &'a Object) -> &'a Object {
    match object.as_reference() {
        Some(id) => pdf.get_object(id).unwrap_or(object),
        None => object,
    }
}

/// `page`'s `key`, its own or the nearest ancestor's.
fn inherited<'a>(pdf: &'a Pdf, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    let mut node = page;
    for _ in 0..32 {
        if let Some(found) = node.get(key) {
            return Some(direct(pdf, found));
        }
        node = direct(pdf, node.get(b"Parent")?).as_dict()?;
    }
    None
}

/// A line for each page of the file `bytes`, `case` and its number: each
/// content stream's digest ([`deep`]: its dictionary, so what marks it as
/// a block of sheaf's, and its data decoded); the names its resources
/// bind, by category; what each `Sh…` name binds; and each annotation's
/// `/AP /N`. A stream sheaf marks as its block, and the appearance of a
/// mark of sheaf's, is read back an operation a line; any other content
/// reads.
fn pages(case: &str, bytes: &[u8]) -> Vec<Case> {
    let pdf = Pdf::load_mem(bytes).unwrap_or_else(|e| panic!("{case}: {e}"));
    let mut cases = Vec::new();
    for (i, id) in pdf.get_pages().into_values().enumerate() {
        let case = format!("{case}:p{i}");
        let page = pdf.get_dictionary(id).expect("a page");
        let mut shown = String::new();
        let streams: Vec<&Object> = match page.get(b"Contents").map(|c| direct(&pdf, c)) {
            None => Vec::new(),
            Some(contents) => match contents.as_array() {
                Some(entries) => entries.iter().map(|e| direct(&pdf, e)).collect(),
                None => vec![contents],
            },
        };
        let resources = inherited(&pdf, page, b"Resources");
        let with = Some((&pdf, resources));
        let mut contents = Vec::new();
        for (n, object) in streams.into_iter().enumerate() {
            let Some(stream) = object.as_stream() else {
                contents.push("none".to_string());
                continue;
            };
            let data = decoded(stream);
            match stream.dict.get(b"SHEF_Block") {
                Some(_) => read_back(&format!("{case} stream {n}"), &data, with),
                None => reads(&format!("{case} stream {n}"), &data, with),
            }
            let _ = writeln!(shown, "--- stream {n}\n{}", text(&data));
            contents.push(deep_digest(&pdf, object));
        }
        let mut names = Vec::new();
        let mut bound = Vec::new();
        let resources = resources.and_then(Object::as_dict);
        let mut categories: Vec<_> = (resources.into_iter().flat_map(Dictionary::iter))
            .filter_map(|(key, c)| Some((text(key), direct(&pdf, c).as_dict()?)))
            .collect();
        categories.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, category) in categories {
            let mut entries: Vec<_> = category.iter().map(|(n, o)| (text(n), o)).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let list: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
            names.push(format!("{key}={}", list.join(",")));
            for (name, object) in &entries {
                if name.starts_with("Sh") {
                    bound.push(format!("{key}/{name}={}", deep_digest(&pdf, object)));
                }
            }
        }
        let mut appearances = Vec::new();
        let annots = page.get(b"Annots").map(|a| direct(&pdf, a));
        for annot in annots.and_then(Object::as_array).unwrap_or_default() {
            let Some(annot) = direct(&pdf, annot).as_dict() else {
                continue;
            };
            let normal = (annot.get(b"AP"))
                .and_then(|ap| direct(&pdf, ap).as_dict())
                .and_then(|ap| ap.get(b"N"));
            let Some(normal) = normal else {
                appearances.push("none".to_string());
                continue;
            };
            if let Some(form) = direct(&pdf, normal).as_stream() {
                let data = decoded(form);
                let what = format!("{case} appearance {}", appearances.len());
                let with = Some((&pdf, form.dict.get(b"Resources")));
                match annot.get(b"SHEF_Annot") {
                    Some(_) => read_back(&what, &data, with),
                    None => reads(&what, &data, with),
                }
                let _ = writeln!(
                    shown,
                    "--- appearance {}\n{}",
                    appearances.len(),
                    text(&data)
                );
            }
            appearances.push(deep_digest(&pdf, normal));
        }
        let line = format!(
            "{case}\tC={}\tR={}\tB={}\tAP={}",
            contents.join(","),
            names.join(";"),
            bound.join(","),
            appearances.join(",")
        );
        cases.push(Case { line, shown });
    }
    cases
}

/// A file of one page for each of `contents`, each its content, under a
/// node whose resources are Helvetica as `/F1`.
fn file_of(contents: &[&[u8]]) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => Object::name("Font"),
        "Subtype" => Object::name("Type1"),
        "BaseFont" => Object::name("Helvetica"),
    });
    let resources = pdf.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let kids: Vec<Object> = (contents.iter())
        .map(|content| {
            let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
            let page = dictionary! {
                "Type" => Object::name("Page"),
                "Parent" => root,
                "Contents" => content,
            };
            pdf.add_object(page).into()
        })
        .collect();
    pdf.objects.insert(
        root,
        Object::from(dictionary! {
            "Type" => Object::name("Pages"), "Kids" => kids, "Count" => contents.len() as i64,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => resources,
        }),
    );
    let catalog =
        pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).expect("writing the file");
    bytes
}

/// A folder of this test's, emptied.
fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let name = format!("sheaf-written-golden-{}-{n}", std::process::id());
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// `bytes` as a file, opened: the ids its edits draw seeded.
fn opened(bytes: &[u8]) -> (PathBuf, Document) {
    let dir = scratch();
    let path = dir.join("a.pdf");
    std::fs::write(&path, bytes).expect("the file written");
    let doc = crate::open::file(&path, None).expect("it opens").doc;
    fastrand::seed(SEED);
    (dir, doc)
}

/// A mark of `kind` saying `contents`, as an op makes it.
fn mark(kind: AnnotKind, contents: &str) -> Annot {
    Annot {
        id: AnnotId(0),
        kind,
        color: [255, 200, 0],
        opacity: 0.5,
        stroke: 1.5,
        author: "Greg".into(),
        contents: contents.into(),
        modified: "D:20261014000000".into(),
        created: "D:20261014000000".into(),
        fill: None,
        dashed: true,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

fn every_page(doc: &Document) -> Vec<PageId> {
    doc.state().order().iter().copied().collect()
}

const BODY: [&[u8]; 3] = [
    b"BT /F1 24 Tf 72 72 Td (page 0) Tj ET",
    b"q 1 0 0 1 20 20 cm\nBT /F1 12 Tf 72 700 Td (Body text) Tj ET\n",
    b"BT /F1 24 Tf 72 72 Td (page 2) Tj ET",
];

/// The day the golden was written, as a header set that day keeps it
/// (`<<date>>`, in the default format).
const GOLDEN_DAY: &str = "11/19/2026";

/// A file of three pages edited by ops: marks on the first (a square, a
/// text box and a highlight), a header and footer with Bates numbers and
/// a watermark on every page, recognized text on the second (whose own
/// content leaves a `q` open), and the third's mark flattened. Written
/// as an Append and as a Rewrite.
fn edited() -> Vec<Case> {
    let (dir, mut doc) = opened(&file_of(&BODY));
    let all = every_page(&doc);
    let at = |x: f32, y: f32| Box2 {
        x,
        y,
        w: 120.0,
        h: 40.0,
    };
    let marks = vec![
        fx::add(
            all[0],
            &mark(
                AnnotKind::Square {
                    rect: at(100.0, 100.0),
                },
                "a square",
            ),
        ),
        fx::add(
            all[0],
            &mark(
                AnnotKind::FreeText {
                    rect: at(100.0, 200.0),
                    size: 11.0,
                },
                "Text (in a box) – é",
            ),
        ),
        fx::add(
            all[0],
            &mark(
                AnnotKind::Highlight {
                    rects: vec![at(72.0, 700.0)],
                },
                "",
            ),
        ),
    ];
    doc.commit_ops(marks, "marks").expect("marks");
    let mut header = fx::header("Page <<1>> of <<N>> (<<label>>)");
    header.texts[4] = "<<bates>>".into();
    header.font = FontChoice::TimesRoman;
    header.bates = Some(BatesSpec {
        prefix: "AB".into(),
        suffix: String::new(),
        digits: 6,
        start: 7,
    });
    let generators = vec![
        fx::set_watermark(fx::watermark("DRAFT"), Z::Over, None),
        fx::set_header(header, None),
    ];
    // A header keeps the day it was set, which its recipe's digest holds
    // and its block's marker says: set on the day the golden was written,
    // whatever today is.
    let Ok(mut cx) = Arc::try_unwrap(doc.lower_cx(Actor::User)) else {
        unreachable!("a context just made is held once");
    };
    cx.host.date = GOLDEN_DAY.into();
    let mut tx = Document::tx_at(&doc.snapshot(), Arc::new(cx));
    tx.apply_batch(generators, &in_progress::NoProgress)
        .expect("generators");
    doc.commit(tx, CommitOpts::labelled("generators"))
        .expect("generators");
    let layer = Arc::new(text_layer::fixture::layer());
    doc.commit_ops(fx::ocr([(all[1], layer)]), "ocr")
        .expect("ocr");
    let flat = mark(
        AnnotKind::Ellipse {
            rect: at(72.0, 400.0),
        },
        "Flat",
    );
    let ops = vec![
        fx::add(all[2], &flat),
        fx::flatten(&[all[2]], FlattenSet::All),
    ];
    doc.commit_ops(ops, "flatten").expect("flatten");
    let mut cases = pages("file:edited:append", &How::Appended.write(&doc));
    cases.extend(pages("file:edited:rewrite", &How::Rewritten.write(&doc)));
    let _ = std::fs::remove_dir_all(dir);
    cases
}

/// The author's running head and foot, each in a `q` of its own, around
/// the page's text, as a word processor marks them; then a watermark
/// another file's generator drew, merged into the stream, its list
/// marked as that generator's.
const RUNNING: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Header>> BDC q BT \
    /F1 10 Tf 72 760 Td (RUNNING HEAD) Tj ET Q EMC BT /F1 12 Tf 72 700 Td (body) Tj ET \
    /Artifact <</Type /Pagination /Subtype /Footer>> BDC q BT /F1 10 Tf 72 30 Td \
    (RUNNING FOOT) Tj ET Q EMC\n\
    /Artifact <</Type /Pagination /Subtype /Watermark /SHEF_Gen (g1) >> BDC q 0.5 g \
    100 100 50 50 re f Q EMC";

/// A second stream's: a running head whose font is set outside a `q`,
/// which no splice takes out.
const UNSAFE: &[u8] = b"BT /F1 12 Tf 72 700 Td (two) Tj ET \
    /Artifact <</Type /Pagination /Subtype /Header>> BDC BT /F1 10 Tf 72 760 Td \
    (STAYS) Tj ET EMC";

/// A file whose pages hold pagination artifacts others drew, the
/// author's headers and footers taken out by an op (the ranges spliced
/// out as a save writes the page), written as an Append and as a
/// Rewrite; and a clean copy of it, its lists written again without
/// sheaf's private keys.
fn stripped() -> Vec<Case> {
    let (dir, mut doc) = opened(&file_of(&[RUNNING, UNSAFE]));
    let all = every_page(&doc);
    let remove = Op::RemoveHeaderFooter {
        pages: fx::ids(&all),
        include_authored: true,
    };
    doc.commit_ops(vec![remove], "remove").expect("removed");
    let mut cases = Vec::new();
    let appended = How::Appended.write(&doc);
    let rewritten = How::Rewritten.write(&doc);
    let clean = doc.export_bytes(Export::clean_copy);
    for (case, bytes) in [
        ("file:stripped:append", &appended),
        ("file:stripped:rewrite", &rewritten),
        ("file:stripped:clean-copy", &clean),
    ] {
        let made = pages(case, bytes);
        // The splice took the head and foot out and left the body, and
        // what no splice takes stays.
        let first = &made[0].shown;
        assert!(!first.contains("RUNNING"), "{case}: {first}");
        assert!(first.contains("(body) Tj"), "{case}: {first}");
        assert!(first.contains("100 100 50 50 re"), "{case}: {first}");
        assert!(made[1].shown.contains("(STAYS)"), "{case}");
        let private = first.contains("SHEF_Gen");
        assert_eq!(
            private,
            case != "file:stripped:clean-copy",
            "{case}: {first}"
        );
        cases.extend(made);
    }
    let _ = std::fs::remove_dir_all(dir);
    cases
}

/// `make`'s cases, made twice: the same lines both times.
fn twice(cases: &mut Cases, make: fn() -> Vec<Case>) {
    let first = make();
    let again = make();
    let lines = |cases: &[Case]| -> Vec<String> { cases.iter().map(|c| c.line.clone()).collect() };
    assert_eq!(
        lines(&first),
        lines(&again),
        "a file's lines differ, made twice"
    );
    assert!(!first.is_empty());
    cases.0.extend(first);
}

/// Every case, in the golden's order.
fn cases() -> Cases {
    let mut cases = Cases::default();
    appearances(&mut cases);
    generators(&mut cases);
    text_layers(&mut cases);
    built(&mut cases);
    encoded(&mut cases);
    twice(&mut cases, edited);
    twice(&mut cases, stripped);
    cases
}

/// What sheaf writes is what the golden holds, line for line.
#[test]
fn what_is_written_is_what_the_golden_holds() {
    let cases = cases();
    let names: BTreeSet<&str> = (cases.0.iter())
        .map(|c| c.line.split('\t').next().unwrap_or_default())
        .collect();
    assert_eq!(names.len(), cases.0.len(), "a case's name is its own");
    // `SHEAF_GOLDEN_SHOW=<part of a case's name>` prints what those cases
    // wrote (with `--nocapture`).
    if let Some(show) = std::env::var_os("SHEAF_GOLDEN_SHOW") {
        let show = show.to_string_lossy().into_owned();
        for case in cases.0.iter().filter(|c| c.line.contains(&show)) {
            eprintln!("{}\n{}", case.line, case.shown);
        }
    }
    let got: String = cases.0.iter().map(|c| format!("{}\n", c.line)).collect();
    if std::env::var_os("SHEAF_GOLDEN_UPDATE").is_some_and(|v| v == "1") {
        std::fs::write(GOLDEN, got).expect("the golden written");
        return;
    }
    let want = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("{GOLDEN}: {e}; SHEAF_GOLDEN_UPDATE=1 writes it"));
    let want: Vec<&str> = want.lines().collect();
    let mut differs = String::new();
    for (i, case) in cases.0.iter().enumerate() {
        let held = want.get(i).copied().unwrap_or("(no line)");
        if held != case.line {
            let _ = writeln!(
                differs,
                "line {}:\n  held {held}\n  got  {}\n{}",
                i + 1,
                case.line,
                case.shown
            );
        }
    }
    for held in want.iter().skip(cases.0.len()) {
        let _ = writeln!(differs, "held, and not written: {held}");
    }
    assert!(
        differs.is_empty(),
        "what's written differs from {GOLDEN}: a byte changed, to find (the golden isn't \
         written again):\n{differs}"
    );
}
