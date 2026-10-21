//! Pages that show a generated copy of their source page: cropped, or
//! with the header/footer the file had drawn stripped out. The copies are
//! made in memory and interned into the document's store, and noted with
//! the document as copies of their originals' pages
//! ([`Document::add_derived`]); a transaction's
//! [`set_page_base`](sheaf_doc::Tx::set_page_base) then moves the
//! document's pages onto them, keeping ids, rotation, bookmarks, marks and
//! OCR text.

use anyhow::Result;
use in_progress::Progress;
use sheaf_doc::content::{ContentError, PaginationKind};
use sheaf_doc::geom::Geom;
use sheaf_doc::save::HeaderKept;
use sheaf_doc::store::{BlobKind, MemoryKind, Origin};
use sheaf_doc::{Derived, Document, PAGES, PageId, Snapshot, Src, Store};

/// A generated PDF holding copies of some pages, and the pages it bases
/// the document's on.
pub struct DerivedSource {
    /// The copy, a source in the document's store.
    pub src: Src,
    /// Which pages of which source it holds copies of.
    pub derived: Derived,
    /// (page, its index in the copy).
    pub pages: Vec<(PageId, u32)>,
}

impl DerivedSource {
    /// The copy noted with `doc`, so a save writes its pages as the ones
    /// they were made from.
    pub fn note(&self, doc: &mut Document) {
        doc.add_derived(&self.src, self.derived.clone());
    }

    /// Each page, shown from the copy at its index there, as
    /// [`Tx::set_page_base`](sheaf_doc::Tx::set_page_base) takes them.
    pub fn bases(&self) -> impl Iterator<Item = (PageId, Src, u32)> + '_ {
        (self.pages.iter()).map(|&(id, index)| (id, self.src.clone(), index))
    }
}

/// `bytes`, sheaf's copy of pages `pages` of `from`, as a source in
/// `store`, showing the document's pages `shown` (each page, and its index
/// in the copy).
pub(crate) fn copy_of(
    store: &Store,
    bytes: Vec<u8>,
    from: &Src,
    pages: Vec<u32>,
    shown: Vec<(PageId, u32)>,
) -> Result<DerivedSource> {
    let kind = BlobKind::Pdf {
        password: None,
        origin: Origin::Memory(MemoryKind::Derived),
    };
    Ok(DerivedSource {
        src: store.open(bytes.into(), kind)?,
        derived: Derived {
            from: from.clone(),
            pages,
        },
        pages: shown,
    })
}

/// Whether page `index` of `src` has pagination artifacts of `kinds`
/// that `stamps::strip` removes, by its blocks: an error names `page`, its
/// place in the document.
fn has_kinds(
    src: &Src,
    index: u32,
    page: u32,
    kinds: &[PaginationKind],
) -> Result<bool, ContentError> {
    let blocks = src.blocks(index).map_err(|e| e.on_page(page))?;
    Ok(sheaf_doc::stamps::stripped(&blocks, kinds).next().is_some())
}

/// Pages whose content has header/footer artifacts sheaf or Acrobat drew,
/// as stripped copies in `store`; an author's own stay. Empty when none
/// have any; a source with none is never loaded.
pub fn strip_stamps(
    store: &Store,
    snap: &Snapshot,
    progress: &dyn Progress,
) -> Result<Vec<DerivedSource>> {
    let pages: Vec<PageId> = snap.state.order().iter().copied().collect();
    strip_stamps_of(store, snap, &pages, progress)
}

/// [`strip_stamps`] of pages `pages` of `snap` alone, as the Header &
/// Footer dialog previews one.
pub fn strip_stamps_of(
    store: &Store,
    snap: &Snapshot,
    pages: &[PageId],
    progress: &dyn Progress,
) -> Result<Vec<DerivedSource>> {
    let kinds = sheaf_doc::stamps::HEADER_FOOTER;
    progress.set_total(Some(pages.len() as u64), PAGES);
    // (page, index in source, place in the document)
    let pages = (pages.iter()).filter_map(|&id| {
        let (def, n) = (snap.state.def(id)?, snap.state.pos_of(id)?);
        Some((def.base.src.clone(), (id, def.base.index, n as u32)))
    });
    let by_source = crate::by_source(pages);
    let mut out = Vec::new();
    let mut done = 0;
    for (src, pages) in by_source {
        let mut keep = Vec::new();
        for (id, index, n) in pages {
            progress.check()?;
            if has_kinds(&src, index, n, kinds)? {
                keep.push((id, index, n));
            }
            done += 1;
            progress.set(done);
        }
        if keep.is_empty() {
            continue;
        }
        let list: Vec<(u32, Option<[f32; 4]>)> = keep.iter().map(|k| (k.1, None)).collect();
        let stripped = HeaderKept::No;
        let bytes = sheaf_doc::save::derived_copy(&src, &list, stripped, &mut |pdf, d, _, at| {
            sheaf_doc::stamps::strip(pdf, d, keep[at].2, kinds)?;
            Ok(())
        })?;
        let pages = keep.iter().map(|k| k.1).collect();
        let shown = (keep.iter().enumerate())
            .map(|(k, &(id, ..))| (id, k as u32))
            .collect();
        out.push(copy_of(store, bytes, &src, pages, shown)?);
    }
    Ok(out)
}

/// Pages with `wm` drawn on (sheaf's and Acrobat's of its kind stripped
/// first), and pages outside `scope` that have one of those stripped;
/// `None` strips only. Each
/// page's rotation is passed on, so the mark sits upright as the page is
/// shown. The copies are sources in `store`.
pub fn watermark(
    store: &Store,
    snap: &Snapshot,
    behind: bool,
    wm: Option<&sheaf_doc::watermark::Watermark>,
    scope: &[PageId],
    progress: &dyn Progress,
) -> Result<Vec<DerivedSource>> {
    let kinds = if behind {
        sheaf_doc::stamps::BACKGROUND
    } else {
        sheaf_doc::stamps::WATERMARK
    };
    progress.set_total(Some(snap.state.page_count() as u64), PAGES);
    // (page, index in source, place in the document, draw?, its rotation)
    let pages = snap.state.pages().enumerate().map(|(n, (id, def, geom))| {
        let draw = wm.is_some() && scope.contains(&id);
        let page = (id, def.base.index, n as u32, draw, geom.rotate);
        (def.base.src.clone(), page)
    });
    let by_source = crate::by_source(pages);
    let mut out = Vec::new();
    let mut done = 0;
    for (src, pages) in by_source {
        let mut keep = Vec::new();
        for page @ (_, index, n, draw, _) in pages {
            progress.check()?;
            if draw || has_kinds(&src, index, n, kinds)? {
                keep.push(page);
            }
            done += 1;
            progress.set(done);
        }
        if keep.is_empty() {
            continue;
        }
        let list: Vec<(u32, Option<[f32; 4]>)> = keep.iter().map(|k| (k.1, None)).collect();
        let kept = HeaderKept::Yes;
        let bytes = sheaf_doc::save::derived_copy(&src, &list, kept, &mut |pdf, d, geom, at| {
            let (_, _, n, draw, rotate) = keep[at];
            sheaf_doc::stamps::strip(pdf, d, n, kinds)?;
            if draw && let Some(wm) = wm {
                sheaf_doc::watermark::install(pdf, d, wm, &Geom { rotate, ..*geom })?;
            }
            Ok(())
        })?;
        let pages = keep.iter().map(|k| k.1).collect();
        let shown = (keep.iter().enumerate())
            .map(|(k, &(id, ..))| (id, k as u32))
            .collect();
        out.push(copy_of(store, bytes, &src, pages, shown)?);
    }
    Ok(out)
}

/// The watermark (or background) settings on the first page that has
/// one sheaf wrote, for the dialog to start from.
pub fn current_watermark(snap: &Snapshot, behind: bool) -> Option<sheaf_doc::watermark::Watermark> {
    let pages = (snap.state.pages()).map(|(_, def, _)| (def.base.src.clone(), def.base.index));
    for (src, indexes) in crate::by_source(pages) {
        let file = sheaf_doc::materialize::graph::eager(&src).ok()?;
        let objs: Vec<_> = file.get_pages().into_values().collect();
        for i in indexes {
            if let Some(d) = objs
                .get(i as usize)
                .and_then(|&o| file.get_dictionary(o).ok())
                && let Some(wm) = sheaf_doc::watermark::read(&file, d, behind)
            {
                return Some(wm);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lopdf::{Stream, dictionary};
    use sheaf_doc::FixedCount;
    use sheaf_doc::store::BlobKind;
    use sheaf_doc::tx::fixture::move_pages;

    use super::*;
    use crate::NoProgress;

    const HEADER: &[u8] =
        b"/Artifact <</Type /Pagination /Subtype /Header /SheafStamp true>> BDC (h) Tj EMC";
    const AUTHORED: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Header>> BDC (h) Tj EMC";

    /// A document of a page for each of `contents`.
    fn doc(contents: &[&[u8]]) -> Document {
        let mut f = sheaf_doc::forms::fixture::Form::new(contents.len());
        for (&page, content) in f.pages.clone().iter().zip(contents) {
            let stream = f
                .pdf
                .add_object(Stream::new(dictionary! {}, content.to_vec()));
            f.dict(page).set("Contents", stream);
        }
        let store = Store::new();
        let kind = BlobKind::file(Path::new("t.pdf"), None);
        let primary = store.open(f.bytes().into(), kind).unwrap();
        Document::new(
            store,
            primary,
            "t.pdf".into(),
            None,
            &[],
            Box::new(FixedCount(10)),
        )
    }

    /// Pages are picked by their blocks: those with a header sheaf drew,
    /// not one with an author's own, and none from a source with none.
    #[test]
    fn pages_are_picked_by_their_blocks() {
        let d = doc(&[b"0 g", HEADER, AUTHORED]);
        let ids: Vec<PageId> = d.state().order().iter().copied().collect();
        let copies = strip_stamps(d.store(), &d.snapshot(), &NoProgress).unwrap();
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].pages, [(ids[1], 0)]);
        assert!(
            !copies[0]
                .src
                .blocks(0)
                .unwrap()
                .has(sheaf_doc::stamps::HEADER_FOOTER)
        );
        let d = doc(&[b"0 g"]);
        assert!(
            strip_stamps(d.store(), &d.snapshot(), &NoProgress)
                .unwrap()
                .is_empty()
        );
        assert!(
            watermark(d.store(), &d.snapshot(), false, None, &[], &NoProgress)
                .unwrap()
                .is_empty()
        );
    }

    /// A page whose content can't be read fails the op, named by its
    /// place in the document.
    #[test]
    fn a_page_that_cant_be_read_is_named() {
        let mut d = doc(&[b"0 g", b"/A BMC"]);
        let ids: Vec<PageId> = d.state().order().iter().copied().collect();
        d.commit_ops(vec![move_pages(&[ids[1]], Some(ids[0]))], "move")
            .unwrap();
        let Err(e) = strip_stamps(d.store(), &d.snapshot(), &NoProgress) else {
            panic!("the page was read");
        };
        assert_eq!(
            e.to_string(),
            "page 1's content can't be read; nothing was changed"
        );
    }
}
