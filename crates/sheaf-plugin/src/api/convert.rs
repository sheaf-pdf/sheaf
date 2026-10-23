//! sheaf's types as the API says them. Geometry is PageSpace on both
//! sides, so nothing is mapped: a page's view matrices go with its info
//! for a plugin to place things where the user sees them.

use sheaf_doc::file::ForeignAnnot;
use sheaf_doc::geom::{Affine, Geom, Rect};
use sheaf_doc::measure::Measured;
use sheaf_doc::{
    Annot, AnnotId, AnnotKind, Bookmark, Box2, DestView, OutlineId, PageId, Pt, VersionId,
};
use sheaf_edit::Rotation;

use super::api::types as w;
use crate::change::{ActorKind, ChangeCause, ChangeInfo, Scopes};
use crate::runtime::DocEvent;

fn rotation_to_wit(r: Rotation) -> w::Rotation {
    match r {
        Rotation::R0 => w::Rotation::R0,
        Rotation::R90 => w::Rotation::R90,
        Rotation::R180 => w::Rotation::R180,
        Rotation::R270 => w::Rotation::R270,
    }
}

fn rgb_to_wit(c: [u8; 3]) -> w::Rgb {
    w::Rgb {
        r: c[0],
        g: c[1],
        b: c[2],
    }
}

pub(super) fn pt_to_wit(p: Pt) -> w::Pt {
    w::Pt { x: p.x, y: p.y }
}

pub(super) fn box_to_wit(b: Box2) -> w::Box2 {
    w::Box2 {
        x: b.x,
        y: b.y,
        w: b.w,
        h: b.h,
    }
}

pub(super) fn box_from_wit(b: w::Box2) -> Box2 {
    Box2 {
        x: b.x,
        y: b.y,
        w: b.w,
        h: b.h,
    }
}

fn rect_to_wit(r: Rect) -> w::Box2 {
    w::Box2 {
        x: r.x0,
        y: r.y0,
        w: r.width(),
        h: r.height(),
    }
}

fn affine_to_wit(m: Affine) -> w::Affine {
    w::Affine {
        a: m.a,
        b: m.b,
        c: m.c,
        d: m.d,
        e: m.e,
        f: m.f,
    }
}

pub(super) fn version_to_wit(instance: u64, v: VersionId) -> w::Version {
    w::Version { instance, id: v.0 }
}

/// Page `id`, of boxes and turn `geom`, called `label`.
pub(super) fn page_info(id: PageId, geom: &Geom, label: String) -> w::PageInfo {
    w::PageInfo {
        id: id.0,
        media: rect_to_wit(geom.media),
        crop: rect_to_wit(geom.crop),
        rotate: rotation_to_wit(geom.rotate),
        label,
        to_view: affine_to_wit(geom.to_view()),
        from_view: affine_to_wit(geom.from_view()),
    }
}

fn dest_to_wit(v: &DestView) -> w::DestView {
    match v {
        DestView::Xyz { left, top, zoom } => w::DestView::Xyz(w::DestXyz {
            left: *left,
            top: *top,
            zoom: *zoom,
        }),
        DestView::Other { kind, params } => w::DestView::Other(w::DestOther {
            kind: kind.clone(),
            params: params.clone(),
        }),
    }
}

pub(super) fn outline_item(depth: u16, id: OutlineId, b: &Bookmark) -> w::OutlineItem {
    w::OutlineItem {
        id: id.0,
        title: b.title.to_string(),
        target: b.page().map(|p| p.0),
        view: b.view().map(dest_to_wit),
        color: b.color.map(rgb_to_wit),
        depth,
    }
}

/// One of sheaf's marks as the API shows it, by `author` (a role,
/// [`Roles`](crate::runtime::Roles)): all of it.
pub(super) fn annot_to_wit(a: &Annot, author: String) -> w::Annot {
    let boxes = |r: &[Box2]| r.iter().copied().map(box_to_wit).collect::<Vec<_>>();
    let points = |p: &[Pt]| p.iter().copied().map(pt_to_wit).collect::<Vec<_>>();
    let kind = match &a.kind {
        AnnotKind::Highlight { rects } => w::AnnotKind::Highlight(boxes(rects)),
        AnnotKind::Note { pos } => w::AnnotKind::Note(pt_to_wit(*pos)),
        AnnotKind::FreeText { rect, size } => w::AnnotKind::FreeText(w::TextBox {
            rect: box_to_wit(*rect),
            size: *size,
        }),
        AnnotKind::Square { rect } => w::AnnotKind::Square(box_to_wit(*rect)),
        AnnotKind::Ellipse { rect } => w::AnnotKind::Ellipse(box_to_wit(*rect)),
        AnnotKind::Line {
            a,
            b,
            arrow,
            arrow_start,
        } => w::AnnotKind::Line(w::LineShape {
            a: pt_to_wit(*a),
            b: pt_to_wit(*b),
            arrow: *arrow,
            arrow_start: *arrow_start,
        }),
        AnnotKind::Ink { paths } => w::AnnotKind::Ink(paths.iter().map(|p| points(p)).collect()),
        AnnotKind::Stamp { rect, glyph } => w::AnnotKind::Stamp(w::StampShape {
            rect: box_to_wit(*rect),
            glyph: glyph.clone(),
        }),
        AnnotKind::Connector {
            rect,
            label,
            partner,
        } => w::AnnotKind::Connector(w::ConnectorShape {
            rect: box_to_wit(*rect),
            label: label.clone(),
            partner: partner.map(|p| p.0),
        }),
        AnnotKind::Tape { rect, size, .. } => w::AnnotKind::Tape(w::TextBox {
            rect: box_to_wit(*rect),
            size: *size,
        }),
        AnnotKind::Underline { rects } => w::AnnotKind::Underline(boxes(rects)),
        AnnotKind::StrikeOut { rects } => w::AnnotKind::StrikeOut(boxes(rects)),
        AnnotKind::Squiggly { rects } => w::AnnotKind::Squiggly(boxes(rects)),
        AnnotKind::Replace { rects, struck, at } => w::AnnotKind::Replace(w::ReplaceShape {
            rects: boxes(rects),
            struck: struck.clone(),
            at: box_to_wit(*at),
        }),
        AnnotKind::Insert { at } => w::AnnotKind::Insert(box_to_wit(*at)),
        AnnotKind::Reply { parent } => w::AnnotKind::Reply(parent.clone()),
        AnnotKind::Status {
            parent,
            model,
            state,
        } => w::AnnotKind::Status(w::StatusShape {
            parent: parent.clone(),
            model: model.clone(),
            state: state.clone(),
        }),
        AnnotKind::PageStamp {
            rect,
            stamp,
            name,
            text,
        } => w::AnnotKind::PageStamp(w::PageStampShape {
            rect: box_to_wit(*rect),
            stamp: stamp.clone(),
            name: name.clone(),
            text: text.clone(),
        }),
        AnnotKind::TextStamp { rect, text, status } => w::AnnotKind::TextStamp(w::TextStampShape {
            rect: box_to_wit(*rect),
            text: text.clone(),
            status: *status,
        }),
        AnnotKind::PageBorder { rect, status } => w::AnnotKind::PageBorder(w::PageBorderShape {
            rect: box_to_wit(*rect),
            status: status.clone(),
        }),
        AnnotKind::Polygon { points: p } => w::AnnotKind::Polygon(points(p)),
        AnnotKind::Polyline { points: p } => w::AnnotKind::Polyline(points(p)),
        AnnotKind::Callout {
            rect,
            knee,
            point,
            size,
        } => w::AnnotKind::Callout(w::CalloutShape {
            rect: box_to_wit(*rect),
            knee: pt_to_wit(*knee),
            point: pt_to_wit(*point),
            size: *size,
        }),
        AnnotKind::FileAttachment {
            pos,
            icon,
            file,
            name,
            size,
        } => w::AnnotKind::FileAttachment(w::AttachmentShape {
            pos: pt_to_wit(*pos),
            icon: icon.clone(),
            file: file.clone(),
            name: name.clone(),
            size: *size,
        }),
        AnnotKind::Measure {
            what,
            points: p,
            scale,
        } => w::AnnotKind::Measure(w::MeasureShape {
            what: match what {
                Measured::Distance => w::Measured::Distance,
                Measured::Perimeter => w::Measured::Perimeter,
                Measured::Area => w::Measured::Area,
            },
            points: points(p),
            scale: w::Scale {
                page: scale.page,
                page_unit: scale.page_unit.clone(),
                real: scale.real,
                unit: scale.unit.clone(),
                precision: scale.precision,
            },
        }),
    };
    w::Annot {
        id: a.id.0,
        kind,
        color: rgb_to_wit(a.color),
        opacity: a.opacity,
        stroke: a.stroke,
        author,
        contents: a.contents.clone(),
        fill: a.fill.map(rgb_to_wit),
        dashed: a.dashed,
        cloudy: a.cloudy,
        created: a.created.clone(),
        modified: a.modified.clone(),
        name: a.name(),
    }
}

/// Another tool's comment `id` as the API shows it, by `author` (a role).
pub(super) fn comment_to_wit(id: AnnotId, c: &ForeignAnnot, author: String) -> w::CommentSummary {
    w::CommentSummary {
        id: id.0,
        subtype: c.subtype.clone(),
        rect: box_to_wit(c.rect),
        contents: c.contents.clone(),
        author,
        name: c.name.clone(),
    }
}

/// An event as a plugin is told it.
pub(crate) fn event_to_wit(e: &DocEvent) -> w::DocEvent {
    match e {
        DocEvent::Opened => w::DocEvent::Opened,
        DocEvent::Changed(c) => w::DocEvent::Changed(change_to_wit(c)),
        DocEvent::Saved(p) => w::DocEvent::Saved(p.clone()),
        DocEvent::SelectionChanged => w::DocEvent::SelectionChanged,
        DocEvent::Closed => w::DocEvent::Closed,
        DocEvent::Region { page, rect } => w::DocEvent::Region(w::Region {
            page: page.0,
            rect: box_to_wit(*rect),
        }),
        DocEvent::RegionCleared => w::DocEvent::RegionCleared,
        DocEvent::EditRefused(why) => w::DocEvent::EditRefused(why.clone()),
    }
}

fn change_to_wit(c: &ChangeInfo) -> w::ChangeInfo {
    let pages = |set: &std::collections::BTreeSet<PageId>| set.iter().map(|p| p.0).collect();
    let Scopes {
        order,
        geometry,
        content,
        annots,
        outline,
        labels,
        form,
        generators,
    } = &c.scopes;
    w::ChangeInfo {
        version: version_to_wit(c.instance, c.version),
        parent: version_to_wit(c.instance, c.parent),
        cause: match c.cause {
            ChangeCause::Opened => w::ChangeCause::Opened,
            ChangeCause::Commit => w::ChangeCause::Commit,
            ChangeCause::Amend => w::ChangeCause::Amend,
            ChangeCause::Undo => w::ChangeCause::Undo,
            ChangeCause::Redo => w::ChangeCause::Redo,
            ChangeCause::Jump => w::ChangeCause::Jump,
            ChangeCause::Saved => w::ChangeCause::Saved,
            ChangeCause::Rollback => w::ChangeCause::Rollback,
            ChangeCause::Revert => w::ChangeCause::Revert,
            ChangeCause::Coalesced => w::ChangeCause::Coalesced,
        },
        actor_kind: match c.actor_kind {
            ActorKind::User => w::ActorKind::User,
            ActorKind::Plugin => w::ActorKind::Plugin,
            ActorKind::Job => w::ActorKind::Job,
            ActorKind::Mixed => w::ActorKind::Mixed,
        },
        scopes: w::Scopes {
            order: *order,
            geometry: pages(geometry),
            content: pages(content),
            annots: pages(annots),
            outline: *outline,
            labels: *labels,
            form: *form,
            generators: *generators,
        },
    }
}

#[cfg(test)]
mod tests {
    use sheaf_doc::geom::{Geom, Rect};
    use sheaf_doc::measure::{Measured, Scale};
    use sheaf_doc::{Annot, AnnotId, AnnotKind, PageId, Pt};
    use sheaf_edit::Rotation;

    use super::w;

    /// `p` mapped by `m`, as a plugin maps it.
    fn apply(m: &w::Affine, p: Pt) -> Pt {
        Pt {
            x: m.a * p.x + m.c * p.y + m.e,
            y: m.b * p.x + m.d * p.y + m.f,
        }
    }

    /// A page's view matrices undo each other, and take PageSpace to the
    /// page as it shows, whichever way it's turned: its visible box's
    /// corners to the shown page's.
    #[test]
    fn a_page_s_view_matrices_undo_each_other() {
        for rotate in [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270] {
            let geom = Geom {
                crop: Rect::new(20.0, 30.0, 600.0, 770.0),
                rotate,
                ..Geom::read(None, None, 0)
            };
            let info = super::page_info(PageId(1), &geom, "iv".into());
            assert_eq!(
                (info.label.as_str(), info.rotate),
                ("iv", super::rotation_to_wit(rotate))
            );
            for p in [
                Pt { x: 20.0, y: 30.0 },
                Pt { x: 600.0, y: 770.0 },
                Pt { x: 7.5, y: 3.0 },
            ] {
                let back = apply(&info.from_view, apply(&info.to_view, p));
                assert!(
                    (back.x - p.x).abs() < 1e-3 && (back.y - p.y).abs() < 1e-3,
                    "{rotate:?}"
                );
            }
            let shown = geom.shown();
            let corners = [Pt { x: 20.0, y: 30.0 }, Pt { x: 600.0, y: 770.0 }]
                .map(|p| apply(&info.to_view, p));
            let (xs, ys) = (corners.map(|c| c.x), corners.map(|c| c.y));
            let span = |v: [f32; 2]| (v[0].min(v[1]), v[0].max(v[1]));
            assert_eq!(
                (span(xs), span(ys)),
                ((0.0, shown.width), (0.0, shown.height))
            );
        }
    }

    fn mark(kind: AnnotKind) -> Annot {
        Annot {
            id: AnnotId(7),
            kind,
            color: [1, 2, 3],
            opacity: 0.5,
            stroke: 2.0,
            author: "Greg".into(),
            contents: "c".into(),
            modified: "D:20260101000000".into(),
            created: "D:20260101000000".into(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        }
    }

    /// A plugin is shown every kind of mark and all a mark says, in
    /// PageSpace as it's kept, by the author it's told.
    #[test]
    fn a_mark_is_shown_whole_by_its_role() {
        let ink = mark(AnnotKind::Ink {
            paths: vec![vec![Pt { x: 1.0, y: 2.0 }, Pt { x: 3.0, y: 4.0 }]],
        });
        let shown = super::annot_to_wit(&ink, "you".into());
        let path = vec![w::Pt { x: 1.0, y: 2.0 }, w::Pt { x: 3.0, y: 4.0 }];
        assert_eq!(shown.kind, w::AnnotKind::Ink(vec![path]));
        assert_eq!(
            (shown.id, shown.author.as_str(), shown.contents.as_str()),
            (7, "you", "c")
        );
        assert_eq!(
            (shown.color, shown.opacity, shown.stroke),
            (w::Rgb { r: 1, g: 2, b: 3 }, 0.5, 2.0)
        );
        let mut area = mark(AnnotKind::Measure {
            what: Measured::Area,
            points: vec![
                Pt { x: 0.0, y: 0.0 },
                Pt { x: 72.0, y: 0.0 },
                Pt { x: 72.0, y: 72.0 },
            ],
            scale: Box::new(Scale::default()),
        });
        area.dashed = true;
        let shown = super::annot_to_wit(&area, "other".into());
        let w::AnnotKind::Measure(m) = &shown.kind else {
            panic!("{:?}", shown.kind)
        };
        assert_eq!((m.what, m.points.len()), (w::Measured::Area, 3));
        assert!(shown.dashed);
        let mut poly = mark(AnnotKind::Polygon {
            points: vec![
                Pt { x: 0.0, y: 0.0 },
                Pt { x: 9.0, y: 0.0 },
                Pt { x: 9.0, y: 9.0 },
            ],
        });
        poly.fill = Some([9, 9, 9]);
        poly.cloudy = true;
        let shown = super::annot_to_wit(&poly, "plugin checker".into());
        assert_eq!(shown.name, "sheaf:7");
        assert_eq!(
            (shown.fill, shown.cloudy, shown.author.as_str()),
            (Some(w::Rgb { r: 9, g: 9, b: 9 }), true, "plugin checker")
        );
    }
}
