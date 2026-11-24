//! The page table: every page the tree lists, numbered as PDFium numbers
//! them, with what each inherits. Walked when a source opens, without
//! reading any page's content.
//!
//! Base engines and overlays address PDFium's pages by index, so the walk
//! is PDFium's: pdf-semantics' by PDFium's rules ([`semantics::pages`],
//! held to PDFium by its own tests). Depth first through `/Kids` from the
//! catalog's `/Pages`, a kid with `/Kids` a node whatever its `/Type`, any
//! other dictionary a page; the walk stops at the count PDFium gives, the
//! root's `/Count` where PDFium counts by it and otherwise its own count
//! of the tree. A leaf whose `/Type` PDFium loads no page of, and a kid
//! that names no dictionary, keep their places as stand-ins, Letter pages
//! with nothing on them. A kid that isn't a reference, though PDFium
//! numbers it too, has no object for sheaf to name a page by, and is left
//! out, so the pages after it are numbered one lower than PDFium numbers
//! them. The inheritable keys come down the `/Parent` chain from the page,
//! as PDFium's `GetPageAttr` reads them, which needn't be the path the
//! walk took to the page, so a page object's facts are the same wherever
//! it's listed, and each reference on the way is followed as far as PDFium
//! follows it: one hop, two for `/Pages`, `/Parent` and `/Rotate`.
//!
//! That holds of what the walk reads: the tree, and a page's
//! `/MediaBox`, `/CropBox`, `/Rotate`, `/UserUnit` and `/Resources`. A
//! page's `/TrimBox`, `/BleedBox` and `/ArtBox`, which pdf-semantics
//! doesn't give, and its `/Annots` are still read here, from the page's
//! own dictionary by the reader's rules and not PDFium's: a reference is
//! followed however many hops it takes, so a `/TrimBox` behind two reads
//! where a `/MediaBox` behind two is none, and a loop of references in
//! one fails the open where the walk reads past it.
//!
//! What sheaf keeps of each page is its own ([`PageFacts`]): its boxes as
//! PDFium sizes them, what it inherits, and what it holds, which a page
//! object listed twice, the pages under one node and the pages naming one
//! `/Annots` array share.
//!
//! Where PDFium stops quietly, the walk fails: a node met again on its
//! own path, a `/Parent` chain that loops, and a tree past the caps
//! ([`PageTreeLimits`](super::PageTreeLimits)) are [`PageTreeError`]s; an
//! object on the way that breaks one of the reader's caps or doesn't
//! decrypt fails the open with the reader's own error. The walk is iterative, so a deep tree is never
//! deep on the stack.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::pdf::semantics::pages::{self, Inheritable, Origin, Page, PdfBox, Sourced};
use crate::pdf::{Dictionary, Error, LazyPdf, Object, ObjectId, Resolve, Unreadable, Value, Why};

use super::{Limits, OpenError, PageTreeError};
use crate::geom::{Geom, Rect};

/// A page, as the tree lists it.
#[derive(Debug, Clone)]
pub struct PageFacts {
    pub id: ObjectId,
    /// Its `/Parent`, which the inheritable keys come down.
    pub parent: Option<Arc<Parent>>,
    /// Its `/Resources`, else the nearest on its `/Parent` chain, as
    /// written: a reference stays one.
    pub resources: Option<Arc<Object>>,
    pub geom: Geom,
    /// The streams its `/Contents` names: one, or a direct array's. A
    /// reference to an array stays that one reference, since telling the
    /// two apart reads the stream it usually is.
    pub contents: Arc<[ObjectId]>,
    /// Its `/Annots` entries in order, as written (a reference each, in a
    /// well-formed file): an annotation's slot is its index here. Pages
    /// whose `/Annots` names one array share it.
    pub annots: Arc<[Object]>,
    pub struct_parents: Option<i64>,
    /// How often the tree listed this page object before: 0 the first
    /// time, 1 the second.
    pub repeat: u32,
}

impl PageFacts {
    /// A place where PDFium loads no page: a kid that names no dictionary,
    /// or a leaf whose `/Type` PDFium loads none of. It keeps its place in
    /// the tree, as PDFium's numbering keeps it: a Letter page with
    /// nothing on it.
    fn stand_in(id: ObjectId) -> PageFacts {
        PageFacts {
            id,
            parent: None,
            resources: None,
            geom: Geom::read(None, None, 0),
            contents: Arc::from([]),
            annots: Arc::from([]),
            struct_parents: None,
            repeat: 0,
        }
    }

    /// The nodes on its `/Parent` chain, nearest first.
    pub fn parents(&self) -> impl Iterator<Item = ObjectId> + '_ {
        std::iter::successors(self.parent.as_deref(), |p| p.up.as_deref()).map(|p| p.id)
    }
}

/// A node on a page's `/Parent` chain, and the chain above it: pages
/// under one node share it.
#[derive(Debug)]
pub struct Parent {
    pub id: ObjectId,
    pub up: Option<Arc<Parent>>,
}

/// `pdf`'s pages.
pub(super) fn table(pdf: &LazyPdf, limits: &Limits) -> Result<Vec<PageFacts>, OpenError> {
    let catalog = pdf.catalog()?;
    let failed = RefCell::new(None);
    let objects = OneHop {
        pdf,
        failed: &failed,
    };
    let walked = match pages::table(objects, catalog, &limits.semantics()) {
        Ok(walked) => walked,
        Err(error) => return Err(tree_error(error, failed.borrow_mut().take())),
    };
    let mut table = Table {
        pdf,
        pages: Vec::with_capacity(walked.len()),
        listed: HashMap::new(),
        resources: HashMap::new(),
        chains: HashMap::new(),
        arrays: HashMap::new(),
    };
    for page in &walked {
        // A place with no object number: a kid written in place, or one
        // that isn't a reference. Nothing of sheaf's can name a page
        // without one.
        let Some(id) = page.id else {
            continue;
        };
        table.push(id, page)?;
    }
    if table.pages.is_empty() {
        return Err(PageTreeError::NoPages.into());
    }
    Ok(table.pages)
}

/// A source's objects as the page tree's walk reads them: one hop a
/// resolution (`LazyPdf::get`, which keeps what it reads), so the walk
/// follows a chain of references only as far as PDFium does. An object
/// that isn't there is none, and one that doesn't read is the file's
/// damage ([`Why::Damaged`]), which the walk takes for one that isn't
/// there; but a cap broken ([`Why::Capped`]) or an object that doesn't
/// decrypt ([`Why::Locked`]) fails the walk, and the reader's own error is
/// kept in `failed`, for the open to give as it gives it from any other
/// read. One hop never meets a cycle of references.
#[derive(Clone, Copy)]
struct OneHop<'a> {
    pdf: &'a LazyPdf,
    failed: &'a RefCell<Option<Error>>,
}

impl<'a> Resolve<'a> for OneHop<'a> {
    fn resolve(&self, id: ObjectId) -> Option<&'a Object> {
        self.pdf.get(id).ok()
    }

    fn try_resolve(&self, id: ObjectId) -> Result<Option<&'a Object>, Unreadable> {
        let error = match self.pdf.get(id) {
            Ok(object) => return Ok(Some(object)),
            Err(Error::MissingXrefEntry | Error::ObjectNotFound(_)) => return Ok(None),
            Err(error) => error,
        };
        let why = match error {
            Error::Limit(_) => Why::Capped,
            Error::Decryption(_) => Why::Locked,
            _ => Why::Damaged,
        };
        let unreadable = Unreadable::new(id, why, error.to_string());
        if why != Why::Damaged {
            self.failed.borrow_mut().get_or_insert(error);
        }
        Err(unreadable)
    }
}

/// Why the walk gave no pages, as the open says it: the reader's own
/// error, `kept`, where an object on the way didn't read.
fn tree_error(error: pages::Error, kept: Option<Error>) -> OpenError {
    match error {
        pages::Error::Cycle => PageTreeError::Cycle.into(),
        pages::Error::ParentLoop => PageTreeError::ParentLoop.into(),
        pages::Error::TooDeep => PageTreeError::TooDeep.into(),
        pages::Error::TooManyPages => PageTreeError::TooManyPages.into(),
        pages::Error::NoPages => PageTreeError::NoPages.into(),
        pages::Error::Unreadable(unreadable) => match kept {
            Some(error) => error.into(),
            None => OpenError::Pdf(Error::Syntax(unreadable.to_string())),
        },
        error => OpenError::Pdf(Error::Syntax(error.to_string())),
    }
}

/// The table being made from the walk's pages, and what its pages share.
struct Table<'a> {
    pdf: &'a LazyPdf,
    pages: Vec<PageFacts>,
    /// Where the table first has each page object listed.
    listed: HashMap<ObjectId, usize>,
    /// The `/Resources` pages inherit, by the address of the object as
    /// written in the node that has it: one `Arc` each.
    resources: HashMap<*const Object, Arc<Object>>,
    /// Each node's `/Parent` chain, from it up, by its id.
    chains: HashMap<ObjectId, Arc<Parent>>,
    /// The `/Annots` arrays read, by the reference pages name them by.
    arrays: HashMap<ObjectId, Arc<[Object]>>,
}

impl<'a> Table<'a> {
    /// The walk's `page`, object `id`, added to the table: the facts it was
    /// first read with, shared, if it was listed before, numbered by how
    /// often it was, which the walk counts ([`Page::repeat`]).
    fn push(&mut self, id: ObjectId, page: &Page<'a>) -> Result<(), OpenError> {
        let facts = match self.listed.get(&id) {
            Some(&first) => self.pages[first].clone(),
            None => {
                self.listed.insert(id, self.pages.len());
                self.facts(id, page)?
            }
        };
        self.pages.push(PageFacts {
            repeat: page.repeat,
            ..facts
        });
        Ok(())
    }

    /// What sheaf keeps of `page`: a stand-in where PDFium loads no page.
    fn facts(&mut self, id: ObjectId, page: &Page<'a>) -> Result<PageFacts, OpenError> {
        let Some(dict) = page.dict else {
            return Ok(PageFacts::stand_in(id));
        };
        let page_box = |key: &[u8]| self.rect(self.found(dict, key)?);
        let geom = Geom {
            trim: page_box(b"TrimBox")?,
            bleed: page_box(b"BleedBox")?,
            art: page_box(b"ArtBox")?,
            user_unit: page.user_unit().value as f32,
            ..Geom::read(
                written(page.media_box()),
                written(page.crop_box()),
                page.rotate().value,
            )
        };
        let resources = (page.attribute(Inheritable::Resources)).map(|resources| {
            if resources.origin == Origin::Own {
                return Arc::new(resources.written.clone());
            }
            let at = std::ptr::from_ref(resources.written);
            (self.resources.entry(at))
                .or_insert_with(|| Arc::new(resources.written.clone()))
                .clone()
        });
        Ok(PageFacts {
            id,
            parent: self.chain(page),
            resources,
            geom,
            contents: crate::content::contents(dict).into(),
            annots: self.annots(dict)?,
            struct_parents: dict.get(b"StructParents").and_then(Object::as_integer),
            repeat: 0,
        })
    }

    /// `page`'s `/Parent` chain, made of the nodes the walk climbed, each
    /// made once and shared by the pages under it.
    fn chain(&mut self, page: &Page<'_>) -> Option<Arc<Parent>> {
        let mut parents = page.parents();
        let nearest = parents.next()?;
        if let Some(chain) = self.chains.get(&nearest) {
            return Some(chain.clone());
        }
        // Up to a node already met, then back down.
        let mut climbed = vec![nearest];
        let mut above = None;
        for id in parents {
            if let Some(chain) = self.chains.get(&id) {
                above = Some(chain.clone());
                break;
            }
            climbed.push(id);
        }
        for id in climbed.into_iter().rev() {
            let node = Arc::new(Parent { id, up: above });
            self.chains.insert(id, node.clone());
            above = Some(node);
        }
        above
    }

    /// A page's `/Annots` entries: an array that pages name by reference
    /// is read once, and shared.
    fn annots(&mut self, dict: &'a Dictionary) -> Result<Arc<[Object]>, OpenError> {
        let shared = dict.get(b"Annots").and_then(Object::as_reference);
        if let Some(annots) = shared.and_then(|id| self.arrays.get(&id)) {
            return Ok(annots.clone());
        }
        let annots: Arc<[Object]> = match self.found(dict, b"Annots")?.and_then(Object::as_array) {
            Some(annots) => annots.into(),
            None => Arc::from([]),
        };
        if let Some(id) = shared {
            self.arrays.insert(id, annots.clone());
        }
        Ok(annots)
    }

    /// The value of `key` in `dict`, references followed: None when it's
    /// missing or names no object.
    fn found(&self, dict: &'a Dictionary, key: &[u8]) -> Result<Option<&'a Object>, OpenError> {
        match dict.get(key) {
            Some(value) => self.resolve(value),
            None => Ok(None),
        }
    }

    fn resolve(&self, value: &'a Object) -> Result<Option<&'a Object>, OpenError> {
        let pdf = self.pdf;
        match pdf.dereference(value) {
            Ok((_, object)) => Ok(Some(object)),
            Err(e) if fatal(&e) => Err(e.into()),
            Err(_) => Ok(None),
        }
    }

    /// A box: four numbers, normalized. None for anything else.
    fn rect(&self, value: Option<&'a Object>) -> Result<Option<Rect>, OpenError> {
        let Some(numbers) = value.and_then(Object::as_array) else {
            return Ok(None);
        };
        let [a, b, c, d] = numbers else {
            return Ok(None);
        };
        let mut corners = [0.0; 4];
        for (n, value) in corners.iter_mut().zip([a, b, c, d]) {
            *n = match self.resolve(value)?.map(Object::value) {
                Some(Value::Integer(i)) => *i as f32,
                Some(Value::Real(r)) => *r as f32,
                _ => 0.0,
            };
        }
        let [a, b, c, d] = corners;
        Ok(Some(Rect::new(a, b, c, d)))
    }
}

/// A media or crop box as the walk read it, normalized: None where the
/// page and its nodes have none that's four numbers, so [`Geom::read`]
/// gives what PDFium makes of none.
fn written(sourced: Sourced<PdfBox>) -> Option<Rect> {
    let [a, b, c, d] = sourced.value.0;
    (sourced.origin != Origin::Default).then(|| Rect::new(a as f32, b as f32, c as f32, d as f32))
}

/// Whether reading an object of a page's own failed in a way that fails
/// the open: a cap broken, a cycle of references, or an object that
/// doesn't decrypt. Any other failure is an object that isn't there, as
/// PDFium takes it.
fn fatal(e: &Error) -> bool {
    matches!(
        e,
        Error::Limit(_) | Error::ReferenceCycle(_) | Error::Decryption(_)
    )
}
