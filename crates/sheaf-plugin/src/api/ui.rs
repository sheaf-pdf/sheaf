//! The `ui` interface: forms, notices and panels, which the app draws.

use std::time::Instant;

use sheaf_doc::PageId;

use super::api::{self, ui as wu};
use crate::runtime::HostState;
use crate::ui::{self, BuiltinPanel, ShowPanel};

fn form_from_wit(f: wu::Form) -> ui::Form {
    ui::Form {
        title: f.title,
        submit_label: f
            .submit_label
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| "OK".into()),
        fields: f
            .fields
            .into_iter()
            .map(|f| ui::Field {
                id: f.id,
                label: f.label,
                kind: match f.kind {
                    wu::FieldKind::Text(t) => ui::FieldKind::Text {
                        default: t.default,
                        placeholder: t.placeholder,
                    },
                    wu::FieldKind::Number(n) => ui::FieldKind::Number {
                        default: n.default,
                        min: n.min,
                        max: n.max,
                    },
                    wu::FieldKind::Choice(c) => ui::FieldKind::Choice {
                        default: (c.default as usize).min(c.options.len().saturating_sub(1)),
                        options: c.options,
                    },
                    wu::FieldKind::Checkbox(b) => ui::FieldKind::Checkbox(b),
                    wu::FieldKind::File(f) => ui::FieldKind::File {
                        save: f.save,
                        extensions: f.extensions,
                    },
                    wu::FieldKind::PageRange(r) => ui::FieldKind::PageRange(r),
                },
            })
            .collect(),
    }
}

fn answer_to_wit(a: ui::FieldAnswer) -> wu::FieldAnswer {
    wu::FieldAnswer {
        id: a.id,
        value: match a.value {
            ui::FieldValue::Text(t) => wu::FieldValue::Text(t),
            ui::FieldValue::Number(n) => wu::FieldValue::Number(n),
            ui::FieldValue::Choice(i) => wu::FieldValue::Choice(i as u32),
            ui::FieldValue::Checkbox(b) => wu::FieldValue::Checkbox(b),
            ui::FieldValue::File(p) => wu::FieldValue::File(p),
            ui::FieldValue::PageRange(ids) => {
                wu::FieldValue::PageRange(ids.into_iter().map(|p| p.0).collect())
            }
        },
    }
}

fn row_from_wit(r: wu::PanelRow) -> ui::PanelRow {
    ui::PanelRow {
        id: r.id,
        label: r.label,
        detail: r.detail,
        badge: r.badge,
        icon: match r.icon {
            wu::Icon::None => ui::Icon::None,
            wu::Icon::Check => ui::Icon::Check,
            wu::Icon::Cross => ui::Icon::Cross,
            wu::Icon::Link => ui::Icon::Link,
            wu::Icon::Warning => ui::Icon::Warning,
            wu::Icon::Info => ui::Icon::Info,
            wu::Icon::Page => ui::Icon::Page,
        },
        depth: r.depth,
        page: r.page.map(PageId),
        actions: r
            .actions
            .into_iter()
            .map(|a| ui::RowAction {
                id: a.id,
                label: a.label,
            })
            .collect(),
    }
}

impl api::ui::Host for HostState {
    fn show_form(&mut self, form: wu::Form) -> Option<Vec<wu::FieldAnswer>> {
        let ui = self.ui()?;
        let started = Instant::now();
        let answer = ui.show_form(form_from_wit(form));
        // The user's thinking time isn't the plugin's.
        self.deadline += started.elapsed();
        answer.map(|a| a.into_iter().map(answer_to_wit).collect())
    }

    fn notice(&mut self, message: String) {
        match self.ui() {
            Some(ui) => ui.notice(message),
            None => log::info!(target: "plugin", "[{}] {message}", self.plugin),
        }
    }

    fn set_panel(&mut self, panel: String, rows: Vec<wu::PanelRow>) {
        if !self.panels.contains(&panel) {
            log::warn!(
                target: "plugin",
                "[{}] set-panel: {panel:?} isn't declared in the manifest",
                self.plugin
            );
            return;
        }
        if let Some(ui) = self.ui() {
            ui.set_panel(panel, rows.into_iter().map(row_from_wit).collect());
        }
    }

    fn show_panel(&mut self, panel: String) {
        // The plugin's own panels first, so a panel it calls "comments"
        // is still its own.
        let which = if self.panels.contains(&panel) {
            ShowPanel::Plugin(panel)
        } else if let Some(b) = BuiltinPanel::from_name(&panel) {
            ShowPanel::Builtin(b)
        } else {
            log::warn!(
                target: "plugin",
                "[{}] show-panel: there's no panel {panel:?}",
                self.plugin
            );
            return;
        };
        if let Some(ui) = self.ui() {
            ui.show_panel(which);
        }
    }
}
