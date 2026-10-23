//! The `render` interface: a page drawn as it shows, as the call's
//! transaction holds it, its pixels mapped back to PageSpace.

use std::time::Instant;

use sheaf_doc::{PageId, Pt, ViewInputs};
use wasmtime::component::Resource;

use super::api::{self, types as w};
use super::convert::{box_from_wit, pt_to_wit};
use crate::render::{MAX_DPI, MAX_LIVE_IMAGES, PIXEL_BUDGET, PageRenderer, RenderedImage};
use crate::runtime::HostState;

impl api::render::Host for HostState {}

impl api::render::HostPageRenderer for HostState {
    /// An unknown page still gets a handle; `render` reports it.
    fn new(&mut self, page: u64) -> Resource<PageRenderer> {
        let r = self
            .table
            .push(PageRenderer { page: PageId(page) })
            .expect("resource table full");
        self.render_handles.renderers.push(r.rep());
        r
    }

    /// As the call's transaction shows the page now; (0, 0) for one it
    /// hasn't got. What the page shows is read when it's rendered.
    fn size(&mut self, this: Resource<PageRenderer>) -> (f32, f32) {
        let Ok(page) = self.table.get(&this).map(|r| r.page) else {
            return (0.0, 0.0);
        };
        let Some((tx, _)) = self.tx() else {
            return (0.0, 0.0);
        };
        tx.state().geom(page).map_or((0.0, 0.0), |g| {
            let shown = g.shown();
            (shown.width, shown.height)
        })
    }

    /// Read: what the page draws as the file will, sheaf's marks with it
    /// unless without markup (M10.5 §14.3).
    fn render(
        &mut self,
        this: Resource<PageRenderer>,
        options: api::render::RenderOptions,
    ) -> Result<Resource<RenderedImage>, String> {
        let page = self.table.get(&this).map_err(|e| e.to_string())?.page;
        let inputs = match options.without_markup {
            true => ViewInputs::print(false),
            false => ViewInputs::plugin(),
        };
        let (d, snap, id) = self.drawn(page.0, &inputs)?;
        let source = d.render.clone().ok_or("rendering isn't available")?;
        let geom = snap.geom(id);
        let shown = geom.shown();
        let dpi = options.dpi.clamp(1, MAX_DPI);
        let scale = dpi as f32 / 72.0;
        let (page_w, page_h) = (
            (shown.width * scale).round().max(1.0) as u32,
            (shown.height * scale).round().max(1.0) as u32,
        );
        let (x0, y0, x1, y1) = match options.region {
            Some(r) => {
                // PageSpace, as the page shows it.
                let r = geom.to_view().apply_box(box_from_wit(r));
                (
                    (r.x.max(0.0) * scale).floor() as u32,
                    (r.y.max(0.0) * scale).floor() as u32,
                    ((r.x + r.w) * scale).ceil().min(page_w as f32) as u32,
                    ((r.y + r.h) * scale).ceil().min(page_h as f32) as u32,
                )
            }
            None => (0, 0, page_w, page_h),
        };
        if x1 <= x0 || y1 <= y0 {
            return Err("the region is outside the page".into());
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let pixels = w as u64 * h as u64;
        if self.render_handles.pixels + pixels > PIXEL_BUDGET {
            return Err(format!(
                "this call has rendered its limit of {} million pixels",
                PIXEL_BUDGET / 1_000_000
            ));
        }
        if self.render_handles.images.len() >= MAX_LIVE_IMAGES {
            return Err(format!(
                "at most {MAX_LIVE_IMAGES} images can be alive at once; drop some first"
            ));
        }
        let started = Instant::now();
        let rgba = (source.render(&snap, id, &inputs, page_w, page_h, (x0, y0, w, h)))
            .map_err(|e| format!("{e:#}"));
        // Rendering is the host's time, not the plugin's.
        self.deadline += started.elapsed();
        let rgba = rgba?;
        self.render_handles.pixels += pixels;
        let gray = matches!(options.format, api::render::PixelFormat::Gray8);
        let data = if gray {
            rgba.as_chunks::<4>()
                .0
                .iter()
                .map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8)
                .collect()
        } else {
            rgba
        };
        let image = RenderedImage {
            width: w,
            height: h,
            gray,
            data,
            origin: (x0 as f32 / scale, y0 as f32 / scale),
            scale,
            from_view: geom.from_view(),
        };
        let r = self.table.push(image).map_err(|e| e.to_string())?;
        self.render_handles.images.push(r.rep());
        Ok(r)
    }

    fn drop(&mut self, this: Resource<PageRenderer>) -> wasmtime::Result<()> {
        let rep = this.rep();
        self.render_handles.renderers.retain(|r| *r != rep);
        self.table.delete(this)?;
        Ok(())
    }
}

impl api::render::HostImage for HostState {
    fn width(&mut self, this: Resource<RenderedImage>) -> u32 {
        self.table.get(&this).map(|i| i.width).unwrap_or(0)
    }

    fn height(&mut self, this: Resource<RenderedImage>) -> u32 {
        self.table.get(&this).map(|i| i.height).unwrap_or(0)
    }

    fn format(&mut self, this: Resource<RenderedImage>) -> api::render::PixelFormat {
        match self.table.get(&this) {
            Ok(i) if !i.gray => api::render::PixelFormat::Rgba8,
            _ => api::render::PixelFormat::Gray8,
        }
    }

    fn to_page(&mut self, this: Resource<RenderedImage>, x: f32, y: f32) -> w::Pt {
        match self.table.get(&this) {
            Ok(i) => pt_to_wit(i.from_view.apply(Pt {
                x: i.origin.0 + x / i.scale,
                y: i.origin.1 + y / i.scale,
            })),
            Err(_) => w::Pt { x: 0.0, y: 0.0 },
        }
    }

    fn read_rows(&mut self, this: Resource<RenderedImage>, y: u32, count: u32) -> Vec<u8> {
        let Ok(i) = self.table.get(&this) else {
            return Vec::new();
        };
        let stride = (i.width * i.bpp()) as usize;
        let start = (y.min(i.height) as usize) * stride;
        let end = ((y.saturating_add(count)).min(i.height) as usize) * stride;
        i.data[start..end].to_vec()
    }

    fn read_rect(
        &mut self,
        this: Resource<RenderedImage>,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> Vec<u8> {
        let Ok(i) = self.table.get(&this) else {
            return Vec::new();
        };
        let bpp = i.bpp() as usize;
        let (x0, x1) = (
            x.min(i.width) as usize,
            x.saturating_add(w).min(i.width) as usize,
        );
        let (y0, y1) = (y.min(i.height), y.saturating_add(h).min(i.height));
        let stride = i.width as usize * bpp;
        let mut out = Vec::with_capacity((x1 - x0) * bpp * (y1 - y0) as usize);
        for row in y0..y1 {
            let base = row as usize * stride;
            out.extend_from_slice(&i.data[base + x0 * bpp..base + x1 * bpp]);
        }
        out
    }

    fn drop(&mut self, this: Resource<RenderedImage>) -> wasmtime::Result<()> {
        let rep = this.rep();
        self.render_handles.images.retain(|r| *r != rep);
        self.table.delete(this)?;
        Ok(())
    }
}
