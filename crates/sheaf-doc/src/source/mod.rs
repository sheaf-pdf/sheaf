//! The files a document's pages come from, read lazily (M10.5 §3.1). A
//! [`Source`] is a PDF blob parsed: lopdf's [`LazyPdf`] reads the file's
//! cross-reference sections and trailer when it opens, and each object
//! when it's first asked for, under one set of caps, [`Limits`]: a hostile
//! file is a typed error, never a panic, a hang or memory without bound.
//!
//! What a source says about itself is a pure function of its bytes, read
//! once: the page table when it opens; its conformance claims, what its
//! pages hold beside their content, its form and signatures, and the
//! header and footer sheaf drew, when first asked; and whether PDFium
//! refuses the bytes, once an engine has tried them. Sources come from the
//! [`Store`](crate::store::Store), one per distinct file.

mod conformance;
#[doc(hidden)]
pub mod fuzz;
#[cfg(test)]
pub(crate) mod lazy_tests;
mod limits;
mod pages;
#[cfg(test)]
mod tests;

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use in_progress::{NoProgress, Progress};
use lopdf::{LazyPdf, Object, ObjectId, XrefFacts};
use sheaf_edit::PageSize;

use crate::content::{ContentError, DrawnForms, PageBlocks};
use crate::forms::FileForm;
use crate::forms::sigs::Signatures;
use crate::geom::Geom;
use crate::model::FlatOutlineItem;
use crate::open::FileAnnots;
use crate::serialize::crypt::Crypt;
use crate::stamps::HeaderFooter;
use crate::store::{Blob, BlobCollision, BlobId, Src};

pub use conformance::{Conformance, PdfA};
pub use limits::{ContentLimits, FormLimits, Limits, PageTreeLimits};
pub use pages::{PageFacts, Parent};

/// A PDF, parsed: its blob, read lazily, how it's encrypted, and its
/// page table.
pub struct Source {
    blob: Blob,
    crypt: Option<Arc<Crypt>>,
    pdf: LazyPdf,
    limits: Limits,
    pages: Arc<[PageFacts]>,
    /// Each page's blocks, once asked for, and what's known of the forms
    /// its pages draw.
    blocks: Box<[OnceLock<Arc<PageBlocks>>]>,
    forms: DrawnForms,
    conformance: OnceLock<Conformance>,
    annots: OnceLock<Arc<FileAnnots>>,
    outline: OnceLock<Arc<[FlatOutlineItem]>>,
    form: OnceLock<Arc<FileForm>>,
    signatures: OnceLock<Arc<Signatures>>,
    header_footer: OnceLock<Option<StoredHeaderFooter>>,
    /// PDFium refused its bytes ([`Source::base_refused`]).
    refused: AtomicBool,
}

/// A header and footer sheaf drew, as its file's catalog stores it, and
/// the indexes of the pages it's on (None: every page).
pub type StoredHeaderFooter = (HeaderFooter, Option<Vec<usize>>);

/// An object of a source: what a record was read from, so what sheaf
/// doesn't model of it survives (M10.5 §2, rule 1). It pins the source.
/// Equal, ordered and hashed by the source's id and the object's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjRef {
    pub src: Src,
    pub id: ObjectId,
}

/// Why a source didn't open.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("the password is wrong, or the file needs one")]
    Password,
    #[error("reading the file: {0}")]
    Pdf(lopdf::Error),
    #[error(transparent)]
    PageTree(#[from] PageTreeError),
    #[error(transparent)]
    Collision(#[from] BlobCollision),
}

impl OpenError {
    /// Whether `err`, anywhere in its chain, is a wrong or missing
    /// password.
    pub fn is_password(err: &anyhow::Error) -> bool {
        err.chain()
            .any(|c| matches!(c.downcast_ref::<OpenError>(), Some(OpenError::Password)))
    }
}

impl From<lopdf::Error> for OpenError {
    fn from(e: lopdf::Error) -> OpenError {
        match e {
            lopdf::Error::InvalidPassword => OpenError::Password,
            e => OpenError::Pdf(e),
        }
    }
}

/// What's wrong with a page tree that has no page table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PageTreeError {
    #[error("the page tree lists a node inside itself")]
    Cycle,
    #[error("a page's /Parent chain loops")]
    ParentLoop,
    #[error("the page tree is too deep")]
    TooDeep,
    #[error("the page tree lists too many pages")]
    TooManyPages,
    #[error("the file has no pages")]
    NoPages,
}

impl Source {
    /// `blob` read under `limits`: its cross-reference sections, trailer,
    /// encryption and page table.
    pub(crate) fn open(blob: Blob, limits: &Limits) -> Result<Source, OpenError> {
        let pdf = LazyPdf::open(blob.bytes().clone(), blob.password(), &limits.reader)?;
        let crypt = Crypt::of(&pdf)?.map(Arc::new);
        let pages: Arc<[PageFacts]> = pages::table(&pdf, &limits.page_tree)?.into();
        Ok(Source {
            blob,
            crypt,
            pdf,
            limits: *limits,
            blocks: pages.iter().map(|_| OnceLock::new()).collect(),
            forms: DrawnForms::default(),
            pages,
            conformance: OnceLock::new(),
            annots: OnceLock::new(),
            outline: OnceLock::new(),
            form: OnceLock::new(),
            signatures: OnceLock::new(),
            header_footer: OnceLock::new(),
            refused: AtomicBool::new(false),
        })
    }

    pub fn id(&self) -> BlobId {
        self.blob.id()
    }

    pub fn blob(&self) -> &Blob {
        &self.blob
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        self.blob.bytes()
    }

    /// The password it was opened with, if it needed one.
    pub fn password(&self) -> Option<&str> {
        self.blob.password()
    }

    pub fn pdf(&self) -> &LazyPdf {
        &self.pdf
    }

    /// The caps it's read under.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// False when lopdf rebuilt the cross-reference table: such a file is
    /// never appended to.
    pub fn base_sound(&self) -> bool {
        self.pdf.base_sound()
    }

    /// Whether PDFium can't read its bytes, as an engine found when it
    /// tried them ([`Source::refuse_base`]): its pages are then all drawn
    /// from realizations, none from its base engine (M10.5 slice 13).
    /// False until an engine has failed, and for the source's life after.
    pub fn base_refused(&self) -> bool {
        self.refused.load(Ordering::Relaxed)
    }

    /// Notes that PDFium refused its bytes.
    pub fn refuse_base(&self) {
        self.refused.store(true, Ordering::Relaxed);
    }

    pub fn xref(&self) -> &XrefFacts {
        self.pdf.xref()
    }

    /// How it's encrypted, for a save to write it encrypted the same way:
    /// its filters (`StmF`, `StrF`, `EFF`), its key, its `/Encrypt` as
    /// stored and its `/ID`. None when it isn't.
    pub fn crypt(&self) -> Option<&Arc<Crypt>> {
        self.crypt.as_ref()
    }

    pub fn object(&self, id: ObjectId) -> lopdf::Result<Arc<Object>> {
        self.pdf.object(id)
    }

    /// Where object `id` is in the bytes. None for an object stream's
    /// member.
    pub fn raw_span(&self, id: ObjectId) -> Option<Range<usize>> {
        self.pdf.raw_span(id)
    }

    /// [`raw_span`](Self::raw_span) of an object a writer may copy as it
    /// is: None too for a stream whose `/Length` is a reference, whose
    /// bytes name the object holding its length, which the object as read
    /// doesn't, for one whose data its `/Length` doesn't fit, and for an
    /// object without its `endobj`, whose copy would be as damaged.
    pub fn raw_span_whole(&self, id: ObjectId) -> Option<Range<usize>> {
        self.pdf.raw_span_whole(id)
    }

    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }

    /// Page `index`, in the order PDFium numbers them.
    pub fn page(&self, index: u32) -> Option<&PageFacts> {
        self.pages.get(index as usize)
    }

    /// Every page, in that order.
    pub fn pages(&self) -> &[PageFacts] {
        &self.pages
    }

    /// Every page's object, in that order: how a reader that walks pages
    /// numbers them.
    pub fn page_ids(&self) -> impl ExactSizeIterator<Item = ObjectId> + Clone + '_ {
        self.pages.iter().map(|page| page.id)
    }

    /// Page `index`'s boxes and its own `/Rotate`; past the last page, a
    /// page's with no boxes, Letter.
    pub fn geom(&self, index: u32) -> Geom {
        self.page(index)
            .map_or_else(|| Geom::read(None, None, 0), |page| page.geom)
    }

    /// Page `index`'s size as shown.
    pub fn page_size(&self, index: u32) -> PageSize {
        self.geom(index).shown()
    }

    /// Page `index`'s content classified (§13): its pagination artifacts
    /// and who drew them, sheaf's blocks, its balance. Read once, its
    /// streams without keeping them; never at open. Past the last page, a
    /// page's with nothing on it.
    pub fn blocks(&self, index: u32) -> Result<Arc<PageBlocks>, ContentError> {
        let (Some(slot), Some(page)) = (self.blocks.get(index as usize), self.page(index)) else {
            return Ok(Arc::default());
        };
        if let Some(blocks) = slot.get() {
            return Ok(blocks.clone());
        }
        let blocks = PageBlocks::read(
            &self.pdf,
            &page.contents,
            page.resources.as_deref(),
            &self.limits,
            &self.forms,
        )
        .map_err(|fault| ContentError::Read { page: index, fault })?;
        Ok(slot.get_or_init(|| Arc::new(blocks)).clone())
    }

    /// Whether page `index`'s blocks have been read.
    #[cfg(test)]
    pub(crate) fn blocks_known(&self, index: u32) -> bool {
        self.blocks
            .get(index as usize)
            .is_some_and(|slot| slot.get().is_some())
    }

    /// What its pages hold beside their content: sheaf's marks and other
    /// tools' comments and links, each with its object and an id, read
    /// once (see [`crate::open::FileAnnots`]).
    pub fn annots(&self) -> &Arc<FileAnnots> {
        self.annots_counting(&NoProgress)
    }

    /// [`annots`](Self::annots), with `p` counting the pages read: all at
    /// once when they were read before.
    pub(crate) fn annots_counting(&self, p: &dyn Progress) -> &Arc<FileAnnots> {
        if let Some(annots) = self.annots.get() {
            p.inc(u64::from(self.page_count()));
            return annots;
        }
        let annots = crate::open::annots::read(self, p);
        self.annots.get_or_init(|| Arc::new(annots))
    }

    /// Its bookmarks in pre-order, read once (see [`crate::open::outline`]):
    /// what pages inserted from it bring.
    pub fn outline(&self) -> &Arc<[FlatOutlineItem]> {
        (self.outline).get_or_init(|| crate::open::outline(self).into())
    }

    /// Its form: the field tree, the widgets on its pages and what sheaf
    /// shows about it, read once (see [`crate::forms::read`]).
    pub fn form(&self) -> &Arc<FileForm> {
        self.form.get_or_init(|| Arc::new(crate::forms::read(self)))
    }

    /// Its signatures, what each allows after it, and Reader's usage
    /// rights, read once (see [`crate::forms::sigs`]).
    pub fn signatures(&self) -> &Arc<Signatures> {
        (self.signatures).get_or_init(|| Arc::new(crate::forms::sigs::read(self)))
    }

    /// The header and footer its catalog says sheaf drew on its pages
    /// ([`crate::stamps::from_catalog`]), read once.
    pub fn header_footer(&self) -> Option<&StoredHeaderFooter> {
        (self.header_footer)
            .get_or_init(|| crate::stamps::from_catalog(&self.pdf))
            .as_ref()
    }

    /// What its XMP metadata claims it conforms to: PDF/A and PDF/UA.
    pub fn conformance(&self) -> anyhow::Result<Conformance> {
        if let Some(&claims) = self.conformance.get() {
            return Ok(claims);
        }
        let claims = conformance::read(&self.pdf, &self.limits)?;
        Ok(*self.conformance.get_or_init(|| claims))
    }
}
