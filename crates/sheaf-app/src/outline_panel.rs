//! The Bookmarks panel: an editable outline tree. Interaction follows
//! Acrobat: click selects and navigates, drag reorders or nests, F2 renames,
//! Ctrl+B adds a bookmark for the current page, Delete removes the subtree,
//! right-click opens a small menu. The PDFlyer extra — moving a bookmark's
//! pages along with it — is on Shift while dropping.

use std::collections::HashSet;

use gpui::{
    AnchoredPositionMode, AnyElement, Context, DragMoveEvent, Entity, Focusable, MouseButton,
    Pixels, Point, Subscription, Window, anchored, deferred, div, point, prelude::*, px, rgb,
};
use sheaf_doc::{Commit, DestView, DocState, OutlineId, PageId};
use sheaf_edit::{BookmarkPatch, BookmarkRef, Op, Patch, Sibling};
use sheaf_ui::list::ListLayout;
use sheaf_ui::text_input::{TextInput, TextInputEvent};
use sheaf_ui::theme;

use crate::docview::{DocView, ScrollTo, State};
use crate::{
    CollapseAllBookmarks, DeleteBookmark, DeselectBookmark, ExpandAllBookmarks, NewBookmark, Redo,
    RenameBookmark, Save, SetBookmarkDestination, Undo,
};

pub const KEY_CONTEXT: &str = "Outline";

const ROW_H: f32 = 24.0;
const INDENT: f32 = 14.0;
const PAD: f32 = 10.0;

/// Where a dragged bookmark would land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineDrop {
    pub row: usize,
    pub pos: DropPos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPos {
    Before,
    After,
    Into,
}

/// An in-progress rename.
pub struct Rename {
    pub id: OutlineId,
    pub input: Entity<TextInput>,
    pub _sub: Subscription,
}

/// Payload of a bookmark drag.
#[derive(Clone)]
struct DragBookmark {
    id: OutlineId,
}

struct DragPreview {
    title: String,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme::ACCENT)
            .text_color(theme::TEXT_BRIGHT)
            .text_sm()
            .child(self.title.clone())
    }
}

impl DocView {
    /// The current view as a bookmark destination, as Acrobat records it:
    /// the page in view and how far down it the top of the window is, in
    /// PageSpace. None for the position when the page's top is in view.
    pub(crate) fn current_view(&self) -> Option<(PageId, Option<DestView>)> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        let page = doc.page_id(doc.current_page())?;
        let pane = doc.pane();
        let Some(r) = pane
            .layout
            .as_ref()
            .and_then(|l| l.rect(doc.current_page()))
        else {
            return Some((page, None));
        };
        let scroll_y = -f32::from(pane.scroll.offset().y);
        let fy = (scroll_y - r.top) / r.h;
        if fy <= 0.0 {
            return Some((page, None));
        }
        // The page's left edge at the window's top, as the pane shows it.
        let geom = doc.pane_geom(doc.current_page())?;
        let (left, top) = geom
            .from_view()
            .apply_axes(Some(0.0), Some(fy.min(1.0) * geom.shown().height));
        Some((
            page,
            Some(DestView::Xyz {
                left,
                top,
                zoom: None,
            }),
        ))
    }

    /// Show a bookmark's page, at its position when it has one.
    pub(crate) fn go_to_bookmark(
        &mut self,
        index: usize,
        view: Option<&DestView>,
        cx: &mut Context<Self>,
    ) {
        let point = match (view, &self.state) {
            (Some(DestView::Xyz { left, top, .. }), State::Ready(doc)) => {
                doc.pane_geom(index).map(|geom| {
                    // Positions are in PageSpace; the pane shows the page
                    // turned, and a missing coordinate is the page's edge.
                    let (x, y) = geom.to_view().apply_axes(*left, *top);
                    let shown = geom.shown();
                    (
                        x.unwrap_or(0.0) / shown.width.max(1.0),
                        y.unwrap_or(0.0) / shown.height.max(1.0),
                    )
                })
            }
            _ => None,
        };
        match point {
            Some((fx, fy)) => self.scroll(
                ScrollTo::Point {
                    page: index,
                    fx: fx.clamp(0.0, 1.0),
                    fy: fy.clamp(0.0, 1.0),
                },
                cx,
            ),
            None => self.go_to_page(index, cx),
        }
    }

    pub(crate) fn new_bookmark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let here = self.current_view();
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let op = new_bookmark_op(doc.model.state(), doc.outline_selected, here);
        let made = self.apply(op, "New bookmark", cx);
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        // Its parent opened as it was made (`DocView::commit`).
        if let Some(id) = made_bookmark(made.as_ref()) {
            doc.outline_selected = Some(id);
            self.start_rename(id, window, cx);
        }
    }

    fn start_rename(&mut self, id: OutlineId, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(node) = doc.model.state().bookmark(id) else {
            return;
        };
        let title = node.title.to_string();
        let input = cx.new(|cx| {
            let mut i = TextInput::new("Bookmark title", cx);
            i.set_text(title, cx);
            i
        });
        let sub = cx.subscribe(&input, move |this, input, ev: &TextInputEvent, cx| {
            let text = input.read(cx).text().trim().to_string();
            match ev {
                TextInputEvent::Confirm if !text.is_empty() => {
                    let patch = BookmarkPatch {
                        title: Some(text),
                        ..BookmarkPatch::default()
                    };
                    this.apply(patched(id, patch), "Rename bookmark", cx);
                }
                _ => {}
            }
            if let State::Ready(doc) = &mut this.state {
                doc.outline_rename = None;
            }
            this.pending_outline_focus = true;
            cx.notify();
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        doc.outline_rename = Some(Rename {
            id,
            input,
            _sub: sub,
        });
        cx.notify();
    }

    fn rename_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        if let Some(id) = doc.outline_selected {
            self.start_rename(id, window, cx);
        }
    }

    fn delete_selected_bookmark(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(id) = doc.outline_selected.take() else {
            return;
        };
        doc.outline_menu = None;
        let bookmarks = vec![BookmarkRef::Id(id)];
        self.apply(Op::DeleteBookmarks { bookmarks }, "Delete bookmark", cx);
    }

    fn set_selected_destination(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(id) = doc.outline_selected else {
            return;
        };
        doc.outline_menu = None;
        let Some((page, view)) = self.current_view() else {
            return;
        };
        let patch = BookmarkPatch {
            target: Patch::Set(page.into()),
            view: view.map_or(Patch::Clear, Patch::Set),
            ..BookmarkPatch::default()
        };
        self.apply(patched(id, patch), "Set destination", cx);
    }

    fn set_selected_color(&mut self, color: Option<[u8; 3]>, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(id) = doc.outline_selected else {
            return;
        };
        doc.outline_menu = None;
        let patch = BookmarkPatch {
            color: color.map_or(Patch::Clear, Patch::Set),
            ..BookmarkPatch::default()
        };
        self.apply(patched(id, patch), "Bookmark color", cx);
    }

    fn set_all_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        doc.outline_menu = None;
        doc.outline_expanded = if expanded {
            doc.model
                .state()
                .bookmarks_preorder()
                .iter()
                .map(|&(_, id, _)| id)
                .collect()
        } else {
            HashSet::new()
        };
        cx.notify();
    }

    fn drop_bookmark(&mut self, id: OutlineId, with_pages: bool, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let Some(drop) = doc.outline_drop.take() else {
            return;
        };
        let Some(&(row_id, _, _)) = doc.outline_rows.get(drop.row) else {
            return;
        };
        if row_id == id {
            return;
        }
        let st = doc.model.state();
        let (parent, after) = match drop.pos {
            DropPos::Into => (Some(row_id), None),
            DropPos::Before => match st.bookmark_place(row_id) {
                Some((parent, prev)) => (
                    parent,
                    if prev == Some(id) {
                        st.bookmark_place(id).and_then(|(_, p)| p)
                    } else {
                        prev
                    },
                ),
                None => return,
            },
            DropPos::After => match st.bookmark_place(row_id) {
                Some((parent, _)) => (parent, Some(row_id)),
                None => return,
            },
        };
        if let Some(p) = parent {
            doc.outline_expanded.insert(p);
        }
        let label = if with_pages {
            "Move bookmark with pages"
        } else {
            "Move bookmark"
        };
        let op = Op::MoveBookmarks {
            bookmarks: vec![BookmarkRef::Id(id)],
            parent: parent.map(BookmarkRef::Id),
            at: after.map_or(Sibling::First, |a| Sibling::After(a.into())),
            with_pages,
        };
        self.apply(op, label, cx);
        if let State::Ready(doc) = &mut self.state {
            doc.outline_selected = Some(id);
        }
    }

    /// Bookmark tree: virtualized rows over the expanded subset.
    pub fn render_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if std::mem::take(&mut self.pending_outline_focus)
            && let State::Ready(doc) = &self.state
        {
            window.focus(&doc.outline_focus, cx);
        }
        let State::Ready(doc) = &mut self.state else {
            return div().p_2().text_sm().child("…").into_any_element();
        };
        if !cx.has_active_drag() {
            doc.outline_drop = None;
        }
        let focus = doc.outline_focus.clone();
        let container = div()
            .id("outline")
            .key_context(KEY_CONTEXT)
            .track_focus(&focus)
            .size_full()
            .on_action(
                cx.listener(|v, _: &RenameBookmark, window, cx| v.rename_selected(window, cx)),
            )
            .on_action(cx.listener(|v, _: &DeleteBookmark, _, cx| v.delete_selected_bookmark(cx)))
            .on_action(
                cx.listener(|v, _: &SetBookmarkDestination, _, cx| v.set_selected_destination(cx)),
            )
            .on_action(cx.listener(|v, _: &ExpandAllBookmarks, _, cx| v.set_all_expanded(true, cx)))
            .on_action(
                cx.listener(|v, _: &CollapseAllBookmarks, _, cx| v.set_all_expanded(false, cx)),
            )
            .on_action(cx.listener(|v, _: &NewBookmark, window, cx| v.new_bookmark(window, cx)))
            .on_action(cx.listener(|v, _: &Undo, _, cx| v.undo(cx)))
            .on_action(cx.listener(|v, _: &Redo, _, cx| v.redo(cx)))
            .on_action(cx.listener(|v, _: &Save, _, cx| v.save(cx).detach()))
            .on_action(cx.listener(|v, _: &DeselectBookmark, _, cx| {
                if let State::Ready(doc) = &mut v.state {
                    doc.outline_selected = None;
                    doc.outline_menu = None;
                    cx.notify();
                }
            }));

        if !doc.model.is_ready() {
            return container
                .p_2()
                .text_sm()
                .text_color(theme::TEXT_MUTED)
                .child("Loading bookmarks…")
                .into_any_element();
        }
        let flat = doc.model.state().bookmarks_preorder();
        if flat.is_empty() {
            return container
                .p_2()
                .text_sm()
                .text_color(theme::TEXT_MUTED)
                .child("No bookmarks — Ctrl+B adds one for the current page")
                .into_any_element();
        }

        // Flatten to the visible rows: skip descendants of collapsed nodes.
        let mut rows: Vec<(usize, OutlineId, u16, Option<usize>)> = Vec::new();
        let mut hide_below: Option<u16> = None;
        for (ix, &(depth, id, node)) in flat.iter().enumerate() {
            if let Some(d) = hide_below {
                if depth > d {
                    continue;
                }
                hide_below = None;
            }
            rows.push((
                ix,
                id,
                depth,
                node.page().and_then(|t| doc.model.index_of(t)),
            ));
            if !node.kids.is_empty() && !doc.outline_expanded.contains(&id) {
                hide_below = Some(depth);
            }
        }
        doc.outline_rows = rows.iter().map(|(_, id, d, t)| (*id, *d, *t)).collect();
        let row_depths: std::sync::Arc<Vec<u16>> =
            std::sync::Arc::new(rows.iter().map(|r| r.2).collect());

        let layout = ListLayout::fixed(rows.len(), ROW_H).padded(0.0, PAD);

        let cur_id = doc.page_id(doc.current_page());
        let selected = doc.outline_selected;
        let drop = doc.outline_drop;
        let rename = doc.outline_rename.as_ref().map(|r| (r.id, r.input.clone()));
        let mut els = Vec::new();
        for r in layout.visible(&doc.outline_scroll) {
            let (ix, id, depth, target) = rows[r];
            let node = flat[ix].2;
            let view = node.view().cloned();
            let orphan = node.page().is_some() && target.is_none();
            let expanded = doc.outline_expanded.contains(&id);
            let active = node.page().is_some() && node.page() == cur_id;
            let is_selected = selected == Some(id);
            let into = drop.is_some_and(|d| d.row == r && d.pos == DropPos::Into);
            let chevron = if node.kids.is_empty() {
                " "
            } else if expanded {
                "▾"
            } else {
                "▸"
            };
            let title_color = match node.color {
                Some([cr, cg, cb]) => rgb(((cr as u32) << 16) | ((cg as u32) << 8) | cb as u32),
                None if target.is_some() => theme::TEXT_SOFT,
                None if orphan => theme::TEXT_ORPHAN,
                None => theme::TEXT_DIM,
            };
            let title_el: AnyElement = match &rename {
                Some((rid, input)) if *rid == id => {
                    div().flex_1().child(input.clone()).into_any_element()
                }
                _ => div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(title_color)
                    .when(node.style.bold, |d| d.font_weight(gpui::FontWeight::BOLD))
                    .when(node.style.italic, |d| d.italic())
                    .child(node.title.to_string())
                    .into_any_element(),
            };
            let title_for_drag = node.title.to_string();
            els.push(
                layout
                    .place(r)
                    .id(("outline", id.0 as usize))
                    .pl(px(6.0 + depth as f32 * INDENT))
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_sm()
                    .cursor_pointer()
                    .when(is_selected, |d| d.bg(theme::SELECTED_STRONG))
                    .when(!is_selected && active, |d| d.bg(theme::ACTIVE_ROW))
                    .when(into, |d| d.bg(theme::SELECTION))
                    .hover(|s| s.bg(theme::HOVER))
                    .on_click(cx.listener(move |v, _, window, cx| {
                        if let State::Ready(doc) = &mut v.state {
                            doc.outline_selected = Some(id);
                            doc.outline_menu = None;
                            window.focus(&doc.outline_focus, cx);
                        }
                        if let Some(p) = target {
                            v.go_to_bookmark(p, view.as_ref(), cx);
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |v, ev: &gpui::MouseDownEvent, window, cx| {
                            if let State::Ready(doc) = &mut v.state {
                                doc.outline_selected = Some(id);
                                doc.outline_menu = Some((id, ev.position));
                                window.focus(&doc.outline_focus, cx);
                                cx.notify();
                            }
                        }),
                    )
                    .on_drag(DragBookmark { id }, move |_, _, _, cx| {
                        let title = title_for_drag.clone();
                        cx.new(|_| DragPreview { title })
                    })
                    .child(
                        div()
                            .id(("outline-toggle", id.0 as usize))
                            .w(px(14.0))
                            .text_color(theme::TEXT_MUTED)
                            .cursor_pointer()
                            .on_click(cx.listener(move |v, _, _, cx| {
                                if let State::Ready(doc) = &mut v.state {
                                    if !doc.outline_expanded.remove(&id) {
                                        doc.outline_expanded.insert(id);
                                    }
                                    cx.notify();
                                }
                                cx.stop_propagation();
                            }))
                            .child(chevron),
                    )
                    .child(title_el),
            );
        }

        // Drop indicator line for Before/After.
        let indicator = drop.and_then(|d| {
            let depth = *row_depths.get(d.row)? as f32;
            let y = match d.pos {
                DropPos::Before => layout.top(d.row),
                DropPos::After => layout.top(d.row) + layout.height(d.row),
                DropPos::Into => return None,
            };
            Some(
                div()
                    .absolute()
                    .left(px(6.0 + depth * INDENT))
                    .right(px(4.0))
                    .top(px(y - 1.0))
                    .h(px(2.0))
                    .bg(theme::ACCENT),
            )
        });

        // Context menu, anchored at the click position in window coordinates.
        let menu = doc.outline_menu.map(|(_, pos)| self_menu(pos, cx));

        let depths = row_depths.clone();
        let drag_layout = layout.clone();
        container
            .track_scroll(&doc.outline_scroll)
            .overflow_y_scroll()
            .on_drag_move(
                cx.listener(move |v, ev: &DragMoveEvent<DragBookmark>, _, cx| {
                    let State::Ready(doc) = &mut v.state else {
                        return;
                    };
                    let local_y = f32::from(ev.event.position.y - ev.bounds.origin.y)
                        - f32::from(doc.outline_scroll.offset().y);
                    let local_x = f32::from(ev.event.position.x - ev.bounds.origin.x);
                    let Some(row) = drag_layout.row_at(local_y) else {
                        return;
                    };
                    let frac = (local_y - drag_layout.top(row)) / drag_layout.height(row);
                    let indent = 6.0 + depths[row] as f32 * INDENT;
                    let pos = if frac < 0.25 {
                        DropPos::Before
                    } else if frac > 0.75 {
                        // Bottom edge, dragged right of the row's own indent: nest.
                        if local_x > indent + 28.0 {
                            DropPos::Into
                        } else {
                            DropPos::After
                        }
                    } else {
                        DropPos::Into
                    };
                    let d = OutlineDrop { row, pos };
                    if doc.outline_drop != Some(d) {
                        doc.outline_drop = Some(d);
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|v, d: &DragBookmark, window, cx| {
                let with_pages = window.modifiers().shift;
                v.drop_bookmark(d.id, with_pages, cx)
            }))
            .child(layout.body().children(els).children(indicator))
            .children(menu)
            .into_any_element()
    }
}

/// The right-click menu for the selected bookmark.
fn self_menu(pos: Point<Pixels>, cx: &mut Context<DocView>) -> AnyElement {
    let item = |id: &'static str,
                label: &'static str,
                cx: &mut Context<DocView>,
                f: fn(&mut DocView, &mut Window, &mut Context<DocView>)| {
        div()
            .id(id)
            .px_3()
            .py_1()
            .text_sm()
            .cursor_pointer()
            .hover(|s| s.bg(theme::HOVER))
            .on_click(cx.listener(move |v, _, window, cx| {
                f(v, window, cx);
                if let State::Ready(doc) = &mut v.state {
                    doc.outline_menu = None;
                }
                cx.notify();
            }))
            .child(label)
    };
    let swatch = |color: Option<[u8; 3]>, cx: &mut Context<DocView>| {
        let fill = color
            .map(|[r, g, b]| rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32))
            .unwrap_or(theme::TEXT_SOFT);
        div()
            .id((
                "swatch",
                color
                    .map(|c| (c[0] as usize) << 16 | (c[1] as usize) << 8 | c[2] as usize)
                    .unwrap_or(0),
            ))
            .w(px(16.0))
            .h(px(16.0))
            .rounded_sm()
            .bg(fill)
            .border_1()
            .border_color(theme::DIVIDER)
            .cursor_pointer()
            .on_click(cx.listener(move |v, _, _, cx| v.set_selected_color(color, cx)))
    };
    let colors: [Option<[u8; 3]>; 7] = [
        None,
        Some([0xe0, 0x40, 0x40]),
        Some([0xe0, 0xa0, 0x00]),
        Some([0x40, 0xb0, 0x40]),
        Some([0x40, 0x90, 0xe0]),
        Some([0xa0, 0x60, 0xe0]),
        Some([0x90, 0x90, 0x90]),
    ];
    deferred(
        anchored()
            .position(pos)
            .position_mode(AnchoredPositionMode::Window)
            .child(
                div()
                    .id("outline-menu")
                    .occlude()
                    .on_mouse_down_out(cx.listener(|v, _, _, cx| {
                        if let State::Ready(doc) = &mut v.state {
                            doc.outline_menu = None;
                            cx.notify();
                        }
                    }))
                    .min_w(px(220.0))
                    .py_1()
                    .rounded_md()
                    .bg(theme::POPOVER)
                    .border_1()
                    .border_color(theme::DIVIDER)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .child(item("menu-rename", "Rename (F2)", cx, |v, w, cx| {
                        v.rename_selected(w, cx)
                    }))
                    .child(item("menu-delete", "Delete", cx, |v, _, cx| {
                        v.delete_selected_bookmark(cx)
                    }))
                    .child(item(
                        "menu-dest",
                        "Set Destination to Current Page",
                        cx,
                        |v, _, cx| v.set_selected_destination(cx),
                    ))
                    .child(item("menu-status", "Set Status…", cx, |_, w, cx| {
                        w.dispatch_action(Box::new(crate::SetStatus), cx)
                    }))
                    .child(item(
                        "menu-status-again",
                        "Set Status Again",
                        cx,
                        |_, w, cx| w.dispatch_action(Box::new(crate::SetStatusAgain), cx),
                    ))
                    .child(item("menu-status-reset", "Reset Status", cx, |v, _, cx| {
                        v.reset_status(false, cx)
                    }))
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().text_color(theme::TEXT_MUTED).child("Color"))
                            .children(colors.iter().map(|c| swatch(*c, cx))),
                    )
                    .child(div().h(px(1.0)).my_1().bg(theme::DIVIDER))
                    .child(item("menu-expand", "Expand All", cx, |v, _, cx| {
                        v.set_all_expanded(true, cx)
                    }))
                    .child(item("menu-collapse", "Collapse All", cx, |v, _, cx| {
                        v.set_all_expanded(false, cx)
                    })),
            ),
    )
    .into_any_element()
}

#[allow(dead_code)]
fn _unused(_: Point<Pixels>) -> Point<Pixels> {
    point(px(0.0), px(0.0))
}

/// Bookmark `id` changed as `patch` says.
fn patched(id: OutlineId, patch: BookmarkPatch) -> Op {
    Op::PatchBookmarks {
        bookmarks: vec![BookmarkRef::Id(id)],
        patch,
    }
}

/// New Bookmark on `st`: an "Untitled" one going to `here` (the page in
/// view, and where on it). As Acrobat places it, it's the `selected`
/// bookmark's next sibling, or else the last at the top.
fn new_bookmark_op(
    st: &DocState,
    selected: Option<OutlineId>,
    here: Option<(PageId, Option<DestView>)>,
) -> Op {
    let (target, view) = here.map_or((None, None), |(page, view)| (Some(page), view));
    let placed = selected.and_then(|sel| Some((sel, st.bookmark_place(sel)?)));
    let (parent, at) = match placed {
        Some((sel, (parent, _))) => (parent, Sibling::After(sel.into())),
        None => (None, Sibling::Last),
    };
    Op::AddBookmark {
        parent: parent.map(BookmarkRef::Id),
        at,
        title: "Untitled".into(),
        target: target.map(Into::into),
        view,
        color: None,
        style: Default::default(),
        local: None,
    }
}

/// The bookmark New Bookmark `made`, which it selects to rename: the one
/// its commit says it made, never a guess at where it went (bug 25). None
/// when it made none.
fn made_bookmark(made: Option<&Commit>) -> Option<OutlineId> {
    made?.created.bookmarks().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sheaf_doc::{Document, FixedCount, FlatOutlineItem, Store};

    /// Two blank pages, and bookmarks A (with A1 and A2 under it) and B.
    fn outlined() -> Document {
        let item = |title: &str, page: u32, depth: u16| FlatOutlineItem {
            title: title.into(),
            page: Some(page),
            depth,
            view: None,
            color: None,
            style: Default::default(),
            open: true,
            object: None,
            kept: false,
        };
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, 2);
        let items = [
            item("A", 0, 0),
            item("A1", 0, 1),
            item("A2", 1, 1),
            item("B", 1, 0),
        ];
        let policy = Box::new(FixedCount(10));
        Document::new(store, primary, "a.pdf".into(), None, &items, policy)
    }

    /// New Bookmark with a bookmark that has children selected selects
    /// the one it made: the selected one's next sibling. One refused
    /// selects none. Before, the selection was a guess (bug 25, which
    /// says pre-order; it was the selected one's next sibling by index,
    /// which picked B, an old bookmark, to rename when the edit failed).
    #[test]
    fn a_new_bookmark_is_the_one_selected() {
        let mut d = outlined();
        let a = d.state().outline_roots()[0];
        let page = d.page(1).unwrap();
        let op = new_bookmark_op(d.state(), Some(a), Some((page, None)));
        let c = d.commit_ops(vec![op], "New bookmark").unwrap();
        let st = d.state();
        let made = made_bookmark(Some(&c)).expect("one was made");
        assert_eq!(st.outline_roots().iter().position(|&r| r == made), Some(1));
        let b = st.bookmark(made).unwrap();
        assert_eq!((&*b.title, b.page()), ("Untitled", Some(page)));
        // Its page is gone: refused, so none is selected.
        let op = new_bookmark_op(d.state(), Some(a), Some((PageId(999), None)));
        let refused = d.commit_ops(vec![op], "New bookmark").ok();
        assert_eq!(made_bookmark(refused.as_ref()), None);
    }
}
