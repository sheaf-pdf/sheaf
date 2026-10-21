//! Rasterization.
//!
//! Two layers:
//! - [`Renderer`]: a synchronous engine interface. [`pdfium::PdfiumEngine`] is
//!   the only implementation for now.
//! - [`EnginePool`]: one worker thread that owns every engine it's asked
//!   for, opening each when a request first needs it and closing them by
//!   its budgets (M10.5 §10.3), so the UI thread never blocks on PDFium.
//!   Replies come back on `oneshot` channels the caller can `.await`.
//!
//! PDFium is process-global and not thread-safe, so every use of it holds one
//! lock (`pdfium::lock`): the pool's worker and the threads that open an
//! engine of their own (a print, an export, a scan for blank pages) take
//! turns, a call into an engine at a time.

pub mod pool;

pub(crate) use pool::Segments;
pub use pool::{Budget, EnginePool, Fallback, Opened, Opener, Owner, Req, Target};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use sheaf_edit::{PageSize, Rotation};
use sheaf_image::raster::{PixelFormat, Raster};

/// Zero-based index of a page within a *source file* (what the engine sees).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageIndex(pub u32);

/// What to rasterize. Sizes are device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Job {
    /// The whole page, scaled to `width` wide (after `rotation`).
    Page { width: u32, rotation: Rotation },
    /// The `w`×`h` region at (`x`, `y`) of the page scaled to
    /// `page_width`×`page_height` (after `rotation`). Lets a zoomed-in page
    /// be rendered a viewport's worth at a time instead of as one huge bitmap.
    Tile {
        page_width: u32,
        page_height: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        rotation: Rotation,
    },
}

impl Job {
    pub fn rotation(&self) -> Rotation {
        match self {
            Job::Page { rotation, .. } | Job::Tile { rotation, .. } => *rotation,
        }
    }

    /// The job turned a further `by`: what the page's engine has, turned
    /// as the page is (M10.5 §5's render delta), then as its pane is.
    pub fn turned(self, by: Rotation) -> Job {
        match self {
            Job::Page { width, rotation } => Job::Page {
                width,
                rotation: rotation.plus(by),
            },
            Job::Tile {
                page_width,
                page_height,
                x,
                y,
                w,
                h,
                rotation,
            } => Job::Tile {
                page_width,
                page_height,
                x,
                y,
                w,
                h,
                rotation: rotation.plus(by),
            },
        }
    }
}

/// What a render draws beside the page's content (M10.5 §10.4): others'
/// annotations (PDFium's `FPDF_ANNOT`), and the form's widgets, which
/// PDFium draws only through `FPDF_FFLDraw`. With `annots` off and `forms`
/// on, a page draws its widgets alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Draw {
    pub annots: bool,
    pub forms: bool,
}

impl Draw {
    /// Everything: what [`Renderer::render`] draws.
    pub const ALL: Draw = Draw {
        annots: true,
        forms: true,
    };
}

/// Axis-aligned rectangle in page points, top-left origin, in the page's
/// *displayed* orientation (i.e. after /Rotate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A page's characters, each with its box (see [`Renderer::page_chars`]).
pub type PageCharList = Vec<(char, Option<Rect>)>;

/// One search match: the page it's on and the glyph boxes that make it up
/// (usually one, more if the match wraps a line).
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub page: u32,
    pub rects: Vec<Rect>,
    /// The matched text.
    pub text: String,
    /// A pattern's capture groups 1…n (None where a group didn't take
    /// part); empty for a literal search.
    pub groups: Vec<Option<String>>,
}

/// What kind of form a document has, as PDFium tells (M10 proposal 19).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FormType {
    /// No `/AcroForm`.
    #[default]
    None,
    /// An AcroForm with no XFA.
    AcroForm,
    /// XFA beside the AcroForm (a hybrid form, or a static XFA one).
    XfaForeground,
    /// XFA that is the form: `/NeedsRendering`. The pages are a
    /// placeholder.
    XfaFull,
}

/// Progress of a document-wide search, streamed as pages are scanned.
#[derive(Debug)]
pub enum SearchEvent {
    /// Hits found on one page (only sent when non-empty).
    Hits(Vec<SearchHit>),
    /// Pages scanned so far.
    Progress(u32),
    Done,
}

/// Synchronous engine interface. Implementations may block.
pub trait Renderer {
    fn page_count(&self) -> u32;
    /// The document's form, as the engine classes it.
    fn form_type(&self) -> FormType;
    fn page_size(&self, page: PageIndex) -> Result<PageSize>;
    /// All matches of `query` on one page.
    fn search_page(&self, page: PageIndex, query: &str, match_case: bool)
    -> Result<Vec<SearchHit>>;
    /// All matches of a regular expression on one page, run over the
    /// page's extracted text.
    fn pattern_search_page(&self, page: PageIndex, re: &regex::Regex) -> Result<Vec<SearchHit>>;
    /// Glyph boxes (merged per line) of text intersecting `rect`, in the
    /// page's displayed space. Used to turn a drag into a text highlight.
    fn text_boxes_in_rect(&self, page: PageIndex, rect: Rect) -> Result<Vec<Rect>>;
    /// How many real characters of text the page has: glyphs the engine
    /// synthesizes (spaces, line breaks) and whitespace don't count. Zero
    /// means an image-only page, an OCR candidate.
    fn page_text_len(&self, page: PageIndex) -> Result<usize>;
    /// The page's text in reading order, as the engine extracts it.
    fn page_text(&self, page: PageIndex) -> Result<String>;
    /// The page's characters in the engine's order, each with its box (in
    /// the displayed space); `None` for the spaces and line breaks the
    /// engine adds between words and lines.
    fn page_chars(&self, page: PageIndex) -> Result<PageCharList>;
    /// Render a whole page scaled so its width is `target_width` device pixels.
    fn render(&self, page: PageIndex, job: Job, format: PixelFormat) -> Result<Raster>;

    /// Render a whole page scaled so its width is `target_width` device pixels.
    fn render_page(
        &self,
        page: PageIndex,
        target_width: u32,
        format: PixelFormat,
    ) -> Result<Raster> {
        self.render(
            page,
            Job::Page {
                width: target_width,
                rotation: Rotation::R0,
            },
            format,
        )
    }
}

/// Why a file couldn't be opened, when the app can do something about it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// The file is encrypted and the password given (if any) is wrong.
    #[error("the file is password protected")]
    Password,
    /// PDFium can't read the file, whatever the password: a format,
    /// security or file error from its load (named), which the same bytes
    /// give again. sheaf may still read it (M10.5 slice 13: its pages are
    /// then drawn from sheaf's copy).
    #[error("PDFium can't read the file ({0})")]
    Refused(String),
}

impl OpenError {
    /// Is this error, anywhere in its chain, a password problem?
    pub fn is_password(err: &anyhow::Error) -> bool {
        err.chain()
            .any(|c| matches!(c.downcast_ref::<OpenError>(), Some(OpenError::Password)))
    }

    /// Is this error, anywhere in its chain, PDFium refusing the file?
    pub fn is_refused(err: &anyhow::Error) -> bool {
        err.chain()
            .any(|c| matches!(c.downcast_ref::<OpenError>(), Some(OpenError::Refused(_))))
    }
}

pub mod pdfium {
    use super::*;
    use parking_lot::{FairMutex, FairMutexGuard};
    use pdfium_render::prelude::*;
    use std::mem::ManuallyDrop;
    use std::sync::OnceLock;

    #[cfg(test)]
    mod bookmarks;
    #[cfg(test)]
    mod encrypted;
    #[cfg(test)]
    mod frames;

    /// Held across every use of PDFium, from a load to the document's
    /// drop: a call into an engine, such as one page of a walk over the
    /// whole document's page sizes. pdfium-render's `thread_safe` feature isn't
    /// enough: 185 of the 467 functions it binds (0.9.4) take no lock,
    /// among them `FPDFDOC_InitFormFillEnvironment` (every load makes it)
    /// and the page size, text and bitmap calls. PDFium's parser keeps
    /// its depth in a process global
    /// (`CPDF_SyntaxParser::s_CurrentRecursionDepth`) that each object
    /// parse saves, adds one to and puts back, so parses on two threads
    /// at once leave it too high. Once it nears its limit (64), objects
    /// stop parsing, and every load after fails with `FormatError`, from
    /// any thread (M10 slice 1's note).
    ///
    /// Fair: a thread that unlocks and locks again at once, as a walk or
    /// a search does, goes behind the threads already waiting. (With
    /// std's mutex it kept winning, and a page-size walk beside another
    /// document's renders took minutes.) Not re-entrant: never call into
    /// an engine, or lock again, while holding it.
    fn lock() -> FairMutexGuard<'static, ()> {
        static LOCK: FairMutex<()> = parking_lot::const_fair_mutex(());
        LOCK.lock()
    }

    /// Locate and bind libpdfium. Search order: `$PDFIUM_LIB_DIR`, next to
    /// the executable (and its `lib/`, how packaged builds ship it),
    /// `vendor/pdfium/<os>-<arch>/` relative to the cwd (and to this
    /// source tree, for `cargo test` run from a crate directory), then the
    /// system library.
    fn bind() -> Result<Pdfium> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(d) = std::env::var("PDFIUM_LIB_DIR") {
            dirs.push(d.into());
        }
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        {
            dirs.push(exe_dir.join("lib"));
            dirs.push(exe_dir);
        }
        let vendored = format!(
            "vendor/pdfium/{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        dirs.push(vendored.clone().into());
        dirs.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(vendored),
        );

        let mut last_err = None;
        for d in &dirs {
            match Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(d)) {
                Ok(b) => return Ok(Pdfium::new(b)),
                Err(e) => last_err = Some(e),
            }
        }
        Pdfium::bind_to_system_library().map(Pdfium::new).with_context(|| {
            format!(
                "no libpdfium found (searched {dirs:?} and system); run scripts/fetch-pdfium.sh. last error: {last_err:?}"
            )
        })
    }

    /// The process-wide PDFium instance. PDFium itself is a global library
    /// (`FPDF_InitLibrary` once per process), so a `'static` handle is the
    /// honest model and sidesteps the `PdfDocument<'a>` borrow. Use it
    /// only under [`lock`].
    fn pdfium() -> Result<&'static Pdfium> {
        static CELL: OnceLock<std::result::Result<Pdfium, String>> = OnceLock::new();
        match CELL.get_or_init(|| bind().map_err(|e| format!("{e:#}"))) {
            Ok(p) => Ok(p),
            Err(e) => anyhow::bail!("{e}"),
        }
    }

    pub struct PdfiumEngine {
        // Dropped under the lock, before `_bytes`: PDFium reads the buffer
        // for as long as the document is open. A document read from
        // segments holds them itself, in its reader.
        doc: ManuallyDrop<PdfDocument<'static>>,
        _bytes: Option<Arc<[u8]>>,
    }

    impl Drop for PdfiumEngine {
        fn drop(&mut self) {
            let _pdfium = lock();
            // SAFETY: `doc` is dropped here only, and not used after.
            unsafe { ManuallyDrop::drop(&mut self.doc) }
        }
    }

    impl PdfiumEngine {
        pub fn open(path: impl AsRef<Path>, password: Option<&str>) -> Result<Self> {
            let path = path.as_ref();
            // Read the whole file up front. PDFium otherwise pulls objects
            // lazily with many small random reads, which is fine on local
            // SSDs but disastrous on network/9P mounts (3.4 s vs 0.27 s to
            // enumerate 7k page sizes in one measurement). One sequential read
            // of even a few hundred MB is cheap by comparison, and it's what a
            // viewer wants anyway. Truly huge files can revisit this.
            let bytes = sheaf_util::read_file(path)
                .with_context(|| format!("reading {}", path.display()))?;
            Self::open_bytes(bytes, password).with_context(|| format!("opening {}", path.display()))
        }

        /// Open a file already in memory, sharing the buffer: the document
        /// model keeps the same bytes, so saves read what PDFium shows.
        pub fn open_bytes(bytes: Arc<[u8]>, password: Option<&str>) -> Result<Self> {
            // SAFETY: the slice points into `bytes`, which the engine owns
            // and never changes, and `doc` (which borrows it) is dropped
            // before it (in `drop`).
            let slice: &'static [u8] =
                unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
            let _pdfium = lock();
            let doc = loaded(pdfium()?.load_pdf_from_byte_slice(slice, password))?;
            Ok(Self {
                doc: ManuallyDrop::new(doc),
                _bytes: Some(bytes),
            })
        }

        /// Open a file read from `segments` one after another (M10.5
        /// §10.1): a source's bytes and a section after them, read where
        /// they are, with no copy of the file. The engine holds them.
        pub fn open_segments(segments: Vec<Arc<[u8]>>, password: Option<&str>) -> Result<Self> {
            let reader = crate::Segments::new(segments);
            let _pdfium = lock();
            let doc = loaded(pdfium()?.load_pdf_from_reader(reader, password))?;
            Ok(Self {
                doc: ManuallyDrop::new(doc),
                _bytes: None,
            })
        }

        /// Page `page` drawn as [`Renderer::render`] draws it, with only
        /// what `draw` asks for beside its content.
        pub fn render_with(
            &self,
            page: PageIndex,
            job: Job,
            format: PixelFormat,
            draw: Draw,
        ) -> Result<Raster> {
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            // The page scaled to `full` and turned, and the part of it
            // drawn: `size` at `origin`.
            let (full, origin, size) = match job {
                Job::Page { width, rotation } => {
                    // Output dims are post-rotation; PDFium's size_x/size_y
                    // are the rotated frame, so fixed size + rotate is
                    // consistent.
                    let shown = PageSize {
                        width: p.width().value,
                        height: p.height().value,
                    }
                    .rotated(rotation);
                    let w = width.max(1);
                    let h = ((w as f32) * shown.height / shown.width.max(1.0))
                        .round()
                        .max(1.0) as u32;
                    ((w, h), (0, 0), (w, h))
                }
                Job::Tile {
                    page_width,
                    page_height,
                    x,
                    y,
                    w,
                    h,
                    ..
                } => (
                    (page_width.max(1), page_height.max(1)),
                    (x, y),
                    (w.max(1), h.max(1)),
                ),
            };
            let mut bmp = PdfBitmap::empty(size.0 as i32, size.1 as i32, PdfBitmapFormat::BGRA)?;
            let cfg = PdfRenderConfig::new()
                .set_format(PdfBitmapFormat::BGRA)
                // pdfium-render asks PDFium for RGBA order by default;
                // keep the bitmap's own BGRA.
                .set_reverse_byte_order(false)
                .render_annotations(draw.annots);
            let cfg = match draw.forms {
                // Scale the page to its full size but draw into a
                // bitmap of the part's size, offset so only the part
                // lands. Unlike a matrix render, this keeps form-field
                // drawing (FPDF_FFLDraw) working.
                true => cfg
                    .set_fixed_size(full.0 as i32, full.1 as i32)
                    .set_origin(-(origin.0 as i32), -(origin.1 as i32))
                    .rotate(turn(job.rotation()), false)
                    .render_form_data(true),
                // A render with a matrix draws no widgets: one that places
                // the page as the other does, pixel for pixel.
                false => cfg
                    .set_fixed_size(size.0 as i32, size.1 as i32)
                    .render_form_data(false)
                    .apply_matrix(placed(&p, full, origin, job.rotation()))?,
            };
            p.render_into_bitmap_with_config(&mut bmp, &cfg)?;
            let data = match format {
                PixelFormat::Bgra => bmp.as_raw_bytes(),
                PixelFormat::Rgba => bmp.as_rgba_bytes(),
            };
            Ok(Raster {
                width: bmp.width() as u32,
                height: bmp.height() as u32,
                format,
                data,
            })
        }

        /// Page `page` drawn as [`render_appearances`] draws it. It
        /// flattens the page in this engine's document, which draws it
        /// flattened from then on: for an engine of the caller's own, never
        /// one an [`EnginePool`] serves.
        pub fn render_appearances(&self, page: PageIndex, width: u32) -> Result<Raster> {
            let _pdfium = lock();
            appearances(&self.doc, page, width)
        }

        /// A page `width` pixels wide, RGBA, transparent where nothing is
        /// drawn: a stamp's picture, to lay over another page.
        pub fn render_transparent(&self, page: PageIndex, width: u32) -> Result<Raster> {
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            let w = width.max(1);
            let h = ((w as f32) * p.height().value / p.width().value.max(1.0))
                .round()
                .max(1.0) as u32;
            let cfg = PdfRenderConfig::new()
                .set_fixed_size(w as i32, h as i32)
                .set_format(PdfBitmapFormat::BGRA)
                .set_reverse_byte_order(false)
                .set_clear_color(PdfColor::new(255, 255, 255, 0));
            let bmp = p.render_with_config(&cfg)?;
            Ok(Raster {
                width: bmp.width() as u32,
                height: bmp.height() as u32,
                format: PixelFormat::Rgba,
                data: bmp.as_rgba_bytes(),
            })
        }
    }

    /// Page `page` of the file in `bytes`, `width` pixels wide, RGBA, as a
    /// viewer that draws only appearance streams shows it: each
    /// annotation's normal appearance (the state its `/AS` names), and
    /// nothing made from a field's value or `NeedAppearances`. The page
    /// view draws widgets with `FPDF_FFLDraw`, which makes an appearance
    /// from `/V` where there's none (M10 bug 10), so tests that check what
    /// other viewers show use this.
    ///
    /// PDFium's own flatten draws the appearances into a private copy of
    /// the page. (With form drawing just turned off, PDFium draws no
    /// widgets at all.) Where a check box or radio button has no `/AS`,
    /// the flatten would draw its first state; here it draws none (poppler
    /// draws its Off state, pdf.js the state of the field's value).
    pub fn render_appearances(
        bytes: &[u8],
        password: Option<&str>,
        page: PageIndex,
        width: u32,
    ) -> Result<Raster> {
        let _pdfium = lock();
        let doc = pdfium()?
            .load_pdf_from_byte_slice(bytes, password)
            .context("pdfium loading")?;
        appearances(&doc, page, width)
    }

    /// [`render_appearances`]'s work, on `doc`.
    fn appearances(doc: &PdfDocument, page: PageIndex, width: u32) -> Result<Raster> {
        let p = flattened(doc, page)?;
        let w = width.max(1);
        let h = ((w as f32) * p.height().value / p.width().value.max(1.0))
            .round()
            .max(1.0) as u32;
        let cfg = PdfRenderConfig::new()
            .set_fixed_size(w as i32, h as i32)
            .set_format(PdfBitmapFormat::BGRA)
            .set_reverse_byte_order(false)
            .render_form_data(false);
        let bmp = p.render_with_config(&cfg)?;
        Ok(Raster {
            width: bmp.width() as u32,
            height: bmp.height() as u32,
            format: PixelFormat::Rgba,
            data: bmp.as_rgba_bytes(),
        })
    }

    /// Page `page`'s text as a viewer that draws only appearance streams
    /// has it: the text PDFium reads from the page flattened as
    /// [`render_appearances`] flattens it, so an annotation's text is
    /// read from the codes and fonts its appearance draws with.
    pub fn appearance_text(
        bytes: &[u8],
        password: Option<&str>,
        page: PageIndex,
    ) -> Result<String> {
        let _pdfium = lock();
        let doc = pdfium()?
            .load_pdf_from_byte_slice(bytes, password)
            .context("pdfium loading")?;
        Ok(flattened(&doc, page)?.text()?.all())
    }

    /// Page `page` of `doc` with the appearances a viewer shows drawn into
    /// it (see [`render_appearances`]), in the box it showed before. The
    /// flatten writes `/MediaBox` and `/CropBox` from the page's own keys
    /// alone, so a page that inherits them comes out Letter (M10.5 slice
    /// 2): the box PDFium showed, its inherited keys read, is put back. And
    /// pdfium-render 0.9.4's flatten loads the page again under a new
    /// handle but leaves its boxes on the one it closed, so the page is
    /// loaded afresh before any box is touched.
    fn flattened<'a>(doc: &PdfDocument<'a>, page: PageIndex) -> Result<PdfPage<'a>> {
        let mut p = doc.pages().get(page.0 as i32)?;
        let visible = p.boundaries().bounding()?.bounds;
        // pdfium-render flattens what would print; make that what shows.
        for mut a in p.annotations_mut().iter() {
            let no_state = a.as_form_field().is_some_and(|f| {
                let button = matches!(
                    f.field_type(),
                    PdfFormFieldType::Checkbox | PdfFormFieldType::RadioButton
                );
                button && f.appearance_stream().is_none()
            });
            let shown = !a.is_hidden() && !a.is_printable_but_not_viewable() && !no_state;
            a.set_is_printed(shown)?;
        }
        p.flatten()?;
        drop(p);
        let mut p = doc.pages().get(page.0 as i32)?;
        let boxes = p.boundaries_mut();
        boxes.set_media(visible)?;
        boxes.set_crop(visible)?;
        Ok(p)
    }

    /// A load's document, or why it failed: [`OpenError::Password`] for a
    /// wrong password or none, [`OpenError::Refused`] for any other error
    /// PDFium's load gives.
    fn loaded<'a>(
        doc: std::result::Result<PdfDocument<'a>, PdfiumError>,
    ) -> Result<PdfDocument<'a>> {
        match doc {
            Ok(d) => Ok(d),
            Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => {
                Err(crate::OpenError::Password.into())
            }
            Err(PdfiumError::PdfiumLibraryInternalError(e)) => {
                Err(crate::OpenError::Refused(format!("{e:?}")).into())
            }
            Err(e) => Err(e).context("pdfium loading"),
        }
    }

    fn turn(rotation: Rotation) -> PdfPageRenderRotation {
        match rotation {
            Rotation::R0 => PdfPageRenderRotation::None,
            Rotation::R90 => PdfPageRenderRotation::Degrees90,
            Rotation::R180 => PdfPageRenderRotation::Degrees180,
            Rotation::R270 => PdfPageRenderRotation::Degrees270,
        }
    }

    /// The matrix that places page `p` as a render without a matrix does:
    /// scaled to `full` pixels once turned by `rotation`, the part at
    /// `origin` at the bitmap's corner. PDFium applies it to the page's
    /// display space, the page as shown (its `/Rotate` applied) at a pixel
    /// a point, y down.
    fn placed(p: &PdfPage, full: (u32, u32), origin: (u32, u32), rotation: Rotation) -> PdfMatrix {
        let (w, h) = (p.width().value, p.height().value);
        let (turned_w, turned_h) = if rotation.is_quarter() {
            (h, w)
        } else {
            (w, h)
        };
        let sx = full.0 as f32 / turned_w.max(1.0);
        let sy = full.1 as f32 / turned_h.max(1.0);
        let (ox, oy) = (origin.0 as f32, origin.1 as f32);
        // x' = a x + c y + e, y' = b x + d y + f.
        let [a, b, c, d, e, f] = match rotation {
            Rotation::R0 => [sx, 0.0, 0.0, sy, -ox, -oy],
            Rotation::R90 => [0.0, sy, -sx, 0.0, sx * h - ox, -oy],
            Rotation::R180 => [-sx, 0.0, 0.0, -sy, sx * w - ox, sy * h - oy],
            Rotation::R270 => [0.0, -sy, sx, 0.0, -ox, sy * w - oy],
        };
        PdfMatrix::new(a, b, c, d, e, f)
    }

    impl Renderer for PdfiumEngine {
        fn page_count(&self) -> u32 {
            let _pdfium = lock();
            self.doc.pages().len() as u32
        }

        fn form_type(&self) -> FormType {
            let _pdfium = lock();
            match self.doc.form().map(|f| f.form_type()) {
                Some(PdfFormType::Acrobat) => FormType::AcroForm,
                Some(PdfFormType::XfaForeground) => FormType::XfaForeground,
                Some(PdfFormType::XfaFull) => FormType::XfaFull,
                Some(PdfFormType::None) | None => FormType::None,
            }
        }

        fn page_size(&self, page: PageIndex) -> Result<PageSize> {
            let _pdfium = lock();
            let r = self.doc.pages().page_size(page.0 as i32)?;
            Ok(PageSize {
                width: r.width().value,
                height: r.height().value,
            })
        }

        fn search_page(
            &self,
            page: PageIndex,
            query: &str,
            match_case: bool,
        ) -> Result<Vec<SearchHit>> {
            if query.is_empty() {
                return Ok(Vec::new());
            }
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            let text = p.text()?;
            let opts = PdfSearchOptions::new().match_case(match_case);
            let search = text.search(query, &opts)?;

            // Map page-space boxes to the displayed orientation via
            // FPDF_PageToDevice, with a scaled "device" so the integer
            // output keeps sub-point precision.
            const SCALE: f32 = 16.0;
            let (pw, ph) = (p.width().value, p.height().value);
            let cfg =
                PdfRenderConfig::new().set_fixed_size((pw * SCALE) as i32, (ph * SCALE) as i32);
            let to_display = |x: f32, y: f32| -> Option<(f32, f32)> {
                let (dx, dy) = p
                    .points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &cfg)
                    .ok()?;
                Some((dx as f32 / SCALE, dy as f32 / SCALE))
            };

            let mut hits = Vec::new();
            for segments in search.iter(PdfSearchDirection::SearchForward) {
                let mut rects = Vec::new();
                let mut text = String::new();
                for seg in segments.iter() {
                    text.push_str(&seg.text());
                    let b = seg.bounds();
                    let (Some((x0, y0)), Some((x1, y1))) = (
                        to_display(b.left().value, b.bottom().value),
                        to_display(b.right().value, b.top().value),
                    ) else {
                        continue;
                    };
                    rects.push(Rect {
                        x: x0.min(x1),
                        y: y0.min(y1),
                        w: (x1 - x0).abs(),
                        h: (y1 - y0).abs(),
                    });
                }
                if !rects.is_empty() {
                    hits.push(SearchHit {
                        page: page.0,
                        rects,
                        text,
                        groups: Vec::new(),
                    });
                }
            }
            Ok(hits)
        }

        fn page_text_len(&self, page: PageIndex) -> Result<usize> {
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            let text = p.text()?;
            Ok(text
                .chars()
                .iter()
                .filter(|c| !c.is_generated().unwrap_or(false))
                .filter(|c| c.unicode_char().is_some_and(|ch| !ch.is_whitespace()))
                .count())
        }

        fn page_chars(&self, page: PageIndex) -> Result<PageCharList> {
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            let text = p.text()?;
            const SCALE: f32 = 16.0;
            let (pw, ph) = (p.width().value, p.height().value);
            let cfg =
                PdfRenderConfig::new().set_fixed_size((pw * SCALE) as i32, (ph * SCALE) as i32);
            let to_display = |x: f32, y: f32| -> Option<(f32, f32)> {
                let (dx, dy) = p
                    .points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &cfg)
                    .ok()?;
                Some((dx as f32 / SCALE, dy as f32 / SCALE))
            };
            let mut out = Vec::new();
            for c in text.chars().iter() {
                let Some(ch) = c.unicode_char() else { continue };
                let rect = if c.is_generated().unwrap_or(false) {
                    None
                } else {
                    c.loose_bounds().ok().and_then(|b| {
                        let (x0, y0) = to_display(b.left().value, b.bottom().value)?;
                        let (x1, y1) = to_display(b.right().value, b.top().value)?;
                        Some(Rect {
                            x: x0.min(x1),
                            y: y0.min(y1),
                            w: (x1 - x0).abs(),
                            h: (y1 - y0).abs(),
                        })
                    })
                };
                out.push((ch, rect));
            }
            Ok(out)
        }

        fn page_text(&self, page: PageIndex) -> Result<String> {
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            Ok(p.text()?.all())
        }

        fn text_boxes_in_rect(&self, page: PageIndex, rect: Rect) -> Result<Vec<Rect>> {
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            let text = p.text()?;
            const SCALE: f32 = 16.0;
            let (pw, ph) = (p.width().value, p.height().value);
            let cfg =
                PdfRenderConfig::new().set_fixed_size((pw * SCALE) as i32, (ph * SCALE) as i32);
            let to_display = |x: f32, y: f32| -> Option<(f32, f32)> {
                let (dx, dy) = p
                    .points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &cfg)
                    .ok()?;
                Some((dx as f32 / SCALE, dy as f32 / SCALE))
            };
            let (rx0, ry0, rx1, ry1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
            let mut rects: Vec<Rect> = Vec::new();
            for c in text.chars().iter() {
                if c.is_generated().unwrap_or(false) {
                    continue;
                }
                let Ok(b) = c.loose_bounds() else { continue };
                let (Some((x0, y0)), Some((x1, y1))) = (
                    to_display(b.left().value, b.bottom().value),
                    to_display(b.right().value, b.top().value),
                ) else {
                    continue;
                };
                let r = Rect {
                    x: x0.min(x1),
                    y: y0.min(y1),
                    w: (x1 - x0).abs(),
                    h: (y1 - y0).abs(),
                };
                // Keep glyphs whose centre is inside the drag rectangle.
                let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                if cx < rx0 || cx > rx1 || cy < ry0 || cy > ry1 {
                    continue;
                }
                match rects.last_mut() {
                    Some(prev)
                        if (prev.y - r.y).abs() < prev.h.max(r.h) * 0.5 && r.x >= prev.x - 1.0 =>
                    {
                        let right = (prev.x + prev.w).max(r.x + r.w);
                        let bottom = (prev.y + prev.h).max(r.y + r.h);
                        prev.x = prev.x.min(r.x);
                        prev.y = prev.y.min(r.y);
                        prev.w = right - prev.x;
                        prev.h = bottom - prev.y;
                    }
                    _ => rects.push(r),
                }
            }
            Ok(rects)
        }

        fn pattern_search_page(
            &self,
            page: PageIndex,
            re: &regex::Regex,
        ) -> Result<Vec<SearchHit>> {
            let _pdfium = lock();
            let p = self.doc.pages().get(page.0 as i32)?;
            let text = p.text()?;
            let chars = text.chars();
            if chars.is_empty() {
                return Ok(Vec::new());
            }

            // Build the page string and remember each char's byte offset and
            // box, so regex byte ranges map back to glyphs. Generated chars
            // (synthesized spaces/newlines) still count — "\b" needs them —
            // but carry no box so they never draw.
            let mut s = String::with_capacity(chars.len());
            let mut starts: Vec<usize> = Vec::with_capacity(chars.len());
            let mut boxes: Vec<Option<PdfRect>> = Vec::with_capacity(chars.len());
            for c in chars.iter() {
                let ch = c.unicode_char().unwrap_or(' ');
                starts.push(s.len());
                s.push(ch);
                let generated = c.is_generated().unwrap_or(false);
                boxes.push(if generated {
                    None
                } else {
                    c.loose_bounds().ok()
                });
            }

            const SCALE: f32 = 16.0;
            let (pw, ph) = (p.width().value, p.height().value);
            let cfg =
                PdfRenderConfig::new().set_fixed_size((pw * SCALE) as i32, (ph * SCALE) as i32);
            let to_display = |x: f32, y: f32| -> Option<(f32, f32)> {
                let (dx, dy) = p
                    .points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &cfg)
                    .ok()?;
                Some((dx as f32 / SCALE, dy as f32 / SCALE))
            };
            let display_rect = |b: &PdfRect| -> Option<Rect> {
                let (x0, y0) = to_display(b.left().value, b.bottom().value)?;
                let (x1, y1) = to_display(b.right().value, b.top().value)?;
                Some(Rect {
                    x: x0.min(x1),
                    y: y0.min(y1),
                    w: (x1 - x0).abs(),
                    h: (y1 - y0).abs(),
                })
            };

            let mut hits = Vec::new();
            for caps in re.captures_iter(&s) {
                let m = caps.get(0).expect("group 0 always matches");
                if m.is_empty() {
                    continue;
                }
                let first = starts.partition_point(|&b| b < m.start());
                let last = starts.partition_point(|&b| b < m.end());
                // Merge glyph boxes into one rect per line.
                let mut rects: Vec<Rect> = Vec::new();
                for b in boxes[first..last].iter().flatten() {
                    let Some(r) = display_rect(b) else { continue };
                    match rects.last_mut() {
                        Some(prev)
                            if (prev.y - r.y).abs() < prev.h.max(r.h) * 0.5
                                && r.x >= prev.x - 1.0 =>
                        {
                            let right = (prev.x + prev.w).max(r.x + r.w);
                            let bottom = (prev.y + prev.h).max(r.y + r.h);
                            prev.x = prev.x.min(r.x);
                            prev.y = prev.y.min(r.y);
                            prev.w = right - prev.x;
                            prev.h = bottom - prev.y;
                        }
                        _ => rects.push(r),
                    }
                }
                if !rects.is_empty() {
                    hits.push(SearchHit {
                        page: page.0,
                        rects,
                        text: m.as_str().to_string(),
                        groups: caps
                            .iter()
                            .skip(1)
                            .map(|g| g.map(|g| g.as_str().to_string()))
                            .collect(),
                    });
                }
            }
            Ok(hits)
        }

        fn render(&self, page: PageIndex, job: Job, format: PixelFormat) -> Result<Raster> {
            self.render_with(page, job, format, Draw::ALL)
        }
    }
}

/// A render as the pool ran it: the page of its engine, and the job it
/// drew the page by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderRequest {
    pub page: PageIndex,
    pub job: Job,
}

/// A finished render plus how long the engine took.
#[derive(Clone)]
pub struct Rendered {
    pub request: RenderRequest,
    pub raster: Raster,
    pub render_time: std::time::Duration,
}

/// What a document-wide search looks for.
#[derive(Clone)]
pub enum Matcher {
    Literal { query: String, match_case: bool },
    Pattern(std::sync::Arc<regex::Regex>),
}
