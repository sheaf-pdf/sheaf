//! Pictures of stamps for the screen, made in memory with PDFium: a
//! stamp's page (its one-page PDF, from the page a mark holds or the
//! library) drawn transparent where the stamp draws nothing, and a turned
//! tape as the file draws it.
//!
//! Drawing asks for a picture with [`picture`], [`library_picture`] or
//! [`tape_picture`]; one that isn't made yet is remembered with where its
//! PDF is, and [`make_wanted`] (called once a frame) makes it off the main
//! thread and redraws the windows when it's done. Pictures are kept for
//! the session: a stamp's by its stem, which names the same page wherever
//! it's found, and a tape's by what it is. One that can't be made isn't
//! asked for again.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

use anyhow::Context as _;
use gpui::RenderImage;
use sheaf_doc::FormRef;
use sheaf_render::PageIndex;

/// Width of a picture in pixels: sharp at a stamp's usual size, zoomed in
/// a little.
const WIDTH: u32 = 600;

/// Where a wanted picture's PDF is.
enum Pdf {
    /// The page a mark holds.
    Stamp(FormRef),
    /// These bytes.
    Bytes(Arc<[u8]>),
}

#[derive(Default)]
struct Previews {
    ready: HashMap<String, Arc<RenderImage>>,
    wanted: HashMap<String, Pdf>,
    making: HashSet<String>,
    /// Those that couldn't be made.
    failed: HashSet<String>,
}

impl Previews {
    /// The picture `key` names when it's made; otherwise it's asked for,
    /// its PDF from `pdf` (called only then), unless it couldn't be made.
    fn get(&mut self, key: &str, pdf: impl FnOnce() -> Option<Pdf>) -> Option<Arc<RenderImage>> {
        if let Some(image) = self.ready.get(key) {
            return Some(image.clone());
        }
        let asked = self.making.contains(key) || self.wanted.contains_key(key);
        if !asked && !self.failed.contains(key) {
            let pdf = pdf()?;
            self.wanted.insert(key.to_string(), pdf);
        }
        None
    }
}

static PREVIEWS: LazyLock<Mutex<Previews>> = LazyLock::new(Default::default);

/// [`Previews::get`] of everyone's pictures.
fn get(key: &str, pdf: impl FnOnce() -> Option<Pdf>) -> Option<Arc<RenderImage>> {
    PREVIEWS.lock().ok()?.get(key, pdf)
}

/// The picture of stamp `stem`, whose page a mark holds as `form`; a mark
/// that holds none shows one only if it's made already.
pub fn picture(stem: &str, form: Option<&FormRef>) -> Option<Arc<RenderImage>> {
    get(stem, || Some(Pdf::Stamp(form?.clone())))
}

/// The picture of a library stamp, `stem`, whose page is `pdf`.
pub fn library_picture(stem: &str, pdf: &Arc<[u8]>) -> Option<Arc<RenderImage>> {
    get(stem, || Some(Pdf::Bytes(pdf.clone())))
}

/// The picture of a turned tape (GPUI can't turn text): its appearance as
/// a one-page PDF, rendered like a stamp's.
pub fn tape_picture(a: &sheaf_doc::Annot) -> Option<Arc<RenderImage>> {
    let key = sheaf_util::checksum(
        serde_json::to_string(&(&a.kind, a.color, &a.contents, a.turn))
            .ok()?
            .as_bytes(),
    );
    // Stems have no colon.
    let key = format!("tape:{key:032x}");
    get(&key, || {
        sheaf_doc::annot::tape_pdf(a).map(|pdf| Pdf::Bytes(pdf.into()))
    })
}

/// Make the pictures asked for since last time.
pub fn make_wanted(cx: &mut gpui::App) {
    let wanted: Vec<(String, Pdf)> = {
        let Ok(mut p) = PREVIEWS.lock() else {
            return;
        };
        let w: Vec<(String, Pdf)> = p.wanted.drain().collect();
        p.making.extend(w.iter().map(|(key, _)| key.clone()));
        w
    };
    if wanted.is_empty() {
        return;
    }
    let task = cx.background_executor().spawn(async move {
        wanted
            .into_iter()
            .map(|(key, pdf)| {
                let made = make(pdf);
                if let Err(e) = &made {
                    log::warn!("stamp picture {key}: {e:#}");
                }
                (key, made.ok())
            })
            .collect::<Vec<_>>()
    });
    cx.spawn(async move |cx| {
        let done = task.await;
        if let Ok(mut p) = PREVIEWS.lock() {
            for (key, image) in done {
                p.making.remove(&key);
                match image {
                    Some(image) => _ = p.ready.insert(key, image),
                    None => _ = p.failed.insert(key),
                }
            }
        }
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// Render a picture's page from `pdf`, as GPUI draws it (BGRA).
fn make(pdf: Pdf) -> anyhow::Result<Arc<RenderImage>> {
    let bytes = match pdf {
        Pdf::Stamp(form) => form.to_pdf()?,
        Pdf::Bytes(bytes) => bytes,
    };
    let engine = sheaf_render::pdfium::PdfiumEngine::open_bytes(bytes, None)?;
    let mut r = engine.render_transparent(PageIndex(0), WIDTH)?;
    for px in r.data.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(r.width, r.height, r.data).context("short render")?;
    Ok(Arc::new(RenderImage::new(smallvec::smallvec![
        image::Frame::new(image)
    ])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture is asked for once until it's made; one that couldn't be
    /// made isn't asked for again, and one a mark holds no page for isn't
    /// asked for at all.
    #[test]
    fn a_picture_is_asked_for_once() {
        let mut p = Previews::default();
        let bytes = || Some(Pdf::Bytes(b"not a PDF".as_slice().into()));
        assert!(p.get("s", bytes).is_none());
        assert!(p.get("s", || panic!("asked again")).is_none());
        // Made, and failed.
        assert!(p.wanted.remove("s").is_some());
        p.failed.insert("s".into());
        assert!(p.get("s", || panic!("asked again")).is_none());
        assert!(p.wanted.is_empty());
        assert!(p.get("t", || Some(Pdf::Stamp(None?))).is_none());
        assert!(p.wanted.is_empty());
    }
}
