//! What sheaf draws of the marks pdf-annots can draw too (M18 slice 24),
//! set beside what the family draws of them, rendered by PDFium: for the
//! kinds the family draws as sheaf does (a highlight; a square, an
//! ellipse and a page border, not cloudy; a polygon and a polyline, not
//! cloudy; ink), each case is saved as sheaf saves it, on its page and
//! flattened, and again with its appearance the family's `synth_*` drawing
//! of the same mark, its form's `/Matrix` the move to its `/Rect`'s
//! corner (the family draws in `[0 0 w h]`); the two are rendered at 4
//! pixels a point and compared pixel by pixel, a line a case in
//! `looks.tsv`. The family's drawing is framed two more ways, to tell
//! what the move costs: drawn in place (its `/BBox` the mark's box, no
//! `/Matrix`), and moved by whole points ([`Frame`]). For the kinds the
//! family draws only once it learns them
//! (a caret, an attachment, a cloudy shape), sheaf's files are kept for
//! that comparison. Every case's files are kept in `looks/`, with the
//! renders of a case that differs.
//!
//! The cases: every kind's marks as the golden fixtures have them, at each
//! turn; each kind's edges (the least and greatest line the tools give,
//! and none; dashed; filled; an opacity under 1; a box under four lines
//! wide; a path of one point and of two; a quad of no height; cloudy);
//! and the corpus's comments of these kinds, as sheaf converts them, read
//! from the files the annotations dump names (`$SHEAF_LOOKS_INDEX`, or the
//! baseline dump of M18 slice 24).
//!
//! Ignored: it writes to `$SHEAF_LOOKS_DIR` (else `target/s24/step2`), and
//! needs libpdfium (`scripts/fetch-pdfium.sh`; `PDFIUM_LIB_DIR` picks
//! another build).

// A test crate of its own, whose files go where its records are kept.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sheaf_doc::annot::emission;
use sheaf_doc::forms::corpus::Corpus;
use sheaf_doc::forms::fixture as forms;
use sheaf_doc::geom::Affine;
use sheaf_doc::import::{Interned, Options};
use sheaf_doc::pdf::annots::{
    AnnotColor, Border, BorderStyle, LineEnding, PaintStyle, Point, PolyKind, QuadPoints,
    ShapeKind, SynthesizedAp, TextMarkupKind, synth_ink, synth_poly, synth_shape,
    synth_text_markup,
};
use sheaf_doc::pdf::gfx::{Canvas, Matrix, Rect};
use sheaf_doc::pdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_doc::serialize::built::Held;
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::fixture::{add, flatten};
use sheaf_doc::{Annot, AnnotKind, Blob, Box2, Document, FixedCount, Pt, Store};
use sheaf_edit::{FlattenSet, Rotation};
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Draw, Job, PageIndex};

/// Pixels a point.
const SCALE: u32 = 4;

const TURNS: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

/// Where the records go.
fn out_dir() -> PathBuf {
    std::env::var_os("SHEAF_LOOKS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/s24/step2"))
}

/// A case: its name, which row of the comparison it's in (1, the family
/// draws it now; 2, once it learns to), the mark, and what the mark's
/// store must hold for it.
struct Case {
    name: String,
    row: u8,
    mark: Annot,
    /// Blobs to intern in the case's store: a stamp's page (`true`), a
    /// file.
    blobs: Vec<(Arc<[u8]>, bool)>,
}

/// Which row `a` is in, or none.
fn row_of(a: &Annot) -> Option<u8> {
    match &a.kind {
        AnnotKind::Highlight { .. } | AnnotKind::PageBorder { .. } | AnnotKind::Ink { .. } => {
            Some(1)
        }
        AnnotKind::Polyline { .. } => Some(1),
        AnnotKind::Square { .. } | AnnotKind::Ellipse { .. } | AnnotKind::Polygon { .. } => {
            Some(if a.cloudy { 2 } else { 1 })
        }
        AnnotKind::Insert { .. } | AnnotKind::Replace { .. } | AnnotKind::FileAttachment { .. } => {
            Some(2)
        }
        _ => None,
    }
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

fn b(x: f32, y: f32, w: f32, h: f32) -> Box2 {
    Box2 { x, y, w, h }
}

fn p(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// A shape made in a box.
type Shape = fn(Box2) -> AnnotKind;

/// The fixtures' marks of both rows at each turn, and each kind's edges.
fn fixture_cases() -> Vec<Case> {
    let marks = forms::every_mark_kind();
    let blobs = || {
        vec![
            (Arc::from(forms::mark_stamp_pdf()), true),
            (Arc::from(forms::MARK_FILE_BYTES), false),
        ]
    };
    let mut cases = Vec::new();
    let mut push = |name: String, mark: Annot| {
        if let Some(row) = row_of(&mark) {
            cases.push(Case {
                name,
                row,
                mark,
                blobs: blobs(),
            });
        }
    };
    let mut kinds: Vec<(String, Annot)> = Vec::new();
    for a in &marks {
        let name = kind_name(&a.kind);
        kinds.push((format!("{name}:fixture"), a.clone()));
        // The fixture's cloudy shapes plain, and its square cloudy.
        if a.kind.cloudable() {
            kinds.push((
                format!(
                    "{name}:fixture-{}",
                    if a.cloudy { "plain" } else { "cloudy" }
                ),
                Annot {
                    cloudy: !a.cloudy,
                    dashed: false,
                    ..a.clone()
                },
            ));
        }
    }
    for (name, a) in &kinds {
        for turn in TURNS {
            push(
                format!("fixture:{name}:turn{}", turn.degrees()),
                Annot { turn, ..a.clone() },
            );
        }
    }
    // Each kind's edges, upright and on a quarter turn.
    let base = |kind: AnnotKind| Annot {
        kind,
        cloudy: false,
        dashed: false,
        fill: None,
        opacity: 1.0,
        stroke: 1.0,
        ..marks[0].clone()
    };
    let shapes = [
        (
            "Square",
            AnnotKind::Square {
                rect: b(100.0, 100.0, 80.0, 50.0),
            },
        ),
        (
            "Ellipse",
            AnnotKind::Ellipse {
                rect: b(100.0, 100.0, 80.0, 50.0),
            },
        ),
        (
            "PageBorder",
            AnnotKind::PageBorder {
                rect: b(100.0, 100.0, 80.0, 50.0),
                status: "Reviewed".into(),
            },
        ),
        (
            "Polygon",
            AnnotKind::Polygon {
                points: vec![p(100.0, 100.0), p(170.0, 110.0), p(130.0, 170.0)],
            },
        ),
        (
            "Polyline",
            AnnotKind::Polyline {
                points: vec![p(100.0, 100.0), p(140.0, 130.0), p(190.0, 100.0)],
            },
        ),
        (
            "Ink",
            AnnotKind::Ink {
                paths: vec![
                    vec![p(100.0, 160.0), p(120.0, 140.0), p(150.0, 155.0)],
                    vec![p(160.0, 100.0), p(190.0, 120.0)],
                ],
            },
        ),
    ];
    let mut edges: Vec<(String, Annot)> = Vec::new();
    for (name, kind) in &shapes {
        let a = base(kind.clone());
        for stroke in [0.5, 5.0, 0.0] {
            edges.push((
                format!("{name}:line{stroke}"),
                Annot {
                    stroke,
                    ..a.clone()
                },
            ));
        }
        // Off the grid of whole points, as most marks are.
        let off = Affine {
            e: 0.3137,
            f: 0.7249,
            ..Affine::IDENTITY
        };
        for stroke in [1.0, 1.5] {
            edges.push((
                format!("{name}:offset-line{stroke}"),
                Annot {
                    stroke,
                    ..base(kind.mapped(&off))
                },
            ));
        }
        edges.push((
            format!("{name}:dashed"),
            Annot {
                dashed: true,
                ..a.clone()
            },
        ));
        edges.push((
            format!("{name}:dashed-thick"),
            Annot {
                dashed: true,
                stroke: 5.0,
                ..a.clone()
            },
        ));
        edges.push((
            format!("{name}:opacity0.25"),
            Annot {
                opacity: 0.25,
                ..a.clone()
            },
        ));
        if a.kind.fillable() {
            let fill = Some([255, 255, 0]);
            edges.push((format!("{name}:filled"), Annot { fill, ..a.clone() }));
            edges.push((
                format!("{name}:filled-translucent-dashed"),
                Annot {
                    fill,
                    opacity: 0.5,
                    dashed: true,
                    ..a.clone()
                },
            ));
            edges.push((
                format!("{name}:cloudy"),
                Annot {
                    cloudy: true,
                    ..a.clone()
                },
            ));
            edges.push((
                format!("{name}:cloudy-thick"),
                Annot {
                    cloudy: true,
                    stroke: 5.0,
                    ..a.clone()
                },
            ));
            edges.push((
                format!("{name}:cloudy-none"),
                Annot {
                    cloudy: true,
                    stroke: 0.0,
                    ..a.clone()
                },
            ));
        }
    }
    // A box under four lines wide, at the least, a usual and the
    // greatest line.
    let smalls: [(&str, Shape); 3] = [
        ("Square", |r| AnnotKind::Square { rect: r }),
        ("Ellipse", |r| AnnotKind::Ellipse { rect: r }),
        ("PageBorder", |r| AnnotKind::PageBorder {
            rect: r,
            status: "Reviewed".into(),
        }),
    ];
    for (name, small) in smalls {
        for (stroke, side) in [(0.5, 1.5), (1.0, 3.0), (5.0, 15.0), (5.0, 8.0), (5.0, 3.0)] {
            edges.push((
                format!("{name}:box{side}-line{stroke}"),
                Annot {
                    stroke,
                    ..base(small(b(100.0, 100.0, side, side)))
                },
            ));
        }
    }
    // Paths of one point and of two.
    edges.push((
        "Ink:one-point".into(),
        base(AnnotKind::Ink {
            paths: vec![vec![p(120.0, 120.0)]],
        }),
    ));
    edges.push((
        "Ink:one-point-thick".into(),
        Annot {
            stroke: 5.0,
            ..base(AnnotKind::Ink {
                paths: vec![vec![p(120.0, 120.0)]],
            })
        },
    ));
    edges.push((
        "Ink:two-points".into(),
        base(AnnotKind::Ink {
            paths: vec![vec![p(100.0, 100.0), p(150.0, 120.0)]],
        }),
    ));
    edges.push((
        "Ink:two-points-same".into(),
        Annot {
            stroke: 3.0,
            ..base(AnnotKind::Ink {
                paths: vec![vec![p(120.0, 120.0), p(120.0, 120.0)]],
            })
        },
    ));
    edges.push((
        "Polyline:two-points".into(),
        base(AnnotKind::Polyline {
            points: vec![p(100.0, 100.0), p(150.0, 120.0)],
        }),
    ));
    edges.push((
        "Polygon:two-points".into(),
        base(AnnotKind::Polygon {
            points: vec![p(100.0, 100.0), p(150.0, 120.0)],
        }),
    ));
    // Highlights: a quad of no height, a thin one, several, translucent.
    let hl = |rects: Vec<Box2>| Annot {
        color: [255, 230, 0],
        ..base(AnnotKind::Highlight { rects })
    };
    edges.push((
        "Highlight:no-height".into(),
        hl(vec![b(100.0, 100.0, 80.0, 0.0)]),
    ));
    edges.push((
        "Highlight:thin".into(),
        hl(vec![b(100.0, 100.0, 80.0, 0.5)]),
    ));
    edges.push((
        "Highlight:lines".into(),
        hl(vec![
            b(100.0, 120.0, 150.0, 12.0),
            b(100.0, 106.0, 90.0, 12.0),
        ]),
    ));
    edges.push((
        "Highlight:opacity0.5".into(),
        Annot {
            opacity: 0.5,
            ..hl(vec![b(100.3, 100.7, 80.4, 11.9)])
        },
    ));
    // Carets and attachments: their sizes, and each icon.
    for h in [0.0, 12.0, 100.0] {
        let at = sheaf_doc::annot::caret_box(150.0, 120.0, h);
        edges.push((format!("Insert:h{h}"), base(AnnotKind::Insert { at })));
    }
    let attachment = (marks.iter())
        .find(|a| matches!(a.kind, AnnotKind::FileAttachment { .. }))
        .expect("an attachment")
        .clone();
    for icon in sheaf_doc::annot::ATTACHMENT_ICONS {
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
        let kind = AnnotKind::FileAttachment {
            pos,
            icon: icon.into(),
            file,
            name,
            size,
        };
        edges.push((
            format!("FileAttachment:{icon}"),
            Annot {
                kind,
                ..attachment.clone()
            },
        ));
    }
    for (name, a) in edges {
        for turn in [Rotation::R0, Rotation::R90] {
            push(
                format!("edge:{name}:turn{}", turn.degrees()),
                Annot { turn, ..a.clone() },
            );
        }
    }
    cases
}

/// The corpus's comments of both rows' kinds, as sheaf converts them (an
/// attachment with its file): read from each file the dump at `index`
/// lists one of them in.
fn corpus_cases(index: &Path) -> Vec<Case> {
    let Some(corpus) = Corpus::or_skip() else {
        return Vec::new();
    };
    let Ok(dump) = std::fs::read_to_string(index) else {
        eprintln!("no corpus cases: {} doesn't read", index.display());
        return Vec::new();
    };
    let kinds = [
        "Highlight",
        "Square",
        "Circle",
        "Polygon",
        "PolyLine",
        "Ink",
        "Caret",
        "FileAttachment",
    ];
    let files: BTreeSet<&str> = (dump.lines())
        .filter(|l| l.contains("\tread\tis=foreign\t"))
        .filter(|l| {
            kinds
                .iter()
                .any(|k| l.contains(&format!("\tsubtype={k}\t")))
        })
        .filter_map(|l| l.split('\t').next())
        .collect();
    let mut cases = Vec::new();
    for path in files {
        let Ok(bytes) = std::fs::read(corpus.root.join(path)) else {
            continue;
        };
        let store = Store::new();
        let Ok(src) = store.open(bytes.into(), BlobKind::file(Path::new(path), None)) else {
            continue;
        };
        let pdf = src.pdf();
        let pages: Vec<(ObjectId, Rotation)> = (src.pages().iter())
            .map(|p| (p.id, p.geom.rotate))
            .collect();
        let found = sheaf_doc::file::annots_in(pdf, pages.iter().copied());
        let mut listed: Vec<_> = found.foreign();
        listed.sort_by_key(|(page, f)| (*page, f.slot));
        for (page, f) in listed {
            if !kinds.contains(&f.subtype.as_str()) {
                continue;
            }
            let (mark, blobs) = match &f.mark {
                Some(mark) => (Some((**mark).clone()), Vec::new()),
                // An attachment converts with its file to hold.
                None if f.subtype == "FileAttachment" => {
                    let files = Interned::new(&store);
                    let dict = (src.page(page))
                        .and_then(|facts| facts.annots.get(f.slot))
                        .and_then(|e| pdf.dereference(e).ok())
                        .and_then(|(_, o)| o.as_dict().cloned());
                    let mark = dict.and_then(|d| {
                        let opts = Options {
                            files: Some(&files),
                        };
                        sheaf_doc::import::convert(pdf, &d, pages[page as usize].1, opts).ok()
                    });
                    let held: Vec<Blob> = files.into_held();
                    let blobs = (held.iter()).map(|b| (b.bytes().clone(), false)).collect();
                    (mark, blobs)
                }
                None => (None, Vec::new()),
            };
            let Some(mut mark) = mark else { continue };
            mark.name = String::new();
            let Some(row) = row_of(&mark) else { continue };
            let name = format!(
                "corpus:{}:p{}:s{}",
                path.replace(['/', ' '], "_"),
                page + 1,
                f.slot
            );
            cases.push(Case {
                name,
                row,
                mark,
                blobs,
            });
        }
    }
    cases
}

/// A one-page file for `rect` (the page's box, with room around it) on a
/// page turned `turn`, nothing on it.
fn blank(rect: Rect, turn: Rotation) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let pages_id = pdf.new_object_id();
    let content = pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
    let media = [
        rect.x0.floor() - 8.0,
        rect.y0.floor() - 8.0,
        rect.x1.ceil() + 8.0,
        rect.y1.ceil() + 8.0,
    ];
    let mut page = dictionary! {
        "Type" => Object::name("Page"), "Parent" => pages_id, "Contents" => content,
        "MediaBox" => media.map(Object::from).to_vec(),
    };
    if turn != Rotation::R0 {
        page.set("Rotate", i64::from(turn.degrees()));
    }
    let page_id = pdf.add_object(page);
    pdf.objects.insert(
        pages_id,
        Object::from(dictionary! {
            "Type" => Object::name("Pages"), "Kids" => vec![Object::from(page_id)], "Count" => 1,
        }),
    );
    let catalog =
        pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let mut out = Vec::new();
    pdf.save_to(&mut out).expect("a blank page");
    out
}

/// Case `c` as sheaf saves it: on its page, and flattened.
fn sheaf_files(c: &Case, rect: Rect) -> (Vec<u8>, Vec<u8>) {
    let store = Store::new();
    let held: Vec<Blob> = (c.blobs.iter())
        .map(|(bytes, stamp)| {
            let kind = match stamp {
                true => BlobKind::stamp(),
                false => BlobKind::Attachment,
            };
            store.intern(bytes.clone(), kind).expect("a blob")
        })
        .collect();
    let primary = (store.open(
        blank(rect, c.mark.turn).into(),
        BlobKind::file(Path::new("case.pdf"), None),
    ))
    .expect("the blank page opens");
    let mut doc = Document::new(
        store,
        primary,
        "case.pdf".into(),
        None,
        &[],
        Box::new(FixedCount(10)),
    );
    let page = doc.page(0).expect("a page");
    doc.commit_ops(vec![add(page, &c.mark)], "mark")
        .expect("the mark is added");
    drop(held);
    let on_page = sheaf_doc::save::rewrite(&doc).expect("saved");
    doc.commit_ops(vec![flatten(&[page], FlattenSet::All)], "flatten")
        .expect("flattened");
    let flat = sheaf_doc::save::rewrite(&doc).expect("saved flattened");
    (on_page, flat)
}

/// How the family's appearance is framed: as design 3 has it, drawn at
/// `[0 0 w h]` and moved onto the mark's box by its `/Matrix` (`Moved`);
/// or, to tell what that move costs, drawn where the mark is on the page,
/// its `/BBox` the mark's box and no `/Matrix` (`InPlace`: the family
/// given a box from the origin to the mark's far corner, its own box then
/// set to the mark's).
#[derive(Clone, Copy, PartialEq)]
enum Frame {
    Moved,
    InPlace,
    /// Moved by whole points: the family given a box from the whole
    /// point at or below the mark's corner, its own box then set to the
    /// mark's less that, and moved there by its `/Matrix`.
    Whole,
}

/// What the family's `synth_*` draws of mark `a`, whose appearance sheaf
/// draws over `rect`, framed as `frame` says: the geometry sheaf draws in
/// PageSpace; a square's, an ellipse's and a page border's `/RD` the box
/// sheaf strokes on less half the line, so the family's border, drawn
/// inside its box, has its centre where sheaf's is. None where the family
/// draws nothing (`synth_*` gives none); an empty appearance is what's
/// drawn then.
fn family(a: &Annot, rect: Rect, frame: Frame) -> (Option<SynthesizedAp>, SynthesizedAp) {
    let user = Affine::upright(a.turn).inverse();
    let shown = a.upright();
    let ub = |r: Box2| {
        let u = user.apply_box(r);
        Rect {
            x0: u.x,
            y0: u.y,
            x1: u.right(),
            y1: u.bottom(),
        }
    };
    let up = |q: Pt| {
        let u = user.apply(q);
        Point::new(u.x, u.y)
    };
    let color = AnnotColor::rgb8(a.color);
    let opacity = a.opacity.clamp(0.0, 1.0);
    let mut border = Border::default();
    border.width = a.stroke;
    if a.dashed {
        border.style = BorderStyle::Dashed;
        border.dash = sheaf_doc::annot::DASH.to_vec();
    }
    let style = PaintStyle::new(Some(color), a.fill.map(AnnotColor::rgb8))
        .with_border(border.clone())
        .with_opacity(opacity);
    // The box the family is given.
    let on = match frame {
        Frame::Moved => rect,
        Frame::InPlace => Rect {
            x0: 0.0,
            y0: 0.0,
            x1: rect.x1,
            y1: rect.y1,
        },
        Frame::Whole => Rect {
            x0: rect.x0.floor(),
            y0: rect.y0.floor(),
            x1: rect.x1,
            y1: rect.y1,
        },
    };
    let inset = |shape: Box2| {
        let s = ub(shape);
        let half = a.stroke / 2.0;
        [
            s.x0 - on.x0 - half,
            s.y0 - on.y0 - half,
            on.x1 - s.x1 - half,
            on.y1 - s.y1 - half,
        ]
    };
    let drawn = match &shown {
        AnnotKind::Highlight { rects } => {
            let quads: Vec<QuadPoints> = rects
                .iter()
                .map(|r| QuadPoints::from_rect(ub(*r)))
                .collect();
            synth_text_markup(TextMarkupKind::Highlight, on, &quads, color, opacity)
        }
        AnnotKind::Square { rect: shape } | AnnotKind::PageBorder { rect: shape, .. } => {
            synth_shape(ShapeKind::Square, on, &style, Some(inset(*shape)))
        }
        AnnotKind::Ellipse { rect: shape } => {
            synth_shape(ShapeKind::Circle, on, &style, Some(inset(*shape)))
        }
        AnnotKind::Polygon { points } | AnnotKind::Polyline { points } => {
            let kind = match shown {
                AnnotKind::Polygon { .. } => PolyKind::Polygon,
                _ => PolyKind::PolyLine,
            };
            let vertices: Vec<Point> = points.iter().map(|q| up(*q)).collect();
            synth_poly(
                kind,
                on,
                &vertices,
                (LineEnding::None, LineEnding::None),
                &style,
            )
        }
        AnnotKind::Ink { paths } => {
            let strokes: Vec<Vec<Point>> = (paths.iter())
                .map(|path| path.iter().map(|q| up(*q)).collect())
                .collect();
            synth_ink(on, &strokes, color, &border, opacity)
        }
        _ => unreachable!("a kind of the first row"),
    };
    let box0 = Rect {
        x0: 0.0,
        y0: 0.0,
        x1: rect.x1 - rect.x0,
        y1: rect.y1 - rect.y0,
    };
    let mut shown_ap = (drawn.clone())
        .unwrap_or_else(|| SynthesizedAp::drawn(box0, Matrix::IDENTITY, Canvas::new()));
    match frame {
        Frame::Moved => {
            shown_ap.matrix = Matrix {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                e: rect.x0,
                f: rect.y0,
            }
        }
        Frame::InPlace => {
            shown_ap.bbox = rect;
            shown_ap.matrix = Matrix::IDENTITY;
        }
        Frame::Whole => {
            shown_ap.bbox = Rect {
                x0: rect.x0 - on.x0,
                y0: rect.y0 - on.y0,
                x1: rect.x1 - on.x0,
                y1: rect.y1 - on.y0,
            };
            shown_ap.matrix = Matrix {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                e: on.x0,
                f: on.y0,
            };
        }
    }
    (drawn, shown_ap)
}

/// The page dictionary of `pdf`'s one page.
fn page_of(pdf: &Pdf) -> ObjectId {
    pdf.page_iter().next().expect("a page")
}

/// `bytes` with the one form sheaf drew for its mark, the annotation's
/// normal appearance (`flat` false) or the form its flatten placed on the
/// page (`flat`), the family's `ap` instead.
fn swapped(bytes: &[u8], ap: &SynthesizedAp, flat: bool) -> Vec<u8> {
    let mut pdf = Pdf::load_mem(bytes).expect("sheaf's file reads");
    let page = page_of(&pdf);
    let form = match flat {
        false => {
            let annots: Vec<ObjectId> = (pdf.get_dictionary(page).ok())
                .and_then(|d| d.get(b"Annots"))
                .and_then(|a| pdf.dereference(a).ok())
                .and_then(|(_, a)| {
                    a.as_array()
                        .map(|a| a.iter().filter_map(Object::as_reference).collect())
                })
                .unwrap_or_default();
            assert_eq!(annots.len(), 1, "one annotation");
            let dict = pdf.get_dictionary(annots[0]).expect("the mark");
            let (_, ap) = pdf
                .dereference(dict.get(b"AP").expect("its /AP"))
                .expect("its /AP");
            ap.as_dict()
                .and_then(|d| d.get(b"N"))
                .and_then(Object::as_reference)
                .expect("its /N")
        }
        true => {
            let resources = pdf.resources_of_page(page).expect("the page's resources");
            let (_, forms) = pdf
                .dereference(resources.get(b"XObject").expect("its forms"))
                .expect("its forms");
            let forms: Vec<ObjectId> = (forms.as_dict().into_iter().flat_map(Dictionary::iter))
                .filter_map(|(_, o)| o.as_reference())
                .collect();
            assert_eq!(forms.len(), 1, "one form placed");
            forms[0]
        }
    };
    pdf.objects.insert(form, Object::from(ap.to_stream()));
    let mut out = Vec::new();
    pdf.save_to(&mut out).expect("written again");
    out
}

/// The most pixels a render is across, for a page whose box is too
/// large for [`SCALE`] (a mark over the whole of user space).
const MOST: f32 = 8192.0;

/// Pixels a point page 1 of `bytes` is rendered at: [`SCALE`], or less
/// where that's past [`MOST`].
fn scale_of(bytes: &[u8]) -> f32 {
    let (w, h) = shown_size(bytes);
    (SCALE as f32).min(MOST / w.max(h))
}

/// Page 1 of `bytes`'s size as shown, turned as it is.
fn shown_size(bytes: &[u8]) -> (f32, f32) {
    let pdf = Pdf::load_mem(bytes).expect("it reads");
    let page = pdf.get_dictionary(page_of(&pdf)).expect("a page");
    let media: Vec<f32> = (page
        .get(b"MediaBox")
        .and_then(Object::as_array)
        .expect("a box")
        .iter())
    .map(|v| v.as_number().unwrap_or(0.0) as f32)
    .collect();
    let (w, h) = (media[2] - media[0], media[3] - media[1]);
    match page.get(b"Rotate").and_then(Object::as_integer) {
        Some(90 | 270) => (h, w),
        _ => (w, h),
    }
}

/// Page 1 of `bytes` at `scale` pixels a point, annotations drawn.
fn render(bytes: &[u8], scale: f32, case: &str) -> Raster {
    let engine = PdfiumEngine::open_bytes(Arc::from(bytes), None).expect("PDFium opens it");
    let (width, _) = shown_size(bytes);
    let job = Job::Page {
        width: (width * scale).round() as u32,
        rotation: Rotation::R0,
    };
    let draw = Draw {
        annots: true,
        forms: false,
    };
    engine
        .render_with(PageIndex(0), job, PixelFormat::Rgba, draw)
        .unwrap_or_else(|e| panic!("{case}: {job:?}: {e:?}"))
}

/// How many pixels of `a` and `b` differ, and by how much at most.
fn compared(a: &Raster, b: &Raster) -> (usize, u8) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut n = 0;
    let mut most = 0;
    for (x, y) in a.data.chunks(4).zip(b.data.chunks(4)) {
        let d = x
            .iter()
            .zip(y)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        if d > 0 {
            n += 1;
            most = most.max(d);
        }
    }
    (n, most)
}

fn png(r: &Raster, to: &Path) {
    image::RgbaImage::from_raw(r.width, r.height, r.data.clone())
        .expect("a raster")
        .save(to)
        .unwrap_or_else(|e| panic!("{}: {e}", to.display()));
}

/// Where `a` and `b` differ, in red, over `a` faded.
fn difference(a: &Raster, b: &Raster) -> Raster {
    let data = (a.data.chunks(4).zip(b.data.chunks(4)))
        .flat_map(|(x, y)| match x == y {
            true => [x[0] / 2 + 128, x[1] / 2 + 128, x[2] / 2 + 128, 255],
            false => [255, 0, 0, 255],
        })
        .collect();
    Raster {
        width: a.width,
        height: a.height,
        format: PixelFormat::Rgba,
        data,
    }
}

/// A file name for case `name`.
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Every case, a line each in `looks.tsv`; sheaf's files of each in
/// `looks/`, and the renders of a case whose two differ.
#[test]
#[ignore = "renders with PDFium into $SHEAF_LOOKS_DIR: M18 slice 24's comparison of looks"]
fn sheaf_s_and_the_family_s_looks_set_beside_each_other() {
    let dir = out_dir();
    let looks = dir.join("looks");
    std::fs::create_dir_all(&looks).expect("the records' folder");
    let index = std::env::var_os("SHEAF_LOOKS_INDEX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/s24/baseline/annots.tsv")
        });
    let mut cases = fixture_cases();
    let corpus = corpus_cases(&index);
    // Of the corpus's, of each kind: every one the family draws nothing
    // of, and the first 200 others.
    let mut kept: std::collections::HashMap<String, usize> = Default::default();
    for c in corpus {
        let kind = kind_name(&c.mark.kind);
        let rect = emission(&c.mark, Held::default()).appearance.bbox;
        let nothing = c.row == 1 && {
            let (drawn, _) = family(&c.mark, rect, Frame::Moved);
            drawn.is_none_or(|d| d.content.is_empty())
        };
        let n = kept.entry(kind).or_default();
        if nothing || *n < 200 {
            *n += usize::from(!nothing);
            cases.push(c);
        }
    }
    let mut tsv = String::from(
        "# case\trow\tkind\tturn\tfamily\tbytes\tpage.px\tpage.most\tflat.px\tflat.most\tscale\tin-place.page.px\tin-place.page.most\tin-place.flat.px\tin-place.flat.most\twhole.page.px\twhole.page.most\twhole.flat.px\twhole.flat.most\n",
    );
    let (mut alike, mut apart) = (0, 0);
    for c in &cases {
        let kind = kind_name(&c.mark.kind);
        let rect = emission(&c.mark, Held::default()).appearance.bbox;
        let (on_page, flat) = sheaf_files(c, rect);
        let name = file_name(&c.name);
        std::fs::write(looks.join(format!("{name}.pdf")), &on_page).expect("kept");
        std::fs::write(looks.join(format!("{name}-flat.pdf")), &flat).expect("kept");
        let mut line = format!("{}\t{}\t{kind}\t{}", c.name, c.row, c.mark.turn.degrees());
        if c.row == 2 {
            line.push_str("\t-\t-\t-\t-\t-\t-\tkept");
            let _ = writeln!(tsv, "{line}");
            continue;
        }
        let (drawn, ap) = family(&c.mark, rect, Frame::Moved);
        let (_, in_place) = family(&c.mark, rect, Frame::InPlace);
        let (_, whole) = family(&c.mark, rect, Frame::Whole);
        let sheaf_ap = emission(&c.mark, Held::default()).appearance;
        let said = match &drawn {
            None => "none",
            Some(d) if d.content.is_empty() => "empty",
            Some(_) => "drawn",
        };
        let bytes = if ap.content == sheaf_ap.content {
            "same"
        } else {
            "differ"
        };
        let mut differs = false;
        let _ = write!(line, "\t{said}\t{bytes}");
        for (what, sheaf, flat_ap) in [("page", &on_page, false), ("flat", &flat, true)] {
            let theirs = swapped(sheaf, &ap, flat_ap);
            let scale = scale_of(sheaf);
            let (a, b) = (
                render(sheaf, scale, &c.name),
                render(&theirs, scale, &c.name),
            );
            let (n, most) = compared(&a, &b);
            let _ = write!(line, "\t{n}\t{most}");
            if n > 0 {
                differs = true;
                std::fs::write(looks.join(format!("{name}-{what}-family.pdf")), &theirs)
                    .expect("kept");
                png(&a, &looks.join(format!("{name}-{what}-sheaf.png")));
                png(&b, &looks.join(format!("{name}-{what}-family.png")));
                png(
                    &difference(&a, &b),
                    &looks.join(format!("{name}-{what}-difference.png")),
                );
            }
        }
        let _ = write!(line, "\t{}", scale_of(&on_page));
        // The same appearance drawn where the mark is, no move: what the
        // move costs, where the two above differ.
        for (ap, sheaf, flat_ap) in [
            (&in_place, &on_page, false),
            (&in_place, &flat, true),
            (&whole, &on_page, false),
            (&whole, &flat, true),
        ] {
            let scale = scale_of(sheaf);
            let theirs = swapped(sheaf, ap, flat_ap);
            let (n, most) = compared(
                &render(sheaf, scale, &c.name),
                &render(&theirs, scale, &c.name),
            );
            let _ = write!(line, "\t{n}\t{most}");
        }
        match differs {
            true => apart += 1,
            false => alike += 1,
        }
        let _ = writeln!(tsv, "{line}");
    }
    std::fs::write(dir.join("looks.tsv"), &tsv).expect("looks.tsv");
    eprintln!(
        "{} cases ({} of the first row: {alike} alike, {apart} not), in {}",
        cases.len(),
        alike + apart,
        dir.display()
    );
}
