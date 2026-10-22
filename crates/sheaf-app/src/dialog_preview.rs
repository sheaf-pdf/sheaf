//! A dialog's preview of its edit (M10.5 §8.6), as the Header & Footer,
//! Watermark and Crop Pages dialogs draw it: what the settings ask lowered
//! on the document as it is now, off the UI thread, again at each change
//! of the settings and each change the document goes through, and the page
//! the dialog shows realized from the state that leaves, as applying would
//! draw it ([`Previewer`](crate::jobs::Previewer)). A lowering's picture
//! shows only while no later one has begun ([`Preview`]).

use std::sync::Arc;

use gpui::{AnyElement, ObjectFit, RenderImage, Task, Window, div, img, prelude::*, px};
use sheaf_doc::EditError;
use sheaf_edit::PageSize;
use sheaf_image::raster::Raster;
use sheaf_ui::{dialog, theme};

/// The previewed page's height on screen, logical px, and its width at
/// most.
const PREVIEW_H: f32 = 220.0;
const PREVIEW_W: f32 = 340.0;

/// How wide the preview of a page that shows `shown` is on screen, logical
/// px: [`PREVIEW_H`] high, unless that's wider than [`PREVIEW_W`].
fn wide(shown: PageSize) -> f32 {
    (PREVIEW_H * shown.width / shown.height.max(1.0)).min(PREVIEW_W)
}

/// The width, device px, the preview of a page that shows `shown` is drawn
/// at, `scale` device px a logical px.
pub(crate) fn drawn_width(shown: PageSize, scale: f32) -> u32 {
    (wide(shown) * scale).round().max(1.0) as u32
}

/// Why a dialog's edit can't be made, as its preview tells it: an invalid
/// one's reason alone ("those margins leave nothing of page 2").
pub(crate) fn why(e: EditError) -> anyhow::Error {
    match e {
        EditError::Invalid { why, .. } => anyhow::anyhow!(why),
        e => e.into(),
    }
}

/// A dialog's preview: lowered and drawn off the UI thread, each lowering
/// numbered, so the picture of one that a later one has begun after never
/// shows.
pub(crate) struct Preview<K> {
    /// The newest lowering's number, and what it lowered (`K`): settings
    /// that ask the same aren't lowered again until the document changes.
    generation: u64,
    key: Option<K>,
    /// Its lowering and drawing: replacing it drops an older one's render
    /// before the worker gets to it.
    pub(crate) task: Option<Task<()>>,
    /// The newest picture shown, and its size on screen; why the newest
    /// lowering drew none.
    pub(crate) picture: Option<(Arc<RenderImage>, (f32, f32))>,
    pub(crate) failed: Option<String>,
    /// Pictures let go, whose textures go at the next frame.
    retired: Vec<Arc<RenderImage>>,
}

impl<K> Default for Preview<K> {
    fn default() -> Self {
        Preview {
            generation: 0,
            key: None,
            task: None,
            picture: None,
            failed: None,
            retired: Vec::new(),
        }
    }
}

impl<K: PartialEq + Clone> Preview<K> {
    /// A lowering of `key` begun: its number, or None when the newest
    /// lowered it already and `again` isn't asked.
    pub(crate) fn begin(&mut self, key: &K, again: bool) -> Option<u64> {
        if !again && self.key.as_ref() == Some(key) {
            return None;
        }
        self.key = Some(key.clone());
        self.generation += 1;
        Some(self.generation)
    }
}

impl<K> Preview<K> {
    /// Lowering `generation`'s picture, drawn at `scale` device px a
    /// logical px, or why it drew none: taken, unless a later lowering has
    /// begun, which makes it stale. Whether it was taken.
    pub(crate) fn take(
        &mut self,
        generation: u64,
        drawn: anyhow::Result<Raster>,
        scale: f32,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        match drawn {
            Ok(raster) => {
                let size = (raster.width as f32 / scale, raster.height as f32 / scale);
                let image = Arc::new(crate::docview::to_render_image(raster));
                let old = self.picture.replace((image, size));
                self.retired.extend(old.map(|(image, _)| image));
                self.failed = None;
            }
            Err(e) => self.failed = Some(format!("{e:#}")),
        }
        true
    }

    /// Let go of the GPU textures of the pictures replaced since the last
    /// frame (at each frame).
    pub(crate) fn drop_retired(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
    }

    /// Let go of every picture's GPU textures (on close).
    pub(crate) fn release(&mut self, window: &mut Window) {
        let shown = self.picture.take().map(|(image, _)| image);
        for image in self.retired.drain(..).chain(shown) {
            let _ = window.drop_image(image);
        }
    }

    /// The picture, at its size, or a box the size of a page that shows
    /// `shown` saying why there's none yet.
    pub(crate) fn element(&self, shown: PageSize) -> AnyElement {
        if let Some((image, (w, h))) = &self.picture {
            let picture = img(image.clone()).size_full().object_fit(ObjectFit::Fill);
            return (div().w(px(*w)).h(px(*h)))
                .border_1()
                .border_color(theme::BORDER)
                .child(picture)
                .into_any_element();
        }
        let said = (self.failed.clone()).unwrap_or_else(|| "Drawing…".into());
        (div().w(px(wide(shown))).h(px(PREVIEW_H)).p_2())
            .border_1()
            .border_color(theme::BORDER)
            .child(dialog::subtitle(said))
            .into_any_element()
    }
}
