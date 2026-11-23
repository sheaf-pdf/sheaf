//! What an appearance form draws, read by sheaf's content interpreter, and
//! whether two forms draw alike within a rounding ([`alike`]), for the text
//! field appearances tier (`fields_corpus.rs`): each glyph shown (its
//! font, code, origin, size, mode and color), each path painted, and each
//! clip. A stroked rectangle is the ring it covers and a stroked straight
//! line the rectangle, so a border stroked and the same border filled as an
//! even-odd ring (as PDFium writes one) are alike.

use std::ops::ControlFlow;

use sheaf_doc::content::interp::{Forms, Kind, Opts, Seg, State, Wants, interpret};
use sheaf_doc::content::{Concat, Fonts};
use sheaf_doc::pdf::content::{LineCap, LineJoin, Paint};
use sheaf_doc::pdf::{Document as Pdf, Object, Stream, StreamExt};

/// The points two drawings may differ by and be alike: a rounding, far
/// below a pixel at any scale a page is drawn at.
pub const EPS: f32 = 1e-3;

/// How far two colors' components may be apart and be alike: under half
/// of one of 256 levels.
const COLOR_EPS: f32 = 1e-3;

/// A rectangle: left, bottom, right, top.
type Box4 = [f32; 4];

/// A color: its space, as the interpreter says it, and its components.
#[derive(Debug, Clone, PartialEq)]
struct Paint4 {
    space: String,
    values: Vec<f32>,
}

/// One glyph drawn.
#[derive(Debug, Clone)]
struct Drew {
    /// Its font: `/BaseFont`, kind and program; `stock` for PDFium's own
    /// Helvetica where a resource is missing.
    font: String,
    code: u32,
    /// Its width in the font, a thousandth of the size.
    width: f32,
    origin: [f32; 2],
    size: f32,
    mode: String,
    color: Paint4,
    /// Its loose box: four corners.
    quad: [[f32; 2]; 4],
}

/// What a path covers.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    /// A rectangle.
    Rect(Box4),
    /// What lies between two rectangles, the second inside the first.
    Ring(Box4, Box4),
    /// A fill of straight subpaths: each closed polygon's corners, from
    /// any of them, either way round.
    Polygons(Vec<Vec<[f32; 2]>>),
    /// Any other path: its points, in order.
    Points(Vec<[f32; 2]>),
}

/// One path painted.
#[derive(Debug, Clone)]
struct Painted {
    /// `fill` for what [`Shape`] says is covered; else how the path is
    /// painted, with its line's width, cap, join and dashes.
    how: String,
    shape: Shape,
    color: Paint4,
}

/// What a form draws.
#[derive(Debug, Default)]
pub struct Drawn {
    /// Its `/BBox` and `/Matrix` (none for the identity).
    bbox: Option<Vec<f32>>,
    matrix: Option<Vec<f32>>,
    glyphs: Vec<Drew>,
    paints: Vec<Painted>,
    clips: Vec<Shape>,
    /// The text its glyphs show, in order, as each font reads its code
    /// (U+FFFD where one reads none).
    text: String,
}

impl Drawn {
    /// The text its glyphs show.
    pub fn text(&self) -> &str {
        &self.text
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPS
}

fn near2(a: [f32; 2], b: [f32; 2]) -> bool {
    near(a[0], b[0]) && near(a[1], b[1])
}

fn near4(a: Box4, b: Box4) -> bool {
    (0..4).all(|i| near(a[i], b[i]))
}

/// `o`, followed where it's a reference, in `pdf`.
fn reached<'p>(pdf: &'p Pdf, o: &'p Object) -> Option<&'p Object> {
    match o.as_reference() {
        Some(id) => pdf.get_object(id).ok(),
        None => Some(o),
    }
}

/// The color of `paint` in `state`: its fill's for a fill, else its
/// stroke's.
fn color(state: &State, stroke: bool) -> Paint4 {
    let (space, values) = match stroke {
        true => (&state.stroke_color_space, &state.stroke_color),
        false => (&state.fill_color_space, &state.fill_color),
    };
    Paint4 {
        space: format!("{space:?}"),
        values: values.as_slice().to_vec(),
    }
}

/// Each subpath of `segs`: its points, and whether it curves.
fn subpaths(segs: &[Seg]) -> Vec<(Vec<[f32; 2]>, bool)> {
    let mut subs: Vec<(Vec<[f32; 2]>, bool)> = Vec::new();
    for seg in segs {
        match *seg {
            Seg::Move(p) => subs.push((vec![[p.x, p.y]], false)),
            Seg::Line(p) => match subs.last_mut() {
                Some((points, _)) => points.push([p.x, p.y]),
                None => subs.push((vec![[p.x, p.y]], false)),
            },
            Seg::Curve(a, b, c) => {
                if subs.is_empty() {
                    subs.push((Vec::new(), true));
                }
                let (points, curved) = subs.last_mut().expect("a subpath");
                points.extend([[a.x, a.y], [b.x, b.y], [c.x, c.y]]);
                *curved = true;
            }
            Seg::Close => {}
        }
    }
    subs
}

/// The rectangle `points` go round, where they're an axis-aligned one.
fn rect(points: &[[f32; 2]]) -> Option<Box4> {
    let mut p = points.to_vec();
    while p.len() > 1 && near2(p[0], p[p.len() - 1]) {
        p.pop();
    }
    if p.len() != 4 {
        return None;
    }
    let aligned = (0..4).all(|i| {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        near(a[0], b[0]) || near(a[1], b[1])
    });
    let (xs, ys) = (p.iter().map(|q| q[0]), p.iter().map(|q| q[1]));
    let [x0, x1] = [
        xs.clone().fold(f32::INFINITY, f32::min),
        xs.fold(f32::NEG_INFINITY, f32::max),
    ];
    let [y0, y1] = [
        ys.clone().fold(f32::INFINITY, f32::min),
        ys.fold(f32::NEG_INFINITY, f32::max),
    ];
    aligned.then_some([x0, y0, x1, y1])
}

/// A closed polygon's corners: `points` without a last one that comes
/// back to the first.
fn corners(mut points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    while points.len() > 1 && near2(points[0], points[points.len() - 1]) {
        points.pop();
    }
    points
}

/// Whether two closed polygons have the same corners, the second's from
/// any of them and either way round.
fn polygons_alike(a: &[[f32; 2]], b: &[[f32; 2]]) -> bool {
    let n = a.len();
    if n != b.len() {
        return false;
    }
    if n == 0 {
        return true;
    }
    (0..n).any(|from| {
        let ahead = (0..n).all(|i| near2(a[i], b[(from + i) % n]));
        let back = (0..n).all(|i| near2(a[i], b[(from + n - i) % n]));
        ahead || back
    })
}

/// Whether `inner` lies within `outer`.
fn inside(outer: Box4, inner: Box4) -> bool {
    outer[0] <= inner[0] + EPS
        && outer[1] <= inner[1] + EPS
        && outer[2] + EPS >= inner[2]
        && outer[3] + EPS >= inner[3]
}

/// `r` grown by `by` on every side (shrunk where `by` is less than 0).
fn grown(r: Box4, by: f32) -> Box4 {
    [r[0] - by, r[1] - by, r[2] + by, r[3] + by]
}

/// What a path painted `paint` in `state` covers, and how.
fn painted(segs: &[Seg], paint: Paint, state: &State) -> Option<Painted> {
    let subs = subpaths(segs);
    let all: Vec<[f32; 2]> = subs.iter().flat_map(|(p, _)| p.iter().copied()).collect();
    let rects: Option<Vec<Box4>> = (subs.iter())
        .map(|(p, curved)| if *curved { None } else { rect(p) })
        .collect();
    let m = state.ctm;
    let plain = near(m.a.abs(), 1.0) && near(m.d.abs(), 1.0) && near(m.b, 0.0) && near(m.c, 0.0);
    let line = |how: &str| {
        format!(
            "{how} w={} cap={:?} join={:?} dash={:?}",
            state.line_width, state.line_cap, state.line_join, state.dash.array
        )
    };
    let fill = |shape| Painted {
        how: "fill".into(),
        shape,
        color: color(state, false),
    };
    let stroked = |shape| Painted {
        how: "fill".into(),
        shape,
        color: color(state, true),
    };
    let w = state.line_width;
    Some(match paint {
        Paint::None => return None,
        Paint::Fill | Paint::FillEvenOdd => match rects.as_deref() {
            Some(&[r]) => fill(Shape::Rect(r)),
            Some(&[a, b]) if paint == Paint::FillEvenOdd && inside(a, b) => fill(Shape::Ring(a, b)),
            Some(&[a, b]) if paint == Paint::FillEvenOdd && inside(b, a) => fill(Shape::Ring(b, a)),
            _ if subs.iter().all(|(_, curved)| !curved) => Painted {
                how: format!("{paint:?}"),
                shape: Shape::Polygons(subs.into_iter().map(|(p, _)| corners(p)).collect()),
                color: color(state, false),
            },
            _ => Painted {
                how: format!("{paint:?}"),
                shape: Shape::Points(all),
                color: color(state, false),
            },
        },
        Paint::Stroke | Paint::CloseStroke if plain && w > 0.0 && state.dash.is_solid() => {
            let single = subs.len() == 1 && !subs[0].1;
            match (single, rects.as_deref()) {
                (true, Some(&[r])) if state.line_join == LineJoin::Miter => {
                    let inner = grown(r, -w / 2.0);
                    match inner[0] < inner[2] && inner[1] < inner[3] {
                        true => stroked(Shape::Ring(grown(r, w / 2.0), inner)),
                        false => stroked(Shape::Rect(grown(r, w / 2.0))),
                    }
                }
                (true, _) if subs[0].0.len() == 2 && paint == Paint::Stroke => {
                    let [a, b] = [subs[0].0[0], subs[0].0[1]];
                    let reach = match state.line_cap {
                        LineCap::Butt => 0.0,
                        LineCap::Square => w / 2.0,
                        _ => {
                            return Some(Painted {
                                how: line("stroke"),
                                shape: Shape::Points(all),
                                color: color(state, true),
                            });
                        }
                    };
                    let h = w / 2.0;
                    let r = match (near(a[0], b[0]), near(a[1], b[1])) {
                        (_, true) => [
                            a[0].min(b[0]) - reach,
                            a[1] - h,
                            a[0].max(b[0]) + reach,
                            a[1] + h,
                        ],
                        (true, _) => [
                            a[0] - h,
                            a[1].min(b[1]) - reach,
                            a[0] + h,
                            a[1].max(b[1]) + reach,
                        ],
                        _ => {
                            return Some(Painted {
                                how: line("stroke"),
                                shape: Shape::Points(all),
                                color: color(state, true),
                            });
                        }
                    };
                    stroked(Shape::Rect(r))
                }
                _ => Painted {
                    how: line("stroke"),
                    shape: Shape::Points(all),
                    color: color(state, true),
                },
            }
        }
        _ => Painted {
            how: line(&format!("{paint:?}")),
            shape: Shape::Points(all),
            color: color(state, matches!(paint, Paint::Stroke | Paint::CloseStroke)),
        },
    })
}

/// The shape a clip's path cuts to.
fn clip_shape(segs: &[Seg]) -> Shape {
    let subs = subpaths(segs);
    match subs.as_slice() {
        [(points, false)] => match rect(points) {
            Some(r) => Shape::Rect(r),
            None => Shape::Points(points.clone()),
        },
        _ => Shape::Points(subs.into_iter().flat_map(|(p, _)| p).collect()),
    }
}

/// What `form`, an appearance stream of `pdf`'s, draws: in its own space,
/// its `/Matrix` and `/BBox` beside.
pub fn drawn(pdf: &Pdf, form: &Stream) -> anyhow::Result<Drawn> {
    let numbers = |key: &[u8]| -> Option<Vec<f32>> {
        let items = reached(pdf, form.dict.get(key)?)?.as_array()?;
        items
            .iter()
            .map(|n| n.as_number().map(|v| v as f32))
            .collect()
    };
    let matrix = numbers(b"Matrix").filter(|m| m.as_slice() != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let resources = form.dict.get(b"Resources").and_then(|r| reached(pdf, r));
    let content = form.decompressed_content()?;
    let opts = Opts {
        forms: Forms::Skip,
        wants: Wants {
            text: true,
            paths: true,
            ..Wants::NONE
        },
    };
    let lim = sheaf_doc::source::Limits::DEFAULT;
    let mut drawn = Drawn {
        bbox: numbers(b"BBox"),
        matrix,
        ..Drawn::default()
    };
    let concat = Concat::new(&[&content]);
    interpret(
        pdf,
        &concat,
        resources,
        &lim,
        &Fonts::default(),
        opts,
        |event| {
            match &event.kind {
                Kind::Text(show) => {
                    let font = match (show.font, show.stock) {
                        (_, true) | (None, _) => "stock".to_string(),
                        (Some(f), false) => {
                            let base = (f.id())
                                .and_then(|id| pdf.get_dictionary(id).ok())
                                .and_then(|d| d.get(b"BaseFont"))
                                .and_then(Object::as_name)
                                .map_or("-".into(), |n| String::from_utf8_lossy(n).into_owned());
                            format!("{base}/{:?}/{:?}", f.kind(), f.program())
                        }
                    };
                    for g in show.glyphs() {
                        let width = show.font.map_or(0.0, |f| f.width(g.code));
                        let text = show.font.and_then(|f| f.code_text(g.code));
                        drawn.text.push_str(text.as_deref().unwrap_or("\u{fffd}"));
                        drawn.glyphs.push(Drew {
                            font: font.clone(),
                            code: g.code,
                            width,
                            origin: [g.origin.x, g.origin.y],
                            size: show.size,
                            mode: format!("{:?}", show.mode),
                            color: color(event.state, false),
                            quad: g.quad.0.map(|p| [p.x, p.y]),
                        });
                    }
                }
                Kind::Path(p) => {
                    if p.clip.is_some() {
                        drawn.clips.push(clip_shape(p.segments));
                    }
                    if let Some(paint) = painted(p.segments, p.paint, event.state) {
                        drawn.paints.push(paint);
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(())
        },
    )?;
    Ok(drawn)
}

fn colors_alike(a: &Paint4, b: &Paint4) -> bool {
    a.space == b.space
        && a.values.len() == b.values.len()
        && (a.values.iter().zip(&b.values)).all(|(x, y)| (x - y).abs() <= COLOR_EPS)
}

fn shapes_alike(a: &Shape, b: &Shape) -> bool {
    match (a, b) {
        (Shape::Rect(a), Shape::Rect(b)) => near4(*a, *b),
        (Shape::Ring(a, c), Shape::Ring(b, d)) => near4(*a, *b) && near4(*c, *d),
        (Shape::Polygons(a), Shape::Polygons(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(p, q)| polygons_alike(p, q))
        }
        (Shape::Points(a), Shape::Points(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(p, q)| near2(*p, *q))
        }
        _ => false,
    }
}

fn numbers_alike(a: &Option<Vec<f32>>, b: &Option<Vec<f32>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(*x, *y)),
        _ => false,
    }
}

/// Whether `ours` and `theirs` draw alike: the same box and matrix, the
/// same glyphs in the same fonts and colors at the same sizes and places,
/// and the same paths painted the same, each number within [`EPS`]; and the
/// same clips, or where `theirs` writes none, one of `ours` that cuts no
/// glyph's box. Else the first thing that differs.
pub fn alike(ours: &Drawn, theirs: &Drawn) -> Result<(), String> {
    if !numbers_alike(&ours.bbox, &theirs.bbox) {
        return Err("bbox".into());
    }
    if !numbers_alike(&ours.matrix, &theirs.matrix) {
        return Err("matrix".into());
    }
    if ours.glyphs.len() != theirs.glyphs.len() {
        return Err(format!(
            "glyphs:{}/{}",
            ours.glyphs.len(),
            theirs.glyphs.len()
        ));
    }
    for (i, (a, b)) in ours.glyphs.iter().zip(&theirs.glyphs).enumerate() {
        let what = match () {
            _ if a.code != b.code => "code",
            _ if a.font != b.font => "font",
            _ if !near(a.width, b.width) => "width",
            _ if a.mode != b.mode => "mode",
            _ if !near(a.size, b.size) => "size",
            _ if !colors_alike(&a.color, &b.color) => "color",
            _ if !near2(a.origin, b.origin) => {
                let (dx, dy) = (b.origin[0] - a.origin[0], b.origin[1] - a.origin[1]);
                return Err(format!("glyph{i}:moved:{dx:.4},{dy:.4}"));
            }
            _ => continue,
        };
        return Err(format!("glyph{i}:{what}"));
    }
    if ours.paints.len() != theirs.paints.len() {
        return Err(format!(
            "paints:{}/{}",
            ours.paints.len(),
            theirs.paints.len()
        ));
    }
    for (i, (a, b)) in ours.paints.iter().zip(&theirs.paints).enumerate() {
        let what = match () {
            _ if a.how != b.how => "how",
            _ if !shapes_alike(&a.shape, &b.shape) => "shape",
            _ if !colors_alike(&a.color, &b.color) => "color",
            _ => continue,
        };
        return Err(format!("paint{i}:{what}"));
    }
    let same_clips = ours.clips.len() == theirs.clips.len()
        && (ours.clips.iter().zip(&theirs.clips)).all(|(a, b)| shapes_alike(a, b));
    if same_clips {
        return Ok(());
    }
    if !theirs.clips.is_empty() {
        return Err("clips".into());
    }
    // Only ours clips: alike where its clip cuts no glyph's box.
    let mut cut: Box4 = [
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::INFINITY,
        f32::INFINITY,
    ];
    for clip in &ours.clips {
        let Shape::Rect(r) = clip else {
            return Err("clips".into());
        };
        cut = [
            cut[0].max(r[0]),
            cut[1].max(r[1]),
            cut[2].min(r[2]),
            cut[3].min(r[3]),
        ];
    }
    let cuts = (ours.glyphs.iter()).flat_map(|g| g.quad).any(|p| {
        p[0] < cut[0] - EPS || p[1] < cut[1] - EPS || p[0] > cut[2] + EPS || p[1] > cut[3] + EPS
    });
    match cuts {
        true => Err("clip-cuts".into()),
        false => Ok(()),
    }
}
