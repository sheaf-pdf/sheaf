//! The `render` interface: read-only page images as WIT resources. Pixels
//! stay on the host; plugins read strips or tiles. Every handle is freed
//! when the call that made it returns.

use anyhow::Result;
use sheaf_doc::geom::Affine;
use sheaf_doc::{PageId, Snapshot, ViewInputs};
use wasmtime::component::Resource;

use crate::runtime::HostState;

/// Most pixels one call may render, all images together.
pub const PIXEL_BUDGET: u64 = 400_000_000;
/// Most images alive at once.
pub const MAX_LIVE_IMAGES: usize = 16;
pub const MAX_DPI: u32 = 600;

/// Renders pages for plugins. The app implements it over its engines:
/// `rect` is a pixel tile of page `page` of `snap`, as the call reads the
/// document, drawn `page_w`×`page_h` as the page shows, turned as it
/// shows, as the file will draw it under `inputs` (M10.5 §14.3): sheaf's
/// marks and the header, footer, watermarks and backgrounds it draws, saved
/// or not, included ([`ViewInputs::plugin`]), or the page without markup
/// ([`ViewInputs::print`]). The result is RGBA, `w`×`h`.
pub trait RenderSource: Send + Sync {
    fn render(
        &self,
        snap: &Snapshot,
        page: PageId,
        inputs: &ViewInputs,
        page_w: u32,
        page_h: u32,
        rect: (u32, u32, u32, u32),
    ) -> Result<Vec<u8>>;
}

/// A `page-renderer` handle: the page, as the call's transaction shows it
/// when it renders.
pub struct PageRenderer {
    pub(crate) page: PageId,
}

/// An `image` handle.
pub struct RenderedImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) gray: bool,
    pub(crate) data: Vec<u8>,
    /// The point of the page as shown at pixel (0, 0), pixels per point,
    /// and the page as shown to PageSpace.
    pub(crate) origin: (f32, f32),
    pub(crate) scale: f32,
    pub(crate) from_view: Affine,
}

impl RenderedImage {
    pub(crate) fn bpp(&self) -> u32 {
        if self.gray { 1 } else { 4 }
    }
}

/// Handles one call created, freed when it returns.
#[derive(Default)]
pub(crate) struct CallHandles {
    pub renderers: Vec<u32>,
    pub images: Vec<u32>,
    pub pixels: u64,
}

impl HostState {
    /// Free every render handle the call made.
    pub(crate) fn drop_render_handles(&mut self) {
        let handles = std::mem::take(&mut self.render_handles);
        for rep in handles.images {
            let _ = self.table.delete::<RenderedImage>(Resource::new_own(rep));
        }
        for rep in handles.renderers {
            let _ = self.table.delete::<PageRenderer>(Resource::new_own(rep));
        }
    }
}
