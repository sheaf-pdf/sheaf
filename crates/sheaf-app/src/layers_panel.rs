//! The Layers panel: optional content groups, shown or hidden for viewing.
//! A change is a view input (M10.5 §10.4): the screen draws the primary
//! file with the hidden layers off, through a patch of its catalog that
//! never reaches disk (`sheaf_doc::layers::ocg_patch`). Saving never writes
//! it.

use std::collections::HashSet;

use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use sheaf_doc::layers::LayerId;
use sheaf_ui::theme;

use crate::docview::{DocView, State};

impl crate::docview::Doc {
    /// Layers as the file has them, once its facts are read.
    pub(crate) fn set_layers(&mut self, layers: Vec<sheaf_doc::layers::Layer>) {
        self.layers_hidden = layers
            .iter()
            .filter(|l| !l.on)
            .filter_map(|l| l.id)
            .collect();
        self.layers = layers;
    }
}

impl DocView {
    pub(crate) fn toggle_layer(&mut self, id: LayerId, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        if !doc.layers_hidden.remove(&id) {
            doc.layers_hidden.insert(id);
        }
        self.apply_layers(cx);
    }

    fn show_all_layers(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let locked: HashSet<LayerId> = doc
            .layers
            .iter()
            .filter(|l| l.locked)
            .filter_map(|l| l.id)
            .collect();
        doc.layers_hidden.retain(|id| locked.contains(id));
        self.apply_layers(cx);
    }

    /// Draw the primary file with the hidden layers off, from now on.
    fn apply_layers(&mut self, cx: &mut Context<Self>) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        doc.layers_chosen = true;
        doc.set_inputs();
        doc.notice = Some(match doc.layers_hidden.len() {
            0 => "All layers shown".into(),
            1 => "1 layer hidden (display only; not saved)".into(),
            n => format!("{n} layers hidden (display only; not saved)"),
        });
        cx.notify();
    }

    /// The Layers panel body.
    pub fn render_layers(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let State::Ready(doc) = &self.state else {
            return div().p_2().text_sm().child("…").into_any_element();
        };
        if doc.layers.is_empty() {
            return div()
                .p_2()
                .text_sm()
                .text_color(theme::TEXT_MUTED)
                .child(if doc.model.is_ready() {
                    "This document has no layers"
                } else {
                    "Reading layers…"
                })
                .into_any_element();
        }
        let rows = doc.layers.iter().enumerate().map(|(i, l)| {
            let indent = px(8.0 + 16.0 * l.depth as f32);
            let row = div()
                .id(("layer", i))
                .pl(indent)
                .pr_2()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .text_sm();
            let Some(id) = l.id else {
                // A heading in the layer order.
                return row
                    .text_color(theme::TEXT_MUTED)
                    .child(l.name.clone())
                    .into_any_element();
            };
            let on = !doc.layers_hidden.contains(&id);
            let row = row
                .child(
                    div()
                        .w(px(16.0))
                        .text_color(if on { theme::ACCENT } else { theme::TEXT_DIM })
                        .child(if on { "☑" } else { "☐" }),
                )
                .child(
                    div()
                        .text_color(if on {
                            theme::TEXT_BRIGHT
                        } else {
                            theme::TEXT_MUTED
                        })
                        .child(l.name.clone()),
                );
            if l.locked {
                row.text_color(theme::TEXT_DIM)
                    .child(div().text_xs().child("locked"))
                    .into_any_element()
            } else {
                row.cursor_pointer()
                    .hover(|s| s.bg(theme::HOVER_SUBTLE))
                    .on_click(cx.listener(move |v, _, _, cx| v.toggle_layer(id, cx)))
                    .into_any_element()
            }
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(theme::DIVIDER)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(theme::TEXT_MUTED)
                            .child("Click to show or hide; display only"),
                    )
                    .child(
                        sheaf_ui::button::Button::new("layers-all", "Show All")
                            .on_click(cx.listener(|v, _, _, cx| v.show_all_layers(cx))),
                    ),
            )
            .child(
                div()
                    .id("layers")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element()
    }
}
