//! Another tool's annotation as a sheaf mark (M9 decision 5). One
//! converter serves both editing a third-party comment in place and
//! importing comments from FDF, XFDF or another PDF: each is, or is made
//! into, a PDF annotation dictionary first.
//!
//! Each dictionary is read by `pdf-annots` under PDFium's rules
//! (`source::annots::read`, M18 slice 24), and the mark made from what
//! it read ([`convert_page`]). What stays sheaf's own read of the dictionary: its
//! names in threads ([`thread_name`], and `/IRT` by its target's name or,
//! as XFDF gives it, a string: [`irt_name`]); the dates, kept as written;
//! a dimension's `/Measure`; an attachment's embedded file; and whether a
//! stamp has a `/Name` at all (one without is named none, as PDFium reads
//! it, where the family gives ISO 32000's default for its icon). Which
//! mark each annotation becomes is sheaf's: the family says what it is,
//! and this says what sheaf holds it as (a dimension by the family's
//! `/IT`, a page stamp by its form, a reply or a replacement by its
//! thread), each color rounded to a byte as sheaf writes it.
//!
//! The mark keeps the annotation's `/NM` ([`Annot::name`]), so replies to
//! it still find it, and a save puts the mark where the annotation was.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::pdf::annots::{
    self, AnnotColor, AnnotSubtype, AppearanceEntry, AttachmentIcon, BorderStyle,
    FileAttachmentAnnot, FreeTextIntent, LineEnding, LineIntent, PolyIntent, PolyKind, ReplyKind,
    ShapeKind, StampName, TextMarkupKind, TypedAnnot,
};
use crate::pdf::gfx::Rect;
use crate::pdf::{Dictionary, Object, ObjectId, StreamExt, Value};
use sheaf_edit::Rotation;

use crate::annot::{Annot, AnnotId, AnnotKind, Box2, Pt, REVIEW};
use crate::attachments::Objects;
use crate::content::{ReadObjects, Unkept};
use crate::file::{pdf_text, thread_name};
use crate::store::{Blob, BlobKind, Store};

/// What a conversion may do beyond reading the dictionary.
#[derive(Clone, Copy, Default)]
pub struct Options<'a> {
    /// Where an attachment's embedded file is kept, which makes it a mark,
    /// and a stamp's page. None when converting a whole file ahead of
    /// need, where copying every attachment would be waste: a stamp then
    /// names its page by stem alone.
    pub files: Option<&'a Interned<'a>>,
}

/// Blobs interned into a document's store for the marks an edit will add:
/// held until it's applied and the marks hold them, as the store keeps
/// none.
pub struct Interned<'a> {
    store: &'a Store,
    held: RefCell<Vec<Blob>>,
}

impl<'a> Interned<'a> {
    pub fn new(store: &'a Store) -> Interned<'a> {
        Interned {
            store,
            held: RefCell::default(),
        }
    }

    /// `bytes` interned as `kind`, and held; None, said, when other bytes
    /// have their checksum.
    pub fn intern(&self, bytes: Arc<[u8]>, kind: BlobKind) -> Option<Blob> {
        let blob = self.store.intern(bytes, kind);
        let blob = blob.inspect_err(|e| log::warn!("{e}")).ok()?;
        self.hold(blob.clone());
        Some(blob)
    }

    /// `blob`, held too.
    pub fn hold(&self, blob: Blob) {
        self.held.borrow_mut().push(blob);
    }

    /// The store they're interned into.
    pub fn store(&self) -> &'a Store {
        self.store
    }

    /// What's held, for the caller to keep until the edit is applied.
    pub fn into_held(self) -> Vec<Blob> {
        self.held.into_inner()
    }
}

/// Why an annotation didn't become a mark.
#[derive(Clone, Debug, PartialEq)]
pub enum NotAMark {
    /// Not a comment: a link, form field or popup. Passed over.
    NotComment,
    /// A comment sheaf can't hold, by subtype. Reported.
    Unsupported(String),
    /// What doesn't read as an annotation, with why: a string still
    /// encrypted, or past one of `pdf-annots`' caps
    /// (`source::annots::limits`).
    Unread(String),
    /// Part of another, which became a mark with it: the crossed-out
    /// text of a replacement.
    Part,
}

/// Each of a page's annotations (`read`, in order, as `pdf-annots` read
/// them) as a mark that reads upright turned `turn`, as the page shows. A
/// replacement's StrikeOut, grouped under its Caret, is folded into the
/// Caret's mark. `pdf` resolves references (`/IRT`, `/AP`, `/FS`).
pub fn convert_page<P: Objects + ?Sized>(
    pdf: &P,
    read: &[annots::Annot<'_>],
    turn: Rotation,
    opts: Options<'_>,
) -> Vec<Result<Annot, NotAMark>> {
    let names: Vec<String> = read.iter().map(|a| thread_name(a.dict())).collect();
    // Grouped parts, by the name of what they're grouped under.
    let mut parts: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, a) in read.iter().enumerate() {
        if is_grouped(a)
            && let Some(parent) = irt_name(pdf, a.dict())
        {
            parts.entry(parent).or_default().push(i);
        }
    }
    let mut out: Vec<Result<Annot, NotAMark>> = read
        .iter()
        .map(|a| convert_read(pdf, a, turn, opts))
        .collect();
    for (i, a) in read.iter().enumerate() {
        let Some(kids) = parts.get(&names[i]) else {
            continue;
        };
        if *a.subtype() != AnnotSubtype::Caret {
            continue;
        }
        let strike =
            (kids.iter().copied()).find(|&k| *read[k].subtype() == AnnotSubtype::StrikeOut);
        let Some(k) = strike else { continue };
        let rects = match &out[k] {
            Ok(Annot {
                kind: AnnotKind::StrikeOut { rects },
                ..
            }) => rects.clone(),
            _ => continue,
        };
        if let Ok(m) = &mut out[i]
            && let AnnotKind::Insert { at } = m.kind
        {
            m.kind = AnnotKind::Replace {
                rects,
                struck: String::new(),
                at,
            };
            out[k] = Err(NotAMark::Part);
        }
    }
    out
}

/// `dict` as a mark that reads upright turned `turn`: read by
/// `pdf-annots` (`source::annots::read`) and made a mark as
/// [`convert_page`] makes each.
pub fn convert<P: Objects + ReadObjects + ?Sized>(
    pdf: &P,
    dict: &Dictionary,
    turn: Rotation,
    opts: Options<'_>,
) -> Result<Annot, NotAMark> {
    let unkept = Unkept::default();
    let read = crate::source::annots::read(pdf, &unkept, dict, None);
    let a = read.map_err(|why| NotAMark::Unread(why.to_string()))?;
    convert_read(pdf, &a, turn, opts)
}

/// `a` as a mark that reads upright turned `turn`. Its geometry is the
/// standard keys', which are PageSpace.
fn convert_read<P: Objects + ?Sized>(
    pdf: &P,
    a: &annots::Annot<'_>,
    turn: Rotation,
    opts: Options<'_>,
) -> Result<Annot, NotAMark> {
    let dict = a.dict();
    let sub = subtype_of(a);
    if matches!(
        sub.as_str(),
        "" | "Link" | "Widget" | "Popup" | "PrinterMark" | "TrapNet" | "Watermark"
    ) {
        return Err(NotAMark::NotComment);
    }
    let unsupported = || NotAMark::Unsupported(sub.clone());
    if !a.has_valid_rect() {
        return Err(unsupported());
    }
    let raw = a.raw_rect();
    let rect = user_box(raw);
    let stroke = a.border().width;
    let rect_diff = match a.typed() {
        TypedAnnot::Shape(s) => s.rect_diff,
        TypedAnnot::FreeText(f) => f.rect_diff,
        TypedAnnot::Caret(c) => c.rect_diff,
        _ => None,
    };
    // The box inside /Rect: less /RD where there is one, else less
    // `border` (half a shape's line, which is drawn on the box).
    let within = |border: f32| match rect_diff {
        Some(d) => user_box([raw[0] + d[0], raw[1] + d[1], raw[2] - d[2], raw[3] - d[3]]),
        None => inset(rect, border),
    };
    let inner = || within(stroke / 2.0);
    let point = |x: f32, y: f32| Pt { x, y };
    let mut color = a.color().and_then(rgb);
    let kind = match a.typed() {
        TypedAnnot::Text(t) => {
            let state = t.state.clone().unwrap_or_default();
            match irt_name(pdf, dict) {
                Some(parent) if !state.is_empty() => AnnotKind::Status {
                    parent,
                    model: (t.state_model.clone())
                        .filter(|m| !m.is_empty())
                        .unwrap_or_else(|| REVIEW.to_string()),
                    state,
                },
                Some(parent) => AnnotKind::Reply { parent },
                None => AnnotKind::Note { pos: icon_at(rect) },
            }
        }
        TypedAnnot::TextMarkup(m) => {
            let mut rects: Vec<Box2> = (m.quad_points.iter()).map(|q| box_of(q.bounds())).collect();
            if rects.is_empty() {
                rects = vec![rect];
            }
            match m.kind {
                TextMarkupKind::Highlight => {
                    color = color.or(Some([255, 230, 0]));
                    AnnotKind::Highlight { rects }
                }
                TextMarkupKind::Underline => AnnotKind::Underline { rects },
                TextMarkupKind::StrikeOut => AnnotKind::StrikeOut { rects },
                TextMarkupKind::Squiggly => AnnotKind::Squiggly { rects },
            }
        }
        TypedAnnot::FreeText(f) => {
            let da = f.da.as_ref();
            // The text's color, not the box's.
            color = (da.and_then(|da| da.color))
                .and_then(|c| rgb(AnnotColor::from(c)))
                .or(Some([0, 0, 0]));
            let size = (da.and_then(|da| da.font_size))
                .filter(|s| *s > 0.0)
                .unwrap_or(12.0);
            let callout = f.intent == FreeTextIntent::Callout;
            match f.callout_line.as_slice() {
                // [point, knee, end], or [point, end] with no knee.
                [p, knee, ..] if callout && f.callout_line.len() <= 3 => AnnotKind::Callout {
                    rect: inner(),
                    point: point(p.x, p.y),
                    knee: point(knee.x, knee.y),
                    size,
                },
                _ => AnnotKind::FreeText {
                    rect: inner(),
                    size,
                },
            }
        }
        TypedAnnot::Shape(s) => match s.kind {
            ShapeKind::Square => AnnotKind::Square { rect: inner() },
            ShapeKind::Circle => AnnotKind::Ellipse { rect: inner() },
        },
        TypedAnnot::Line(l) => {
            let (from, to) = l.endpoints.ok_or_else(unsupported)?;
            AnnotKind::Line {
                a: point(from.x, from.y),
                b: point(to.x, to.y),
                arrow: is_arrow(l.endings.1),
                arrow_start: is_arrow(l.endings.0),
            }
        }
        TypedAnnot::Ink(i) => {
            let paths: Vec<Vec<Pt>> = (i.strokes.iter())
                .filter(|s| !s.is_empty())
                .map(|s| s.iter().map(|p| point(p.x, p.y)).collect())
                .collect();
            if paths.is_empty() {
                return Err(unsupported());
            }
            AnnotKind::Ink { paths }
        }
        TypedAnnot::Poly(p) => {
            let points: Vec<Pt> = p.vertices.iter().map(|v| point(v.x, v.y)).collect();
            if points.len() < 2 {
                return Err(unsupported());
            }
            match p.kind {
                PolyKind::Polygon => AnnotKind::Polygon { points },
                PolyKind::PolyLine => AnnotKind::Polyline { points },
            }
        }
        TypedAnnot::Caret(_) => AnnotKind::Insert { at: within(0.0) },
        // A stamp with no `/Name` has a name of none, where the family's
        // view gives ISO's default, `/Draft`, for its icon.
        TypedAnnot::Stamp(s) => {
            let name = dict.get(b"Name").map(|_| &s.name);
            stamp(pdf, a, name, rect, opts.files)
        }
        TypedAnnot::FileAttachment(f) => match opts.files {
            Some(files) => attachment(pdf, dict, f, rect, files).ok_or_else(unsupported)?,
            None => return Err(unsupported()),
        },
        _ => return Err(unsupported()),
    };
    // A dimension with a scale is a measurement (decision 9). Its value
    // leads its text, as Acrobat writes it; sheaf draws that itself.
    let measured = match a.typed() {
        TypedAnnot::Line(l) => {
            (l.intent == Some(LineIntent::Dimension)).then_some(crate::measure::Measured::Distance)
        }
        TypedAnnot::Poly(p) => match p.intent {
            Some(PolyIntent::LineDimension) => Some(crate::measure::Measured::Perimeter),
            Some(PolyIntent::PolygonDimension) => Some(crate::measure::Measured::Area),
            _ => None,
        },
        _ => None,
    };
    let mut contents = a.contents().unwrap_or_default().to_string();
    let kind = match (measured, scale(pdf, dict), kind) {
        (Some(what), Some(scale), AnnotKind::Line { a, b, .. }) => {
            measure(what, vec![a, b], scale, &mut contents)
        }
        (
            Some(what),
            Some(scale),
            AnnotKind::Polyline { points } | AnnotKind::Polygon { points },
        ) => measure(what, points, scale, &mut contents),
        (_, _, kind) => kind,
    };
    let interior = match a.typed() {
        TypedAnnot::Shape(s) => s.interior_color,
        TypedAnnot::Poly(p) => p.interior_color,
        TypedAnnot::Line(l) => l.interior_color,
        _ => None,
    };
    let fillable = kind.fillable();
    let markup = a.markup();
    let text = |key: &[u8]| dict.get(key).map(pdf_text).unwrap_or_default();
    Ok(Annot {
        id: AnnotId(0),
        kind,
        color: color.unwrap_or([0, 0, 0]),
        opacity: markup.map_or(1.0, |m| m.opacity),
        stroke,
        author: (markup.and_then(|m| m.author.clone())).unwrap_or_default(),
        contents,
        modified: text(b"M"),
        created: text(b"CreationDate"),
        fill: interior.and_then(rgb).filter(|_| fillable),
        dashed: a.border().style == BorderStyle::Dashed,
        cloudy: a.border().cloudy_intensity.is_some(),
        turn,
        name: thread_name(dict),
    })
}

/// Its `/Measure` as a scale.
fn scale<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary) -> Option<crate::measure::Scale> {
    let (_, m) = pdf.dereference(dict.get(b"Measure")?).ok()?;
    crate::measure::scale_of(pdf, m.as_dict()?)
}

/// A measurement of `points`; the value its text starts with comes off,
/// since sheaf draws and writes it from the points.
fn measure(
    what: crate::measure::Measured,
    points: Vec<Pt>,
    scale: crate::measure::Scale,
    contents: &mut String,
) -> AnnotKind {
    let first = contents.lines().next().unwrap_or_default().trim();
    if !first.is_empty() && first.ends_with(scale.unit.as_str()) {
        *contents = contents
            .split_once('\n')
            .map(|(_, rest)| rest.trim_start().to_string())
            .unwrap_or_default();
    }
    AnnotKind::Measure {
        what,
        points,
        scale: Box::new(scale),
    }
}

/// A stamp, `a`, named `name` (none where its dictionary has no `/Name`):
/// its appearance as one of ours, a one-page PDF it names by its blob's
/// stem, when its normal appearance is a form sheaf can read; else a text
/// stamp saying its name. The page is interned through `files` and held
/// there; a file converted ahead of need, with none, has only the stem,
/// which the form the page is made of stands for in the file
/// ([`ForeignAnnot::stamp`](crate::file::ForeignAnnot::stamp)).
fn stamp<P: Objects + ?Sized>(
    pdf: &P,
    a: &annots::Annot<'_>,
    name: Option<&StampName>,
    rect: Box2,
    files: Option<&Interned<'_>>,
) -> AnnotKind {
    let name =
        (name.map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())).unwrap_or_default();
    // Acrobat's own are "SBApproved", "SHSignHere"…; custom ones "#…".
    let shown = match name.as_str() {
        n if n.starts_with('#') || n.is_empty() => "Stamp".to_string(),
        n if n.len() > 2
            && n[..2].chars().all(|c| c.is_ascii_uppercase())
            && n[2..].starts_with(|c: char| c.is_ascii_uppercase()) =>
        {
            n[2..].to_string()
        }
        n => n.to_string(),
    };
    let page = normal_form(a).and_then(|f| crate::stamp_lib::form_to_pdf(pdf, f).ok());
    let stem = page.and_then(|page| match files {
        Some(files) => Some(files.intern(page.into(), BlobKind::stamp())?.id().stem()),
        None => Some(crate::store::blob_id(&page).stem()),
    });
    match stem {
        Some(stamp) => AnnotKind::PageStamp {
            rect,
            stamp,
            name: shown,
            text: None,
        },
        None => AnnotKind::TextStamp {
            rect,
            text: shown.to_uppercase(),
            status: false,
        },
    }
}

/// The form `a`'s normal appearance is (`/AP /N`), where it's one form:
/// what a stamp's page is made of.
pub(crate) fn normal_form(a: &annots::Annot<'_>) -> Option<ObjectId> {
    match a.appearances()?.normal.as_ref()? {
        AppearanceEntry::Single(id) => Some(*id),
        AppearanceEntry::States(_) => None,
    }
}

/// An attachment, `f` as the family read it of `dict`: its file's name and
/// icon from there, its embedded file from the dictionary's `/FS`,
/// interned through `files`.
fn attachment<P: Objects + ?Sized>(
    pdf: &P,
    dict: &Dictionary,
    f: &FileAttachmentAnnot,
    rect: Box2,
    files: &Interned<'_>,
) -> Option<AnnotKind> {
    let deref_dict = |o: &Object| pdf.dereference(o).ok()?.1.as_dict().cloned();
    let fs = deref_dict(dict.get(b"FS")?)?;
    let ef = deref_dict(fs.get(b"EF")?)?;
    let at = ef.get(b"UF").or_else(|| ef.get(b"F"))?.as_reference()?;
    let s = pdf.get_object(at).ok()?.as_stream()?;
    let bytes = (s.decompressed_content()).unwrap_or_else(|_| s.raw_bytes().to_vec());
    let size = bytes.len() as u64;
    let file = files
        .intern(bytes.into(), BlobKind::Attachment)?
        .id()
        .stem();
    let name = (f.file_name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "attachment".into());
    let icon = match &f.icon {
        AttachmentIcon::PushPin => "PushPin".to_string(),
        AttachmentIcon::Graph => "Graph".to_string(),
        AttachmentIcon::Paperclip => "Paperclip".to_string(),
        AttachmentIcon::Tag => "Tag".to_string(),
        AttachmentIcon::Other(name) => String::from_utf8_lossy(name).into_owned(),
        _ => String::new(),
    };
    let icon = Some(icon)
        .filter(|n| crate::annot::ATTACHMENT_ICONS.contains(&n.as_str()))
        .unwrap_or_else(|| "PushPin".into());
    Some(AnnotKind::FileAttachment {
        pos: icon_at(rect),
        icon,
        file,
        name,
        size,
    })
}

/// `/Subtype` as the family read it, as text.
pub(crate) fn subtype_of(a: &annots::Annot<'_>) -> String {
    String::from_utf8_lossy(a.subtype().as_bytes()).into_owned()
}

/// `/RT /Group`: part of what its `/IRT` is, not a reply to it.
pub(crate) fn is_grouped(a: &annots::Annot<'_>) -> bool {
    a.markup().is_some_and(|m| m.reply_kind == ReplyKind::Group)
}

/// What `/IRT` names: the annotation it refers to, by its name, or (as
/// XFDF gives it) the name itself.
pub fn irt_name<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary) -> Option<String> {
    let o = dict.get(b"IRT")?;
    match o.value() {
        Value::Ref(id) => pdf.get_dictionary(*id).ok().map(thread_name),
        Value::String(..) => Some(pdf_text(o)).filter(|n| !n.is_empty()),
        _ => None,
    }
}

/// A color as sheaf's, each component rounded to a byte, as sheaf writes
/// `/C` to thousandths and reads it back: none for a transparent one. Gray
/// and CMYK are made RGB.
pub(crate) fn rgb(color: AnnotColor) -> Option<[u8; 3]> {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    match color {
        AnnotColor::Transparent => None,
        AnnotColor::Gray(g) => Some([byte(g); 3]),
        AnnotColor::Rgb([r, g, b]) => Some([byte(r), byte(g), byte(b)]),
        AnnotColor::Cmyk([c, m, y, k]) => Some([
            byte((1.0 - c) * (1.0 - k)),
            byte((1.0 - m) * (1.0 - k)),
            byte((1.0 - y) * (1.0 - k)),
        ]),
    }
}

/// Whether a line ending is an arrow sheaf draws: an open or closed one,
/// either way round.
fn is_arrow(end: LineEnding) -> bool {
    matches!(
        end,
        LineEnding::OpenArrow
            | LineEnding::ClosedArrow
            | LineEnding::ROpenArrow
            | LineEnding::RClosedArrow
    )
}

/// A user-space box `[x0 y0 x1 y1]`, in either order, as a mark's.
fn user_box(r: [f32; 4]) -> Box2 {
    Box2::from_corners(Pt { x: r[0], y: r[1] }, Pt { x: r[2], y: r[3] })
}

/// A family rectangle, normalized, as a mark's box.
fn box_of(r: Rect) -> Box2 {
    Box2 {
        x: r.x0,
        y: r.y0,
        w: (r.x1 - r.x0).max(0.0),
        h: (r.y1 - r.y0).max(0.0),
    }
}

/// Where an icon's corner goes for it to sit in the middle of `rect`.
fn icon_at(rect: Box2) -> Pt {
    Pt {
        x: rect.x + (rect.w - crate::annot::NOTE_SIZE) / 2.0,
        y: rect.y + (rect.h - crate::annot::NOTE_SIZE) / 2.0,
    }
}

/// `r` with `d` off each side, but never past its middle.
fn inset(r: Box2, d: f32) -> Box2 {
    let d = d.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    Box2 {
        x: r.x + d,
        y: r.y + d,
        w: r.w - 2.0 * d,
        h: r.h - 2.0 * d,
    }
}

#[cfg(test)]
mod tests;
