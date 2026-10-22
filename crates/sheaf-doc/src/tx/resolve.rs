//! An op's references made ids (M10.5 §6.6), the first of `Tx::apply`'s
//! stages: positions against the transaction's state as the op finds it
//! (1-based, so `[delete-pages 1, rotate-pages {"pos": 2}]` turns the page
//! that's second after the delete), locals against what earlier ops made,
//! titles and `/NM`s against the outline and the page, and blobs against
//! the store and the transaction's grants. Every reference must resolve
//! (§6.4): an unknown id is an error, never skipped.
//!
//! The host's half of resolving, done before a transaction lowers an
//! insert, is [`incoming`]: the files and stamp pages the inserted pages'
//! marks hold, decoded and interned, so lowering never sees bytes (§3.1).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use sheaf_edit::{
    AnnotRef, BlobSel, BookmarkPatch, BookmarkRef, BoxSpec, Bring, Color, Delta, DestView, GridPos,
    HeaderFooterSpec, Imported, Local, MarkKind, MarkPatch, NewMark, Op, PageRef, PageSel,
    PageSource, Patch, ScopeSel, Sibling, Style, TextStyle, Turn, Z,
};

use super::Made;
use super::error::EditError;
use super::{Grant, LowerCx};
use crate::annot::{AnnotId, AnnotKind, Box2};
use crate::labels::LabelSpec;
use crate::model::{DocState, Foreign, ImageRef, OutlineId, PageForm, PageId, WatermarkContent};
use crate::open::FileBody;
use crate::prim::Key;
use crate::store::{Blob, BlobId, BlobKind, BlobRef, Src, Store};
use crate::text_layer::TextLayer;

/// An op with its references resolved: ids for pages, bookmarks and
/// annotations, and for blobs what they are. What a step keeps, and what
/// describe, the capabilities and lowering read.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ResolvedOp {
    MovePages {
        pages: Vec<PageId>,
        before: Option<PageId>,
    },
    RotatePages {
        pages: Vec<PageId>,
        by: Turn,
    },
    DeletePages {
        pages: Vec<PageId>,
    },
    DuplicatePages {
        pages: Vec<PageId>,
        locals: Vec<Local>,
    },
    /// `pages` of `from`, 0-based and ascending, named `name`.
    InsertPages {
        from: Src,
        name: Arc<str>,
        pages: Vec<u32>,
        before: Option<PageId>,
        bring: Bring,
        locals: Vec<Local>,
    },
    /// `pages` shown from `with` of `from`, 0-based and ascending. The
    /// pages keep the document's bookmarks: `bring.outline` isn't read.
    ReplacePages {
        pages: Vec<PageId>,
        from: Src,
        name: Arc<str>,
        with: Vec<u32>,
        bring: Bring,
    },
    SetPageBoxes {
        pages: Vec<PageId>,
        crop: Patch<BoxSpec>,
        trim: Patch<BoxSpec>,
        bleed: Patch<BoxSpec>,
        art: Patch<BoxSpec>,
    },
    SetPageLabels {
        ranges: Option<Vec<LabelSpec>>,
    },
    AddBookmark {
        parent: Option<OutlineId>,
        at: At,
        title: String,
        target: Option<PageId>,
        view: Option<DestView>,
        color: Option<Color>,
        style: TextStyle,
        local: Option<Local>,
    },
    PatchBookmarks {
        bookmarks: Vec<OutlineId>,
        patch: BookmarkChange,
    },
    MoveBookmarks {
        bookmarks: Vec<OutlineId>,
        parent: Option<OutlineId>,
        at: At,
        with_pages: bool,
    },
    DeleteBookmarks {
        bookmarks: Vec<OutlineId>,
    },
    /// `held` is what its asset resolved to; `mark.asset` is left out.
    AddMark {
        page: PageId,
        mark: NewMark,
        held: Option<Grant>,
        reply_to: Option<AnnotId>,
        local: Option<Local>,
        imported: Option<Imported>,
    },
    AddConnector {
        a: (PageId, Box2),
        b: (PageId, Box2),
        label: String,
        style: Style,
        locals: Vec<Local>,
    },
    PatchMarks {
        marks: Vec<AnnotId>,
        patch: MarkPatch,
        imported: Option<Imported>,
    },
    MoveMarks {
        marks: Vec<AnnotId>,
        by: Delta,
        to_page: Option<PageId>,
    },
    DeleteAnnotations {
        annots: Vec<AnnotId>,
    },
    AdoptAnnotations {
        annots: Vec<AnnotId>,
    },
    SetOcrText {
        page: PageId,
        layer: Arc<TextLayer>,
    },
    ClearOcrText {
        pages: Vec<PageId>,
    },
    /// `content` with its file held; `scope` None: every page, those added
    /// later too.
    SetWatermark {
        content: WatermarkContent,
        position: GridPos,
        z: Z,
        scope: Option<im::OrdSet<PageId>>,
    },
    /// `z` None: the watermark and the background. `others` is what its
    /// lowering found of others' on its pages, none before.
    RemoveWatermark {
        z: Option<Z>,
        pages: Vec<PageId>,
        include_authored: bool,
        others: OthersFound,
    },
    /// `scope` None: every page, those added later too.
    SetHeaderFooter {
        spec: HeaderFooterSpec,
        scope: Option<im::OrdSet<PageId>>,
    },
    /// `others` is what its lowering found of others' on its pages, none
    /// before.
    RemoveHeaderFooter {
        pages: Vec<PageId>,
        include_authored: bool,
        others: OthersFound,
    },
    /// A group's versions taken back, the group called `label`: what
    /// History's "Undo all of …" makes, and a failed call's rollback. No
    /// `Op` says it, and it's never lowered: its step holds the group's
    /// prims swapped ([`Document::revert_group`]).
    ///
    /// [`Document::revert_group`]: crate::model::Document::revert_group
    Revert {
        label: Arc<str>,
    },
}

/// What a [`ResolvedOp::RemoveWatermark`] or a
/// [`ResolvedOp::RemoveHeaderFooter`] found of others' artifacts of its
/// kinds on its pages, as its lowering counted them (§6.5, D61), each
/// artifact once: those it takes out, by who drew them, and those it
/// leaves in place. What its line says, and what a dialog shows
/// ([`others_on`](super::others_on)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OthersFound {
    /// Taken out: Acrobat's; sheaf's in another file, which no generator
    /// of the document's backs; sheaf's from before M10.5; and the author's
    /// own, with `include_authored`.
    pub acrobat: usize,
    pub other_files: usize,
    pub older_sheaf: usize,
    pub authored: usize,
    /// Left in place: the author's, without `include_authored` or where a
    /// splice refuses them; and others' a splice refuses, which taking out
    /// would change what else the page draws.
    pub authored_left: usize,
    pub refused: usize,
    /// Pages that draw one inside a form, left in place, and pages whose
    /// content can't be read.
    pub in_forms: usize,
    pub unreadable: usize,
}

impl OthersFound {
    /// Its counts and `other`'s, added.
    pub fn plus(self, other: OthersFound) -> OthersFound {
        OthersFound {
            acrobat: self.acrobat + other.acrobat,
            other_files: self.other_files + other.other_files,
            older_sheaf: self.older_sheaf + other.older_sheaf,
            authored: self.authored + other.authored,
            authored_left: self.authored_left + other.authored_left,
            refused: self.refused + other.refused,
            in_forms: self.in_forms + other.in_forms,
            unreadable: self.unreadable + other.unreadable,
        }
    }
}

/// Where among its siblings a bookmark goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    First,
    Last,
    After(OutlineId),
}

/// [`BookmarkPatch`], its target resolved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BookmarkChange {
    pub title: Option<String>,
    pub target: Patch<PageId>,
    pub view: Patch<DestView>,
    pub color: Patch<Color>,
    pub style: Option<TextStyle>,
}

/// What an op is called in Op JSON, and in its errors.
pub fn op_name(op: &Op) -> &'static str {
    match op {
        Op::MovePages { .. } => "move-pages",
        Op::RotatePages { .. } => "rotate-pages",
        Op::DeletePages { .. } => "delete-pages",
        Op::DuplicatePages { .. } => "duplicate-pages",
        Op::InsertPages { .. } => "insert-pages",
        Op::ReplacePages { .. } => "replace-pages",
        Op::SetPageBoxes { .. } => "set-page-boxes",
        Op::SetPageLabels { .. } => "set-page-labels",
        Op::AddBookmark { .. } => "add-bookmark",
        Op::PatchBookmarks { .. } => "patch-bookmarks",
        Op::MoveBookmarks { .. } => "move-bookmarks",
        Op::DeleteBookmarks { .. } => "delete-bookmarks",
        Op::AddMark { .. } => "add-mark",
        Op::AddConnector { .. } => "add-connector",
        Op::PatchMarks { .. } => "patch-marks",
        Op::MoveMarks { .. } => "move-marks",
        Op::DeleteAnnotations { .. } => "delete-annotations",
        Op::AdoptAnnotations { .. } => "adopt-annotations",
        Op::SetOcrText { .. } => "set-ocr-text",
        Op::ClearOcrText { .. } => "clear-ocr-text",
        Op::SetWatermark { .. } => "set-watermark",
        Op::RemoveWatermark { .. } => "remove-watermark",
        Op::SetHeaderFooter { .. } => "set-header-footer",
        Op::RemoveHeaderFooter { .. } => "remove-header-footer",
        _ => "op",
    }
}

impl ResolvedOp {
    /// What it's called in Op JSON, and a group's revert as `revert`.
    pub fn name(&self) -> &'static str {
        use ResolvedOp as R;
        match self {
            R::MovePages { .. } => "move-pages",
            R::RotatePages { .. } => "rotate-pages",
            R::DeletePages { .. } => "delete-pages",
            R::DuplicatePages { .. } => "duplicate-pages",
            R::InsertPages { .. } => "insert-pages",
            R::ReplacePages { .. } => "replace-pages",
            R::SetPageBoxes { .. } => "set-page-boxes",
            R::SetPageLabels { .. } => "set-page-labels",
            R::AddBookmark { .. } => "add-bookmark",
            R::PatchBookmarks { .. } => "patch-bookmarks",
            R::MoveBookmarks { .. } => "move-bookmarks",
            R::DeleteBookmarks { .. } => "delete-bookmarks",
            R::AddMark { .. } => "add-mark",
            R::AddConnector { .. } => "add-connector",
            R::PatchMarks { .. } => "patch-marks",
            R::MoveMarks { .. } => "move-marks",
            R::DeleteAnnotations { .. } => "delete-annotations",
            R::AdoptAnnotations { .. } => "adopt-annotations",
            R::SetOcrText { .. } => "set-ocr-text",
            R::ClearOcrText { .. } => "clear-ocr-text",
            R::SetWatermark { .. } => "set-watermark",
            R::RemoveWatermark { .. } => "remove-watermark",
            R::SetHeaderFooter { .. } => "set-header-footer",
            R::RemoveHeaderFooter { .. } => "remove-header-footer",
            R::Revert { .. } => "revert",
        }
    }

    /// The locals it names what it makes by, in order.
    pub(crate) fn locals(&self) -> &[Local] {
        use ResolvedOp as R;
        match self {
            R::DuplicatePages { locals, .. }
            | R::InsertPages { locals, .. }
            | R::AddConnector { locals, .. } => locals,
            R::AddBookmark { local, .. } | R::AddMark { local, .. } => local.as_slice(),
            _ => &[],
        }
    }
}

/// Resolves one op's references against a transaction's state: what the
/// op reads to resolve them is noted ([`reads`](Self::reads)).
pub(super) struct Resolver<'a> {
    pub(super) st: &'a DocState,
    pub(super) locals: &'a im::HashMap<Local, Made>,
    pub(super) cx: &'a LowerCx,
    pub(super) grants: &'a [Grant],
    pub(super) named: &'a HashMap<String, usize>,
    pub(super) op: &'static str,
    /// The slots resolving read: lowering reads (§6.3).
    pub(super) reads: BTreeSet<Key>,
}

impl Resolver<'_> {
    fn not_found(&self, what: impl std::fmt::Display) -> EditError {
        EditError::not_found(self.op, what)
    }

    fn invalid(&self, why: impl std::fmt::Display) -> EditError {
        EditError::invalid(self.op, why)
    }

    /// `op`, resolved.
    pub(super) fn op(&mut self, op: &Op) -> Result<ResolvedOp, EditError> {
        use ResolvedOp as R;
        self.declared(op)?;
        Ok(match op {
            Op::MovePages { pages, before } => {
                let pages = self.pages(pages)?;
                let before = self.maybe_page(before.as_ref())?;
                if before.is_some_and(|b| pages.contains(&b)) {
                    return Err(self.invalid("pages can't move before one of themselves"));
                }
                R::MovePages { pages, before }
            }
            Op::RotatePages { pages, by } => R::RotatePages {
                pages: self.pages(pages)?,
                by: *by,
            },
            Op::DeletePages { pages } => R::DeletePages {
                pages: self.pages(pages)?,
            },
            Op::DuplicatePages { pages, locals } => R::DuplicatePages {
                pages: self.pages(pages)?,
                locals: locals.clone(),
            },
            Op::InsertPages {
                from,
                pages,
                before,
                bring,
                locals,
            } => {
                let (from, name) = self.source(from)?;
                R::InsertPages {
                    pages: self.source_pages(&from, pages.as_ref())?,
                    from,
                    name,
                    before: self.maybe_page(before.as_ref())?,
                    bring: bring.clone(),
                    locals: locals.clone(),
                }
            }
            Op::ReplacePages {
                pages,
                from,
                with,
                bring,
            } => {
                let (from, name) = self.source(from)?;
                let mut ids = Vec::with_capacity(pages.len());
                for p in pages {
                    ids.push(self.page(p)?);
                }
                R::ReplacePages {
                    pages: once_each(ids),
                    with: self.source_pages(&from, with.as_ref())?,
                    from,
                    name,
                    bring: bring.clone(),
                }
            }
            Op::SetPageBoxes {
                pages,
                crop,
                trim,
                bleed,
                art,
            } => R::SetPageBoxes {
                pages: self.pages(pages)?,
                crop: crop.clone(),
                trim: trim.clone(),
                bleed: bleed.clone(),
                art: art.clone(),
            },
            Op::SetPageLabels { ranges } => R::SetPageLabels {
                ranges: match ranges {
                    None => None,
                    Some(ranges) => Some(
                        (ranges.iter())
                            .map(|r| {
                                Ok(LabelSpec {
                                    start: self.page(&r.start)?,
                                    style: r.style,
                                    prefix: r.prefix.clone(),
                                    first: r.first,
                                })
                            })
                            .collect::<Result<_, EditError>>()?,
                    ),
                },
            },
            Op::AddBookmark {
                parent,
                at,
                title,
                target,
                view,
                color,
                style,
                local,
            } => {
                let parent = self.maybe_bookmark(parent.as_ref())?;
                R::AddBookmark {
                    at: self.sibling(parent, at, None)?,
                    parent,
                    title: title.clone(),
                    target: self.maybe_page(target.as_ref())?,
                    view: view.clone(),
                    color: *color,
                    style: *style,
                    local: *local,
                }
            }
            Op::PatchBookmarks { bookmarks, patch } => {
                let BookmarkPatch {
                    title,
                    target,
                    view,
                    color,
                    style,
                } = patch;
                R::PatchBookmarks {
                    bookmarks: self.bookmarks(bookmarks)?,
                    patch: BookmarkChange {
                        title: title.clone(),
                        target: match target {
                            Patch::Keep => Patch::Keep,
                            Patch::Clear => Patch::Clear,
                            Patch::Set(p) => Patch::Set(self.page(p)?),
                        },
                        view: view.clone(),
                        color: color.clone(),
                        style: *style,
                    },
                }
            }
            Op::MoveBookmarks {
                bookmarks,
                parent,
                at,
                with_pages,
            } => {
                let bookmarks = self.bookmarks(bookmarks)?;
                let parent = self.maybe_bookmark(parent.as_ref())?;
                let under = |&id: &OutlineId| {
                    parent.is_some_and(|p| super::bookmarks::subtree(self.st, id).contains(&p))
                };
                if bookmarks.iter().any(under) {
                    return Err(self.invalid("a bookmark can't go under itself"));
                }
                R::MoveBookmarks {
                    at: self.sibling(parent, at, Some(&bookmarks))?,
                    bookmarks,
                    parent,
                    with_pages: *with_pages,
                }
            }
            Op::DeleteBookmarks { bookmarks } => R::DeleteBookmarks {
                bookmarks: self.bookmarks(bookmarks)?,
            },
            Op::AddMark {
                page,
                mark,
                reply_to,
                local,
                imported,
            } => {
                let held = match &mark.asset {
                    Some(sel) => Some(self.held(sel, &mark.kind)?),
                    None => None,
                };
                let page = self.page(page)?;
                let reply_to = self.maybe_annot(reply_to.as_ref())?;
                self.threaded(page, &mark.kind, reply_to)?;
                R::AddMark {
                    page,
                    mark: NewMark {
                        asset: None,
                        ..mark.clone()
                    },
                    held,
                    reply_to,
                    local: *local,
                    imported: imported.clone(),
                }
            }
            Op::AddConnector {
                a,
                b,
                label,
                style,
                locals,
            } => R::AddConnector {
                a: (self.page(&a.page)?, a.rect),
                b: (self.page(&b.page)?, b.rect),
                label: label.clone(),
                style: style.clone(),
                locals: locals.clone(),
            },
            Op::PatchMarks {
                marks,
                patch,
                imported,
            } => {
                let marks = self.marks(marks)?;
                // An imported comment's name is the mark's own.
                if let Some(imp) = imported {
                    for &id in &marks {
                        let name = self.st.annot(id).expect("resolved").name();
                        if imp.nm != name {
                            return Err(self.invalid(format!(
                                "the comment's name “{}” isn't the mark's, “{name}”",
                                imp.nm
                            )));
                        }
                    }
                }
                R::PatchMarks {
                    marks,
                    patch: patch.clone(),
                    imported: imported.clone(),
                }
            }
            Op::MoveMarks { marks, by, to_page } => {
                let marks = self.marks(marks)?;
                let to_page = self.maybe_page(to_page.as_ref())?;
                if let Some(to) = to_page {
                    self.stay_in_threads(&marks, to)?;
                }
                R::MoveMarks {
                    marks,
                    by: *by,
                    to_page,
                }
            }
            Op::DeleteAnnotations { annots } => {
                let annots = self.annots(annots)?;
                let widget = |&id: &AnnotId| self.st.annot(id).and_then(|a| a.widget()).is_some();
                if annots.iter().any(widget) {
                    return Err(self.invalid("a form field's widget goes only with its page"));
                }
                R::DeleteAnnotations { annots }
            }
            Op::AdoptAnnotations { annots } => {
                let annots = self.annots(annots)?;
                let mut adopted = Vec::with_capacity(annots.len());
                for &id in &annots {
                    let rec = self.st.annot(id).expect("resolved");
                    let Some(f) = rec.foreign() else {
                        return Err(self
                            .invalid(format!("annotation {} isn't another tool's comment", id.0)));
                    };
                    if f.summary.mark.is_none() || f.summary.is_link() {
                        return Err(self.invalid("sheaf can't hold that kind of comment"));
                    }
                    adopted.push((rec.page, f));
                }
                // A part grouped under a comment goes when the comment is
                // adopted, which draws it: it isn't adopted beside it.
                let under = |page, parent: &str| {
                    (adopted.iter()).any(|&(p, f)| p == page && f.summary.name == parent)
                };
                let part = adopted.iter().any(|&(page, f)| {
                    f.summary.grouped && f.summary.parent.as_deref().is_some_and(|p| under(page, p))
                });
                if part {
                    return Err(self.invalid(
                        "a part grouped under a comment goes with it: adopt the comment alone",
                    ));
                }
                for &(page, f) in &adopted {
                    self.threaded_on_its_page(page, f)?;
                }
                R::AdoptAnnotations { annots }
            }
            Op::SetOcrText { page, layer } => R::SetOcrText {
                page: self.page(page)?,
                layer: layer.clone(),
            },
            Op::ClearOcrText { pages } => R::ClearOcrText {
                pages: self.pages(pages)?,
            },
            Op::SetWatermark { spec, z, scope } => R::SetWatermark {
                content: self.watermark(&spec.content, *z)?,
                position: spec.position,
                z: *z,
                scope: match scope {
                    ScopeSel::All => None,
                    ScopeSel::Pages(pages) => Some(self.pages(pages)?.into_iter().collect()),
                },
            },
            Op::RemoveWatermark {
                z,
                pages,
                include_authored,
            } => R::RemoveWatermark {
                z: *z,
                pages: self.pages(pages)?,
                include_authored: *include_authored,
                others: OthersFound::default(),
            },
            Op::SetHeaderFooter { spec, scope } => R::SetHeaderFooter {
                spec: spec.clone(),
                scope: match scope {
                    ScopeSel::All => None,
                    ScopeSel::Pages(pages) => Some(self.pages(pages)?.into_iter().collect()),
                },
            },
            Op::RemoveHeaderFooter {
                pages,
                include_authored,
            } => R::RemoveHeaderFooter {
                pages: self.pages(pages)?,
                include_authored: *include_authored,
                others: OthersFound::default(),
            },
            _ => return Err(EditError::internal(format!("{op:?} has no lowering"))),
        })
    }

    /// The locals `op` names what it makes by: none named before in the
    /// transaction, and none twice.
    fn declared(&self, op: &Op) -> Result<(), EditError> {
        let named: Vec<Local> = match op {
            Op::DuplicatePages { locals, .. }
            | Op::InsertPages { locals, .. }
            | Op::AddConnector { locals, .. } => locals.clone(),
            Op::AddBookmark { local, .. } | Op::AddMark { local, .. } => {
                local.iter().copied().collect()
            }
            _ => Vec::new(),
        };
        for (i, l) in named.iter().enumerate() {
            if self.locals.contains_key(l) || named[..i].contains(l) {
                return Err(self.invalid(format!("{l} names something already")));
            }
        }
        Ok(())
    }

    /// What local `l` names.
    fn local(&self, l: Local) -> Result<Made, EditError> {
        (self.locals.get(&l).copied()).ok_or_else(|| self.not_found(l))
    }

    pub(super) fn page(&mut self, r: &PageRef) -> Result<PageId, EditError> {
        let id = match *r {
            PageRef::Id(id) => id,
            PageRef::New(l) => match self.local(l)? {
                Made::Page(id) => id,
                _ => return Err(self.invalid(format!("{l} isn't a page"))),
            },
            PageRef::Pos(pos) => {
                self.reads.insert(Key::Order);
                let at = (pos as usize)
                    .checked_sub(1)
                    .and_then(|i| self.st.page_at(i));
                let count = self.st.page_count();
                return at.ok_or_else(|| self.not_found(format!("page {pos} of {count}")));
            }
        };
        match self.st.def(id) {
            Some(_) => Ok(id),
            None => Err(self.not_found(format!("page {}", id.0))),
        }
    }

    fn maybe_page(&mut self, r: Option<&PageRef>) -> Result<Option<PageId>, EditError> {
        r.map(|r| self.page(r)).transpose()
    }

    /// The pages `sel` names, each once, in the order it names them.
    pub(super) fn pages(&mut self, sel: &PageSel) -> Result<Vec<PageId>, EditError> {
        let mut out = Vec::new();
        match sel {
            PageSel::Ids(refs) => {
                for r in refs {
                    out.push(self.page(r)?);
                }
                out = once_each(out);
            }
            PageSel::All => {
                self.reads.insert(Key::Order);
                out.extend(self.st.order().iter().copied());
            }
            PageSel::Span { first, last } => {
                let (a, b) = (self.page(first)?, self.page(last)?);
                self.reads.insert(Key::Order);
                let pos = |id| self.st.pos_of(id).expect("a page there is");
                let (a, b) = (pos(a), pos(b));
                let (a, b) = (a.min(b), a.max(b));
                out.extend(self.st.order().iter().skip(a).take(b - a + 1).copied());
            }
            PageSel::Positions(set) => {
                self.reads.insert(Key::Order);
                let count = self.st.page_count();
                for pos in set.iter() {
                    let at = (pos as usize)
                        .checked_sub(1)
                        .and_then(|i| self.st.page_at(i));
                    out.push(at.ok_or_else(|| self.not_found(format!("page {pos} of {count}")))?);
                }
            }
            PageSel::Bookmark(r) => {
                let id = self.bookmark(r)?;
                self.reads.insert(Key::Order);
                out = (super::bookmarks::bookmark_span(self.st, id))
                    .filter(|span| !span.is_empty())
                    .ok_or_else(|| self.invalid("the bookmark goes to no page"))?;
            }
            _ => return Err(EditError::internal(format!("{sel:?} isn't resolved"))),
        }
        Ok(out)
    }

    fn bookmark(&mut self, r: &BookmarkRef) -> Result<OutlineId, EditError> {
        let id = match r {
            BookmarkRef::Id(id) => *id,
            BookmarkRef::New(l) => match self.local(*l)? {
                Made::Bookmark(id) => id,
                _ => return Err(self.invalid(format!("{l} isn't a bookmark"))),
            },
            BookmarkRef::Path(titles) => return self.bookmark_at(titles),
        };
        match self.st.bookmark(id) {
            Some(_) => Ok(id),
            None => Err(self.not_found(format!("bookmark {}", id.0))),
        }
    }

    /// The bookmark at the end of `titles`, from the top of the outline
    /// down.
    fn bookmark_at(&mut self, titles: &[String]) -> Result<OutlineId, EditError> {
        let mut kids = self.st.outline_roots().clone();
        self.reads.insert(Key::OutlineRoots);
        let mut at = None;
        for (depth, title) in titles.iter().enumerate() {
            let path = || titles[..=depth].join(" › ");
            let mut found = (kids.iter().copied())
                .filter(|&id| self.st.bookmark(id).is_some_and(|b| *b.title == **title));
            let id = found
                .next()
                .ok_or_else(|| self.not_found(format!("bookmark “{}”", path())))?;
            if found.next().is_some() {
                return Err(EditError::Ambiguous {
                    op: self.op,
                    what: format!("“{}”", path()),
                });
            }
            self.reads.insert(Key::Bookmark(id));
            kids = self.st.bookmark(id).expect("found").kids.clone();
            at = Some(id);
        }
        at.ok_or_else(|| self.invalid("a bookmark's path names at least its own title"))
    }

    fn maybe_bookmark(&mut self, r: Option<&BookmarkRef>) -> Result<Option<OutlineId>, EditError> {
        r.map(|r| self.bookmark(r)).transpose()
    }

    fn bookmarks(&mut self, refs: &[BookmarkRef]) -> Result<Vec<OutlineId>, EditError> {
        let mut out = Vec::with_capacity(refs.len());
        for r in refs {
            out.push(self.bookmark(r)?);
        }
        Ok(once_each(out))
    }

    /// Where under `parent` a bookmark goes; one `moving` can't go after
    /// one of those moving.
    fn sibling(
        &mut self,
        parent: Option<OutlineId>,
        at: &Sibling,
        moving: Option<&[OutlineId]>,
    ) -> Result<At, EditError> {
        Ok(match at {
            Sibling::First => At::First,
            Sibling::Last => At::Last,
            Sibling::After(r) => {
                let after = self.bookmark(r)?;
                let kids = match parent {
                    None => self.st.outline_roots().clone(),
                    Some(p) => self.st.bookmark(p).expect("resolved").kids.clone(),
                };
                if !kids.contains(&after) {
                    return Err(self.invalid("the bookmark it goes after isn't among its siblings"));
                }
                if moving.is_some_and(|m| m.contains(&after)) {
                    return Err(self.invalid("a bookmark can't go after itself"));
                }
                At::After(after)
            }
        })
    }

    fn annot(&mut self, r: &AnnotRef) -> Result<AnnotId, EditError> {
        let id = match r {
            AnnotRef::Id(id) => *id,
            AnnotRef::New(l) => match self.local(*l)? {
                Made::Annot(id) => id,
                _ => return Err(self.invalid(format!("{l} isn't an annotation"))),
            },
            AnnotRef::Nm { nm, page } => {
                let pages = match page {
                    Some(p) => vec![self.page(p)?],
                    None => {
                        self.reads.insert(Key::Order);
                        self.st.order().iter().copied().collect()
                    }
                };
                let mut found = Vec::new();
                for p in pages {
                    self.reads.insert(Key::PageAnnots(p));
                    found.extend(self.st.annots_named(p, nm));
                }
                return match found.as_slice() {
                    [] => Err(self.not_found(format!("an annotation named “{nm}”"))),
                    [one] => Ok(*one),
                    _ => Err(EditError::Ambiguous {
                        op: self.op,
                        what: format!("“{nm}”"),
                    }),
                };
            }
        };
        match self.st.annot(id) {
            Some(_) => Ok(id),
            None => Err(self.not_found(format!("annotation {}", id.0))),
        }
    }

    fn maybe_annot(&mut self, r: Option<&AnnotRef>) -> Result<Option<AnnotId>, EditError> {
        r.map(|r| self.annot(r)).transpose()
    }

    fn annots(&mut self, refs: &[AnnotRef]) -> Result<Vec<AnnotId>, EditError> {
        let mut out = Vec::with_capacity(refs.len());
        for r in refs {
            out.push(self.annot(r)?);
        }
        Ok(once_each(out))
    }

    /// The annotations `refs` names, each one of sheaf's marks.
    fn marks(&mut self, refs: &[AnnotRef]) -> Result<Vec<AnnotId>, EditError> {
        let ids = self.annots(refs)?;
        for &id in &ids {
            if self.st.annot(id).and_then(|a| a.mark()).is_none() {
                return Err(self.invalid(format!(
                    "annotation {} isn't one of sheaf's marks: make it one first",
                    id.0
                )));
            }
        }
        Ok(ids)
    }

    /// Whether a mark of `kind` on `page` may be in the thread of
    /// `reply_to`: a reply or a status is, to a comment on its page, and
    /// nothing else is in one.
    fn threaded(
        &self,
        page: PageId,
        kind: &MarkKind,
        reply_to: Option<AnnotId>,
    ) -> Result<(), EditError> {
        let threaded = matches!(kind, MarkKind::Reply {} | MarkKind::Status { .. });
        match reply_to {
            Some(id) if threaded => {
                let rec = self.st.annot(id).expect("resolved");
                if rec.page != page {
                    return Err(self.invalid("a reply goes on the page of the comment it's to"));
                }
                if rec.widget().is_some() {
                    return Err(self.invalid("a form field's widget has no replies"));
                }
                Ok(())
            }
            Some(_) => Err(self.invalid("only a reply or a status goes in a thread")),
            None => Ok(()),
        }
    }

    /// Whether another tool's comment `f` on `page` may be adopted as its
    /// threads are: a reply sheaf holds is to a comment on its page, as
    /// an added one is (§3.6). So not a reply to a comment on another
    /// page, nor a comment one of sheaf's replies on another page is to.
    fn threaded_on_its_page(&self, page: PageId, f: &Foreign) -> Result<(), EditError> {
        let mark = f.summary.mark.as_deref().expect("one sheaf can hold");
        let parent =
            (mark.thread_parent()).and(f.summary.parent.as_deref().or(mark.thread_parent()));
        if let Some(parent) = parent
            && self.st.annot_by_nm(page, parent).is_none()
            && (self.st.annots()).any(|(_, a)| a.page != page && a.name() == parent)
        {
            return Err(self.invalid("a reply goes on the page of the comment it's to"));
        }
        let name = f.summary.name.as_str();
        let elsewhere = |(at, m): (PageId, &crate::annot::Annot)| {
            at != page && m.thread_parent() == Some(name) && self.st.annot_by_nm(at, name).is_none()
        };
        if !name.is_empty() && self.st.marks().any(elsewhere) {
            return Err(self.invalid("a reply to it is on another page"));
        }
        Ok(())
    }

    /// Whether each of `marks` may move onto page `to`: a mark in a thread,
    /// a reply or one replied to, stays on its page, where threads are by
    /// `/NM` (§3.6), unless it's there already.
    fn stay_in_threads(&self, marks: &[AnnotId], to: PageId) -> Result<(), EditError> {
        for &id in marks {
            let rec = self.st.annot(id).expect("resolved");
            if rec.page == to {
                continue;
            }
            let name = rec.name();
            let replied = (self.st.annots_on(rec.page)).any(|(_, a)| a.parent() == Some(&name));
            if rec.parent().is_some() || replied {
                return Err(
                    self.invalid("a comment in a thread moves to another page only with its page")
                );
            }
        }
        Ok(())
    }

    /// The blob `sel` names: by id from the store, which a plugin may not
    /// name (it uses its grants), or by a grant.
    fn blob(&self, sel: &BlobSel) -> Result<Grant, EditError> {
        match sel {
            BlobSel::Id(id) => {
                if matches!(self.cx.actor, super::Actor::Plugin { .. }) {
                    return Err(EditError::Refused(super::Cap::User));
                }
                let blob = (self.cx.store.get(*id))
                    .ok_or_else(|| self.not_found(format!("blob {}", id.stem())))?;
                Ok(Grant::Blob(blob.into()))
            }
            BlobSel::With(name) => (self.named.get(name))
                .map(|&n| self.grants[n].clone())
                .ok_or_else(|| self.not_found(format!("the file named “{name}”"))),
            BlobSel::Asset(n) => (self.grants.get(*n as usize).cloned())
                .ok_or_else(|| self.not_found(format!("asset {n}"))),
        }
    }

    /// What a watermark of `content` draws, its file as `content` names
    /// it held: an image's an image file, a page's a PDF that has the
    /// page.
    fn watermark(
        &self,
        content: &sheaf_edit::WatermarkContent,
        z: Z,
    ) -> Result<WatermarkContent, EditError> {
        use sheaf_edit::WatermarkContent as C;
        // What the watermark at `z` draws now, which one naming no file
        // keeps.
        let drawn = || self.st.watermark(z).and_then(|g| g.spec.watermark());
        Ok(match content {
            C::Text {
                text,
                font,
                size,
                color,
                opacity,
                angle,
            } => WatermarkContent::Text {
                text: text.as_str().into(),
                font: *font,
                size: *size,
                color: *color,
                opacity: *opacity,
                angle: *angle,
            },
            C::Image {
                image: None,
                scale,
                opacity,
            } => match drawn().map(|wm| &wm.content) {
                Some(WatermarkContent::Image { image, .. }) => WatermarkContent::Image {
                    image: image.clone(),
                    scale: *scale,
                    opacity: *opacity,
                },
                _ => return Err(self.invalid("no image is drawn at its z to keep")),
            },
            C::Page {
                pdf: None,
                scale,
                opacity,
                ..
            } => match drawn().map(|wm| &wm.content) {
                Some(WatermarkContent::Page { page, .. }) => WatermarkContent::Page {
                    page: page.clone(),
                    scale: *scale,
                    opacity: *opacity,
                },
                _ => return Err(self.invalid("no page is drawn at its z to keep")),
            },
            C::Image {
                image: Some(image),
                scale,
                opacity,
            } => {
                // By its bytes: a blob keeps the kind it was first
                // interned as, an attachment's say.
                let image = self.file(image)?;
                if BlobKind::image(image.bytes()).is_none() {
                    return Err(self.invalid("the watermark's image isn't an image file"));
                }
                WatermarkContent::Image {
                    image: ImageRef::Blob(image),
                    scale: *scale,
                    opacity: *opacity,
                }
            }
            C::Page {
                pdf: Some(pdf),
                page,
                scale,
                opacity,
            } => {
                // By its bytes, as an image is.
                let pdf = self.file(pdf)?;
                if BlobKind::image(pdf.bytes()).is_some() {
                    return Err(self.invalid("the watermark's page isn't a PDF's"));
                }
                let src = (self.cx.store.source(pdf.blob()))
                    .map_err(|e| self.invalid(format!("the watermark's PDF can't be read: {e}")))?;
                let count = src.page_count();
                let index = match page.checked_sub(1) {
                    None => return Err(self.invalid("the watermark's page is 1-based")),
                    Some(index) if index >= count => {
                        return Err(self.not_found(format!("page {page} of the file's {count}")));
                    }
                    Some(index) => index,
                };
                WatermarkContent::Page {
                    page: PageForm::Blob { pdf, index },
                    scale: *scale,
                    opacity: *opacity,
                }
            }
        })
    }

    /// The file `sel` names: a blob, not a stamp's page.
    fn file(&self, sel: &BlobSel) -> Result<BlobRef, EditError> {
        match self.blob(sel)? {
            Grant::Blob(b) => Ok(b),
            Grant::Stamp { .. } => Err(self.invalid("a stamp's page isn't a file")),
        }
    }

    /// The source `from` is, and the name it goes by.
    fn source(&self, from: &PageSource) -> Result<(Src, Arc<str>), EditError> {
        let PageSource::File { file, name } = from;
        let blob = match self.blob(file)? {
            Grant::Blob(b) => b,
            Grant::Stamp { .. } => return Err(self.invalid("a stamp's page isn't a file of pages")),
        };
        let src = (self.cx.store.source(blob.blob()))
            .map_err(|e| self.invalid(format!("{name} can't be read as a PDF: {e}")))?;
        Ok((src, name.as_str().into()))
    }

    /// The pages of `src` that `pages` names, 1-based, as 0-based indexes;
    /// every page when none.
    fn source_pages(
        &self,
        src: &Src,
        pages: Option<&sheaf_edit::RangeSet>,
    ) -> Result<Vec<u32>, EditError> {
        let count = src.page_count();
        match pages {
            None => Ok((0..count).collect()),
            Some(set) if set.last() > count => {
                Err(self.not_found(format!("page {} of the file's {count}", set.last())))
            }
            Some(set) => Ok(set.iter().map(|p| p - 1).collect()),
        }
    }

    /// What a mark of `kind` holds as `sel` names it: a page stamp's page,
    /// or an attachment's file.
    fn held(&self, sel: &BlobSel, kind: &MarkKind) -> Result<Grant, EditError> {
        let grant = self.blob(sel)?;
        Ok(match (kind, grant) {
            (MarkKind::PageStamp { .. }, Grant::Blob(b)) => Grant::Stamp {
                stem: b.id().stem(),
                form: crate::model::FormRef::Blob(b),
            },
            (MarkKind::PageStamp { .. }, stamp @ Grant::Stamp { .. }) => stamp,
            (MarkKind::FileAttachment { .. }, file @ Grant::Blob(_)) => file,
            (MarkKind::FileAttachment { .. }, Grant::Stamp { .. }) => {
                return Err(self.invalid("an attachment holds a file, not a stamp's page"));
            }
            _ => return Err(self.invalid("only page stamps and attachments hold an asset")),
        })
    }
}

/// `ids`, each where it's first named.
fn once_each<T: Copy + Eq + std::hash::Hash>(ids: Vec<T>) -> Vec<T> {
    let mut seen = std::collections::HashSet::with_capacity(ids.len());
    ids.into_iter().filter(|&id| seen.insert(id)).collect()
}

/// What the marks on pages `pages` of `src` hold that the document's store
/// may not have yet, decoded and interned there for an insert to lower:
/// each attachment's file, and the pages of stamps found by name
/// ([`find_stamp`](crate::model::find_stamp), the generated cache
/// included until slice 19). The caller pins them in the transaction
/// ([`Tx::pin`](super::Tx::pin)) until it's committed. Off the UI thread
/// when the caller can (§3.1). A file that can't be decoded is said, and
/// its mark holds none.
pub fn incoming(store: &Store, primary: &Src, src: &Src, pages: &[u32]) -> Vec<Blob> {
    let fact = src.annots().clone();
    let mut out = Vec::new();
    for &index in pages {
        for f in fact.on(index) {
            let FileBody::Mark { annot, held } = &f.body else {
                continue;
            };
            let found = match (&annot.kind, held) {
                (AnnotKind::FileAttachment { name, size, .. }, Some(stream)) => {
                    crate::model::file_in(store, src, *stream, *size)
                        .inspect_err(|e| log::warn!("attachment {name}: {e:#}"))
                        .ok()
                }
                (AnnotKind::FileAttachment { file, size, .. }, None) => {
                    crate::model::find_file(store, primary, file, *size)
                }
                (AnnotKind::PageStamp { stamp, .. }, None) => {
                    match crate::model::find_stamp(store, primary, stamp) {
                        Some(crate::model::FormRef::Blob(b)) => Some(b),
                        _ => None,
                    }
                }
                _ => None,
            };
            out.extend(found.map(|b: BlobRef| b.blob().clone()));
        }
    }
    out
}

/// What an insert of pages `pages` of `src` reads, read now, off the UI
/// thread when the caller can (§3.1): the source's marks, form and outline,
/// which lowering takes as facts, and what its marks hold ([`incoming`]),
/// for the caller to pin.
pub fn ready_to_insert(store: &Store, primary: &Src, src: &Src, pages: &[u32]) -> Vec<Blob> {
    // Each is read once, and kept with the source.
    src.form();
    src.outline();
    incoming(store, primary, src, pages)
}

/// The file blob `stem` names in `store`: an inserted mark's attachment,
/// which [`incoming`] interned.
pub(super) fn file_named(store: &Store, stem: &str) -> Option<BlobRef> {
    let id = BlobId::from_stem(stem)?;
    store.get(id).map(BlobRef::from)
}
