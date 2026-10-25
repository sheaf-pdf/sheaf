//! Pages sheaf makes in memory (M10.5 §3.9): blank pages, and images as
//! pages ([`crate::images`]), PDFs written through sheaf's serializer
//! ([`full::write_built`]) and interned as [`Memory`] blobs, which an
//! insert brings in as it brings a file's pages. Nothing is written to
//! disk. The same inputs make the same bytes, so the same blob: an insert
//! lowered again, or replayed, has the pages it made.
//!
//! [`full::write_built`]: crate::serialize::full::write_built
//! [`Memory`]: crate::store::Origin::Memory

use anyhow::{Context as _, Result, bail};
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};
use sheaf_edit::{Fit, PageSize};

use crate::store::{BlobKind, MemoryKind, Src, Store};

/// `count` blank pages of `width` × `height` points, which share one
/// empty content stream.
pub fn blank_pages(width: f32, height: f32, count: u32) -> Result<Vec<u8>> {
    if count == 0 {
        bail!("no blank pages to make");
    }
    pages_pdf(width, height, count, None)
}

/// `count` blank pages, none at all when it's 0, with `info` as the
/// trailer's `/Info` when there is one. No pages, no content stream:
/// nothing would use it.
fn pages_pdf(width: f32, height: f32, count: u32, info: Option<Dictionary>) -> Result<Vec<u8>> {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let kids: Vec<Object> = if count == 0 {
        Vec::new()
    } else {
        let content = pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
        (0..count)
            .map(|_| {
                pdf.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "MediaBox" => vec![0.into(), 0.into(), Object::from(width), Object::from(height)],
                    "Contents" => content,
                    "Resources" => dictionary! {},
                })
                .into()
            })
            .collect()
    };
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => i64::from(count) }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    if let Some(info) = info {
        let info = pdf.add_object(info);
        pdf.trailer.set("Info", info);
    }
    crate::serialize::full::write_built(&pdf)
}

/// `count` blank pages of `size`, interned in `store` and parsed: a
/// [`MemoryKind::Blank`] source.
pub fn blank(store: &Store, size: PageSize, count: u32) -> Result<Src> {
    let pdf = blank_pages(size.width, size.height, count)?;
    Ok(store.open(pdf.into(), BlobKind::memory(MemoryKind::Blank))?)
}

/// A PDF of no pages, interned in `store` and parsed: what a document
/// made in memory starts from (sheaf-ops' `Session::new_empty`), its
/// pages all inserted, a [`MemoryKind::Blank`] source.
pub fn empty(store: &Store) -> Result<Src> {
    let pdf = pages_pdf(0.0, 0.0, 0, None)?;
    Ok(store.open(pdf.into(), BlobKind::memory(MemoryKind::Blank))?)
}

/// [`empty`], its `/Info` saying it's `title`, that sheaf made it, and
/// when: what a document sheaf makes whole (the comment summary) starts
/// from, since a save keeps its primary's `/Info`. Made at a new time
/// each time, so not the same bytes twice.
pub fn titled(store: &Store, title: &str) -> Result<Src> {
    let info = dictionary! {
        "Title" => crate::save::text_string(title),
        "Producer" => Object::string_literal("sheaf"),
        "CreationDate" => Object::string_literal(crate::annot::pdf_date_now()),
    };
    let pdf = pages_pdf(0.0, 0.0, 0, Some(info))?;
    Ok(store.open(pdf.into(), BlobKind::memory(MemoryKind::Blank))?)
}

/// A page for each image in `images`, image files (a TIFF may hold
/// several), in order, each as `fit` says, interned in `store` and
/// parsed: a [`MemoryKind::Images`] source.
pub fn images(store: &Store, images: &[&[u8]], fit: Fit) -> Result<Src> {
    let mut pages = Vec::with_capacity(images.len());
    for (i, bytes) in images.iter().enumerate() {
        let read = sheaf_image::io::read_bytes(bytes.to_vec());
        pages.extend(read.with_context(|| format!("image {}", i + 1))?);
    }
    let pdf = crate::images::to_pdf(&pages, fit)?;
    Ok(store.open(pdf.into(), BlobKind::memory(MemoryKind::Images))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blank pages share their content, and the same pages make the same
    /// bytes, so one blob.
    #[test]
    fn blank_pages_are_the_same_bytes_each_time() {
        let a = blank_pages(612.0, 792.0, 3).unwrap();
        assert_eq!(blank_pages(612.0, 792.0, 3).unwrap(), a);
        assert_ne!(blank_pages(595.0, 842.0, 3).unwrap(), a);
        let pdf = Pdf::load_mem(&a).unwrap();
        assert_eq!(pdf.get_pages().len(), 3);
        let contents: std::collections::BTreeSet<_> = (pdf.page_iter())
            .map(|p| pdf.get_dictionary(p).unwrap().get(b"Contents").unwrap())
            .map(|c| c.as_reference().unwrap())
            .collect();
        assert_eq!(contents.len(), 1);
        assert!(blank_pages(612.0, 792.0, 0).is_err());
        let store = Store::new();
        let size = PageSize {
            width: 612.0,
            height: 792.0,
        };
        let src = blank(&store, size, 3).unwrap();
        assert_eq!(**src.blob().bytes(), *a);
        assert_eq!(blank(&store, size, 3).unwrap(), src);
    }

    /// A file of no pages opens as what a document made in memory starts
    /// from, and as nothing else: the same bytes read from a file don't.
    /// It holds its catalog and its page tree, and no content stream for
    /// no page.
    #[test]
    fn a_file_of_no_pages_is_sheaf_s_own() {
        let store = Store::new();
        let empty = empty(&store).unwrap();
        assert_eq!(empty.page_count(), 0);
        let bytes = empty.blob().bytes().clone();
        let objects = Pdf::load_mem(&bytes).unwrap().objects;
        let types: Vec<&[u8]> = (objects.values())
            .filter_map(|o| o.as_dict().ok()?.get(b"Type").ok()?.as_name().ok())
            .collect();
        assert_eq!((objects.len(), types), (2, vec![&b"Pages"[..], b"Catalog"]));
        drop(empty);
        let file = BlobKind::file(std::path::Path::new("empty.pdf"), None);
        let refused = Store::new().open(bytes, file).unwrap_err();
        assert!(matches!(
            refused,
            crate::source::OpenError::PageTree(crate::source::PageTreeError::NoPages)
        ));
    }
}
