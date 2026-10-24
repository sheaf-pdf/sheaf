//! Scanned pages into a document, a page at a time (M5 proposal 1): the
//! one pipeline image files and scanners share. Pages come from a stream
//! ([`Pages`]); each is cleaned up, left out when it's blank and that was
//! asked, and recognized when OCR is on, then encoded at once (JPEG for
//! color and gray, CCITT G4 for black and white) and its pixels dropped,
//! so memory stays flat however many pages there are. Pages land in
//! batches ([`Batching`], by default 8 pages or 2 seconds since the last
//! landing) through a [`Landing`]: a Session's ([`SessionLanding`])
//! commits each as the scan job's, in one group, so an intake is one undo
//! step however many batches it took, and undoing it stops it. Every
//! landing makes a batch's pages one source ([`batch_source`]) and puts
//! them in a transaction with their text ([`apply_batch`]), a Session's
//! or the app's. Whatever stops an intake but its landing (a cancel, a
//! page that can't be read, cleaned or recognized), the pages it had ready
//! land first.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use in_progress::{Level, NoProgress, Progress};
use sheaf_doc::images::placement;
use sheaf_doc::store::{BlobKind, MemoryKind, Src, Store};
use sheaf_doc::tx::Made;
use sheaf_doc::{Actor, Box2, CommitOpts, GroupId, GroupKind, JobKind, PAGES, PageId, Tx};
use sheaf_edit::{BlobSel, Bring, Fit, Op, PageRef, PageSource};
use sheaf_image::clean::Mode;
use sheaf_image::io::{Frames, PageImage};
use sheaf_ocr::{OcrEngine, OcrImage, OcrWord};

use crate::scan::{ScanOptions, ScanReport, clean_image, is_blank_page};
use crate::session::Session;

/// Color and gray pages are stored as JPEG at this quality.
const JPEG_QUALITY: u8 = 80;

/// A page as it came in, before cleanup: its pixels, and what messages
/// call it ("scan.png", "fax.tif, page 2").
pub struct RawPage {
    pub image: image::RgbImage,
    pub name: String,
}

/// Where an intake's pages come from, one at a time.
pub trait Pages {
    /// The next page, or None when there are no more. `p` is the
    /// intake's, for what the stream has to say.
    fn next(&mut self, p: &dyn Progress) -> Result<Option<RawPage>>;
    /// How many pages the stream has in all, when it knows.
    fn count(&self) -> Option<usize>;
}

/// Image files' pages, in order: a TIFF's one by one, read from the file
/// as they're asked for ([`sheaf_image::io::frames`]).
pub struct Files {
    paths: Vec<PathBuf>,
    /// The pages in all, a TIFF's counted from its headers.
    pages: usize,
    /// How many files were opened.
    opened: usize,
    /// The file being read, and how many of its pages it gave.
    reading: Option<(Frames, usize)>,
}

impl Files {
    /// `paths`' pages; each file is opened to count them, and one that
    /// can't be is an error before any page is read.
    pub fn new(paths: &[PathBuf]) -> Result<Files> {
        if paths.is_empty() {
            anyhow::bail!("no images given");
        }
        let mut pages = 0;
        for path in paths {
            pages += sheaf_image::io::count_frames(path)?;
        }
        Ok(Files {
            paths: paths.to_vec(),
            pages,
            opened: 0,
            reading: None,
        })
    }
}

impl Pages for Files {
    fn next(&mut self, _: &dyn Progress) -> Result<Option<RawPage>> {
        loop {
            if let Some((frames, given)) = &mut self.reading
                && let Some(frame) = frames.next()
            {
                *given += 1;
                let path = self.paths[self.opened - 1].display();
                let name = match *given {
                    1 => path.to_string(),
                    k => format!("{path}, page {k}"),
                };
                return Ok(Some(RawPage {
                    image: frame?,
                    name,
                }));
            }
            let Some(path) = self.paths.get(self.opened) else {
                self.reading = None;
                return Ok(None);
            };
            self.reading = Some((sheaf_image::io::frames(path)?, 0));
            self.opened += 1;
        }
    }

    fn count(&self) -> Option<usize> {
        Some(self.pages)
    }
}

/// When a batch of pages lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Batching {
    /// A batch lands once it holds this many pages.
    pub pages: usize,
    /// Or once this long has passed since the last landing (the intake's
    /// start, for the first), checked as each page is done: a slow
    /// scanner's pages show up as they come.
    pub wait: Option<Duration>,
}

impl Batching {
    /// 8 pages, or 2 seconds.
    pub const DEFAULT: Batching = Batching {
        pages: 8,
        wait: Some(Duration::from_secs(2)),
    };

    /// Whether a batch of `pages` lands, `since_last` after the last
    /// landing. An empty one never does.
    pub fn due(&self, pages: usize, since_last: Duration) -> bool {
        pages > 0 && (pages >= self.pages || self.wait.is_some_and(|w| since_last >= w))
    }
}

/// Pages ready to land: encoded, each as `fit` places it, with its words
/// in points on its page when OCR ran, as `engine` (its name and version)
/// read them.
pub struct Batch {
    pub pages: Vec<PageImage>,
    pub words: Option<Vec<Vec<sheaf_doc::OcrWord>>>,
    pub fit: Fit,
    pub engine: String,
}

/// Where an intake's batches go.
pub trait Landing {
    /// `batch` put into the document, or why not. An error stops the
    /// intake before it reads another page.
    fn land(&mut self, batch: Batch, p: &dyn Progress) -> Result<()>;
}

/// A Session's landing: each batch inserted at the end with its text, in
/// one commit of the scan job's, in one group, labelled `label`. Once the
/// group's edits are undone, a batch is
/// [`EditError::GroupEnded`](sheaf_doc::EditError::GroupEnded), and the
/// intake stops.
pub struct SessionLanding<'a> {
    session: &'a mut Session,
    group: GroupId,
    label: Arc<str>,
}

impl<'a> SessionLanding<'a> {
    /// The group begun, which takes the batches until [`finish`](Self::finish).
    pub fn new(session: &'a mut Session, label: &str) -> SessionLanding<'a> {
        let group = session.doc.begin_group(GroupKind::Scan);
        SessionLanding {
            session,
            group,
            label: label.into(),
        }
    }

    /// The document the batches land in.
    pub fn session(&mut self) -> &mut Session {
        self.session
    }

    /// The intake is over: its group takes no more commits.
    pub fn finish(self) {
        self.session.doc.end_group(self.group);
    }
}

impl Landing for SessionLanding<'_> {
    fn land(&mut self, batch: Batch, p: &dyn Progress) -> Result<()> {
        let src = batch_source(self.session.doc.store(), &batch.pages, batch.fit)?;
        self.session.hold(src.blob().clone());
        let mut tx = self.session.tx(Actor::Job(JobKind::Scan));
        apply_batch(&mut tx, &src, batch.words, &batch.engine, p)?;
        let opts = CommitOpts {
            group: Some(self.group),
            ..CommitOpts::labelled(self.label.clone())
        };
        self.session.doc.commit(tx, opts)?;
        Ok(())
    }
}

/// `pages`' pages, cleaned up as `opts` says, recognized by `engine` when
/// there is one, landed in `landing` as `batching` says: a page at a time,
/// each encoded once it's cleaned and its pixels dropped. A landing that
/// fails stops it before another page is read. Anything else that stops it
/// (a cancel; a page that can't be read, cleaned or recognized) lands the
/// pages already made first, so they're kept, and returns its own error
/// (the first, if that landing fails too); that landing reports to no
/// progress, so a cancel doesn't stop it.
pub fn intake(
    pages: &mut dyn Pages,
    opts: &ScanOptions,
    mut engine: Option<&mut dyn OcrEngine>,
    landing: &mut dyn Landing,
    batching: Batching,
    p: &dyn Progress,
) -> Result<ScanReport> {
    let params = opts.params();
    let fit = opts.page.fit(0.0);
    let engine_name = (engine.as_ref())
        .map(|e| e.describe().name)
        .unwrap_or_default();
    let ocr = engine.is_some();
    let empty = || Batch {
        pages: Vec::new(),
        words: ocr.then(Vec::new),
        fit,
        engine: engine_name.clone(),
    };
    let mut batch = empty();
    let mut report = ScanReport::default();
    let share = pages.count().map_or(0.0, |n| 1.0 / n.max(1) as f64);
    p.set_total(pages.count().map(|n| n as u64), PAGES);
    let mut landed = Instant::now();
    // Every error but a landing's leaves `batch` holding what's ready.
    let mut read = || -> Result<()> {
        for k in 1.. {
            p.check()?;
            let Some(RawPage { image, name }) = pages.next(p)? else {
                break;
            };
            let step = p.child(&format!("page {k}"), share);
            let cleaned = clean_image(
                image,
                &params,
                &*step.child("cleaning", 0.6),
                &mut |_, _| {},
            )
            .with_context(|| format!("cleaning up {name}"))?;
            if cleaned.quad.is_some() {
                report.cropped += 1;
            }
            if cleaned.turns != 0 {
                step.message(
                    Level::Info,
                    &format!("{name}: turned {}°", cleaned.turns * 90),
                );
            }
            if opts.drop_blanks && is_blank_page(&cleaned.image) {
                step.message(Level::Info, &format!("{name}: blank, left out"));
                report.dropped += 1;
            } else {
                let found = match engine.as_deref_mut() {
                    Some(engine) => {
                        step.check()?;
                        let img = match engine.input() {
                            sheaf_ocr::InputKind::Gray => OcrImage::Gray(cleaned.image.to_luma8()),
                            sheaf_ocr::InputKind::Rgb => OcrImage::Rgb(cleaned.image.to_rgb8()),
                        };
                        let found = (engine.recognize(img, opts.dpi))
                            .with_context(|| format!("recognizing {name}"))?;
                        report.words += found.len();
                        Some(found)
                    }
                    None => None,
                };
                let page = match opts.clean.mode {
                    // Black and white goes in as CCITT G4.
                    Mode::Bw => PageImage::from_image(cleaned.image, opts.dpi as f32),
                    _ => PageImage::jpeg(&cleaned.image, opts.dpi as f32, JPEG_QUALITY)?,
                };
                if let (Some(words), Some(found)) = (&mut batch.words, found) {
                    words.push(to_page(&found, &page, fit));
                }
                batch.pages.push(page);
                report.pages += 1;
            }
            p.set(k);
            if batching.due(batch.pages.len(), landed.elapsed()) {
                let full = std::mem::replace(&mut batch, empty());
                landing.land(full, &*p.child("landing", 0.0))?;
                landed = Instant::now();
            }
        }
        Ok(())
    };
    let read = read();
    let rest = if batch.pages.is_empty() {
        Ok(())
    } else if read.is_ok() {
        landing.land(batch, &*p.child("landing", 0.0))
    } else {
        landing.land(batch, &NoProgress)
    };
    read.and(rest)?;
    Ok(report)
}

/// `pages` made one PDF in memory ([`sheaf_doc::images::to_pdf`], each as
/// `fit` says), interned in `store` and parsed: a [`MemoryKind::Images`]
/// source, which whoever lands it holds until its insert is committed. The
/// pages are made from images already read, decoded and cleaned, so they
/// go in as they're made, rather than as image files the insert reads
/// again.
pub fn batch_source(store: &Store, pages: &[PageImage], fit: Fit) -> Result<Src> {
    let bytes = sheaf_doc::images::to_pdf(pages, fit)?;
    Ok(store.open(bytes.into(), BlobKind::memory(MemoryKind::Images))?)
}

/// A batch put into `tx`: every page of `src`, its source
/// ([`batch_source`]), inserted at the end, then, when OCR ran, each given
/// its `words`, in points on its page, as `engine` (its name and version)
/// read them. Words that aren't a list for each page it made are an
/// error.
pub fn apply_batch(
    tx: &mut Tx,
    src: &Src,
    words: Option<Vec<Vec<sheaf_doc::OcrWord>>>,
    engine: &str,
    p: &dyn Progress,
) -> Result<()> {
    let made = tx.apply(insert_at_end(src, "scan"), p)?.made;
    let Some(words) = words else {
        return Ok(());
    };
    let pages: Vec<PageId> = (made.into_iter())
        .filter_map(|m| match m {
            Made::Page(id) => Some(id),
            _ => None,
        })
        .collect();
    anyhow::ensure!(
        pages.len() == words.len(),
        "a batch of {} images made {} pages",
        words.len(),
        pages.len()
    );
    // As the app's OCR gives a page its text.
    for (id, words) in pages.into_iter().zip(words) {
        let geom = (tx.state().geom(id).cloned()).context("an inserted page's boxes")?;
        let layer = sheaf_doc::text_layer::from_engine(words, &geom, engine);
        let set = Op::SetOcrText {
            page: PageRef::Id(id),
            layer: Arc::new(layer),
        };
        tx.apply(set, p)?;
    }
    Ok(())
}

/// The insert of all of `src`'s pages at the end, under `name`.
pub(crate) fn insert_at_end(src: &Src, name: &str) -> Op {
    Op::InsertPages {
        from: PageSource::File {
            file: BlobSel::Id(src.id()),
            name: name.to_string(),
        },
        pages: None,
        before: None,
        bring: Bring::default(),
        locals: Vec::new(),
    }
}

/// An engine's words on a cleaned image, in points on its page.
fn to_page(found: &[OcrWord], page: &PageImage, fit: Fit) -> Vec<sheaf_doc::OcrWord> {
    let place = placement(page, fit).image;
    let sx = place.w / page.width as f32;
    let sy = place.h / page.height as f32;
    found
        .iter()
        .map(|w| {
            let [l, t, r, b] = w.bbox_px;
            sheaf_doc::OcrWord {
                text: w.text.clone(),
                rect: Box2 {
                    x: place.x + l.min(r) * sx,
                    y: place.y + t.min(b) * sy,
                    w: (r - l).abs() * sx,
                    h: (b - t).abs() * sy,
                },
                confidence: w.confidence,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A batch lands at its count of pages, or once the wait has passed
    /// with a page in it; an empty one never does, and with no wait only
    /// the count lands one.
    #[test]
    fn a_batch_is_due_at_its_count_or_after_its_wait() {
        let b = Batching::DEFAULT;
        let (none, short, long) = (
            Duration::ZERO,
            Duration::from_millis(1999),
            Duration::from_secs(2),
        );
        assert!(!b.due(0, none) && !b.due(0, long), "nothing to land");
        assert!(!b.due(7, short), "under the count, inside the wait");
        assert!(b.due(8, none) && b.due(9, none), "at the count");
        assert!(b.due(1, long), "one page, the wait passed");
        let counted = Batching {
            pages: 3,
            wait: None,
        };
        assert!(!counted.due(2, Duration::from_secs(3600)));
        assert!(counted.due(3, none));
    }
}
