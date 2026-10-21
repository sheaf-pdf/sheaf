//! Ops as tests write them (M10.5 slice 11): a version made of the user's
//! ops, the pages, bookmarks and files they name, and a fixture's marks
//! said as the ops that make and change them. For tests here and in other
//! crates (the `fixtures` feature).

use std::sync::Arc;

use in_progress::NoProgress;
use sheaf_edit::{
    Anchor, AnnotRef, BlobSel, BookmarkPatch, BookmarkRef, Box2, BoxSpec, Bring, FontChoice,
    GridPos, HeaderFooterSpec, LabelStyle, Margins, NewMark, Op, PageLabelRange, PageRef, PageSel,
    PageSource, Patch, RangeSet, ScopeSel, Sibling, Turn, WatermarkContent, WatermarkSpec, Z,
};

use super::{Actor, Commit, CommitOpts, Derive, EditError, JobKind, Tx};
use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::labels::LabelSpec;
use crate::model::{Document, GroupId, GroupKind, OutlineId, PageId};
use crate::store::{BlobId, Src};
use crate::text_layer::TextLayer;

impl Document {
    /// `ops` as one version of the user's, labelled `label`: all of them,
    /// or none and why.
    pub fn commit_ops(&mut self, ops: Vec<Op>, label: &str) -> Result<Commit, EditError> {
        let tx = self.lowered(ops)?;
        self.commit(tx, CommitOpts::labelled(label))
    }

    /// `ops` as group `g`'s next commit, labelled `label`, by whom the
    /// group's kind takes commits from: the OCR job, or the plugin called
    /// (as version 1.0). One in a group that has ended is the user's.
    pub fn commit_ops_in_group(
        &mut self,
        g: GroupId,
        ops: Vec<Op>,
        label: &str,
    ) -> Result<Commit, EditError> {
        let actor = match self.group_kind(g) {
            Some(GroupKind::Ocr) => Actor::Job(JobKind::Ocr),
            Some(GroupKind::PluginCall { plugin }) => Actor::Plugin {
                id: plugin.clone(),
                version: "1.0".into(),
            },
            None => Actor::User,
        };
        let tx = self.lowered_as(actor, ops)?;
        let opts = CommitOpts {
            group: Some(g),
            ..CommitOpts::labelled(label)
        };
        self.commit(tx, opts)
    }

    /// Each of `pages` shown from page `index` of `src`, a copy a derived
    /// flow made as `why` says ([`Tx::set_page_base`]), as a version of the
    /// user's labelled `label`.
    pub fn commit_page_base(
        &mut self,
        pages: Vec<(PageId, Src, u32)>,
        why: Derive,
        label: &str,
    ) -> Result<Commit, EditError> {
        let mut tx = self.tx(Actor::User);
        tx.set_page_base(pages, why)?;
        self.commit(tx, CommitOpts::labelled(label))
    }

    /// `ops` lowered in a transaction of the user's, what the pages they
    /// insert bring held as a caller holds it ([`hold_incoming`]).
    fn lowered(&self, ops: Vec<Op>) -> Result<Tx, EditError> {
        self.lowered_as(Actor::User, ops)
    }

    /// [`lowered`](Self::lowered), in a transaction of `actor`'s.
    fn lowered_as(&self, actor: Actor, ops: Vec<Op>) -> Result<Tx, EditError> {
        let mut tx = self.tx(actor);
        hold_incoming(&mut tx, &ops);
        tx.apply_batch(ops, &NoProgress)?;
        Ok(tx)
    }
}

/// What the pages `ops` insert or put in place from files the store holds
/// bring, decoded and pinned in `tx` ([`incoming`](super::incoming)), as
/// a caller resolving its ops does: their marks' attachments and stamp
/// pages.
pub fn hold_incoming(tx: &mut Tx, ops: &[Op]) {
    for op in ops {
        let (from, pages) = match op {
            Op::InsertPages { from, pages, .. } => (from, pages),
            Op::ReplacePages { from, with, .. } => (from, with),
            _ => continue,
        };
        let PageSource::File {
            file: BlobSel::Id(id),
            ..
        } = from
        else {
            continue;
        };
        let cx = tx.cx.clone();
        let Some(src) = (cx.store.get(*id)).and_then(|b| cx.store.source(&b).ok()) else {
            continue;
        };
        let pages: Vec<u32> = match pages {
            Some(set) => set.iter().map(|p| p - 1).collect(),
            None => (0..src.page_count()).collect(),
        };
        for blob in super::incoming(&cx.store, &cx.primary, &src, &pages) {
            tx.pin(blob);
        }
    }
}

/// Pages `pages`.
pub fn ids(pages: &[PageId]) -> PageSel {
    PageSel::Ids(refs(pages))
}

/// `pages` moved before `before`, or to the end.
pub fn move_pages(pages: &[PageId], before: Option<PageId>) -> Op {
    Op::MovePages {
        pages: ids(pages),
        before: before.map(PageRef::Id),
    }
}

pub fn delete(pages: &[PageId]) -> Op {
    Op::DeletePages { pages: ids(pages) }
}

pub fn duplicate(pages: &[PageId]) -> Op {
    Op::DuplicatePages {
        pages: ids(pages),
        locals: Vec::new(),
    }
}

/// The recognized text taken off `pages`.
pub fn clear_ocr(pages: &[PageId]) -> Op {
    Op::ClearOcrText { pages: ids(pages) }
}

/// Annotations `annots`, and their threads, deleted.
pub fn delete_annots(annots: &[AnnotId]) -> Op {
    Op::DeleteAnnotations {
        annots: annots.iter().map(|&id| AnnotRef::Id(id)).collect(),
    }
}

/// The comments on `page` named `names` deleted, and their threads.
pub fn delete_named(page: PageId, names: &[&str]) -> Op {
    let named = |&nm: &&str| AnnotRef::Nm {
        nm: nm.into(),
        page: Some(PageRef::Id(page)),
    };
    Op::DeleteAnnotations {
        annots: names.iter().map(named).collect(),
    }
}

/// Bookmark `id` changed as `patch` says.
pub fn patch_bookmark(id: OutlineId, patch: BookmarkPatch) -> Op {
    Op::PatchBookmarks {
        bookmarks: vec![BookmarkRef::Id(id)],
        patch,
    }
}

/// Bookmark `id` and what's under it deleted.
pub fn delete_bookmark(id: OutlineId) -> Op {
    Op::DeleteBookmarks {
        bookmarks: vec![BookmarkRef::Id(id)],
    }
}

/// Pages `pages`, one by one.
pub fn refs(pages: &[PageId]) -> Vec<PageRef> {
    pages.iter().map(|&id| PageRef::Id(id)).collect()
}

/// A file's pages at `indexes` (from 0), as the positions (from 1) an op
/// names them by.
pub fn positions(indexes: impl IntoIterator<Item = u32>) -> RangeSet {
    RangeSet::from_runs(indexes.into_iter().map(|i| i + 1..=i + 1)).expect("a page")
}

/// `pages` turned by `by`.
pub fn rotate(pages: &[PageId], by: Turn) -> Op {
    Op::RotatePages {
        pages: ids(pages),
        by,
    }
}

/// `pages` cropped by `m`, off each side as it shows.
pub fn crop(pages: &[PageId], m: Margins) -> Op {
    Op::SetPageBoxes {
        pages: ids(pages),
        crop: Patch::Set(BoxSpec::Margins(m)),
        trim: Patch::Keep,
        bleed: Patch::Keep,
        art: Patch::Keep,
    }
}

/// `page` cropped to `to`, in PageSpace.
pub fn crop_to(page: PageId, to: Box2) -> Op {
    Op::SetPageBoxes {
        pages: ids(&[page]),
        crop: Patch::Set(BoxSpec::Rect(to)),
        trim: Patch::Keep,
        bleed: Patch::Keep,
        art: Patch::Keep,
    }
}

/// File `src`, as the reader picked it by `name`.
pub fn file(src: &Src, name: &str) -> PageSource {
    PageSource::File {
        file: BlobSel::Id(src.id()),
        name: name.into(),
    }
}

/// Pages `pages` (from 0) of `src`, named `name`, before `before` or at
/// the end, with what they bring.
pub fn insert(
    src: &Src,
    pages: impl IntoIterator<Item = u32>,
    name: &str,
    before: Option<PageId>,
) -> Op {
    Op::InsertPages {
        from: file(src, name),
        pages: Some(positions(pages)),
        before: before.map(PageRef::Id),
        bring: Bring::default(),
        locals: Vec::new(),
    }
}

/// Pages `pages` shown from pages `with` (from 0) of `src`, named `name`.
pub fn replace(pages: &[PageId], src: &Src, with: impl IntoIterator<Item = u32>, name: &str) -> Op {
    Op::ReplacePages {
        pages: refs(pages),
        from: file(src, name),
        with: Some(positions(with)),
        bring: Bring::default(),
    }
}

/// Recognized text `layers`, each on its page.
pub fn ocr(layers: impl IntoIterator<Item = (PageId, Arc<TextLayer>)>) -> Vec<Op> {
    (layers.into_iter())
        .map(|(page, layer)| Op::SetOcrText {
            page: PageRef::Id(page),
            layer,
        })
        .collect()
}

/// After `after` among its siblings, or first.
pub fn sibling(after: Option<OutlineId>) -> Sibling {
    match after {
        Some(a) => Sibling::After(BookmarkRef::Id(a)),
        None => Sibling::First,
    }
}

/// A bookmark titled `title` under `parent`, after `after` there, going to
/// the top of `target`.
pub fn bookmark(
    parent: Option<OutlineId>,
    after: Option<OutlineId>,
    title: &str,
    target: Option<PageId>,
) -> Op {
    Op::AddBookmark {
        parent: parent.map(BookmarkRef::Id),
        at: sibling(after),
        title: title.into(),
        target: target.map(PageRef::Id),
        view: None,
        color: None,
        style: Default::default(),
        local: None,
    }
}

/// A label range from `start`.
pub fn label(start: PageId, style: LabelStyle, prefix: &str, first: u32) -> PageLabelRange {
    PageLabelRange {
        start: PageRef::Id(start),
        style,
        prefix: prefix.into(),
        first,
    }
}

/// The page labels `ranges` say, or none.
pub fn labels(ranges: &[LabelSpec]) -> Op {
    let range = |r: &LabelSpec| label(r.start, r.style, &r.prefix, r.first);
    Op::SetPageLabels {
        ranges: (!ranges.is_empty()).then(|| ranges.iter().map(range).collect()),
    }
}

/// A header of `text`, top left, in the header and footer dialog's
/// defaults.
pub fn header(text: &str) -> HeaderFooterSpec {
    let mut texts: [String; 6] = Default::default();
    texts[0] = text.into();
    HeaderFooterSpec {
        texts,
        size: 10.0,
        color: [0, 0, 0],
        margins: Margins {
            top: 36.0,
            bottom: 36.0,
            left: 72.0,
            right: 72.0,
        },
        first_number: 1,
        bates: None,
    }
}

/// `spec` set on `pages`, or on every page.
pub fn set_header(spec: HeaderFooterSpec, pages: Option<&[PageId]>) -> Op {
    Op::SetHeaderFooter {
        spec,
        scope: match pages {
            None => ScopeSel::All,
            Some(pages) => ScopeSel::Pages(ids(pages)),
        },
    }
}

/// A watermark of `text`, in the middle of the page, in the watermark
/// dialog's defaults.
pub fn watermark(text: &str) -> WatermarkSpec {
    WatermarkSpec {
        content: WatermarkContent::Text {
            text: text.into(),
            font: FontChoice::HelveticaBold,
            size: 72.0,
            color: [200, 30, 30],
            opacity: 0.3,
            angle: 45.0,
        },
        position: GridPos::Center,
    }
}

/// `spec` set at `z` on `pages`, or on every page.
pub fn set_watermark(spec: WatermarkSpec, z: Z, pages: Option<&[PageId]>) -> Op {
    Op::SetWatermark {
        spec,
        z,
        scope: match pages {
            None => ScopeSel::All,
            Some(pages) => ScopeSel::Pages(ids(pages)),
        },
    }
}

/// The watermark at `z`, or both, taken off `pages`.
pub fn remove_watermark(z: Option<Z>, pages: &[PageId]) -> Op {
    Op::RemoveWatermark {
        z,
        pages: ids(pages),
        include_authored: false,
    }
}

/// `a`, a fixture's mark, as a mark to make: by its author when it names
/// one, holding what its stem names in the store.
pub fn new_mark(a: &Annot) -> NewMark {
    let stem = match &a.kind {
        AnnotKind::PageStamp { stamp, .. } => Some(stamp),
        AnnotKind::FileAttachment { file, .. } => Some(file),
        _ => None,
    };
    NewMark {
        kind: super::mark_kind(&a.kind),
        style: super::mark_style(a),
        contents: a.contents.clone(),
        author: (!a.author.is_empty()).then(|| a.author.clone()),
        turn: a.turn,
        asset: (stem.and_then(|s| BlobId::from_stem(s))).map(BlobSel::Id),
    }
}

/// `a` made on `page`: a reply or status in the thread of the comment
/// there its parent names. An imported comment's name is the caller's to
/// say ([`Op::AddMark`]'s `imported`).
pub fn add(page: PageId, a: &Annot) -> Op {
    assert!(
        a.name.is_empty(),
        "{:?} is named: say it's imported",
        a.name
    );
    Op::AddMark {
        page: PageRef::Id(page),
        mark: new_mark(a),
        reply_to: a.thread_parent().map(|nm| AnnotRef::Nm {
            nm: nm.into(),
            page: Some(PageRef::Id(page)),
        }),
        local: None,
        imported: None,
    }
}

/// A connector pair, `a`'s and `b`'s ends of the label and style `a`
/// says.
pub fn connect(a: (PageId, &Annot), b: (PageId, &Annot)) -> Op {
    let end = |(page, end): (PageId, &Annot)| match &end.kind {
        AnnotKind::Connector { rect, label, .. } => {
            let page = PageRef::Id(page);
            (Anchor { page, rect: *rect }, label.clone())
        }
        other => panic!("{other:?} isn't a connector"),
    };
    let ((at_a, label), (at_b, _)) = (end(a), end(b));
    Op::AddConnector {
        a: at_a,
        b: at_b,
        label,
        style: super::mark_style(a.1),
        locals: Vec::new(),
    }
}

/// Other tools' comments `annots` made marks, as Make Editable does.
pub fn adopt(annots: &[AnnotId]) -> Op {
    Op::AdoptAnnotations {
        annots: annots.iter().map(|&id| AnnotRef::Id(id)).collect(),
    }
}

/// Mark `a.id` made all `a` says ([`mark_patch`](super::mark_patch)).
pub fn update(a: &Annot) -> Op {
    Op::PatchMarks {
        marks: vec![AnnotRef::Id(a.id)],
        patch: super::mark_patch(a),
        imported: None,
    }
}
