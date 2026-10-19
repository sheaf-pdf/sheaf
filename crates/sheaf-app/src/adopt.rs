//! Another tool's comment becomes one of our marks when it's edited (M9
//! decision 5): dragged on the page, its text edited (double-click, or F2
//! in the Comments panel), or Make Editable from the panel's menu. Its
//! mark was made when the file was read (`ForeignAnnot::mark`); adopting
//! it keeps the comment's id and name, so replies stay with it, and a
//! save puts it where the comment was.

use gpui::{Context, Window};
use sheaf_doc::{Annot, AnnotId, AnnotKind, Box2, PageId, Pt};
use sheaf_edit::{AnnotRef, Op};

use crate::docview::{DocView, State};

impl DocView {
    /// The other tool's comment under `p` (in the page's edit frame) on
    /// `page_id` that sheaf can make its own, topmost first: shown, and not
    /// a reply or status (those are edited from the panel).
    pub(crate) fn foreign_at(&self, page_id: PageId, p: Pt, tolerance: f32) -> Option<AnnotId> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        if doc.comments_hidden {
            return None;
        }
        let to_edit = doc.model.geom(page_id).to_view();
        let under = |rect: Box2| {
            let r = to_edit.apply_box(rect);
            p.x >= r.x - tolerance
                && p.x <= r.right() + tolerance
                && p.y >= r.y - tolerance
                && p.y <= r.bottom() + tolerance
        };
        let hidden = doc.comment_view.as_ref().map(|(_, v)| &v.hidden_foreign);
        let theirs: Vec<_> = doc.model.state().foreign_on(page_id).collect();
        theirs
            .into_iter()
            .rev()
            .find(|(id, f)| {
                let f = &f.summary;
                !f.is_link()
                    && !f.hidden
                    && !hidden.is_some_and(|h| h.contains(id))
                    && f.mark.as_ref().is_some_and(|m| {
                        !matches!(m.kind, AnnotKind::Reply { .. } | AnnotKind::Status { .. })
                    })
                    && under(f.rect)
            })
            .map(|(id, _)| id)
    }

    /// The page another tool's comment `id` is on, and the mark it becomes.
    pub(crate) fn foreign_mark(&self, id: AnnotId) -> Option<(PageId, Annot)> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        let a = doc.model.state().annot(id)?;
        Some((a.page, (**a.foreign()?.summary.mark.as_ref()?).clone()))
    }

    /// Make another tool's comment `id` one of our marks, and then what
    /// `then` does to it, as one undo step called `label`, and select it:
    /// the mark keeps the comment's id. Its page, or None when sheaf can't
    /// hold it or the edit failed.
    pub(crate) fn adopt_foreign(
        &mut self,
        id: AnnotId,
        then: Option<Op>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<PageId> {
        let Some((page, _)) = self.foreign_mark(id) else {
            self.set_notice("sheaf can't edit that kind of comment", cx);
            return None;
        };
        let adopt = Op::AdoptAnnotations {
            annots: vec![AnnotRef::Id(id)],
        };
        self.apply_batch([adopt].into_iter().chain(then).collect(), label, cx)?;
        let State::Ready(doc) = &mut self.state else {
            return None;
        };
        doc.selected_foreign = None;
        doc.select_mark(Some((page, id)));
        cx.notify();
        Some(page)
    }

    /// F2 or a double-click on another tool's comment: make it ours, then
    /// edit its text (a reply's in the reply box).
    pub(crate) fn edit_foreign(
        &mut self,
        id: AnnotId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = self.adopt_foreign(id, None, "Make comment editable", cx) else {
            return;
        };
        let kind = match &self.state {
            State::Ready(doc) => doc
                .model
                .state()
                .find_annot(id)
                .map(|(_, a)| a.kind.clone()),
            _ => None,
        };
        match kind {
            Some(AnnotKind::Reply { .. }) => {
                self.start_reply(crate::comments_panel::Item::Ours(page, id), window, cx)
            }
            Some(AnnotKind::Status { .. }) | None => {}
            Some(_) => self.start_annot_editor_for(page, id, window, cx),
        }
    }
}
