//! What a generator draws on a page (M10.5 §3.5): a watermark or
//! background, or the header and footer, laid out when a save writes the
//! page, on the page as it shows (its visible box, turned as the page is
//! turned), so it stays upright through a turn and on the page's own boxes
//! through a crop.
//!
//! - [`layout`]: a generator's content on one page, a [`Drawn`]: an
//!   `/Artifact` whose property list (`/Type /Pagination`, its kind's
//!   `/Subtype`, and `/SHEF_Gen` naming the generator) is named in the
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
//! - [`catalog_json`]: what the catalog's `/SHEF_Generators` says of the
//!   document's generators, from which an open finds them again: versioned
//!   JSON, its specs without paths or object numbers (an image or a page is
//!   `"placed"`, the XObject its blocks draw, with the digest of the
//!   resource it was drawn as and an image's turns), a header's date and
//!   file; its Bates numbers are its blocks' (`/N`).
//! - [`read`]: what an open finds ([`Saved`]): the generators
//!   `/SHEF_Generators` says there are, on the pages whose streams'
//!   markers name them (empty blocks too), an image or a page bound to the
//!   XObject a block of it draws, a header's Bates numbers its blocks'.
//!   [`unfit`] and [`header::unfit`] say what a generator can't draw, for
//!   an op and for what's read.
//! - [`header`]: the header and footer's slots, their tokens read on a
//!   page and placed on it.

use std::sync::Arc;

use lopdf::{Dictionary, Object, StringFormat, dictionary};
use serde::{Deserialize, Serialize};
use sheaf_edit::{FontChoice, GridPos, HeaderFooterSpec, Z};

use crate::annot::{Box2, Pt};
use crate::content::{
    Color, ContentWriter, Font, GState, PaginationKind, Res, ResBody, ResId, Reserved,
    ResourceNeeds, Std14,
};
use crate::geom::{Affine, Geom, Rect};
use crate::materialize::Private;
use crate::model::{
    DocState, GenSpec, Generator, PageCtx, PageId, Placed, Scope, WatermarkContent, WatermarkGen,
};

pub use saved::{Saved, read};

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
/// resources bind already; its lists without `/SHEF_Gen` when `private`
/// strips it (§15). Empty where it draws nothing.
pub fn layout(
    g: &Generator,
    ctx: &PageCtx,
    art: Option<&Art>,
    reserved: &Reserved,
    private: Private,
) -> Drawn {
    let drawn = write(g, ctx, art, private, |out| {
        let (ops, res) = out.named(reserved);
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
    write(g, ctx, art, Private::Keep, digest).unwrap_or_else(|| Drawn::empty().layout_digest)
}

/// A digest of what `out` draws: its content named as though no page's
/// resources bound anything, so the names are the resources' own, and
/// their ids.
fn digest(out: &ContentWriter) -> u128 {
    let (mut bytes, needs) = out.named(&Reserved::default());
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
    private: Private,
    done: impl FnOnce(&ContentWriter) -> R,
) -> Option<R> {
    match &g.spec {
        GenSpec::Watermark(wm) => watermark((g, private), wm, &ctx.geom, art, done),
        GenSpec::HeaderFooter(hf) => header_footer((g, private), hf, ctx, done),
    }
}

/// The list a generator's artifact of `kind` is marked with: named in the
/// page's `/Properties`, it says whose it is, unless `private` strips that.
fn list(g: &Generator, kind: PaginationKind, private: Private) -> Dictionary {
    let mut list = dictionary! {
        "Type" => "Pagination",
        "Subtype" => Object::Name(kind.name().to_vec()),
    };
    if private == Private::Keep {
        let id = Object::String(g.id.to_string().into_bytes(), StringFormat::Literal);
        list.set("SHEF_Gen", id);
    }
    list
}

/// Header and footer `hf`, generator `g`'s, on the page `ctx` says, handed
/// to `done`: each slot that isn't blank there an artifact of its own,
/// marked as the header's or the footer's, its text in `hf`'s face along
/// the page as it shows, its baseline where [`header::layout`] places it.
/// None when every slot is blank there.
fn header_footer<R>(
    (g, private): (&Generator, Private),
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
        out.begin_marked("Artifact", list(g, kind, private));
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
    (g, private): (&Generator, Private),
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
    out.begin_marked("Artifact", list(g, WatermarkGen::kind_at(g.z), private));
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
/// read it, and its Bates number, fixed when it was set. A page of an
/// export kept as the document numbers it reads its place, the count and
/// its label there ([`DocState::numbered_in`]).
pub(crate) fn page_ctx(
    st: &DocState,
    g: &Generator,
    id: PageId,
    position: usize,
    geom: &Geom,
) -> PageCtx {
    let (st, position) = st.numbered_in(id).unwrap_or((st, position));
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

/// The catalog's `/SHEF_Generators`, the document's generators as JSON in
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

/// The version of `/SHEF_Generators`' JSON.
const STORED_VERSION: u32 = 1;

/// `/SHEF_Generators`, as JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Stored {
    v: u32,
    generators: Vec<StoredGen>,
}

/// A generator as `/SHEF_Generators` holds it: its id as its blocks name it.
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
mod saved;
#[cfg(test)]
mod tests;
