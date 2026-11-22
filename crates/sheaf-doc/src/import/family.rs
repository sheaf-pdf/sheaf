//! Another tool's annotation as a sheaf mark, read through `pdf-annots`
//! (M18 slice 24): each dictionary read once by the family's
//! [`Annot::from_dict_with`](Read::from_dict_with) under PDFium's rules,
//! and the mark made from what it read, where [`super::convert`] reads
//! the dictionary key by key. Set beside it by the annotations dump
//! (`annot::corpus`, `SHEAF_ANNOTS_DUMP_FAMILY`) until the open reads
//! through it.
//!
//! What stays sheaf's own read of the dictionary: its names in threads
//! ([`thread_name`], and `/IRT` by its target's name or, as XFDF gives
//! it, a string: [`irt_name`]); the dates, kept as written (the family's
//! `Dated` keeps no text for a date that reads); a dimension's
//! `/Measure`; an attachment's embedded file; and a stamp's form made a
//! page.

use std::collections::HashMap;

use crate::pdf::annots::{
    Annot as Read, AnnotColor, AnnotSubtype, AppearanceEntry, AttachmentIcon, BorderStyle, Error,
    FileAttachmentAnnot, FreeTextIntent, Limits, LineEnding, LineIntent, PolyIntent, PolyKind,
    ReplyKind, Rules, ShapeKind, StampName, TextMarkupKind, TypedAnnot,
};
use crate::pdf::gfx::Rect;
use crate::pdf::{Dictionary, Object, ObjectId, Resolve, StreamExt};
use sheaf_edit::Rotation;

use super::{Interned, NotAMark, Options, icon_at, inset, irt_name, measure, scale, user_box};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2, Pt, REVIEW};
use crate::attachments::Objects;
use crate::file::{pdf_text, thread_name};
use crate::store::BlobKind;

/// An annotation as the family reads it.
pub(crate) type ReadAnnot<'o> = Read<'o>;

/// Any of sheaf's objects as the family's readers resolve them: object
/// `id`, its references followed, kept by the source as
/// [`Objects::get_object`] keeps what it reads; one that doesn't read is
/// absent.
pub(crate) struct Get<'a, P: ?Sized>(pub(crate) &'a P);

impl<P: ?Sized> Clone for Get<'_, P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: ?Sized> Copy for Get<'_, P> {}

impl<'a, P: Objects + ?Sized> Resolve<'a> for Get<'a, P> {
    fn resolve(&self, id: ObjectId) -> Option<&'a Object> {
        self.0.get_object(id).ok()
    }
}

/// The caps an annotation is read under: the family's, but for the ones
/// sheaf has none of (a page's annotations, a markup's quadrilaterals, a
/// path's points), past reach, and references followed as deep as
/// sheaf's reader follows them.
pub(crate) fn limits() -> Limits {
    let mut limits = Limits::DEFAULT;
    limits.hops = crate::source::Limits::DEFAULT.reader.max_depth;
    limits.annots_per_page = usize::MAX;
    limits.quads = usize::MAX;
    limits.points = usize::MAX;
    limits
}

/// Annotation `dict` of `pdf`, object `id` (None for one written in a
/// page's `/Annots`), as the family reads it under PDFium's rules.
pub(crate) fn read<'o, P: Objects + ?Sized>(
    pdf: &'o P,
    dict: &'o Dictionary,
    id: Option<ObjectId>,
) -> Result<Read<'o>, Error> {
    Read::from_dict_with(Get(pdf), dict, id, Rules::Pdfium, &limits(), &mut ())
}

/// A family color as sheaf's, each component rounded to a byte as
/// [`super::convert`] rounds `/C`: none for a transparent one.
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

/// A family rectangle as a mark's box.
pub(crate) fn box_of(r: Rect) -> Box2 {
    Box2 {
        x: r.x0,
        y: r.y0,
        w: (r.x1 - r.x0).max(0.0),
        h: (r.y1 - r.y0).max(0.0),
    }
}

/// `/Subtype` as the family reads it, as text.
pub(crate) fn subtype_of(a: &Read<'_>) -> String {
    String::from_utf8_lossy(a.subtype().as_bytes()).into_owned()
}

/// Whether `a` is grouped with what its `/IRT` names (`/RT /Group`).
pub(crate) fn is_grouped(a: &Read<'_>) -> bool {
    a.markup().is_some_and(|m| m.reply_kind == ReplyKind::Group)
}

/// Its stamp's normal appearance, a single form.
pub(crate) fn normal_form(a: &Read<'_>) -> Option<ObjectId> {
    match a.appearances()?.normal.as_ref()? {
        AppearanceEntry::Single(id) => Some(*id),
        AppearanceEntry::States(_) => None,
    }
}

/// [`super::convert_page`] over what the family read: each of a page's
/// annotations (`read`, each with its dictionary, in order) as a mark that
/// reads upright turned `turn`, a replacement's StrikeOut folded into its
/// Caret's mark.
pub(crate) fn convert_page<P: Objects + ?Sized>(
    pdf: &P,
    read: &[(&Dictionary, &Read<'_>)],
    turn: Rotation,
    opts: Options<'_>,
) -> Vec<Result<Annot, NotAMark>> {
    let names: Vec<String> = read.iter().map(|(d, _)| thread_name(d)).collect();
    let mut parts: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (d, a)) in read.iter().enumerate() {
        if is_grouped(a)
            && let Some(parent) = irt_name(pdf, d)
        {
            parts.entry(parent).or_default().push(i);
        }
    }
    let mut out: Vec<Result<Annot, NotAMark>> = (read.iter())
        .map(|(d, a)| convert(pdf, d, a, turn, opts))
        .collect();
    for (i, (_, a)) in read.iter().enumerate() {
        let Some(kids) = parts.get(&names[i]) else {
            continue;
        };
        if *a.subtype() != AnnotSubtype::Caret {
            continue;
        }
        let strike =
            (kids.iter().copied()).find(|&k| *read[k].1.subtype() == AnnotSubtype::StrikeOut);
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

/// Points as a mark's.
fn pts(points: &[crate::pdf::gfx::Point]) -> Vec<Pt> {
    points.iter().map(|p| Pt { x: p.x, y: p.y }).collect()
}

/// [`super::convert`] over what the family read of `dict` (`a`): a mark
/// that reads upright turned `turn`. Its geometry is the standard keys',
/// which are PageSpace.
pub(crate) fn convert<P: Objects + ?Sized>(
    pdf: &P,
    dict: &Dictionary,
    a: &Read<'_>,
    turn: Rotation,
    opts: Options<'_>,
) -> Result<Annot, NotAMark> {
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
                [point, knee, ..] if callout && f.callout_line.len() <= 3 => AnnotKind::Callout {
                    rect: inner(),
                    point: Pt {
                        x: point.x,
                        y: point.y,
                    },
                    knee: Pt {
                        x: knee.x,
                        y: knee.y,
                    },
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
            let (pa, pb) = l.endpoints.ok_or_else(unsupported)?;
            AnnotKind::Line {
                a: Pt { x: pa.x, y: pa.y },
                b: Pt { x: pb.x, y: pb.y },
                arrow: is_arrow(l.endings.1),
                arrow_start: is_arrow(l.endings.0),
            }
        }
        TypedAnnot::Ink(i) => {
            let paths: Vec<Vec<Pt>> = (i.strokes.iter())
                .filter(|s| !s.is_empty())
                .map(|s| pts(s))
                .collect();
            if paths.is_empty() {
                return Err(unsupported());
            }
            AnnotKind::Ink { paths }
        }
        TypedAnnot::Poly(p) => {
            let points = pts(&p.vertices);
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

/// [`super::stamp`] over what the family read: its appearance as one of
/// ours, a one-page PDF it names by its blob's stem, when its normal
/// appearance is a form sheaf can read; else a text stamp saying its name.
fn stamp<P: Objects + ?Sized>(
    pdf: &P,
    a: &Read<'_>,
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

/// [`super::attachment`] over what the family read: its file's name and
/// icon from there, the embedded file from the dictionary's `/FS`.
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

/// What sheaf writes, read back both ways: every kind of mark
/// (`forms::fixture::every_mark_kind`, with the variants
/// `content::written_golden` draws besides) at each turn, saved as a save
/// writes them (their companions and what they hold), each dictionary of
/// the page converted by [`super::convert_page`] and by
/// [`convert_page`], ahead of need and with files to hold, a line each:
/// the case, its slot, its `/Subtype`, the mark each makes (its JSON's
/// length and checksum, or why none), whether they're the same, and
/// where they aren't, each field that differs with its value both ways.
/// Written to `$SHEAF_ANNOTS_OWN` (a mark another tool touched since
/// sheaf wrote it is read back through this path, and the corpus holds
/// none of sheaf's dictionaries).
#[test]
#[ignore = "sheaf's own marks read back both ways, written to $SHEAF_ANNOTS_OWN"]
fn own_marks_read_both_ways() {
    use crate::forms::fixture as forms;
    use crate::import::Interned;
    use crate::store::Store;

    let Some(to) = std::env::var_os("SHEAF_ANNOTS_OWN") else {
        eprintln!("skipped: SHEAF_ANNOTS_OWN is unset");
        return;
    };
    let marks = forms::every_mark_kind();
    let of = |kind: fn(&AnnotKind) -> bool| -> Annot {
        (marks.iter().find(|a| kind(&a.kind)))
            .expect("a mark of every kind")
            .clone()
    };
    // The variants written_golden draws besides.
    let square = of(|k| matches!(k, AnnotKind::Square { .. }));
    let mut cases: Vec<(String, Vec<Annot>)> = vec![("every".into(), marks.clone())];
    let mut more = vec![
        Annot {
            opacity: 1.0,
            dashed: false,
            fill: None,
            ..square.clone()
        },
        Annot {
            opacity: 0.0,
            ..square.clone()
        },
        Annot {
            cloudy: true,
            dashed: false,
            ..square
        },
    ];
    for kind in [
        of(|k| matches!(k, AnnotKind::Line { .. })),
        of(|k| matches!(k, AnnotKind::Polygon { .. })),
        of(|k| matches!(k, AnnotKind::Polyline { .. })),
        of(|k| matches!(k, AnnotKind::Ink { .. })),
    ] {
        more.push(Annot {
            dashed: true,
            cloudy: false,
            ..kind
        });
    }
    let free = of(|k| matches!(k, AnnotKind::FreeText { .. }));
    more.push(Annot {
        contents: "Café “x” – € 漢 (a\\b) and enough words after it to wrap".into(),
        ..free
    });
    let callout = of(|k| matches!(k, AnnotKind::Callout { .. }));
    more.push(Annot {
        contents: "naïve (x) 漢".into(),
        ..callout
    });
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
        more.push(Annot {
            kind: AnnotKind::FileAttachment {
                pos,
                icon: icon.into(),
                file,
                name,
                size,
            },
            ..attachment.clone()
        });
    }
    cases.push(("variants".into(), more));
    let turns = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];
    let json = |m: &Result<Annot, NotAMark>| match m {
        Ok(a) => {
            let bytes = serde_json::to_vec(a).expect("a mark is JSON");
            format!("{}:{}", bytes.len(), sheaf_util::checksum_hex(&bytes))
        }
        Err(why) => format!("-:{why:?}"),
    };
    let mut lines = Vec::new();
    let (mut same, mut differ) = (0, 0);
    for (name, marks) in &cases {
        for turn in turns {
            let turned: Vec<Annot> = (marks.iter())
                .map(|a| Annot { turn, ..a.clone() })
                .collect();
            let held = forms::holding(&turned);
            let mut f = forms::Form::new(1);
            let mut page = f.dict(f.pages[0]).clone();
            forms::write_marks(&mut f.pdf, &mut page, &held);
            *f.dict(f.pages[0]) = page;
            let pdf = &f.pdf;
            let annots = f.annots(0);
            let dicts: Vec<(ObjectId, &Dictionary)> = (annots.iter())
                .map(|&id| (id, pdf.get_dictionary(id).expect("a mark's dictionary")))
                .collect();
            let read: Vec<ReadAnnot<'_>> = (dicts.iter())
                .map(|&(id, d)| read(pdf, d, Some(id)).expect("sheaf's marks read"))
                .collect();
            let pairs: Vec<(&Dictionary, &ReadAnnot<'_>)> =
                dicts.iter().map(|(_, d)| *d).zip(&read).collect();
            let plain: Vec<&Dictionary> = dicts.iter().map(|(_, d)| *d).collect();
            let store = Store::new();
            let files = Interned::new(&store);
            for (opts_name, opts) in [
                ("ahead", Options::default()),
                (
                    "files",
                    Options {
                        files: Some(&files),
                    },
                ),
            ] {
                let today = super::convert_page(pdf, &plain, turn, opts);
                let family = convert_page(pdf, &pairs, turn, opts);
                for (slot, ((t, fam), d)) in today.iter().zip(&family).zip(&plain).enumerate() {
                    let sub = super::subtype(d);
                    let (x, y) = (json(t), json(fam));
                    let mut line = format!(
                        "{name}:{}:{opts_name}\t{slot}\t{sub}\ttoday={x}\tfamily={y}",
                        turn.degrees()
                    );
                    if x == y {
                        same += 1;
                        line.push_str("\tsame");
                    } else {
                        differ += 1;
                        line.push_str("\tdiffer");
                        let value = |m: &Result<Annot, NotAMark>| match m {
                            Ok(a) => serde_json::to_value(a).expect("a mark is JSON"),
                            Err(why) => serde_json::Value::String(format!("{why:?}")),
                        };
                        for (path, a, b) in json_diff("", &value(t), &value(fam)) {
                            line.push_str(&format!("\t{path}: {a} -> {b}"));
                        }
                    }
                    lines.push(line);
                }
            }
        }
    }
    eprintln!("{} lines: {same} the same, {differ} not", lines.len());
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(&to, text).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
}

/// The fields of two JSON values that differ, each by its path, with its
/// value in each.
#[cfg(test)]
fn json_diff(
    at: &str,
    a: &serde_json::Value,
    b: &serde_json::Value,
) -> Vec<(String, String, String)> {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            (keys.into_iter())
                .flat_map(|k| {
                    let null = Value::Null;
                    json_diff(
                        &format!("{at}.{k}"),
                        x.get(k).unwrap_or(&null),
                        y.get(k).unwrap_or(&null),
                    )
                })
                .collect()
        }
        _ if a == b => Vec::new(),
        _ => vec![(at.to_string(), a.to_string(), b.to_string())],
    }
}
