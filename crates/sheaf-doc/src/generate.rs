//! What a generator draws on a page (M10.5 §3.5): a watermark or
//! background, or the header and footer, laid out when a save writes the
//! page, on the page as it shows (its visible box, turned as the page is
//! turned), so it stays upright through a turn and on the page's own boxes
//! through a crop.
//!
//! - [`layout`]: a generator's content on one page, a [`Drawn`]: an
//!   `/Artifact` whose property list (`/Type /Pagination`, its kind's
//!   `/Subtype`, and `/SheafGen` naming the generator) is named in the
//!   page's `/Properties`, its graphics state isolated in `q`…`Q`. A
//!   watermark's text is in its [`FontChoice`]'s standard 14 face (a
//!   character the face can't draw is `?`); an image or a PDF's page is
//!   [`Art`] the emitter gives it, `scale` of the page's width wide. It's
//!   placed in a cell of a 3×3 grid over the page, 36 pt in from a side it
//!   keeps to, its rotated bounding box inside. The header and footer's
//!   slots are each an artifact of their own, `/Header` or `/Footer`, their
//!   text in the spec's face along the page as it shows, their tokens read
//!   of the page [`PageCtx`] says ([`header`]). [`layout_digest`] is its
//!   digest alone (`/L`), what a block laid out alike again is told by.
//! - [`recipe_digest`]: what a generator draws on each page it's on, which
//!   its blocks say they were drawn from (`/R`).
//! - [`catalog_json`]: what the catalog's `/SheafGenerators` says of the
//!   document's generators, from which an open finds them again: versioned
//!   JSON, its specs without paths or object numbers (an image or a page is
//!   `"placed"`, the XObject its blocks draw, with the digest of the
//!   resource it was drawn as and an image's turns), a header's date and
//!   file; its Bates numbers are its blocks' (`/N`).
//! - [`read`]: what an open finds ([`Saved`]): the generators
//!   `/SheafGenerators` says there are, on the pages whose streams'
//!   markers name them (empty blocks too), an image or a page bound to the
//!   XObject a block of it draws, a header's Bates numbers its blocks'.
//!   [`unfit`] and [`header::unfit`] say what a generator can't draw, for
//!   an op and for what's read.
//! - [`header`]: the header and footer's slots, their tokens read on a
//!   page and placed on it.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use lopdf::{Dictionary, LazyPdf, Object, ObjectId, StringFormat, dictionary};
use serde::{Deserialize, Serialize};
use sheaf_edit::{FontChoice, GridPos, HeaderFooterSpec, Z};

use crate::annot::{Box2, Pt};
use crate::content::{
    self, BlockAt, Color, ContentWriter, Font, GState, GenId, Marker, PaginationKind, Res, ResBody,
    ResCx, ResId, Reserved, ResourceNeeds, SheafBlock, Std14, Tok,
};
use crate::geom::{Affine, Geom, Rect};
use crate::materialize::emit::catalog::GENERATORS;
use crate::model::{
    DocState, Frozen, GenSpec, Generator, ImageRef, PageCtx, PageForm, PageId, Placed, Scope,
    WatermarkContent, WatermarkGen,
};
use crate::source::{ObjRef, Source};
use crate::store::Src;

/// What a generator draws on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    /// The content, its resources named as `res` names them. Empty when
    /// it draws nothing there.
    pub ops: Arc<[u8]>,
    pub res: ResourceNeeds,
    /// A digest of what it draws: its operators and the resources they
    /// draw with, by their ids, never their names.
    pub layout_digest: u128,
}

/// A watermark's image or PDF page, as the file being written has it: a
/// resource to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Art {
    /// An image XObject `width` by `height` pixels, which shows upright
    /// turned `turns` quarter turns clockwise.
    Image {
        res: Res,
        width: u32,
        height: u32,
        turns: u8,
    },
    /// A form XObject that draws the page, its `/BBox` and `/Matrix`.
    Form {
        res: Res,
        bbox: Rect,
        matrix: Affine,
    },
}

/// Space kept from a side for a watermark placed at it, in points.
const EDGE: f32 = 36.0;

/// Text's height, for placing it: about Helvetica's cap height, a part of
/// its size, and taken for every Std14 face's.
const CAP_HEIGHT: f32 = 0.72;

/// Generator `g` on the page `ctx` says, a watermark's image or page `art`
/// (None for text, or one that can't be had, which draws nothing); the
/// names it draws with kept clear of `reserved`, those the page's
/// resources bind already. Empty where it draws nothing.
pub fn layout(g: &Generator, ctx: &PageCtx, art: Option<&Art>, reserved: &Reserved) -> Drawn {
    let drawn = write(g, ctx, art, |out| {
        let (ops, res) = out.named(reserved, &[]);
        Drawn {
            ops: ops.into(),
            res,
            layout_digest: digest(out),
        }
    });
    drawn.unwrap_or_else(Drawn::empty)
}

/// [`layout`]'s digest alone: what a block of it there says it is
/// (`/L`), with no names to choose.
pub fn layout_digest(g: &Generator, ctx: &PageCtx, art: Option<&Art>) -> u128 {
    write(g, ctx, art, digest).unwrap_or_else(|| Drawn::empty().layout_digest)
}

/// A digest of what `out` draws: its content named as though no page's
/// resources bound anything, so the names are the resources' own, and
/// their ids.
fn digest(out: &ContentWriter) -> u128 {
    let (mut bytes, needs) = out.named(&Reserved::default(), &[]);
    for need in &needs.0 {
        bytes.extend(need.res.id.0.to_be_bytes());
    }
    sheaf_util::checksum(&bytes)
}

/// What [`layout`] writes, handed to `done`: None when it draws nothing.
fn write<R>(
    g: &Generator,
    ctx: &PageCtx,
    art: Option<&Art>,
    done: impl FnOnce(&ContentWriter) -> R,
) -> Option<R> {
    match &g.spec {
        GenSpec::Watermark(wm) => watermark(g, wm, &ctx.geom, art, done),
        GenSpec::HeaderFooter(hf) => header_footer(g, hf, ctx, done),
    }
}

/// The list a generator's artifact of `kind` is marked with: named in the
/// page's `/Properties`, it says whose it is.
fn list(g: &Generator, kind: PaginationKind) -> Dictionary {
    dictionary! {
        "Type" => "Pagination",
        "Subtype" => Object::Name(kind.name().to_vec()),
        "SheafGen" => Object::String(g.id.to_string().into_bytes(), StringFormat::Literal),
    }
}

/// Header and footer `hf`, generator `g`'s, on the page `ctx` says, handed
/// to `done`: each slot that isn't blank there an artifact of its own,
/// marked as the header's or the footer's, its text in `hf`'s face along
/// the page as it shows, its baseline where [`header::layout`] places it.
/// None when every slot is blank there.
fn header_footer<R>(
    g: &Generator,
    hf: &HeaderFooterSpec,
    ctx: &PageCtx,
    done: impl FnOnce(&ContentWriter) -> R,
) -> Option<R> {
    let face = Std14::from(hf.font);
    let lines = header::layout(hf, &g.frozen, ctx, &face);
    if lines.is_empty() {
        return None;
    }
    // The page as shown to user space: where its x axis and its up (its y
    // axis down) go.
    let to_user = ctx.geom.from_view();
    let o = to_user.apply(Pt { x: 0.0, y: 0.0 });
    let ex = to_user.apply(Pt { x: 1.0, y: 0.0 });
    let ey = to_user.apply(Pt { x: 0.0, y: 1.0 });
    let mut out = ContentWriter::new();
    for line in &lines {
        let kind = match line.header {
            true => PaginationKind::Header,
            false => PaginationKind::Footer,
        };
        let at = to_user.apply(Pt {
            x: line.x,
            y: line.baseline,
        });
        out.begin_marked("Artifact", list(g, kind));
        out.save();
        out.begin_text();
        out.fill_color(Color::rgb8(hf.color));
        out.font(&face, line.size);
        out.text_matrix(Affine {
            a: ex.x - o.x,
            b: ex.y - o.y,
            c: o.x - ey.x,
            d: o.y - ey.y,
            e: at.x,
            f: at.y,
        });
        out.text(&line.text);
        out.end_text();
        out.restore();
        out.end_marked();
    }
    Some(done(&out))
}

/// Watermark `wm`, generator `g`'s, on a page of `geom`, its image or page
/// `art`, handed to `done`: None when it draws nothing.
fn watermark<R>(
    g: &Generator,
    wm: &WatermarkGen,
    geom: &Geom,
    art: Option<&Art>,
    done: impl FnOnce(&ContentWriter) -> R,
) -> Option<R> {
    let size = geom.shown();
    // What it draws, its size before it's turned, in points, and its
    // angle.
    let (draws, opacity, w, h, angle) = match (&wm.content, art) {
        (
            WatermarkContent::Text {
                text,
                font,
                size,
                color,
                opacity,
                angle,
            },
            _,
        ) => {
            let face = Std14::from(*font);
            let width = face.width(text, *size);
            let text = Draws::Text {
                face,
                text,
                size: *size,
                color: *color,
            };
            (text, *opacity, width, size * CAP_HEIGHT, *angle)
        }
        (
            WatermarkContent::Image { scale, opacity, .. },
            Some(Art::Image {
                res,
                width,
                height,
                turns,
            }),
        ) => {
            let (across, up) = match turns % 2 {
                0 => (*width, *height),
                _ => (*height, *width),
            };
            let w = size.width * scale;
            let h = w * up.max(1) as f32 / across.max(1) as f32;
            (Draws::Image(res, *turns), *opacity, w, h, 0.0)
        }
        (WatermarkContent::Page { scale, opacity, .. }, Some(Art::Form { res, bbox, matrix })) => {
            let shown = matrix.bounds(*bbox);
            let w = size.width * scale;
            let h = w * shown.height() / shown.width();
            (Draws::Form(res, *bbox, *matrix), *opacity, w, h, 0.0)
        }
        _ => return None,
    };
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let frame = frame(geom, wm.position, w, h, angle);
    let mut out = ContentWriter::new();
    out.begin_marked("Artifact", list(g, WatermarkGen::kind_at(g.z)));
    out.save();
    out.gstate(GState {
        fill_alpha: Some(opacity),
        stroke_alpha: Some(opacity),
        blend: None,
    });
    out.transform(frame);
    match &draws {
        Draws::Text {
            face,
            text,
            size,
            color,
        } => {
            out.begin_text();
            out.fill_color(Color::rgb8(*color));
            out.font(face, *size);
            out.text(text);
            out.end_text();
        }
        Draws::Image(res, turns) => {
            out.save();
            out.transform(turned(*turns, w, h));
            out.draw((*res).clone());
            out.restore();
        }
        Draws::Form(res, bbox, matrix) => {
            let target = Box2 {
                x: 0.0,
                y: 0.0,
                w,
                h,
            };
            out.place_form((*res).clone(), *bbox, *matrix, target);
        }
    }
    out.restore();
    out.end_marked();
    Some(done(&out))
}

/// The image or page `placed` is (an image when `image`), as a resource
/// drawn as `body`: an image's size and the turns it shows upright at, a
/// page's frame, read from its XObject's dictionary. None when that isn't
/// one.
pub(crate) fn placed_art(placed: &Placed, image: bool, body: ResBody) -> Option<Art> {
    let res = Res {
        id: placed.res,
        body,
    };
    let pdf = placed.at.src.pdf();
    if !image {
        let (bbox, matrix) = crate::stamp_lib::form_frame(pdf, placed.at.id)?;
        return Some(Art::Form { res, bbox, matrix });
    }
    let dict = &pdf.get(placed.at.id).ok()?.as_stream().ok()?.dict;
    let size = |key: &[u8]| -> Option<u32> {
        let (_, n) = pdf.dereference(dict.get(key).ok()?).ok()?;
        u32::try_from(n.as_i64().ok()?).ok()
    };
    Some(Art::Image {
        res,
        width: size(b"Width")?,
        height: size(b"Height")?,
        turns: placed.turns,
    })
}

/// What a watermark draws on a page, resolved.
enum Draws<'a> {
    Text {
        face: Std14,
        text: &'a str,
        size: f32,
        color: [u8; 3],
    },
    /// An image, and the quarter turns clockwise it shows upright at.
    Image(&'a Res, u8),
    /// A form, its `/BBox` and `/Matrix`.
    Form(&'a Res, Rect, Affine),
}

impl Drawn {
    /// Nothing drawn.
    fn empty() -> Drawn {
        Drawn {
            ops: Arc::from([]),
            res: ResourceNeeds::default(),
            layout_digest: sheaf_util::checksum(&[]),
        }
    }
}

/// The map from a watermark's own frame (x along it, y up, its bottom-left
/// corner at the origin), `w` by `h` and turned `angle` degrees
/// counterclockwise on the page as it shows, to the page's user space: in
/// the cell `at` of the page of `geom`, as shown.
fn frame(geom: &Geom, at: GridPos, w: f32, h: f32, angle: f32) -> Affine {
    let size = geom.shown();
    // In the page as shown, y down: x along the watermark, y up from it.
    let a = angle.to_radians();
    let ux = (a.cos(), -a.sin());
    let uy = (-a.sin(), -a.cos());
    // Its turned bounding box, to keep one placed at a side on the page.
    let bw = (w * a.cos()).abs() + (h * a.sin()).abs();
    let bh = (w * a.sin()).abs() + (h * a.cos()).abs();
    let place = |slot: u8, page: f32, extent: f32| match slot {
        0 => EDGE + extent / 2.0,
        2 => page - EDGE - extent / 2.0,
        _ => page / 2.0,
    };
    let (across, down) = cell(at);
    let cx = place(across, size.width, bw);
    let cy = place(down, size.height, bh);
    // Its bottom-left corner.
    let ox = cx - ux.0 * w / 2.0 - uy.0 * h / 2.0;
    let oy = cy - ux.1 * w / 2.0 - uy.1 * h / 2.0;
    // The page as shown to user space, an affine map: where its axes go.
    let to_user = geom.from_view();
    let o = to_user.apply(Pt { x: 0.0, y: 0.0 });
    let ex = to_user.apply(Pt { x: 1.0, y: 0.0 });
    let ey = to_user.apply(Pt { x: 0.0, y: 1.0 });
    let map = |(vx, vy): (f32, f32)| {
        (
            (ex.x - o.x) * vx + (ey.x - o.x) * vy,
            (ex.y - o.y) * vx + (ey.y - o.y) * vy,
        )
    };
    let origin = to_user.apply(Pt { x: ox, y: oy });
    let (a, b) = map(ux);
    let (c, d) = map(uy);
    Affine {
        a,
        b,
        c,
        d,
        e: origin.x,
        f: origin.y,
    }
}

/// A grid cell's column and row: 0 left or top, 1 the middle, 2 right or
/// bottom.
fn cell(at: GridPos) -> (u8, u8) {
    match at {
        GridPos::TopLeft => (0, 0),
        GridPos::Top => (1, 0),
        GridPos::TopRight => (2, 0),
        GridPos::Left => (0, 1),
        GridPos::Center => (1, 1),
        GridPos::Right => (2, 1),
        GridPos::BottomLeft => (0, 2),
        GridPos::Bottom => (1, 2),
        GridPos::BottomRight => (2, 2),
    }
}

/// The map that draws an image, which shows upright turned `turns` quarter
/// turns clockwise, into a box `w` by `h` from the origin, upright: the
/// image's unit square onto the box, turned.
fn turned(turns: u8, w: f32, h: f32) -> Affine {
    let (a, b, c, d, e, f) = match turns % 4 {
        0 => (w, 0.0, 0.0, h, 0.0, 0.0),
        1 => (0.0, -h, w, 0.0, 0.0, h),
        2 => (-w, 0.0, 0.0, -h, w, h),
        _ => (0.0, h, -w, 0.0, w, 0.0),
    };
    Affine { a, b, c, d, e, f }
}

/// What generator `g` draws on a page, the render key's digest of it.
pub(crate) use crate::realize::recipe_digest;

/// What generator `g`'s layout reads of page `id` of `st`, at place
/// `position`, its boxes and turn `geom`: its label only when its tokens
/// read it, and its Bates number, fixed when it was set.
pub(crate) fn page_ctx(
    st: &DocState,
    g: &Generator,
    id: PageId,
    position: usize,
    geom: &Geom,
) -> PageCtx {
    let label = match g.needs().label {
        true => st.label_of(id).unwrap_or_default(),
        false => Arc::default(),
    };
    PageCtx {
        geom: *geom,
        position: position as u32,
        count: st.page_count() as u32,
        label,
        bates: g.frozen.bates_number(id),
    }
}

/// The catalog's `/SheafGenerators`, the document's generators as JSON in
/// ASCII: None when it has none. `drawn_as` says what each image or page
/// is drawn as: its resource's digest, and the quarter turns an image
/// shows upright at.
pub fn catalog_json(
    st: &DocState,
    mut drawn_as: impl FnMut(&WatermarkContent) -> (ResId, u8),
) -> Option<Vec<u8>> {
    let generators: Vec<StoredGen> = (st.generators().values())
        .map(|g| StoredGen::of(g, &mut drawn_as))
        .collect();
    if generators.is_empty() {
        return None;
    }
    let stored = Stored {
        v: STORED_VERSION,
        generators,
    };
    let json = serde_json::to_string(&stored).expect("generators serialize");
    Some(ascii(&json))
}

/// The version of `/SheafGenerators`' JSON.
const STORED_VERSION: u32 = 1;

/// `/SheafGenerators`, as JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Stored {
    v: u32,
    generators: Vec<StoredGen>,
}

/// A generator as `/SheafGenerators` holds it: its id as its blocks name it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
struct StoredGen {
    id: String,
    kind: StoredKind,
    z: Z,
    scope: StoredScope,
    replaces: Vec<StoredPagination>,
    spec: StoredSpec,
    /// What a header and footer fixed when it was set, but its Bates
    /// numbers, which are its blocks' (`/N`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frozen: Option<StoredFrozen>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum StoredKind {
    Watermark,
    HeaderFooter,
}

/// The pages it's on: every page, or those whose blocks name it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum StoredScope {
    All,
    Pages,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum StoredPagination {
    Header,
    Footer,
    Watermark,
    Background,
}

impl StoredPagination {
    fn of(kind: PaginationKind) -> StoredPagination {
        match kind {
            PaginationKind::Header => StoredPagination::Header,
            PaginationKind::Footer => StoredPagination::Footer,
            PaginationKind::Watermark => StoredPagination::Watermark,
            PaginationKind::Background => StoredPagination::Background,
        }
    }
}

/// What it draws: a watermark's content and place, or a header and
/// footer's spec, its face said.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
enum StoredSpec {
    Watermark(StoredWatermark),
    HeaderFooter(HeaderFooterSpec),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct StoredWatermark {
    content: StoredContent,
    position: GridPos,
}

/// What a header and footer's `<<date>>` and `<<file>>` say.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct StoredFrozen {
    date: String,
    file: String,
}

/// What it draws: an image or a page is the XObject its blocks draw.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum StoredContent {
    Text {
        text: String,
        font: FontChoice,
        size: f32,
        color: [u8; 3],
        opacity: f32,
        angle: f32,
    },
    Image {
        image: StoredArt,
        scale: f32,
        opacity: f32,
    },
    Page {
        pdf: StoredArt,
        scale: f32,
        opacity: f32,
    },
}

/// An image or a page: the XObject the generator's blocks draw, placed as
/// the resource whose digest `placed` is (32 hex digits), and the quarter
/// turns clockwise an image shows upright at.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct StoredArt {
    placed: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    turns: u8,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

impl StoredArt {
    fn of((res, turns): (ResId, u8)) -> StoredArt {
        StoredArt {
            placed: format!("{:032x}", res.0),
            turns,
        }
    }

    /// The resource it was drawn as.
    fn res(&self) -> Option<ResId> {
        let hex = self.placed.as_str();
        (hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| u128::from_str_radix(hex, 16).ok().map(ResId))
            .flatten()
    }
}

impl StoredGen {
    fn of(g: &Generator, drawn_as: &mut impl FnMut(&WatermarkContent) -> (ResId, u8)) -> StoredGen {
        let (kind, spec, frozen) = match &g.spec {
            GenSpec::Watermark(wm) => {
                let content = match &wm.content {
                    WatermarkContent::Text {
                        text,
                        font,
                        size,
                        color,
                        opacity,
                        angle,
                    } => StoredContent::Text {
                        text: text.to_string(),
                        font: *font,
                        size: *size,
                        color: *color,
                        opacity: *opacity,
                        angle: *angle,
                    },
                    WatermarkContent::Image { scale, opacity, .. } => StoredContent::Image {
                        image: StoredArt::of(drawn_as(&wm.content)),
                        scale: *scale,
                        opacity: *opacity,
                    },
                    WatermarkContent::Page { scale, opacity, .. } => StoredContent::Page {
                        pdf: StoredArt::of(drawn_as(&wm.content)),
                        scale: *scale,
                        opacity: *opacity,
                    },
                };
                let spec = StoredWatermark {
                    content,
                    position: wm.position,
                };
                (StoredKind::Watermark, StoredSpec::Watermark(spec), None)
            }
            GenSpec::HeaderFooter(hf) => {
                let frozen = StoredFrozen {
                    date: g.frozen.date.to_string(),
                    file: g.frozen.file.to_string(),
                };
                let spec = StoredSpec::HeaderFooter(HeaderFooterSpec::clone(hf));
                (StoredKind::HeaderFooter, spec, Some(frozen))
            }
        };
        StoredGen {
            id: g.id.to_string(),
            kind,
            z: g.z,
            scope: match g.scope {
                Scope::All => StoredScope::All,
                Scope::Pages(_) => StoredScope::Pages,
            },
            replaces: (g.replaces.iter())
                .map(|&kind| StoredPagination::of(kind))
                .collect(),
            spec,
            frozen,
        }
    }

    /// The kinds of pagination artifact one of its kind at its z replaces,
    /// as sheaf writes it: a watermark's or background's own, a header and
    /// footer's both.
    fn replacing(&self) -> Vec<StoredPagination> {
        match self.kind {
            StoredKind::Watermark => vec![StoredPagination::of(WatermarkGen::kind_at(self.z))],
            StoredKind::HeaderFooter => vec![StoredPagination::Header, StoredPagination::Footer],
        }
    }
}

/// A generator a file's `/SheafGenerators` and its pages' blocks say sheaf
/// drew on it (§3.5), as [`read`] finds it: what an open makes a generator
/// of ([`Saved::generator`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub id: GenId,
    /// The pages holding a block of it, by index, in order: its scope,
    /// with those it draws nothing on. None when it's on every page, those
    /// added later too.
    pub pages: Option<Vec<u32>>,
    stored: StoredGen,
    /// The XObject its image or page is: the one its first block that
    /// draws one draws.
    placed: Option<ObjectId>,
    /// A header's Bates numbers, by page index: its blocks' (`/N`).
    numbers: BTreeMap<u32, u64>,
    /// Its blocks in streams of their own that say both digests, in order:
    /// each one's page, by index, its stream, and its layout's and recipe's
    /// digests (`/L`, `/R`).
    blocks: Vec<(u32, ObjectId, u128, u128)>,
    /// The pages a header with Bates numbers is on whose blocks another
    /// tool merged into a stream of the page's, which keeps no number.
    unnumbered: Vec<u32>,
}

impl Saved {
    /// It as a generator of a document whose pages of `src`, its file, are
    /// `page` by index: None when none of the pages it's on is one. A
    /// header's Bates numbers are on the pages its blocks number.
    pub fn generator(&self, src: &Src, page: impl Fn(u32) -> Option<PageId>) -> Option<Generator> {
        let scope = match &self.pages {
            None => Scope::All,
            Some(pages) => {
                let on: im::OrdSet<PageId> = pages.iter().filter_map(|&i| page(i)).collect();
                if on.is_empty() {
                    return None;
                }
                Scope::Pages(on)
            }
        };
        let stored = &self.stored;
        let (spec, frozen) = match &stored.spec {
            StoredSpec::Watermark(wm) => {
                let spec = WatermarkGen {
                    content: self.content(src, &wm.content)?,
                    position: wm.position,
                };
                (GenSpec::Watermark(Arc::new(spec)), Frozen::default())
            }
            StoredSpec::HeaderFooter(hf) => {
                let bates = hf.bates.as_ref().map(|_| {
                    let numbers = self.numbers.iter();
                    Arc::new(numbers.filter_map(|(&i, &n)| Some((page(i)?, n))).collect())
                });
                let frozen = stored.frozen.as_ref();
                let frozen = Frozen {
                    date: frozen.map_or("", |f| &f.date).into(),
                    file: frozen.map_or("", |f| &f.file).into(),
                    bates,
                };
                (GenSpec::HeaderFooter(Arc::new(hf.clone())), frozen)
            }
        };
        // What [`read`] checks it says.
        let replaces = match &spec {
            GenSpec::Watermark(_) => BTreeSet::from([WatermarkGen::kind_at(stored.z)]),
            GenSpec::HeaderFooter(_) => header::kinds(),
        };
        Some(Generator {
            id: self.id,
            spec,
            z: stored.z,
            scope,
            frozen,
            replaces,
        })
    }

    /// The pages of its file, by index and in order, that a header with
    /// Bates numbers is on but whose numbers can't be read: another tool
    /// merged their blocks into a stream of the page's (§3.5).
    pub fn unnumbered(&self) -> &[u32] {
        &self.unnumbered
    }

    /// Its blocks in streams of their own that say both their digests: each
    /// one's page, by index, its stream, and its layout's and recipe's
    /// digests. What the opened file's record says it drew
    /// ([`drawn_of`](crate::materialize::record::drawn_of)).
    pub fn blocks(&self) -> &[(u32, ObjectId, u128, u128)] {
        &self.blocks
    }

    /// A watermark's content as `stored` says it, its image or page the
    /// XObject a block of it draws in `src`.
    fn content(&self, src: &Src, stored: &StoredContent) -> Option<WatermarkContent> {
        let placed = |art: &StoredArt| {
            Some(Placed {
                at: ObjRef {
                    src: src.clone(),
                    id: self.placed?,
                },
                res: art.res()?,
                turns: art.turns % 4,
            })
        };
        Some(match stored {
            StoredContent::Text {
                text,
                font,
                size,
                color,
                opacity,
                angle,
            } => WatermarkContent::Text {
                text: text.as_str().into(),
                font: *font,
                size: *size,
                color: *color,
                opacity: *opacity,
                angle: *angle,
            },
            StoredContent::Image {
                image,
                scale,
                opacity,
            } => WatermarkContent::Image {
                image: ImageRef::Placed(placed(image)?),
                scale: *scale,
                opacity: *opacity,
            },
            StoredContent::Page {
                pdf,
                scale,
                opacity,
            } => WatermarkContent::Page {
                page: PageForm::Placed(placed(pdf)?),
                scale: *scale,
                opacity: *opacity,
            },
        })
    }
}

/// The generators sheaf drew on `src` (§3.5): what its catalog's
/// `/SheafGenerators` says each is, on the pages whose blocks name it: by
/// their streams' markers ([`Source::markers`]), and on a page holding no
/// block in a stream of its own whose resources name a list of sheaf's,
/// by the ranges its content marks, where another tool merged a block into
/// a stream of its own. One said to be on every page that a page whose
/// content can be read holds no block of, as when another tool added the
/// page, is on the pages that hold one. An image or a page is the XObject
/// a block of it draws in a stream of its own, named in its page's
/// resources. A header's Bates numbers are its blocks' (`/N`); a page whose
/// block another tool merged has none, which is logged and which a save
/// notes. Passed over, each with a log line: JSON that isn't the version
/// sheaf writes; a generator that can't be read, whose id isn't one, that
/// says what it draws, its z or what it replaces in a way sheaf wouldn't,
/// is on some pages but none holds a block of it, or whose image or page
/// no block draws; a second of an id, or of its kind at its z (a second
/// header). Never at open: a file's whole page table is read here, in
/// [`crate::open::finish`].
pub fn read(src: &Source) -> Vec<Saved> {
    let pdf = src.pdf();
    let Some(json) = stored_json(pdf) else {
        return Vec::new();
    };
    // Each generator read on its own, so one that can't be is passed over
    // alone.
    #[derive(Deserialize)]
    struct Listed {
        v: u32,
        generators: Vec<serde_json::Value>,
    }
    let stored = match serde_json::from_slice::<Listed>(&json) {
        Ok(stored) if stored.v == STORED_VERSION => stored,
        Ok(stored) => {
            log::warn!(
                "/SheafGenerators is version {}, not one sheaf reads",
                stored.v
            );
            return Vec::new();
        }
        Err(e) => {
            log::warn!("/SheafGenerators can't be read: {e}");
            return Vec::new();
        }
    };
    // Each generator's blocks in streams of their own; and the pages each
    // holds a block on, merged too, and those only merged.
    let mut blocks: BTreeMap<GenId, Vec<Block>> = BTreeMap::new();
    let mut holding: BTreeMap<GenId, BTreeSet<u32>> = BTreeMap::new();
    let mut merged_on: BTreeMap<GenId, Vec<u32>> = BTreeMap::new();
    for index in 0..src.page_count() {
        let markers = src.markers(index);
        for &(stream, marker) in markers.iter() {
            if let Marker::Gen {
                id,
                layout,
                recipe,
                number,
            } = marker
            {
                let block = Block {
                    index,
                    stream,
                    number,
                    digests: layout.zip(recipe),
                };
                blocks.entry(id).or_default().push(block);
                holding.entry(id).or_default().insert(index);
            }
        }
        let streams = markers.iter().any(|(_, m)| matches!(m, Marker::Gen { .. }));
        if !streams && names_a_list(src, index) {
            for id in merged(src, index) {
                holding.entry(id).or_default().insert(index);
                merged_on.entry(id).or_default().push(index);
            }
        }
    }
    let mut kinds = BTreeSet::new();
    let mut found = Vec::new();
    for g in stored.generators {
        let g: StoredGen = match serde_json::from_value(g) {
            Ok(g) => g,
            Err(e) => {
                log::warn!("a generator /SheafGenerators says can't be read: {e}");
                continue;
            }
        };
        let Some(id) = GenId::parse(g.id.as_bytes()) else {
            log::warn!("/SheafGenerators names a generator {:?}", g.id);
            continue;
        };
        // A header and footer is over the page's content, and says what it
        // fixed.
        let fits = match (&g.kind, &g.spec) {
            (StoredKind::Watermark, StoredSpec::Watermark(_)) => true,
            (StoredKind::HeaderFooter, StoredSpec::HeaderFooter(_)) => {
                g.z == Z::Over && g.frozen.is_some()
            }
            _ => false,
        };
        if !fits {
            log::warn!(
                "generator {id} isn't a {:?} as sheaf writes one at {:?}",
                g.kind,
                g.z
            );
            continue;
        }
        if g.replaces != g.replacing() {
            log::warn!("generator {id} replaces {:?}", g.replaces);
            continue;
        }
        let on = blocks.get(&id).map_or(&[][..], Vec::as_slice);
        let held = holding.get(&id);
        let holds = |index: u32| held.is_some_and(|h| h.contains(&index));
        // A page added since that it isn't on: one whose content can be
        // read, as each it was drawn on was.
        let added = || (0..src.page_count()).find(|&i| !holds(i) && src.blocks(i).is_ok());
        let pages = match g.scope {
            StoredScope::All => match added() {
                None => None,
                Some(index) => {
                    log::warn!(
                        "generator {id} is on every page, but page {} holds no block of it: \
                         it's on the pages that do",
                        index + 1
                    );
                    Some(held.map_or_else(Vec::new, |h| h.iter().copied().collect()))
                }
            },
            StoredScope::Pages => Some(held.map_or_else(Vec::new, |h| h.iter().copied().collect())),
        };
        if pages.as_ref().is_some_and(Vec::is_empty) {
            log::warn!("generator {id} is on no page");
            continue;
        }
        let placed = match &g.spec {
            StoredSpec::Watermark(StoredWatermark {
                content: StoredContent::Text { .. },
                ..
            })
            | StoredSpec::HeaderFooter(_) => None,
            StoredSpec::Watermark(StoredWatermark {
                content:
                    content @ (StoredContent::Image { image: art, .. }
                    | StoredContent::Page { pdf: art, .. }),
                ..
            }) => {
                if art.res().is_none() {
                    log::warn!("generator {id}'s image or page says {:?}", art.placed);
                    continue;
                }
                let subtype = match content {
                    StoredContent::Image { .. } => b"Image".as_slice(),
                    _ => b"Form",
                };
                Some(rebind(src, on, subtype))
            }
        };
        if placed == Some(None) {
            log::warn!("generator {id}'s image or page isn't drawn by a block of it");
            continue;
        }
        let unfit = match &g.spec {
            StoredSpec::Watermark(wm) => unfit_values(wm.content.values()),
            StoredSpec::HeaderFooter(hf) => header::unfit(hf),
        };
        if let Some(why) = unfit {
            log::warn!("generator {id} can't be drawn: {why}");
            continue;
        }
        if found.iter().any(|f: &Saved| f.id == id) || !kinds.insert((g.kind, g.z)) {
            log::warn!("generator {id} is a second of its id, or of its kind at its z");
            continue;
        }
        let numbered = match &g.spec {
            StoredSpec::HeaderFooter(hf) => hf.bates.is_some(),
            StoredSpec::Watermark(_) => false,
        };
        let numbers: BTreeMap<u32, u64> = (on.iter())
            .filter(|_| numbered)
            .filter_map(|b| Some((b.index, b.number?)))
            .collect();
        let unnumbered: Vec<u32> = match numbered {
            true => merged_on.remove(&id).unwrap_or_default(),
            false => Vec::new(),
        };
        if let Some(&first) = unnumbered.first() {
            log::warn!(
                "{} pages of header {id}, from page {}, hold its blocks merged into their \
                 content, whose Bates numbers can't be read",
                unnumbered.len(),
                first + 1
            );
        }
        let blocks = (on.iter())
            .filter_map(|b| {
                let (layout, recipe) = b.digests?;
                Some((b.index, b.stream, layout, recipe))
            })
            .collect();
        found.push(Saved {
            id,
            pages,
            stored: g,
            placed: placed.flatten(),
            numbers,
            unnumbered,
            blocks,
        });
    }
    found
}

/// A generator's block in a stream of its own, as its marker says it.
struct Block {
    /// Its page, by index.
    index: u32,
    stream: ObjectId,
    /// Its Bates number (`/N`).
    number: Option<u64>,
    /// Its layout's and recipe's digests (`/L`, `/R`), when it says both.
    digests: Option<(u128, u128)>,
}

/// Whether page `index` of `src` names a list of sheaf's generators' in
/// its resources' `/Properties`: where a block another tool merged into a
/// stream of its own may be.
fn names_a_list(src: &Source, index: u32) -> bool {
    let pdf = src.pdf();
    let dict = |obj| pdf.dereference(obj).ok()?.1.as_dict().ok();
    let Some(resources) = src.page(index).and_then(|f| f.resources.as_deref()) else {
        return false;
    };
    let lists = dict(resources).and_then(|res| dict(res.get(b"Properties").ok()?));
    lists.is_some_and(|lists| {
        (lists.iter()).any(|(_, list)| dict(list).is_some_and(|l| l.has(b"SheafGen")))
    })
}

/// The generators whose blocks page `index` of `src` holds merged into a
/// stream of its own, as the ranges its content marks name them; none when
/// it can't be read.
fn merged(src: &Source, index: u32) -> Vec<GenId> {
    let Ok(blocks) = src.blocks(index) else {
        return Vec::new();
    };
    (blocks.sheaf.iter())
        .filter_map(|b| match (b.block, b.at) {
            (SheafBlock::Gen(id), BlockAt::Marked(_)) => Some(id),
            _ => None,
        })
        .collect()
}

/// The catalog's `/SheafGenerators` string, when it has one.
fn stored_json(pdf: &LazyPdf) -> Option<Vec<u8>> {
    let catalog = pdf.catalog().ok()?;
    let (_, json) = pdf
        .dereference(catalog.get(GENERATORS.as_bytes()).ok()?)
        .ok()?;
    Some(json.as_str().ok()?.to_vec())
}

/// The XObject the first of `blocks` that draws one draws, when its page's
/// resources name it and it's of `subtype` (`/Image` or `/Form`).
fn rebind(src: &Source, blocks: &[Block], subtype: &[u8]) -> Option<ObjectId> {
    let lim = src.limits();
    let pdf = src.pdf();
    blocks.iter().find_map(|&Block { index, stream, .. }| {
        let stream = pdf.object_unkept(stream).ok()?;
        let data = (stream.as_stream().ok()?)
            .decompressed_content_with_limit(lim.content.decompressed)
            .ok()?;
        let mut operand = None;
        let mut name = None;
        for token in content::lex(&data, &ResCx::none(), lim) {
            match token.ok()?.1 {
                Tok::Operand(o) => operand = Some(o),
                Tok::Op(b"Do") => {
                    name = operand.and_then(content::name);
                    break;
                }
                _ => operand = None,
            }
        }
        let name = name?;
        let resources = src.page(index)?.resources.as_deref()?;
        let resources = pdf.dereference(resources).ok()?.1.as_dict().ok()?;
        let xobjects = pdf.dereference(resources.get(b"XObject").ok()?).ok()?.1;
        let id = xobjects
            .as_dict()
            .ok()?
            .get(&name)
            .ok()?
            .as_reference()
            .ok()?;
        let dict = &pdf.get(id).ok()?.as_stream().ok()?.dict;
        (dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(subtype)).then_some(id)
    })
}

/// Why a watermark can't draw `content`, when it can't: some text, a size
/// above 0, an opacity of 0 to 1, a scale above 0 up to 2, an angle that's
/// a number.
pub(crate) fn unfit(content: &WatermarkContent) -> Option<String> {
    unfit_values(match *content {
        WatermarkContent::Text {
            ref text,
            size,
            opacity,
            angle,
            ..
        } => Values::Text {
            text,
            size,
            opacity,
            angle,
        },
        WatermarkContent::Image { scale, opacity, .. }
        | WatermarkContent::Page { scale, opacity, .. } => Values::Art { scale, opacity },
    })
}

/// What [`unfit`] checks of a watermark.
enum Values<'a> {
    Text {
        text: &'a str,
        size: f32,
        opacity: f32,
        angle: f32,
    },
    Art {
        scale: f32,
        opacity: f32,
    },
}

impl StoredContent {
    fn values(&self) -> Values<'_> {
        match *self {
            StoredContent::Text {
                ref text,
                size,
                opacity,
                angle,
                ..
            } => Values::Text {
                text,
                size,
                opacity,
                angle,
            },
            StoredContent::Image { scale, opacity, .. }
            | StoredContent::Page { scale, opacity, .. } => Values::Art { scale, opacity },
        }
    }
}

fn unfit_values(values: Values) -> Option<String> {
    let opacity =
        |o: f32| (!(0.0..=1.0).contains(&o)).then(|| format!("the opacity is {o}, not 0 to 1"));
    match values {
        Values::Text {
            text,
            size,
            opacity: o,
            angle,
        } => {
            if text.trim().is_empty() {
                return Some("the text is empty".into());
            }
            if !(size.is_finite() && size > 0.0) {
                return Some(format!("the size is {size}, not above 0"));
            }
            if !angle.is_finite() {
                return Some(format!("the angle is {angle}, not a number"));
            }
            opacity(o)
        }
        Values::Art { scale, opacity: o } => {
            if !(scale > 0.0 && scale <= 2.0) {
                return Some(format!("the scale is {scale}, not above 0 up to 2"));
            }
            opacity(o)
        }
    }
}

/// `json` in ASCII: each other character escaped as JSON escapes it
/// (`\u` and its UTF-16), so no reader that takes the string for text in
/// another encoding changes it.
fn ascii(json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(json.len());
    for c in json.chars() {
        if c.is_ascii() {
            out.push(c as u8);
            continue;
        }
        let mut units = [0; 2];
        for unit in c.encode_utf16(&mut units) {
            out.extend(format!("\\u{unit:04x}").into_bytes());
        }
    }
    out
}

#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod header;
#[cfg(test)]
mod tests;
