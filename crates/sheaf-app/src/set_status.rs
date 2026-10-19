//! Set Status, as in PDFlyer (Document > Set Status, or the bookmark's
//! right-click menu): on the selected bookmark, color and style it, draw a
//! border around each of its pages (`DocState::bookmark_pages`), and
//! optionally stamp each page's top left with a dynamic status stamp
//! ("REVIEWED GE 10/14/2026"). Reset Status takes the color, style and
//! borders off, and the stamps when asked (PDFlyer leaves them). Each is
//! one undo step, and the last choices are kept for Set Status Again.

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton,
    Subscription, Window, div, prelude::*, px,
};
use serde::{Deserialize, Serialize};
use sheaf_doc::tx::Reader;
use sheaf_doc::{Annot, AnnotKind, Box2, OutlineId, TextStyle};
use sheaf_edit::{AnnotRef, BookmarkPatch, BookmarkRef, Op, Patch};
use sheaf_ui::button::{Button, Chip};
use sheaf_ui::text_input::{TextInput, TextInputEvent};
use sheaf_ui::{dialog, theme};

use crate::docview::{DocView, State};
use crate::{DialogCancel, DialogRun};

pub const KEY_CONTEXT: &str = "SetStatusDialog";

/// A status: what Set Status does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusSpec {
    pub name: String,
    pub color: [u8; 3],
    pub bold: bool,
    pub italic: bool,
    pub border: bool,
    /// The border's line, in points.
    pub width: f32,
    /// A stamp on each page: the name, and the user's initials and date.
    pub stamp: bool,
}

impl Default for StatusSpec {
    fn default() -> Self {
        Self {
            name: "Reviewed".into(),
            color: [30, 140, 60],
            bold: true,
            italic: false,
            border: true,
            width: 2.0,
            stamp: true,
        }
    }
}

impl StatusSpec {
    /// The stamp's template: the name, then initials and date.
    pub fn stamp_text(&self) -> String {
        format!("{}\n<<initials>> <<date>>", self.name.trim().to_uppercase())
    }
}

/// Colors on offer, as the bookmark tools have them.
const PALETTE: [([u8; 3], &str); 6] = [
    ([30, 140, 60], "Green"),
    ([200, 30, 30], "Red"),
    ([20, 80, 200], "Blue"),
    ([220, 120, 0], "Orange"),
    ([130, 50, 170], "Purple"),
    ([90, 90, 90], "Gray"),
];
const WIDTHS: [f32; 3] = [1.0, 2.0, 4.0];
/// Pages' borders sit this far in from the edge, and stamps this far from
/// the top left.
const INSET: f32 = 6.0;
const STAMP_AT: f32 = 18.0;

pub enum SetStatusEvent {
    Set(StatusSpec),
    Reset { stamps: bool },
    Dismiss,
}

pub struct SetStatusDialog {
    focus: FocusHandle,
    name: Entity<TextInput>,
    spec: StatusSpec,
    /// Reset takes the stamps off too.
    remove_stamps: bool,
    bookmark: String,
    pages: usize,
    _subs: Vec<Subscription>,
}

impl EventEmitter<SetStatusEvent> for SetStatusDialog {}

impl Focusable for SetStatusDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SetStatusDialog {
    pub fn new(
        spec: StatusSpec,
        bookmark: String,
        pages: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            let mut t = TextInput::new("e.g. Reviewed", cx);
            t.set_text(spec.name.clone(), cx);
            t
        });
        let subs = vec![
            cx.subscribe(&name, |d: &mut Self, _, ev: &TextInputEvent, cx| match ev {
                TextInputEvent::Confirm => d.go(cx),
                TextInputEvent::Cancel => cx.emit(SetStatusEvent::Dismiss),
            }),
            cx.observe(&name, |_, _, cx| cx.notify()),
        ];
        window.focus(&name.read(cx).focus_handle(cx), cx);
        Self {
            focus: cx.focus_handle(),
            name,
            spec,
            remove_stamps: false,
            bookmark,
            pages,
            _subs: subs,
        }
    }

    fn spec(&self, cx: &App) -> StatusSpec {
        StatusSpec {
            name: self.name.read(cx).text().trim().to_string(),
            ..self.spec.clone()
        }
    }

    fn go(&mut self, cx: &mut Context<Self>) {
        let spec = self.spec(cx);
        if spec.name.is_empty() {
            return;
        }
        cx.emit(SetStatusEvent::Set(spec));
    }
}

impl Render for SetStatusDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = |label: &str, el: AnyElement| {
            dialog::field_row(label.to_string(), px(120.0), false, el).into_any_element()
        };
        let toggle =
            |id: &'static str, label: &'static str, on: bool| Chip::new(id, label).selected(on);
        let colors = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(PALETTE.iter().enumerate().map(|(i, (c, label))| {
                let c = *c;
                Chip::new(("ss-color", i), *label)
                    .selected(self.spec.color == c)
                    .on_click(cx.listener(move |d, _, _, cx| {
                        d.spec.color = c;
                        cx.notify();
                    }))
            }));
        let style = div()
            .flex()
            .gap_1()
            .child(
                toggle("ss-bold", "Bold", self.spec.bold).on_click(cx.listener(|d, _, _, cx| {
                    d.spec.bold = !d.spec.bold;
                    cx.notify();
                })),
            )
            .child(
                toggle("ss-italic", "Italic", self.spec.italic).on_click(cx.listener(
                    |d, _, _, cx| {
                        d.spec.italic = !d.spec.italic;
                        cx.notify();
                    },
                )),
            );
        let border = div()
            .flex()
            .gap_1()
            .child(
                toggle("ss-border", "Border", self.spec.border).on_click(cx.listener(
                    |d, _, _, cx| {
                        d.spec.border = !d.spec.border;
                        cx.notify();
                    },
                )),
            )
            .children(WIDTHS.iter().enumerate().map(|(i, &w)| {
                Chip::new(("ss-width", i), format!("{w} pt"))
                    .selected(self.spec.border && (self.spec.width - w).abs() < 0.01)
                    .on_click(cx.listener(move |d, _, _, cx| {
                        d.spec.border = true;
                        d.spec.width = w;
                        cx.notify();
                    }))
            }));
        let spec = self.spec(cx);
        let stamp = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                toggle("ss-stamp", "Stamp", self.spec.stamp).on_click(cx.listener(
                    |d, _, _, cx| {
                        d.spec.stamp = !d.spec.stamp;
                        cx.notify();
                    },
                )),
            )
            .child(dialog::subtitle(format!(
                "“{}” at each page's top left",
                spec.stamp_text().replace('\n', " ")
            )));
        let pages = format!(
            "{} and its {} page{}",
            self.bookmark,
            self.pages,
            if self.pages == 1 { "" } else { "s" }
        );
        dialog::backdrop("ss-backdrop")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|d, _: &DialogRun, _, cx| d.go(cx)))
            .on_action(cx.listener(|_, _: &DialogCancel, _, cx| cx.emit(SetStatusEvent::Dismiss)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(SetStatusEvent::Dismiss)),
            )
            .child(
                dialog::card("ss-dialog", px(620.0))
                    .child(dialog::header("Set Status").child(dialog::subtitle(pages)))
                    .child(row(
                        "Status",
                        div()
                            .w(px(220.0))
                            .child(self.name.clone())
                            .into_any_element(),
                    ))
                    .child(row("Color", colors.into_any_element()))
                    .child(row("Bookmark", style.into_any_element()))
                    .child(row("Pages", border.into_any_element()))
                    .child(row("", stamp.into_any_element()))
                    .child(
                        dialog::footer()
                            .child(
                                Button::new("ss-reset", "Reset Status").on_click(cx.listener(
                                    |d, _, _, cx| {
                                        cx.emit(SetStatusEvent::Reset {
                                            stamps: d.remove_stamps,
                                        })
                                    },
                                )),
                            )
                            .child(
                                toggle("ss-reset-stamps", "and its stamps", self.remove_stamps)
                                    .on_click(cx.listener(|d, _, _, cx| {
                                        d.remove_stamps = !d.remove_stamps;
                                        cx.notify();
                                    })),
                            )
                            .child(div().flex_1())
                            .child(Button::new("ss-cancel", "Cancel").on_click(
                                cx.listener(|_, _, _, cx| cx.emit(SetStatusEvent::Dismiss)),
                            ))
                            .child(
                                Button::new("ss-go", "Set Status")
                                    .primary()
                                    .on_click(cx.listener(|d, _, _, cx| d.go(cx))),
                            ),
                    ),
            )
            .text_color(theme::TEXT_SOFT)
    }
}

impl DocView {
    /// The bookmark Set Status works on: the selected one, with its title
    /// and how many pages it has.
    pub(crate) fn status_bookmark(&self) -> Option<(OutlineId, String, usize)> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        let id = doc.outline_selected?;
        let st = doc.model.state();
        let title = st.bookmark(id)?.title.to_string();
        let pages = st.bookmark_pages(id).map_or(0, |p| p.len());
        Some((id, title, pages))
    }

    /// Set Status on the selected bookmark, as one undo step; the choices
    /// are kept for Set Status Again.
    pub(crate) fn set_status(&mut self, spec: StatusSpec, cx: &mut Context<Self>) {
        let Some((id, title, _)) = self.status_bookmark() else {
            return self.set_notice("Select a bookmark first", cx);
        };
        // The stamp is laid out as the host fills its tokens in as it's
        // made: the user's initials, and now.
        let prefs = crate::prefs::get(cx);
        let now = chrono::Local::now();
        let shown = sheaf_doc::stamp_lib::fill(
            &spec.stamp_text(),
            &prefs.identity.name(),
            &prefs.identity.initials(),
            &now.format(prefs.stamps.date_format()).to_string(),
            &now.format(prefs.stamps.time_format()).to_string(),
        );
        let stamp = sheaf_doc::annot::text_stamp_size(&shown, prefs.stamps.size());
        let Some(mut tx) = self.user_tx() else {
            return;
        };
        let ops = status_ops(&mut tx.reader(), id, &spec, stamp, &|kind| {
            let mut a = self.new_annot(kind, spec.color);
            a.stroke = spec.width;
            a
        });
        let label = format!("Set status {} on “{title}”", spec.name);
        self.submit(tx, ops, &label, cx);
        crate::state::update(cx, |s| s.set_status = Some(spec));
        self.wanted_panel = Some("bookmarks");
    }

    /// Set Status Again: the last status, on the selected bookmark. False
    /// when there's none yet (the dialog asks).
    pub(crate) fn set_status_again(&mut self, cx: &mut Context<Self>) -> bool {
        match crate::state::get(cx).set_status.clone() {
            Some(spec) => {
                self.set_status(spec, cx);
                true
            }
            None => false,
        }
    }

    /// Reset Status on the selected bookmark: no color or style, and no
    /// borders on its pages; its status stamps too when `stamps`.
    pub(crate) fn reset_status(&mut self, stamps: bool, cx: &mut Context<Self>) {
        let Some((id, title, _)) = self.status_bookmark() else {
            return self.set_notice("Select a bookmark first", cx);
        };
        let Some(mut tx) = self.user_tx() else {
            return;
        };
        let ops = reset_ops(&mut tx.reader(), id, stamps);
        self.submit(tx, ops, &format!("Reset status on “{title}”"), cx);
    }
}

/// What Set Status does to bookmark `id` and its pages, as `r` reads
/// them: the bookmark's color and style, the old borders' deletions, then
/// each page's border and stamp (`stamp` big, its template's tokens the
/// host's to fill in), laid out on the page as it shows and drawn as
/// `sign` says.
fn status_ops(
    r: &mut Reader,
    id: OutlineId,
    spec: &StatusSpec,
    stamp: (f32, f32),
    sign: &dyn Fn(AnnotKind) -> Annot,
) -> Vec<Op> {
    let mut ops = vec![Op::PatchBookmarks {
        bookmarks: vec![BookmarkRef::Id(id)],
        patch: BookmarkPatch {
            color: Patch::Set(spec.color),
            style: Some(TextStyle {
                bold: spec.bold,
                italic: spec.italic,
            }),
            ..BookmarkPatch::default()
        },
    }];
    let mut borders = Vec::new();
    let mut marks = Vec::new();
    for page in r.bookmark_pages(id).unwrap_or_default() {
        let Some(edit) = r.geom(page).map(|g| **g) else {
            continue;
        };
        let size = edit.shown();
        // A page's border is replaced, not doubled.
        borders.extend(
            (r.marks_on(page))
                .filter(|a| matches!(a.kind, AnnotKind::PageBorder { .. }))
                .map(|a| AnnotRef::Id(a.id)),
        );
        if spec.border {
            let kind = AnnotKind::PageBorder {
                rect: Box2 {
                    x: INSET,
                    y: INSET,
                    w: size.width - 2.0 * INSET,
                    h: size.height - 2.0 * INSET,
                },
                status: spec.name.clone(),
            };
            marks.push((page, sign(kind).to_page(&edit)));
        }
        if spec.stamp {
            let (w, h) = stamp;
            let kind = AnnotKind::TextStamp {
                rect: Box2 {
                    x: STAMP_AT,
                    y: STAMP_AT,
                    w,
                    h,
                },
                text: spec.stamp_text(),
                status: true,
            };
            marks.push((page, sign(kind).to_page(&edit)));
        }
    }
    if !borders.is_empty() {
        ops.push(Op::DeleteAnnotations { annots: borders });
    }
    ops.extend(marks.into_iter().map(|(page, a)| Op::AddMark {
        page: page.into(),
        mark: crate::docview::new_mark(&a, None),
        reply_to: None,
        local: None,
        imported: None,
    }));
    ops
}

/// What Reset Status does to bookmark `id`, as `r` reads it: no color or
/// style, no borders on its pages, and no status stamps when `stamps`.
fn reset_ops(r: &mut Reader, id: OutlineId, stamps: bool) -> Vec<Op> {
    let mut ops = vec![Op::PatchBookmarks {
        bookmarks: vec![BookmarkRef::Id(id)],
        patch: BookmarkPatch {
            color: Patch::Clear,
            style: Some(TextStyle::default()),
            ..BookmarkPatch::default()
        },
    }];
    let mut gone = Vec::new();
    for page in r.bookmark_pages(id).unwrap_or_default() {
        gone.extend(
            (r.marks_on(page))
                .filter(|a| match &a.kind {
                    AnnotKind::PageBorder { .. } => true,
                    AnnotKind::TextStamp { status, .. } => *status && stamps,
                    _ => false,
                })
                .map(|a| AnnotRef::Id(a.id)),
        );
    }
    if !gone.is_empty() {
        ops.push(Op::DeleteAnnotations { annots: gone });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use sheaf_doc::tx::fixture;
    use sheaf_doc::{Actor, AnnotId, CommitOpts, Document, FixedCount, FlatOutlineItem, Store};

    /// The ops `make` reads off `d`, committed as the user's.
    fn commit(d: &mut Document, make: impl FnOnce(&mut Reader) -> Vec<Op>) {
        let mut tx = d.tx(Actor::User);
        let ops = make(&mut tx.reader());
        tx.apply_batch(ops, &sheaf_ops::NoProgress).unwrap();
        d.commit(tx, CommitOpts::default()).unwrap();
    }

    fn signed(kind: AnnotKind) -> Annot {
        Annot {
            id: AnnotId(0),
            kind,
            color: [30, 140, 60],
            opacity: 1.0,
            stroke: 2.0,
            author: "Pat".into(),
            contents: String::new(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: sheaf_edit::Rotation::R0,
            name: String::new(),
        }
    }

    /// Five blank pages; "Cash" owns the first three.
    fn binder() -> (Document, OutlineId) {
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, 5);
        let d = Document::new(
            store,
            primary,
            "binder.pdf".into(),
            None,
            &[
                FlatOutlineItem {
                    title: "Cash".into(),
                    page: Some(0),
                    depth: 0,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
                FlatOutlineItem {
                    title: "Receivables".into(),
                    page: Some(3),
                    depth: 0,
                    view: None,
                    color: None,
                    style: Default::default(),
                    open: true,
                    object: None,
                    kept: false,
                },
            ],
            Box::new(FixedCount(10)),
        );
        let cash = d.state().outline_roots()[0];
        (d, cash)
    }

    #[test]
    fn set_status_borders_and_stamps_a_bookmarks_pages_then_resets() {
        let (mut d, cash) = binder();
        let spec = StatusSpec::default();
        let set = |d: &mut Document| {
            commit(d, |r| status_ops(r, cash, &spec, (90.0, 40.0), &signed));
        };
        set(&mut d);
        // Cash owns pages 1-3: each has a border and a stamp; 4 and 5 none.
        let count = |d: &Document, i: usize| d.state().marks_on(d.page(i).unwrap()).count();
        assert_eq!(
            (0..5).map(|i| count(&d, i)).collect::<Vec<_>>(),
            [2, 2, 2, 0, 0]
        );
        let node = d.state().bookmark(cash).unwrap();
        assert_eq!(node.color, Some(spec.color));
        assert!(node.style.bold);
        // Again: the borders are replaced, the stamps added.
        set(&mut d);
        assert_eq!(count(&d, 0), 3);
        // Reset keeps the stamps unless asked.
        commit(&mut d, |r| reset_ops(r, cash, false));
        assert_eq!(count(&d, 0), 2);
        assert_eq!(d.state().bookmark(cash).unwrap().color, None);
        commit(&mut d, |r| reset_ops(r, cash, true));
        assert_eq!(count(&d, 0), 0);
    }

    /// On a turned page the border goes round the page and the stamp sits
    /// at its top left as it shows, both reading as it does.
    #[test]
    fn a_status_is_laid_out_on_the_page_as_it_shows() {
        let (mut d, cash) = binder();
        let first = d.page(0).unwrap();
        let turn = fixture::rotate(&[first], sheaf_edit::Turn::Quarter);
        d.commit_ops(vec![turn], "turn").unwrap();
        let spec = StatusSpec::default();
        commit(&mut d, |r| {
            status_ops(r, cash, &spec, (90.0, 40.0), &signed)
        });
        let geom = d.geom(d.page(0).unwrap());
        let size = geom.shown();
        let marks: Vec<Annot> = d.state().marks_on(first).cloned().collect();
        let [border, stamp] = marks.as_slice() else {
            panic!()
        };
        let shown = |a: &Annot| geom.to_view().apply_box(a.page_bounds());
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let b = shown(border);
        assert!(
            near(b.x, INSET)
                && near(b.y, INSET)
                && near(b.w, size.width - 2.0 * INSET)
                && near(b.h, size.height - 2.0 * INSET),
            "{b:?}"
        );
        let s = shown(stamp);
        assert!(near(s.x, STAMP_AT) && near(s.y, STAMP_AT), "{s:?}");
        assert!(s.w > s.h, "{s:?}");
        assert_eq!((border.turn, stamp.turn), (geom.rotate, geom.rotate));
        assert_eq!(geom.rotate, sheaf_edit::Rotation::R90);
    }
}
