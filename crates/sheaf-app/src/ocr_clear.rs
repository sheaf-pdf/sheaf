//! Document > Remove Recognized Text…: the text OCR put on pages taken off
//! them, on the selected pages or every page, once the reader says so
//! (`ClearOcrText`, M10.5 slice 18). What sheaf recognized goes, unsaved
//! or in the file; what the pages draw stays, and so does text another program
//! recognized. It's planned as a header's removal is, each page's content
//! read off the UI thread, and committed on the version the question was
//! asked on: one undo step. On the command line it's `sheaf edit`'s
//! `clear-ocr-text`.

use gpui::{Context, PromptButton, PromptLevel, Window};
use sheaf_doc::{PageId, Snapshot};
use sheaf_edit::{Op, PageSel};

use crate::docview::{DocView, State};

/// What the question offers: the selected pages, or every page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    Selected,
    All,
}

/// The question for a document of `total` pages, `selected` of them
/// selected: its message and detail, and its buttons with what each
/// takes the text off; Cancel, last, takes nothing. With none selected, or
/// all of them, it asks about every page alone.
fn question(selected: usize, total: usize) -> (String, &'static str, Vec<(PromptButton, Which)>) {
    let detail = "The text sheaf recognized comes off, saved or not. What the pages draw \
                  stays, and so does text another program recognized. Undo puts it back.";
    if selected == 0 || selected == total {
        let message = "Remove recognized text from every page?".to_string();
        return (
            message,
            detail,
            vec![(PromptButton::ok("Remove"), Which::All)],
        );
    }
    let message = match selected {
        1 => "Remove recognized text from the selected page, or from every page?".to_string(),
        n => format!("Remove recognized text from the {n} selected pages, or from every page?"),
    };
    let buttons = vec![
        (PromptButton::ok("Selected Pages"), Which::Selected),
        (PromptButton::new("All Pages"), Which::All),
    ];
    (message, detail, buttons)
}

impl DocView {
    /// Document > Remove Recognized Text…: asks which pages, then takes
    /// their recognized text off ([`remove_recognized_text`]).
    ///
    /// [`remove_recognized_text`]: Self::remove_recognized_text
    pub(crate) fn remove_recognized_text_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let opened = doc.model.snapshot();
        let order = opened.state.order();
        let selected: Vec<PageId> = (order.iter())
            .filter(|p| doc.selection.contains(p))
            .copied()
            .collect();
        let (message, detail, choices) = question(selected.len(), order.len());
        let mut buttons: Vec<PromptButton> = choices.iter().map(|(b, _)| b.clone()).collect();
        buttons.push(PromptButton::cancel("Cancel"));
        let rx = sheaf_ui::prompts::ask(
            window,
            cx,
            PromptLevel::Warning,
            &message,
            Some(detail),
            &buttons,
        );
        cx.spawn(async move |this, cx| {
            let Some(&(_, which)) = rx.await.ok().and_then(|i| choices.get(i)) else {
                return;
            };
            let pages = match which {
                Which::Selected => selected,
                Which::All => opened.state.order().iter().copied().collect(),
            };
            this.update(cx, |v, cx| v.remove_recognized_text(opened, pages, cx))
                .ok();
        })
        .detach();
    }

    /// The recognized text taken off `pages` of `opened`, the version the
    /// question was asked on, as a planner ([`plan_removal`]): the notice
    /// is its line, with what stays said, or that there was none.
    ///
    /// [`plan_removal`]: Self::plan_removal
    pub(crate) fn remove_recognized_text(
        &mut self,
        opened: Snapshot,
        pages: Vec<PageId>,
        cx: &mut Context<Self>,
    ) {
        let clear = Op::ClearOcrText {
            pages: PageSel::ids(pages),
        };
        let (label, none) = ("Remove recognized text", "No recognized text to remove");
        self.plan_removal(opened, clear, label, none, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With some pages selected it offers them or every page; with none,
    /// or all, every page alone. Cancel is the caller's, last.
    #[test]
    fn it_asks_about_the_selected_pages_or_every_page() {
        let offered = |selected, total| {
            let (message, _, buttons) = question(selected, total);
            let buttons: Vec<(String, Which)> = (buttons.into_iter())
                .map(|(b, which)| (b.label().to_string(), which))
                .collect();
            (message, buttons)
        };
        let every = (
            "Remove recognized text from every page?".to_string(),
            vec![("Remove".to_string(), Which::All)],
        );
        assert_eq!(offered(0, 5), every);
        assert_eq!(offered(5, 5), every);
        let (message, buttons) = offered(2, 5);
        assert_eq!(
            message,
            "Remove recognized text from the 2 selected pages, or from every page?"
        );
        assert_eq!(
            buttons,
            [
                ("Selected Pages".to_string(), Which::Selected),
                ("All Pages".to_string(), Which::All)
            ]
        );
        let (message, _) = offered(1, 5);
        assert!(message.contains("the selected page,"), "{message}");
    }
}
