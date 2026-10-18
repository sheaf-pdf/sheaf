//! The Attach File tool (Shift+J from Stamp): click where the icon goes,
//! then pick the file. Its bytes go into the document's store, and the
//! mark holds them until a save embeds them. Double-click the icon, or use
//! the Attachments panel, to open or save the file; the properties bar
//! picks the icon (push pin, paperclip, graph or tag).

use gpui::{Context, Window};
use sheaf_doc::attachments::{Attachment, AttachmentRef};
use sheaf_doc::store::BlobKind;
use sheaf_doc::{AnnotKind, PageId, Pt};

use crate::docview::{DocView, State};
use crate::markup_tools::Tool;

impl DocView {
    /// Attach File: ask for the file, then put it at `p` on `page`.
    pub(crate) fn attach_file_at(&mut self, page: PageId, p: Pt, cx: &mut Context<Self>) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let store = doc.model.store().clone();
        let rx = crate::file_dialog::open_filtered(
            cx,
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Attach".into()),
            },
            crate::file_dialog::Filter::Any,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            // Read and interned, and held until the mark is added.
            let read = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move {
                        let bytes = sheaf_core::read_file(&path)?;
                        anyhow::Ok(store.intern(bytes, BlobKind::Attachment)?)
                    }
                })
                .await;
            this.update(cx, |v, cx| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "attachment".into());
                match read {
                    Ok(held) => {
                        let kind = AnnotKind::FileAttachment {
                            pos: Pt {
                                x: p.x - sheaf_doc::annot::NOTE_SIZE / 2.0,
                                y: p.y - sheaf_doc::annot::NOTE_SIZE / 2.0,
                            },
                            icon: v.attach_icon.clone(),
                            file: held.id().stem(),
                            name: name.clone(),
                            size: held.len() as u64,
                        };
                        let color = crate::markup_tools::color_of(
                            crate::markup_tools::Tool::AttachFile,
                            cx,
                        );
                        let a = v.new_annot(kind, color, cx);
                        v.add_annot(page, a, &format!("Attach {name}"), cx);
                        drop(held);
                    }
                    Err(e) => v.set_notice(format!("Couldn't attach {name}: {e:#}"), cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Double-click on an attachment icon: open its file.
    pub(crate) fn open_attached(
        &mut self,
        page: PageId,
        id: sheaf_doc::AnnotId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let a = match &self.state {
            State::Ready(doc) => doc
                .model
                .state()
                .find_annot(id)
                .filter(|(on, _)| *on == page)
                .map(|(_, a)| a.clone()),
            _ => None,
        };
        let Some(sheaf_doc::Annot {
            kind: AnnotKind::FileAttachment { name, size, .. },
            modified,
            created,
            ..
        }) = a
        else {
            return;
        };
        self.open_attachment_item(
            Attachment {
                at: AttachmentRef::Mark(id),
                name,
                description: String::new(),
                size: Some(size),
                modified,
                created,
                folder: String::new(),
                fields: Vec::new(),
            },
            window,
            cx,
        );
    }

    /// Shift+J: Stamp, then Attach File.
    pub(crate) fn cycle_stamp_tools(&mut self, cx: &mut Context<Self>) {
        let next = if self.tool == Tool::Stamp {
            Tool::AttachFile
        } else {
            Tool::Stamp
        };
        self.set_tool(next, cx);
        self.set_notice(next.label(), cx);
    }
}
