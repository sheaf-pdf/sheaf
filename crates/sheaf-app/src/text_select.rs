//! Selecting text on the page with the Select tool, and what acts on it:
//! Copy, and the text markup tools (select, then press U, as Acrobat's
//! quick actions). It works on a page's characters ([`PageChars`]) as the
//! screen shows the page, from the document's text (`sheaf_ops::text`,
//! M10.5 §10.5), each page read where it's drawn from: an inserted,
//! duplicated or cropped page, a header and recognized text not yet
//! saved, and the pages of a file PDFium refuses select as they show. The
//! calculator's picks and markup dragged over text read a page's
//! characters the same way ([`DocView::with_chars`]).
//!
//! A drag that starts on text selects from there to the pointer, in
//! reading order; a double-click takes a word. A selection is on one page,
//! of the characters it was made in, which it keeps: an edit that changes
//! how the page draws but not its text (a comment on it hidden, a page
//! numbered after it deleted) leaves it painted and copied as it was.
//! Points here are in PageSpace, as the characters are.

use std::sync::Arc;

use gpui::{ClipboardItem, Context};
use sheaf_doc::{AnnotKind, Box2, PageChars, PageId, Pt, render_key};
use sheaf_edit::Rotation;

use crate::docview::{DocView, State};
use crate::markup_tools::Tool;

/// Text selected on one page: two boundaries in the characters it was
/// selected in.
#[derive(Debug, Clone)]
pub struct TextSel {
    pub page: PageId,
    pub chars: Arc<PageChars>,
    pub anchor: usize,
    pub head: usize,
}

impl TextSel {
    /// The selected characters, as boundaries in order.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// A drag selecting text, from where it started.
#[derive(Debug, Clone, Copy)]
pub struct TextDrag {
    pub page: PageId,
    /// The page's index, which pointer positions come with.
    pub index: usize,
    /// In PageSpace.
    pub start: Pt,
    pub current: Pt,
    /// Points from a character that still count as on it.
    pub tolerance: f32,
    /// The button came up before the page's characters were read; the
    /// drag ends when they are.
    pub released: bool,
}

impl DocView {
    /// Page `page`'s characters as the screen shows it now, if they've
    /// been read.
    pub(crate) fn cached_chars(&self, page: PageId) -> Option<Arc<PageChars>> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        doc.text.cached(doc.model.state(), page, &doc.inputs)
    }

    /// Run `f` with page `page`'s characters as the screen shows it: now
    /// if they're at hand, or once the render worker has read them. None
    /// when they can't be read.
    pub(crate) fn with_chars(
        &mut self,
        page: PageId,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, Option<Arc<PageChars>>, &mut Context<Self>) + 'static,
    ) {
        if let Some(chars) = self.cached_chars(page) {
            return f(self, Some(chars), cx);
        }
        let State::Ready(doc) = &mut self.state else {
            return f(self, None, cx);
        };
        let st = doc.model.state();
        let asked = render_key(st, page, &doc.inputs);
        let rx = doc.text.chars(st, page, &doc.inputs);
        doc.chars_loading.insert(page);
        cx.spawn(async move |this, cx| {
            let read = rx.await.ok().and_then(|r| r.ok());
            this.update(cx, |v, cx| {
                let now = match &mut v.state {
                    State::Ready(doc) => {
                        doc.chars_loading.remove(&page);
                        render_key(doc.model.state(), page, &doc.inputs)
                    }
                    _ => None,
                };
                // The page changed while it was read, so what was read
                // isn't what it shows: read it as it is now.
                if read.is_some() && now != asked {
                    return v.with_chars(page, cx, f);
                }
                f(v, read, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Read page `page`'s characters ahead of need (the pointer is over
    /// it with the Select tool), unless that's done or under way.
    pub(crate) fn prefetch_chars(&mut self, page: PageId, cx: &mut Context<Self>) {
        let loading = match &self.state {
            State::Ready(doc) => doc.chars_loading.contains(&page),
            _ => true,
        };
        if !loading && self.cached_chars(page).is_none() {
            self.with_chars(page, cx, |v, _, cx| {
                v.update_text_hover(cx);
            });
        }
    }

    /// Whether `p` on page `page` is over text, as far as is known yet.
    pub(crate) fn over_text(&self, page: PageId, p: Pt, tolerance: f32) -> bool {
        self.cached_chars(page)
            .is_some_and(|c| c.boundary_at(p, tolerance).is_some())
    }

    fn update_text_hover(&mut self, cx: &mut Context<Self>) {
        let over = self.last_hover.is_some_and(|(page, p, tolerance)| {
            (self.tool == Tool::Select || self.tool.marks_text())
                && self.over_text(page, p, tolerance)
        });
        if over != self.text_hover {
            self.text_hover = over;
            cx.notify();
        }
    }

    /// The pointer moved over page `page` (no button down).
    pub(crate) fn hover_page(
        &mut self,
        page: PageId,
        p: Pt,
        tolerance: f32,
        cx: &mut Context<Self>,
    ) {
        self.last_hover = Some((page, p, tolerance));
        if self.tool == Tool::Select || self.tool.marks_text() {
            self.prefetch_chars(page, cx);
        }
        self.update_text_hover(cx);
    }

    pub(crate) fn hover_off_pages(&mut self, cx: &mut Context<Self>) {
        self.last_hover = None;
        self.update_text_hover(cx);
    }

    /// A press on the page with the Select tool away from any mark, or on
    /// text with a text tool: start selecting text there (a double-click
    /// takes the word). Drops any selection before.
    pub(crate) fn start_text_drag(
        &mut self,
        page: PageId,
        index: usize,
        p: Pt,
        tolerance: f32,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        doc.selected_marks.clear();
        doc.text_sel = None;
        cx.notify();
        if clicks >= 2 {
            self.with_chars(page, cx, move |v, chars, cx| {
                let Some(c) = chars else { return };
                let Some(at) = c.boundary_at(p, tolerance) else {
                    return;
                };
                let (a, b) = c.word_at(at);
                if a == b {
                    return;
                }
                let sel = TextSel {
                    page,
                    chars: c,
                    anchor: a,
                    head: b,
                };
                if let State::Ready(doc) = &mut v.state {
                    doc.text_sel = Some(sel.clone());
                }
                v.text_selected(sel, cx);
                cx.notify();
            });
            return;
        }
        doc.text_drag = Some(TextDrag {
            page,
            index,
            start: p,
            current: p,
            tolerance,
            released: false,
        });
        self.with_chars(page, cx, |v, _, cx| {
            v.update_text_drag();
            if let State::Ready(doc) = &v.state
                && doc.text_drag.is_some_and(|d| d.released)
            {
                v.finish_text_drag(cx);
            }
            cx.notify();
        });
    }

    /// The selection a text drag makes, from what's known of the page.
    fn update_text_drag(&mut self) {
        let Some(d) = (match &self.state {
            State::Ready(doc) => doc.text_drag,
            _ => None,
        }) else {
            return;
        };
        let Some(c) = self.cached_chars(d.page) else {
            return;
        };
        let sel = c.boundary_at(d.start, d.tolerance).map(|anchor| TextSel {
            page: d.page,
            anchor,
            head: c.boundary_near(d.current),
            chars: c.clone(),
        });
        if let State::Ready(doc) = &mut self.state {
            doc.text_sel = sel;
        }
    }

    /// The pointer moved during a text drag. False when there's none.
    pub(crate) fn move_text_drag(&mut self, index: usize, p: Pt, cx: &mut Context<Self>) -> bool {
        let State::Ready(doc) = &mut self.state else {
            return false;
        };
        let Some(d) = &mut doc.text_drag else {
            return false;
        };
        if d.index == index {
            d.current = p;
            self.update_text_drag();
            cx.notify();
        }
        true
    }

    /// The button came up on a text drag. False when there's none.
    pub(crate) fn end_text_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let page = match &self.state {
            State::Ready(doc) => doc.text_drag.map(|d| d.page),
            _ => None,
        };
        let Some(page) = page else {
            return false;
        };
        let read = self.cached_chars(page).is_some();
        let State::Ready(doc) = &mut self.state else {
            return false;
        };
        match &mut doc.text_drag {
            Some(d) if !read => d.released = true,
            _ => self.finish_text_drag(cx),
        }
        cx.notify();
        true
    }

    /// A text drag is over: keep what it selected (an empty selection is
    /// a click), and let the tool act on it.
    fn finish_text_drag(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        doc.text_drag = None;
        let sel = doc.text_sel.clone();
        if sel.as_ref().is_some_and(|s| s.is_empty()) {
            doc.text_sel = None;
        }
        if let Some(sel) = sel {
            self.text_selected(sel, cx);
        }
    }

    /// Text was selected (or, when `sel` is empty, clicked) with the
    /// current tool: the text tools act on it straight away.
    fn text_selected(&mut self, sel: TextSel, cx: &mut Context<Self>) {
        match self.tool {
            Tool::Highlight | Tool::Underline | Tool::StrikeOut | Tool::Squiggly
                if !sel.is_empty() =>
            {
                self.markup_selection(self.tool, cx)
            }
            Tool::TextEdit if sel.is_empty() => self.start_insert(&sel, cx),
            Tool::TextEdit => self.start_replace(cx),
            _ => {}
        }
    }

    /// Drop the text selection. Returns whether there was one.
    pub(crate) fn clear_text_sel(&mut self, cx: &mut Context<Self>) -> bool {
        let State::Ready(doc) = &mut self.state else {
            return false;
        };
        if doc.text_sel.take().is_some() {
            cx.notify();
            true
        } else {
            false
        }
    }

    /// The selection, when it has any text.
    fn selection(&self) -> Option<&TextSel> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        doc.text_sel.as_ref().filter(|s| !s.is_empty())
    }

    /// The selected text, and the page and boxes it covers, when there is
    /// a selection.
    pub(crate) fn selected_text(&self) -> Option<(PageId, String, Vec<Box2>)> {
        let sel = self.selection()?;
        let (a, b) = sel.range();
        Some((sel.page, sel.chars.text(a, b), sel.chars.rects(a, b)))
    }

    /// Copy the selected text. False when nothing is selected.
    pub(crate) fn copy_text(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((_, text, _)) = self.selected_text() else {
            return false;
        };
        let n = text.chars().filter(|c| !c.is_whitespace()).count();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        if let State::Ready(doc) = &mut self.state {
            doc.notice = Some(if n == 1 {
                "Copied 1 character".into()
            } else {
                format!("Copied {n} characters")
            });
        }
        cx.notify();
        true
    }

    /// A text tool's key or button: with text selected, act on it and
    /// keep the tool there was (Acrobat's quick actions); otherwise pick
    /// the tool.
    pub(crate) fn markup_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if self.selected_text().is_some() {
            match tool {
                Tool::Highlight | Tool::Underline | Tool::StrikeOut | Tool::Squiggly => {
                    return self.markup_selection(tool, cx);
                }
                Tool::TextEdit => return self.start_replace(cx),
                _ => {}
            }
        }
        self.set_tool(tool, cx);
    }

    /// Shift+U: the next text markup tool.
    pub(crate) fn cycle_text_markup(&mut self, cx: &mut Context<Self>) {
        let next = self.tool.next_text_markup();
        self.set_tool(next, cx);
        if let State::Ready(doc) = &mut self.state {
            doc.notice = Some(next.label().into());
        }
    }

    /// Mark up the selected text with `tool`'s markup.
    fn markup_selection(&mut self, tool: Tool, cx: &mut Context<Self>) {
        let Some(upright) = self.selection().map(|s| s.chars.upright) else {
            return;
        };
        let Some((page, _, rects)) = self.selected_text() else {
            return;
        };
        if rects.is_empty() {
            return;
        }
        self.add_text_markup(tool, page, rects, upright, cx);
        self.clear_text_sel(cx);
    }

    /// Add `tool`'s markup over `rects` (in PageSpace) on `page`, turned
    /// `turn`, as the text under it reads.
    pub(crate) fn add_text_markup(
        &mut self,
        tool: Tool,
        page: PageId,
        rects: Vec<Box2>,
        turn: Rotation,
        cx: &mut Context<Self>,
    ) {
        let kind = match tool {
            Tool::Highlight => AnnotKind::Highlight { rects },
            Tool::Underline => AnnotKind::Underline { rects },
            Tool::StrikeOut => AnnotKind::StrikeOut { rects },
            Tool::Squiggly => AnnotKind::Squiggly { rects },
            _ => return,
        };
        let mut a = self.new_annot(kind, crate::markup_tools::color_of(tool, cx));
        a.turn = turn;
        if tool == Tool::Highlight {
            a.opacity = 0.6;
        }
        self.add_page_annot(page, a, tool.label(), cx);
    }

    /// Text Edits on the selection: cross it out, with a caret after it,
    /// and open the editor for the replacement.
    fn start_replace(&mut self, cx: &mut Context<Self>) {
        let Some((page, struck, rects)) = self.selected_text() else {
            return;
        };
        // After the last character crossed out.
        let at = (self.selection())
            .and_then(|sel| Some((sel.chars.caret_at(sel.range().1)?, sel.chars.upright)));
        let Some((at, turn)) = at else {
            return;
        };
        let kind = AnnotKind::Replace { rects, struck, at };
        let color = crate::markup_tools::color_of(Tool::TextEdit, cx);
        let mut a = self.new_annot(kind, color);
        // The caret stands as the text reads.
        a.turn = turn;
        let made = self.add_page_annot(page, a, "Replace text", cx);
        self.clear_text_sel(cx);
        self.pending_edit = made.map(|id| (page, id));
        cx.notify();
    }

    /// Text Edits at a click in the text, `sel` empty: a caret where it
    /// is, and the editor for what to insert.
    fn start_insert(&mut self, sel: &TextSel, cx: &mut Context<Self>) {
        let page = sel.page;
        let Some(at) = sel.chars.caret_at(sel.anchor) else {
            return;
        };
        let turn = sel.chars.upright;
        let color = crate::markup_tools::color_of(Tool::TextEdit, cx);
        let mut a = self.new_annot(AnnotKind::Insert { at }, color);
        a.turn = turn;
        let made = self.add_page_annot(page, a, "Insert text", cx);
        self.pending_edit = made.map(|id| (page, id));
        cx.notify();
    }
}
