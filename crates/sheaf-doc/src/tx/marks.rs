//! Annotation ops lowered (M10.5 §3.6, §6.8): sheaf's marks made, changed,
//! moved and deleted, connector pairs, and other tools' comments deleted or
//! made marks.
//!
//! What a mark is beside what a caller says of it is the host's: its id
//! and `/NM`, its dates, its author when none is given (a plugin's marks
//! are by the plugin's id, as History names it), the identity tokens in
//! its text (`<<name>>` and `<<initials>>` stay as they are for a plugin,
//! which never learns who the user is unless the user granted it
//! `identity`, M10.5 proposal 20), its style kept to what its kind draws,
//! a connector's partner, the comment a reply is to by its `/NM`, and the
//! stems that name the stamp page or file it holds. An imported comment
//! keeps its own name, dates and author ([`Imported`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use im::Vector;
use sheaf_edit::{Delta, Imported, MarkKind, MarkPatch, NewMark, Patch, Rotation, Style};

use super::error::EditError;
use super::{Actor, Grant, Lower, Made, Notice, put_page_annots};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2, Pt};
use crate::forms::FieldId;
use crate::model::{AnnotBody, AnnotRec, FieldNode, Mark, PageId};
use crate::prim::{Key, Rec, Val};

/// What a caller says of a mark's kind: `kind` without what the host owns
/// of it (a connector's partner, a reply's comment, a stamp's or
/// attachment's stem and an attachment's size).
pub fn mark_kind(kind: &AnnotKind) -> MarkKind {
    use AnnotKind as A;
    match kind.clone() {
        A::Highlight { rects } => MarkKind::Highlight { rects },
        A::Note { pos } => MarkKind::Note { pos },
        A::FreeText { rect, size } => MarkKind::FreeText { rect, size },
        A::Square { rect } => MarkKind::Square { rect },
        A::Ellipse { rect } => MarkKind::Ellipse { rect },
        A::Line {
            a,
            b,
            arrow,
            arrow_start,
        } => MarkKind::Line {
            a,
            b,
            arrow,
            arrow_start,
        },
        A::Ink { paths } => MarkKind::Ink { paths },
        A::Stamp { rect, glyph } => MarkKind::Stamp { rect, glyph },
        A::Connector { rect, label, .. } => MarkKind::Connector { rect, label },
        A::Tape {
            rect,
            size,
            tape,
            look,
        } => MarkKind::Tape {
            rect,
            size,
            tape,
            look,
        },
        A::Underline { rects } => MarkKind::Underline { rects },
        A::StrikeOut { rects } => MarkKind::StrikeOut { rects },
        A::Squiggly { rects } => MarkKind::Squiggly { rects },
        A::Replace { rects, struck, at } => MarkKind::Replace { rects, struck, at },
        A::Insert { at } => MarkKind::Insert { at },
        A::Reply { .. } => MarkKind::Reply {},
        A::Status { model, state, .. } => MarkKind::Status { model, state },
        A::PageStamp {
            rect, name, text, ..
        } => MarkKind::PageStamp { rect, name, text },
        A::TextStamp { rect, text, status } => MarkKind::TextStamp { rect, text, status },
        A::PageBorder { rect, status } => MarkKind::PageBorder { rect, status },
        A::Polygon { points } => MarkKind::Polygon { points },
        A::Polyline { points } => MarkKind::Polyline { points },
        A::Callout {
            rect,
            knee,
            point,
            size,
        } => MarkKind::Callout {
            rect,
            knee,
            point,
            size,
        },
        A::FileAttachment {
            pos, icon, name, ..
        } => MarkKind::FileAttachment { pos, icon, name },
        A::Measure {
            what,
            points,
            scale,
        } => MarkKind::Measure {
            what,
            points,
            scale,
        },
    }
}

/// How `a` is drawn, as a caller says it.
pub fn mark_style(a: &Annot) -> Style {
    Style {
        color: a.color,
        opacity: a.opacity,
        stroke: a.stroke,
        fill: a.fill,
        dashed: a.dashed,
        cloudy: a.cloudy,
    }
}

/// Everything `a` says that a patch can: its kind and shape, its style
/// and its text. Who made it and when, and what the host owns of it, stay
/// as the mark has them.
pub fn mark_patch(a: &Annot) -> MarkPatch {
    MarkPatch {
        kind: Some(mark_kind(&a.kind)),
        color: Some(a.color),
        opacity: Some(a.opacity),
        stroke: Some(a.stroke),
        fill: match a.fill {
            Some(c) => Patch::Set(c),
            None => Patch::Clear,
        },
        dashed: Some(a.dashed),
        cloudy: Some(a.cloudy),
        contents: Some(a.contents.clone()),
    }
}

/// What a caller changed of a mark it read as `was` and made `now`: a
/// patch naming only that, so what else changed meanwhile stays (§8.6).
pub fn mark_changes(was: &Annot, now: &Annot) -> MarkPatch {
    let changed = |a: f32, b: f32| (a != b).then_some(b);
    MarkPatch {
        kind: (now.kind != was.kind).then(|| mark_kind(&now.kind)),
        color: (now.color != was.color).then_some(now.color),
        opacity: changed(was.opacity, now.opacity),
        stroke: changed(was.stroke, now.stroke),
        fill: match now.fill {
            _ if now.fill == was.fill => Patch::Keep,
            Some(c) => Patch::Set(c),
            None => Patch::Clear,
        },
        dashed: (now.dashed != was.dashed).then_some(now.dashed),
        cloudy: (now.cloudy != was.cloudy).then_some(now.cloudy),
        contents: (now.contents != was.contents).then(|| now.contents.clone()),
    }
}

/// What the host owns of a mark's kind: its connector partner, the `/NM`
/// of the comment it's a reply or status to, its page stamp's stem, and
/// its attachment's stem and size.
#[derive(Default)]
struct Owned {
    partner: Option<AnnotId>,
    parent: Option<String>,
    stamp: Option<String>,
    file: Option<(String, u64)>,
}

impl Owned {
    /// What `kind` has of it.
    fn of(kind: &AnnotKind) -> Owned {
        match kind {
            AnnotKind::Connector { partner, .. } => Owned {
                partner: *partner,
                ..Owned::default()
            },
            AnnotKind::Reply { parent } | AnnotKind::Status { parent, .. } => Owned {
                parent: Some(parent.clone()),
                ..Owned::default()
            },
            AnnotKind::PageStamp { stamp, .. } => Owned {
                stamp: Some(stamp.clone()),
                ..Owned::default()
            },
            AnnotKind::FileAttachment { file, size, .. } => Owned {
                file: Some((file.clone(), *size)),
                ..Owned::default()
            },
            _ => Owned::default(),
        }
    }
}

/// `kind` with what the host owns of it: an error says what's missing.
fn annot_kind(kind: MarkKind, owned: Owned) -> Result<AnnotKind, &'static str> {
    use MarkKind as M;
    Ok(match kind {
        M::Highlight { rects } => AnnotKind::Highlight { rects },
        M::Note { pos } => AnnotKind::Note { pos },
        M::FreeText { rect, size } => AnnotKind::FreeText { rect, size },
        M::Square { rect } => AnnotKind::Square { rect },
        M::Ellipse { rect } => AnnotKind::Ellipse { rect },
        M::Line {
            a,
            b,
            arrow,
            arrow_start,
        } => AnnotKind::Line {
            a,
            b,
            arrow,
            arrow_start,
        },
        M::Ink { paths } => AnnotKind::Ink { paths },
        M::Stamp { rect, glyph } => AnnotKind::Stamp { rect, glyph },
        M::Connector { rect, label } => AnnotKind::Connector {
            rect,
            label,
            partner: owned.partner,
        },
        M::Tape {
            rect,
            size,
            tape,
            look,
        } => AnnotKind::Tape {
            rect,
            size,
            tape,
            look,
        },
        M::Underline { rects } => AnnotKind::Underline { rects },
        M::StrikeOut { rects } => AnnotKind::StrikeOut { rects },
        M::Squiggly { rects } => AnnotKind::Squiggly { rects },
        M::Replace { rects, struck, at } => AnnotKind::Replace { rects, struck, at },
        M::Insert { at } => AnnotKind::Insert { at },
        M::Reply {} => AnnotKind::Reply {
            parent: owned.parent.ok_or("a reply needs the comment it's to")?,
        },
        M::Status { model, state } => AnnotKind::Status {
            parent: owned
                .parent
                .ok_or("a status needs the comment it's set on")?,
            model,
            state,
        },
        M::PageStamp { rect, name, text } => AnnotKind::PageStamp {
            rect,
            stamp: owned.stamp.ok_or("a page stamp needs its page")?,
            name,
            text,
        },
        M::TextStamp { rect, text, status } => AnnotKind::TextStamp { rect, text, status },
        M::PageBorder { rect, status } => AnnotKind::PageBorder { rect, status },
        M::Polygon { points } => AnnotKind::Polygon { points },
        M::Polyline { points } => AnnotKind::Polyline { points },
        M::Callout {
            rect,
            knee,
            point,
            size,
        } => AnnotKind::Callout {
            rect,
            knee,
            point,
            size,
        },
        M::FileAttachment { pos, icon, name } => {
            let (file, size) = owned.file.ok_or("an attachment needs its file")?;
            AnnotKind::FileAttachment {
                pos,
                icon,
                file,
                name,
                size,
            }
        }
        M::Measure {
            what,
            points,
            scale,
        } => AnnotKind::Measure {
            what,
            points,
            scale,
        },
    })
}

/// A mark on `page`, as `mark` says, a reply or status to `reply_to` (on
/// its page), holding `held`.
pub(crate) fn add(
    lw: &mut Lower,
    page: PageId,
    mark: &NewMark,
    held: Option<&Grant>,
    reply_to: Option<AnnotId>,
    imported: Option<&Imported>,
) -> Result<(), EditError> {
    let parent = reply_to.map(|id| lw.rec.annot(id).expect("resolved").name());
    let (stamp, file) = match held {
        Some(Grant::Stamp { stem, form }) => (Some((stem.clone(), form.clone())), None),
        Some(Grant::Blob(b)) => (None, Some(b.clone())),
        None => (None, None),
    };
    let owned = Owned {
        partner: None,
        parent,
        stamp: stamp.as_ref().map(|(stem, _)| stem.clone()),
        file: file.as_ref().map(|f| (f.id().stem(), f.id().len)),
    };
    let kind = annot_kind(mark.kind.clone(), owned).map_err(|why| lw.invalid(why))?;
    let id = lw.annot_id();
    let annot = made(
        lw,
        id,
        kind,
        &mark.style,
        &mark.contents,
        mark.author.as_deref(),
        mark.turn,
        imported,
    );
    if annot.unencodable() {
        lw.notices.push(Notice::Unencodable);
    }
    let mark = Mark {
        annot,
        src: None,
        file,
        stamp: stamp.map(|(_, form)| form),
    };
    add_annot(&mut lw.rec, page, mark);
    lw.made.push(Made::Annot(id));
    Ok(())
}

/// A connector pair labeled `label`, one end at `a` and one at `b`, each
/// the other's partner, reading upright as its page is turned.
pub(crate) fn connect(
    lw: &mut Lower,
    a: (PageId, Box2),
    b: (PageId, Box2),
    label: &str,
    style: &Style,
) {
    let ids = [lw.annot_id(), lw.annot_id()];
    let mut unencodable = false;
    for (i, (page, rect)) in [a, b].into_iter().enumerate() {
        lw.read(Key::Geom(page));
        let turn = lw.rec.geom(page).expect("resolved").rotate;
        let kind = AnnotKind::Connector {
            rect,
            label: label.to_string(),
            partner: Some(ids[1 - i]),
        };
        let annot = made(lw, ids[i], kind, style, "", None, turn, None);
        unencodable |= annot.unencodable();
        add_annot(&mut lw.rec, page, Mark::from(annot));
        lw.made.push(Made::Annot(ids[i]));
    }
    if unencodable {
        lw.notices.push(Notice::Unencodable);
    }
}

/// `patch` made to each of `ids`, sheaf's marks: what it names, the rest
/// as it is now; an imported comment's own name, dates and author with
/// `imported`, whose name must be the mark's.
pub(crate) fn patch(
    lw: &mut Lower,
    ids: &[AnnotId],
    patch: &MarkPatch,
    imported: Option<&Imported>,
) -> Result<(), EditError> {
    for &id in ids {
        let (page, was) = mark_of(lw, id);
        let mut a = was.annot.clone();
        if let Some(kind) = &patch.kind {
            // Its partner's partner is this one: the pair stays a pair.
            let connector = |k: &AnnotKind| matches!(k, AnnotKind::Connector { .. });
            if connector(&a.kind) && !matches!(kind, MarkKind::Connector { .. }) {
                return Err(lw.invalid("a connector can't change to another kind of mark"));
            }
            let owned = Owned::of(&a.kind);
            a.kind = annot_kind(kind.clone(), owned)
                .map_err(|why| lw.invalid(format!("{why}: a mark's kind can't change to it")))?;
        }
        let MarkPatch {
            kind: _,
            color,
            opacity,
            stroke,
            fill,
            dashed,
            cloudy,
            contents,
        } = patch;
        a.color = color.unwrap_or(a.color);
        a.opacity = opacity.unwrap_or(a.opacity);
        a.stroke = stroke.unwrap_or(a.stroke);
        fill.clone().apply(&mut a.fill);
        a.dashed = dashed.unwrap_or(a.dashed);
        a.cloudy = cloudy.unwrap_or(a.cloudy);
        if let Some(text) = contents {
            a.contents = match imported {
                Some(_) => text.clone(),
                None => filled(lw, text),
            };
        }
        if imported.is_none() {
            fill_kind(lw, &mut a.kind);
        }
        kept_to_kind(&mut a);
        match imported {
            Some(imp) => {
                a.created = imp.created.clone();
                a.modified = imp.modified.clone();
                a.author = imp.author.clone();
            }
            // Said again as it is, it hasn't changed.
            None if a == was.annot => continue,
            None => a.modified = lw.cx.host.now.to_string(),
        }
        if a.unencodable() {
            lw.notices.push(Notice::Unencodable);
        }
        let mark = Mark {
            annot: a,
            ..Mark::clone(&was)
        };
        put_mark(&mut lw.rec, page, mark);
    }
    Ok(())
}

/// Moves each of `ids`, sheaf's marks, by `by`, onto `to_page` when
/// that's another page.
pub(crate) fn move_marks(
    lw: &mut Lower,
    ids: &[AnnotId],
    by: Delta,
    to_page: Option<PageId>,
) -> Result<(), EditError> {
    for &id in ids {
        let (page, was) = mark_of(lw, id);
        let (dx, dy) = match by {
            Delta::Page(v) => (v.x, v.y),
            Delta::View(v) => {
                // The view's x right and y down, in PageSpace.
                lw.read(Key::Geom(page));
                let m = lw.rec.geom(page).expect("a mark's page").from_view();
                let (o, p) = (
                    m.apply(Pt { x: 0.0, y: 0.0 }),
                    m.apply(Pt { x: v.x, y: v.y }),
                );
                (p.x - o.x, p.y - o.y)
            }
        };
        if (dx, dy) == (0.0, 0.0) && to_page.is_none_or(|to| to == page) {
            continue;
        }
        let mut a = was.annot.clone();
        a.kind = a.kind.translated(dx, dy);
        a.modified = lw.cx.host.now.to_string();
        let mark = Mark {
            annot: a,
            ..Mark::clone(&was)
        };
        match to_page.filter(|&to| to != page) {
            Some(to) => {
                let mut ids = lw.rec.annot_ids_on(page).cloned().unwrap_or_default();
                ids.retain(|&other| other != id);
                put_page_annots(&mut lw.rec, page, ids);
                lw.rec.put(Key::Annot(id), None);
                add_annot(&mut lw.rec, to, mark);
            }
            None => put_mark(&mut lw.rec, page, mark),
        }
    }
    Ok(())
}

/// Deletes `ids`: marks, other tools' comments and links, not a form
/// field's widget, which goes only with its page. Their threads go with
/// them ([`super::cascade`]).
pub(crate) fn delete(lw: &mut Lower, ids: &[AnnotId]) {
    let mut by_page: BTreeMap<PageId, HashSet<AnnotId>> = BTreeMap::new();
    for &id in ids {
        let rec = lw.rec.annot(id).expect("resolved");
        by_page.entry(rec.page).or_default().insert(id);
    }
    for (page, gone) in by_page {
        drop_annots(&mut lw.rec, page, |id, _| gone.contains(&id));
    }
}

/// Makes each of `ids`, another tool's comment, a mark that keeps its id
/// and `/NM`, its thread, and the object it is (unless it's a copy's,
/// whose object is its original's): what it is as one of sheaf's marks,
/// read with the file ([`ForeignAnnot::mark`](crate::file::ForeignAnnot::mark)).
/// The parts grouped under it (`/RT /Group`), which the mark draws itself,
/// go; its replies stay, in the mark's thread.
pub(crate) fn adopt(lw: &mut Lower, ids: &[AnnotId]) -> Result<(), EditError> {
    for &id in ids {
        // Resolve refuses a part beside the comment it's grouped under,
        // whose adoption takes it.
        let Some(rec) = lw.rec.annot(id) else {
            return Err(EditError::internal(format!(
                "annotation {} went before it was adopted",
                id.0
            )));
        };
        let page = rec.page;
        let f = rec.foreign().cloned().expect("another tool's comment");
        let read = f.summary.mark.as_deref().expect("one sheaf can hold");
        let name = f.summary.name.clone();
        let mut annot = Annot {
            id,
            name: name.clone(),
            ..read.clone()
        };
        // Its thread is the record's: one an insert or a duplicate moved
        // to a copy of the comment it's in, not the file's.
        if let AnnotKind::Reply { parent } | AnnotKind::Status { parent, .. } = &mut annot.kind
            && let Some(now) = &f.summary.parent
        {
            parent.clone_from(now);
        }
        let (stamp, file) = match &annot.kind {
            // A comment's stamp holds the form its appearance is in its
            // file, which reading the file found it by.
            AnnotKind::PageStamp { stamp, name, .. } => {
                let found = crate::model::find_stamp(&lw.cx.store, &lw.cx.primary, stamp);
                (
                    Some(found.ok_or_else(|| lw.not_found(format!("the stamp {name}")))?),
                    None,
                )
            }
            AnnotKind::FileAttachment { file, name, .. } => {
                let found = super::resolve::file_named(&lw.cx.store, file);
                (
                    None,
                    Some(found.ok_or_else(|| lw.not_found(format!("the attached file {name}")))?),
                )
            }
            _ => (None, None),
        };
        if annot.unencodable() {
            lw.notices.push(Notice::Unencodable);
        }
        drop_annots(&mut lw.rec, page, |other, a| {
            other != id
                && a.foreign().is_some_and(|f| {
                    f.summary.grouped && f.summary.parent.as_deref() == Some(&name)
                })
        });
        let mark = Mark {
            annot,
            src: f.src.clone().filter(|_| f.copy == 0),
            file,
            stamp,
        };
        put_mark(&mut lw.rec, page, mark);
    }
    Ok(())
}

/// Mark `id`, one of sheaf's, and its page.
fn mark_of(lw: &Lower, id: AnnotId) -> (PageId, Arc<Mark>) {
    let rec = lw.rec.annot(id).expect("resolved");
    (rec.page, rec.mark_record().expect("a mark").clone())
}

/// A new mark's annotation, made `id` and filled in as the host fills it
/// (§6.8): an imported comment's own name, dates and author, else now,
/// the author given or the host's, and the identity tokens in its text.
#[allow(clippy::too_many_arguments)]
fn made(
    lw: &Lower,
    id: AnnotId,
    mut kind: AnnotKind,
    style: &Style,
    contents: &str,
    author: Option<&str>,
    turn: Rotation,
    imported: Option<&Imported>,
) -> Annot {
    let host = &lw.cx.host;
    let author = match &lw.cx.actor {
        Actor::Plugin { id, .. } => id.to_string(),
        _ => (author.filter(|a| !a.trim().is_empty()))
            .map_or_else(|| host.person.to_string(), str::to_string),
    };
    let contents = match imported {
        Some(_) => contents.to_string(),
        None => {
            fill_kind(lw, &mut kind);
            filled(lw, contents)
        }
    };
    let mut a = Annot {
        id,
        kind,
        color: style.color,
        opacity: style.opacity,
        stroke: style.stroke,
        author,
        contents,
        modified: host.now.to_string(),
        created: host.now.to_string(),
        fill: style.fill,
        dashed: style.dashed,
        cloudy: style.cloudy,
        turn,
        name: String::new(),
    };
    if let Some(imp) = imported {
        a.name = imp.nm.clone();
        a.created = imp.created.clone();
        a.modified = imp.modified.clone();
        a.author = imp.author.clone();
    }
    kept_to_kind(&mut a);
    a
}

/// `text` with the identity tokens filled in as dynamic stamps fill them:
/// the date and time always, the user's name and initials unless a plugin
/// is the actor, which never learns who the user is unless the user
/// granted it `identity`.
fn filled(lw: &Lower, text: &str) -> String {
    if !text.contains("<<") {
        return text.to_string();
    }
    let host = &lw.cx.host;
    match &lw.cx.actor {
        Actor::Plugin { .. } if !lw.cx.identity => {
            (text.replace("<<date>>", &host.date)).replace("<<time>>", &host.time)
        }
        _ => crate::stamp_lib::fill(text, &host.person, &host.initials, &host.date, &host.time),
    }
}

/// The tokens in the text a stamp kind draws, filled in.
fn fill_kind(lw: &Lower, kind: &mut AnnotKind) {
    match kind {
        AnnotKind::TextStamp { text, .. }
        | AnnotKind::PageStamp {
            text: Some(text), ..
        } => *text = filled(lw, text),
        _ => {}
    }
}

/// `a`'s style kept to what it can be (§6.8): opacity 0–1, a stroke no
/// thinner than none, a fill only on a kind with an inside, and a cloudy
/// border only on one that can have one.
fn kept_to_kind(a: &mut Annot) {
    a.opacity = match a.opacity.is_nan() {
        true => 1.0,
        false => a.opacity.clamp(0.0, 1.0),
    };
    a.stroke = match a.stroke.is_finite() {
        true => a.stroke.max(0.0),
        false => 0.0,
    };
    if !a.kind.fillable() {
        a.fill = None;
    }
    if !a.kind.cloudable() {
        a.cloudy = false;
    }
}

/// Puts `mark` as its annotation, on page `page`, where it is.
fn put_mark(rec: &mut Rec, page: PageId, mark: Mark) {
    let id = mark.annot.id;
    let body = AnnotBody::Mark(Arc::new(mark));
    rec.put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
}

/// Adds `mark` after page `page`'s others.
pub(crate) fn add_annot(rec: &mut Rec, page: PageId, mark: Mark) {
    let id = mark.annot.id;
    put_mark(rec, page, mark);
    let mut ids = rec.annot_ids_on(page).cloned().unwrap_or_default();
    ids.push_back(id);
    put_page_annots(rec, page, ids);
}

/// Takes page `page`'s annotations that `gone` says go off it. A widget
/// that goes leaves its field's list; a field with none left goes by the
/// `widget.field` cascade.
pub(crate) fn drop_annots(rec: &mut Rec, page: PageId, gone: impl Fn(AnnotId, &AnnotRec) -> bool) {
    let dropped: HashMap<AnnotId, Option<FieldId>> = (rec.annots_on(page))
        .filter(|&(id, a)| gone(id, a))
        .map(|(id, a)| (id, a.widget().map(|w| w.field)))
        .collect();
    if dropped.is_empty() {
        return;
    }
    for &id in dropped.keys() {
        rec.put(Key::Annot(id), None);
    }
    let mut ids: Vector<AnnotId> = rec.annot_ids_on(page).cloned().unwrap_or_default();
    ids.retain(|id| !dropped.contains_key(id));
    put_page_annots(rec, page, ids);
    let fields: std::collections::BTreeSet<FieldId> = dropped.values().flatten().copied().collect();
    for field in fields {
        let Some(was) = rec.field(field) else {
            continue;
        };
        let mut node = FieldNode::clone(was);
        if let Some(term) = &mut node.term {
            Arc::make_mut(term)
                .widgets
                .retain(|w| !dropped.contains_key(w));
        }
        rec.put(Key::Field(field), Some(Val::Field(Arc::new(node))));
    }
}
