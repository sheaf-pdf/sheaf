//! What sheaf draws of the marks pdf-annots draws for it now (M18 slice
//! 24), set beside what it drew of them before: for the kinds the family
//! draws as sheaf did (a highlight; a square, a page border, a polygon
//! and a polyline, not cloudy; ink) and those it learned from sheaf (a
//! caret, an attachment's icon, a cloudy square, ellipse or polygon), and
//! a plain ellipse, which sheaf still draws, each case is saved as sheaf
//! saves it now, on its page and flattened, and rendered by PDFium at 4
//! pixels a point beside the files sheaf saved of it before the family
//! drew it (`$SHEAF_LOOKS_BEFORE`, else M18 slice 24's step 2 records):
//! compared pixel by pixel, a line a case in `looks.tsv`, each case's
//! files kept in `looks/` with the renders of a case that differs.
//!
//! The cases are the ones the records before hold, by name: every kind's
//! marks as the golden fixtures have them, at each turn; each kind's edges
//! (the least and greatest line the tools give, and none; dashed; filled;
//! an opacity under 1; a box under four lines wide; a path of one point
//! and of two; a quad of no height; cloudy; off the grid of whole points);
//! and the corpus's comments of these kinds, as sheaf converts them, read
//! from the files the annotations dump names (`$SHEAF_LOOKS_INDEX`, or the
//! baseline dump of M18 slice 24).
//!
//! Ignored: it writes to `$SHEAF_LOOKS_DIR` (else `target/s24/step4`), and
//! needs libpdfium (`scripts/fetch-pdfium.sh`; `PDFIUM_LIB_DIR` picks
//! another build).

// A test crate of its own, whose files go where its records are kept.
#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sheaf_doc::annot::emission;
use sheaf_doc::forms::corpus::Corpus;
use sheaf_doc::forms::fixture as forms;
use sheaf_doc::geom::Affine;
use sheaf_doc::import::{Interned, Options};
use sheaf_doc::pdf::gfx::Rect;
use sheaf_doc::pdf::{
    Dictionary, Document as Pdf, Object, ObjectId, Stream, StreamExt, dictionary,
};
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

/// A folder of records: `$var`, else `step`'s of M18 slice 24.
fn records(var: &str, step: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/s24")
            .join(step)
    })
}

/// A case: its name, which row of the comparison it was in before (1, a
/// kind the family drew as sheaf did; 2, one it learned from sheaf), the
/// mark, and what the mark's store must hold for it.
struct Case {
    name: String,
    row: u8,
    mark: Annot,
    /// Blobs to intern in the case's store: a stamp's page (`true`), a
    /// file.
    blobs: Vec<(Arc<[u8]>, bool)>,
}

/// Which row `a` was in, or none: a plain ellipse was the first row's
/// before it stayed sheaf's drawing.
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

/// The page dictionary of `pdf`'s one page.
fn page_of(pdf: &Pdf) -> ObjectId {
    pdf.page_iter().next().expect("a page")
}

/// The contents of the forms sheaf drew for its mark in `bytes` (a
/// replacement's caret and its strikeout), decoded: each annotation's
/// normal appearance (`flat` false), or the forms its flatten placed on the
/// page (`flat`). What differs between two saves of a mark but for its id
/// and dates.
fn form_contents(bytes: &[u8], flat: bool) -> Vec<Vec<u8>> {
    let pdf = Pdf::load_mem(bytes).expect("sheaf's file reads");
    let page = page_of(&pdf);
    let forms: Vec<ObjectId> = match flat {
        false => {
            let annots: Vec<ObjectId> = (pdf.get_dictionary(page).ok())
                .and_then(|d| d.get(b"Annots"))
                .and_then(|a| pdf.dereference(a).ok())
                .and_then(|(_, a)| {
                    a.as_array()
                        .map(|a| a.iter().filter_map(Object::as_reference).collect())
                })
                .unwrap_or_default();
            (annots.iter())
                .map(|&id| {
                    let dict = pdf.get_dictionary(id).expect("a mark");
                    let (_, ap) = pdf
                        .dereference(dict.get(b"AP").expect("its /AP"))
                        .expect("its /AP");
                    ap.as_dict()
                        .and_then(|d| d.get(b"N"))
                        .and_then(Object::as_reference)
                        .expect("its /N")
                })
                .collect()
        }
        true => {
            let resources = pdf.resources_of_page(page).expect("the page's resources");
            let (_, forms) = pdf
                .dereference(resources.get(b"XObject").expect("its forms"))
                .expect("its forms");
            (forms.as_dict().into_iter().flat_map(Dictionary::iter))
                .filter_map(|(_, o)| o.as_reference())
                .collect()
        }
    };
    assert!(!forms.is_empty(), "a form");
    (forms.into_iter())
        .map(|form| {
            let stream = (pdf.get_object(form).ok())
                .and_then(|o| o.as_stream())
                .expect("a form");
            stream.decompressed_content().expect("its content decodes")
        })
        .collect()
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

/// Whether sheaf draws `a` itself now, or the family does.
fn drawn_by(a: &Annot) -> &'static str {
    match (&a.kind, a.cloudy) {
        (AnnotKind::Ellipse { .. }, false) => "sheaf",
        _ => "family",
    }
}

/// The cases the records before hold, by name, each with its row: the
/// first line a header.
fn cases_before(dir: &Path) -> BTreeMap<String, u8> {
    let tsv = dir.join("looks.tsv");
    let text = std::fs::read_to_string(&tsv)
        .unwrap_or_else(|e| panic!("{}: {e}: the records before", tsv.display()));
    (text.lines())
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let mut fields = l.split('\t');
            let name = fields.next().expect("a case").to_string();
            let row = (fields.next().and_then(|r| r.parse().ok())).expect("its row");
            (name, row)
        })
        .collect()
}

/// Every case the records before hold, a line each in `looks.tsv`: its
/// files now (kept in `looks/`, as the records before keep theirs) set
/// beside its files then, rendered, and the renders of a case whose two
/// differ kept beside them.
#[test]
#[ignore = "renders with PDFium into $SHEAF_LOOKS_DIR: M18 slice 24's comparison of looks"]
fn what_sheaf_draws_now_set_beside_what_it_drew_before() {
    let dir = records("SHEAF_LOOKS_DIR", "step4");
    let before = records("SHEAF_LOOKS_BEFORE", "step2");
    let looks = dir.join("looks");
    std::fs::create_dir_all(&looks).expect("the records' folder");
    let index = std::env::var_os("SHEAF_LOOKS_INDEX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/s24/baseline/annots.tsv")
        });
    let wanted = cases_before(&before);
    let mut cases = fixture_cases();
    cases.extend(corpus_cases(&index));
    cases.retain(|c| wanted.contains_key(&c.name));
    let found: BTreeSet<&str> = cases.iter().map(|c| c.name.as_str()).collect();
    let missing: Vec<&String> = (wanted.keys())
        .filter(|n| !found.contains(n.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "cases the records before have: {missing:?}"
    );
    assert_eq!(found.len(), cases.len(), "each case once");
    let mut tsv = String::from(
        "# case\trow\tkind\tturn\tdrawn\tpage.content\tflat.content\tpage.px\tpage.most\tflat.px\tflat.most\tscale\n",
    );
    let (mut alike, mut apart) = (0, 0);
    for c in &cases {
        assert_eq!(wanted[&c.name], c.row, "{}: its row", c.name);
        let kind = kind_name(&c.mark.kind);
        let rect = emission(&c.mark, Held::default()).appearance.bbox;
        let name = file_name(&c.name);
        let (on_page, flat) = sheaf_files(c, rect);
        std::fs::write(looks.join(format!("{name}.pdf")), &on_page).expect("kept");
        std::fs::write(looks.join(format!("{name}-flat.pdf")), &flat).expect("kept");
        let mut line = format!(
            "{}\t{}\t{kind}\t{}\t{}",
            c.name,
            c.row,
            c.mark.turn.degrees(),
            drawn_by(&c.mark)
        );
        let mut renders = Vec::new();
        for (what, now) in [("page", &on_page), ("flat", &flat)] {
            let suffix = if what == "flat" { "-flat" } else { "" };
            let then_path = before.join("looks").join(format!("{name}{suffix}.pdf"));
            let then = std::fs::read(&then_path)
                .unwrap_or_else(|e| panic!("{}: {e}", then_path.display()));
            let flat = what == "flat";
            let same = match form_contents(&then, flat) == form_contents(now, flat) {
                true => "same",
                false => "differ",
            };
            let _ = write!(line, "\t{same}");
            renders.push((what, then, now.clone()));
        }
        let mut differs = false;
        for (what, then, now) in &renders {
            let scale = scale_of(then);
            assert_eq!(scale, scale_of(now), "{}: the same page", c.name);
            let (a, b) = (render(then, scale, &c.name), render(now, scale, &c.name));
            let (n, most) = compared(&a, &b);
            let _ = write!(line, "\t{n}\t{most}");
            if n > 0 {
                differs = true;
                png(&a, &looks.join(format!("{name}-{what}-before.png")));
                png(&b, &looks.join(format!("{name}-{what}-now.png")));
                png(
                    &difference(&a, &b),
                    &looks.join(format!("{name}-{what}-difference.png")),
                );
            }
        }
        let _ = write!(line, "\t{}", scale_of(&on_page));
        match differs {
            true => apart += 1,
            false => alike += 1,
        }
        let _ = writeln!(tsv, "{line}");
    }
    std::fs::write(dir.join("looks.tsv"), &tsv).expect("looks.tsv");
    eprintln!(
        "{} cases: {alike} alike, {apart} not, in {}",
        cases.len(),
        dir.display()
    );
}
