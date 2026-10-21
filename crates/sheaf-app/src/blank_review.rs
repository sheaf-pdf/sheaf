//! Document > Find Blank Pages…: the search runs as a planner (jobs.rs)
//! over a snapshot, its pages read through the document's views
//! (`sheaf_ops::blank::find`), then a review sheet over the document
//! shows each page found, with a thumbnail, why, and a checkbox. Blank
//! and "intentionally blank" pages start checked, "nearly blank" ones
//! don't. "Delete N pages" removes the checked ones in one undo step, on
//! the version the search measured (M10.5 §8.5): a page changed since
//! conflicts, and the delete is lowered again over what else changed.
//! Nothing is deleted without that click.

use std::collections::HashSet;

use gpui::{AnyElement, Context, ObjectFit, ScrollHandle, div, img, prelude::*, px};
use sheaf_doc::{Actor, CommitOpts, Document, PageId, Snapshot, Stale, ViewInputs};
use sheaf_edit::{Op, PageSel};
use sheaf_ops::blank::{BlankParams, Candidate, Reason};
use sheaf_render::Job;
use sheaf_ui::button::{Button, Chip};
use sheaf_ui::{dialog, theme};

use crate::docview::{DocView, State, Want};

/// Thumbnail width in the review sheet.
const THUMB: f32 = 120.0;

pub(crate) struct Review {
    /// The document as the search measured it.
    pub planning: Snapshot,
    pub candidates: Vec<Candidate>,
    pub checked: HashSet<PageId>,
    pub scroll: ScrollHandle,
}

impl DocView {
    /// Look for blank pages among `pages` (0-based; none: all).
    pub(crate) fn find_blank_pages(
        &mut self,
        pages: Option<Vec<usize>>,
        params: BlankParams,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let planning = doc.model.snapshot();
        let (snapshot, views) = (planning.clone(), doc.views.clone());
        self.blank_review = None;
        self.run_planner(
            "Find blank pages",
            move |p| sheaf_ops::blank::find(&snapshot, &views, pages.as_deref(), params, p),
            |s| {
                Some(format!(
                    "Looking for blank pages: {} of {}…",
                    s.done, s.total?
                ))
            },
            |v, found, cx| {
                if found.is_empty() {
                    return Some("No blank pages found".into());
                }
                let n = found.len();
                let checked = found
                    .iter()
                    .filter(|c| c.drop_by_default())
                    .map(|c| c.id)
                    .collect();
                v.blank_review = Some(Review {
                    planning,
                    candidates: found,
                    checked,
                    scroll: ScrollHandle::new(),
                });
                cx.notify();
                Some(format!(
                    "{n} page{} to review",
                    if n == 1 { "" } else { "s" }
                ))
            },
            cx,
        );
    }

    /// Close the review sheet. Returns whether one was open.
    pub(crate) fn close_blank_review(&mut self, cx: &mut Context<Self>) -> bool {
        if self.blank_review.take().is_none() {
            return false;
        }
        if let State::Ready(doc) = &mut self.state {
            doc.review_keep.clear();
        }
        cx.notify();
        true
    }

    pub(crate) fn delete_reviewed(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.blank_review.take() else {
            return;
        };
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        doc.review_keep.clear();
        // In document order; pages deleted since the search are skipped.
        let ids: Vec<PageId> = doc
            .model
            .state()
            .order()
            .iter()
            .copied()
            .filter(|id| review.checked.contains(id))
            .collect();
        if ids.is_empty() {
            cx.notify();
            return;
        }
        let n = ids.len();
        let label = format!("Delete {n} blank page{}", if n == 1 { "" } else { "s" });
        // What the search measured of each is what the delete is based on.
        let planning = review.planning;
        let mut tx = Document::tx_at(&planning, doc.model.lower_cx(Actor::User));
        for &id in &ids {
            tx.read_render(id, &ViewInputs::content_only());
        }
        let pages = PageSel::ids(ids);
        let Some(tx) = self.lower_in(tx, vec![Op::DeletePages { pages }], &label, cx) else {
            return;
        };
        let opts = CommitOpts::based(planning.version, Stale::Relower, label.as_str());
        if self.commit(tx, opts, cx).is_ok() {
            self.set_notice(format!("{label} — Ctrl+Z brings them back"), cx);
        }
    }

    /// The review sheet, over the document, when one is open.
    pub(crate) fn render_blank_review(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let review = self.blank_review.as_ref()?;
        let State::Ready(doc) = &mut self.state else {
            return None;
        };
        let st = doc.model.state();
        // Candidates still in the document, with where they are now.
        let rows: Vec<(usize, Candidate)> = review
            .candidates
            .iter()
            .filter_map(|c| Some((st.pos_of(c.id)?, c.clone())))
            .collect();
        // Each page as the screen draws it, upright.
        let viewed: Vec<_> = (rows.iter())
            .filter_map(|(at, c)| {
                let id = st.order()[*at];
                let view = doc.views.view(st, id, &doc.inputs).ok()?;
                Some((*at, c, id, view, st.geom(id)?.shown()))
            })
            .collect();
        let mut wants = Vec::new();
        doc.review_keep.clear();
        let mut tiles = Vec::new();
        for (at, c, id, view, size) in viewed {
            let rotation = view.job.rotation;
            doc.review_keep.insert((id, view.key, rotation));
            wants.push(Want {
                id,
                view,
                job: Job::Page {
                    width: 320,
                    rotation,
                },
                priority: 3,
            });
            let h = THUMB * size.height / size.width.max(1.0);
            let key = doc.image_key(id, view.key, rotation);
            let image = doc.previews.get(&(key, rotation)).cloned();
            let checked = review.checked.contains(&id);
            let why = match c.reason {
                Reason::NearlyBlank => format!("nearly blank, {:.2}% ink", c.ink * 100.0),
                r => r.label().to_string(),
            };
            tiles.push(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w(px(THUMB + 8.0))
                    .child(
                        div()
                            .id(("blank-thumb", at))
                            .w(px(THUMB))
                            .h(px(h))
                            .bg(theme::PAGE)
                            .border_2()
                            .border_color(if checked { theme::ERROR } else { theme::BORDER })
                            .cursor_pointer()
                            .when_some(image, |d, im| {
                                d.child(img(im).size_full().object_fit(ObjectFit::Fill))
                            })
                            .on_click(cx.listener(move |v, _, _, cx| v.go_to_page(at, cx))),
                    )
                    .child(
                        Chip::new(
                            ("blank-check", at),
                            format!("{} p. {}", if checked { "☑" } else { "☐" }, at + 1),
                        )
                        .selected(checked)
                        .on_click(cx.listener(move |v, _, _, cx| {
                            if let Some(r) = &mut v.blank_review
                                && !r.checked.remove(&id)
                            {
                                r.checked.insert(id);
                            }
                            cx.notify();
                        })),
                    )
                    .child(div().text_xs().text_color(theme::TEXT_MUTED).child(why))
                    .into_any_element(),
            );
        }
        let n = rows
            .iter()
            .filter(|(_, c)| review.checked.contains(&c.id))
            .count();
        let scroll = review.scroll.clone();
        let sheet = div()
            .id("blank-review")
            .occlude()
            .absolute()
            .top_2()
            .right_2()
            .bottom_2()
            .w(px(4.0 * (THUMB + 8.0) + 48.0))
            .flex()
            .flex_col()
            .bg(theme::CARD)
            .border_1()
            .border_color(theme::BORDER_CARD)
            .rounded_md()
            .shadow_lg()
            .p_3()
            .gap_2()
            .text_color(theme::TEXT_SOFT)
            .child(dialog::header("Blank Pages").child(dialog::subtitle(
                "checked pages are deleted; click a page to see it",
            )))
            .child(
                div()
                    .id("blank-review-list")
                    .track_scroll(&scroll)
                    .overflow_y_scroll()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(div().flex().flex_wrap().gap_2().children(tiles)),
            )
            .child(
                dialog::footer()
                    .child(dialog::hint("Esc closes"))
                    .child(Button::new("blank-cancel", "Cancel").on_click(cx.listener(
                        |v, _, _, cx| {
                            v.close_blank_review(cx);
                        },
                    )))
                    .child(
                        Button::new(
                            "blank-delete",
                            format!("Delete {n} Page{}", if n == 1 { "" } else { "s" }),
                        )
                        .primary()
                        .on_click(cx.listener(|v, _, _, cx| v.delete_reviewed(cx))),
                    ),
            )
            .into_any_element();
        for want in wants {
            self.ensure_rendered(want, cx);
        }
        Some(sheet)
    }
}
