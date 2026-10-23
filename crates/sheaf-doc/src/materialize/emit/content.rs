//! What sheaf draws on a page (M10.5 §11.4).
//!
//! - **The generators on the page and its layers**, laid out on it
//!   ([`generate::layout`], `text_layer::block`, [`flattened`]) and
//!   assembled around its own content: `/Contents [under… QOpen^open base′…
//!   QClose^close text… over… flat…]`, a background under it, and over it
//!   its recognized text, then a watermark, then the header and footer
//!   ([`generators_on`]'s order), then what a flatten drew there, last, as
//!   the annotations showed over everything, so the page's own text reads
//!   first, then the recognized words, then the header. Each block is a
//!   stream of its own marked `/SheafBlock`. A generator's says `<< /K /Gen
//!   /G (g…) /L <…> /R <…> /N (…) >>`, its generator, its layout's digest,
//!   its recipe's and, on a page the header numbers, its Bates number;
//!   identical blocks are one object ([`ObjKey::Block`]), so a watermark on
//!   7,000 pages alike is one. A header's tokens read the page as the
//!   document being written has it: its place there, the page count and its
//!   label, and its Bates number, fixed when it was set (§3.5). A page with
//!   nothing to draw for one gets an empty block, so its scope and numbers
//!   can be read back. The recognized text's says `<< /K /Ocr /B (b…) >>`,
//!   its layer's id; it's invisible text in a `q` of its own, marked
//!   `/ShOCR` with a list that names its block, in one glyphless font a
//!   file. A flatten's says `<< /K /Flat /B (b…) >>`: each form it places
//!   drawn in a `q` of its own through its matrix, marked `/Artifact` on a
//!   page of a tagged file, a source's appearance copied with the page, and
//!   a mark's drawn again from what it's pinned to, once a file
//!   ([`ObjKey::Pinned`]). Layers' blocks are written as they're serialized,
//!   their data made then ([`Deferred`]). The page's own content is isolated
//!   by `q`s and `Q`s, shared marked streams ([`Shared::QOpen`],
//!   [`Shared::QClose`]) counted by its balance (§3.4): `open = 1 + d` and
//!   `close = 1 + d + net`, `d = max(0, −min_prefix)`, so no `Q` of its
//!   undoes theirs, and nothing it leaves set reaches what's drawn over it.
//!   Its streams are referenced, never re-encoded. A page whose content
//!   can't be read gets none of them, said once in a note, and the save goes
//!   on. The page's resources are a new dictionary ([`ObjKey::PageRes`]):
//!   its base page's, the categories it adds to copied one level down, the
//!   names it draws with added clear of the names they bind; pages that
//!   share base resources and additions share one. A save notes each
//!   generator's block on the pages it writes, for its record
//!   (`SaveRecord::drawn`), and the saved file's block there that a
//!   generator lays out otherwise now from the same recipe as one the save
//!   may unlink, once every page it was on draws it again or is gone: a live
//!   redraw (§11.9's allowed drops), a page number drawn where its page is
//!   now.
//! - **base′**, the page's own content as a save writes it, when its file
//!   holds sheaf's blocks (by its streams' markers, [`Source::markers`]; in
//!   a file whose catalog doesn't say so, as one saved with layers alone, by
//!   the page's blocks, read when something is drawn on it) or a filter
//!   changes it. The blocks of the document's generators
//!   ([`DocState::backs`]) are set aside, so one gone or changed leaves no
//!   trace and nesting never grows; another file's are kept, with the
//!   streams that isolate them, until a filter takes them out; isolating
//!   streams are otherwise set aside too. The layers' blocks the file holds
//!   (recognized text, flattened annotations) are static content drawn over
//!   the page's: kept as they are, in their order, recognized text after its
//!   isolated content, before the layers and generators drawn over it, and
//!   flattened annotations last, after those drawn anew, and the page's
//!   content is isolated from them even with nothing else drawn, so nesting
//!   never grows as a page is flattened again. A block laid out alike again
//!   (its `/L`) from the same recipe (its `/R`, which says its Bates number
//!   too) is the file's stream, and a stream of the file's that isolates as
//!   many as base′ needs is used again; a page whose blocks are all laid out
//!   alike, in place around its isolated content and the layers' blocks
//!   after it, is written as it is ([`intact`], which the realizer asks
//!   too). A header's layout is digested once for every version of the
//!   document that lays its page out alike, whoever asks
//!   ([`header_digest`]). A filter's ranges are spliced out of the streams
//!   they're in, each a copy of its own ([`ObjKey::Filtered`]), so a stream
//!   pages share stays whole on the others: another file's blocks merged
//!   into a stream, and others' pagination artifacts, as a Remove selects
//!   them and as the generators drawn on a page replace those of their kinds
//!   ([`replacing`]), every tool's but the author's (§11.4): an Acrobat
//!   header, or one an older sheaf drew, under a header set now. One a
//!   splice refuses stays, counted in a note; another file's block of a kind
//!   replaced, a stream of its own, is left out whole. The recognized text
//!   the file holds that a re-OCR or a Clear takes out goes the same way,
//!   found by the page's blocks: its blocks, streams of their own, are left
//!   out, and so is the layer an older sheaf wrote with the `q` before the
//!   page's content that its first `Q` ends, both or neither; one merged
//!   into another stream is cut out of it. A layer's stream without its `q`,
//!   or a cut a splice refuses, stays, counted in a note. sheaf's private
//!   data a filter takes out of the page ([`Filter::StripLegacy`], §15): a
//!   range kept whose list written in the content holds some has it written
//!   again without it, in the same copy, one that can't be keeping it,
//!   counted in a note; a stream sheaf marked is a copy of its own without
//!   the mark; the lists named in the page's resources lose it; and each
//!   name an older sheaf gave a resource is given a new one, where the
//!   resources bind it and in the content that draws with it, so what it
//!   drew is drawn as it was. What's left is counted again, and the names
//!   only what's set aside or cut used are dropped from the page's resources
//!   (one level down), so the form an Acrobat watermark draws is no longer
//!   the page's, but those a block drawn anew binds to the same resource.
//! - **Without sheaf's private data** (`Private::Strip`), what's drawn
//!   anew is unmarked: a block's stream says no `/SheafBlock`, a
//!   generator's list no `/SheafGen`, recognized text is marked `/ShOCR`
//!   with no list, and the streams that isolate the page's content say
//!   nothing either.
//!
//! An overlay draws the recognized text only when its view asks for it,
//! what a flatten drew always, and the generators as a save draws them
//! when it asks for those, finding where a page is and its label from the
//! state's own tables, never by a walk of the pages.
//!
//! What's drawn adds resources keyed by digest: a resource
//! ([`ObjKey::Res`], its other objects [`ObjKey::ResPart`]) is written once
//! a file, so every OCR page draws in one font, every watermark in one
//! font, graphics state and image, and every header in one font.
//!
//! [`ObjKey::Res`]: crate::materialize::ObjKey::Res
//! [`ObjKey::ResPart`]: crate::materialize::ObjKey::ResPart
//! [`ObjKey::Block`]: crate::materialize::ObjKey::Block
//! [`ObjKey::PageRes`]: crate::materialize::ObjKey::PageRes
//! [`ObjKey::Filtered`]: crate::materialize::ObjKey::Filtered
//! [`ObjKey::Pinned`]: crate::materialize::ObjKey::Pinned
//! [`Deferred`]: super::Deferred
//! [`Shared::QOpen`]: crate::materialize::key::Shared::QOpen
//! [`Shared::QClose`]: crate::materialize::key::Shared::QClose
//! [`Source::markers`]: crate::source::Source::markers
//! [`DocState::backs`]: crate::model::DocState::backs

use std::borrow::Cow;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat, dictionary};

use super::{Cx, Deferred, PdfVersion};
use crate::content::sink::{self, Sink as _};
use crate::content::{
    self, Balance, BlockAt, BlockId, Concat, ContentWriter, GenId, LEGACY_OCR, Marker, NameSet,
    PagOrigin, PageBlocks, PageContent, Pagination, PaginationKind, Pos, Props, Res, ResBody,
    ResCx, ResId, ResKind, Reserved, ResourceNeeds, SheafBlock, SpanAcross,
};
use crate::generate::{self, Art};
use crate::geom::Geom;
use crate::materialize::copy::Scope;
use crate::materialize::key::{ObjKey, Shared};
use crate::materialize::record::DrawnBlock;
use crate::model::{
    BlockKind, DocState, Filter, FormRef, GenSpec, Generator, ImageRef, Layer, LegacySel,
    MarkedSel, PageCtx, PageDef, PageForm, PageId, PlaceTag, Placement, Static, StreamSel,
    WatermarkContent,
};
use crate::private;
use crate::source::ObjRef;
use crate::store::{BlobId, Src};
use crate::text_layer::TextLayer;

/// The PDF version what sheaf draws over a page showing `def` needs, the
/// generators' aside: its layers' ([`layer_needs`]).
pub(crate) fn needs(def: &PageDef) -> Option<PdfVersion> {
    (def.content.over.iter())
        .filter_map(|l| layer_needs(&l.body))
        .max()
}

/// The PDF version a layer drawing `body` needs: OCR text's fonts, and
/// the artifacts a flatten marks on a page of a tagged file.
fn layer_needs(body: &Static) -> Option<PdfVersion> {
    match body {
        Static::OcrText(_) => Some(PdfVersion::CID_FONTS),
        Static::Placements(placed) => (placed.iter())
            .any(|p| p.tag == PlaceTag::Artifact)
            .then_some(PdfVersion::ARTIFACTS),
    }
}

/// Whether what sheaf draws on `st`'s pages needs a later PDF version than
/// `version`: a generator's marked content (a watermark's transparency
/// too), or its layers' ([`needs`]). The planner reads it, as an Append
/// that needs a later version than its file says so in the catalog
/// (§11.7), which a signature counts as any other change (§9.3).
pub(crate) fn needs_after(st: &DocState, version: PdfVersion) -> bool {
    (version < PdfVersion::ARTIFACTS && !st.generators().is_empty())
        || (version < PdfVersion::ARTIFACTS
            && (st.pages()).any(|(_, def, _)| needs(def).is_some_and(|v| v > version)))
}

/// The generators on page `id` of `st`, in the order they're drawn: a
/// background under the page's content, then over it a watermark, then
/// the header and footer. What a save draws on it, and an overlay whose
/// view draws generators; the order its blocks are found in again
/// ([`intact`]).
pub(crate) fn generators_on(st: &DocState, id: PageId) -> Vec<&Arc<Generator>> {
    if st.generators().is_empty() {
        return Vec::new();
    }
    let mut on: Vec<&Arc<Generator>> = (st.generators().values())
        .filter(|g| g.covers(id))
        .collect();
    on.sort_by_key(|g| {
        let over = match g.spec {
            GenSpec::Watermark(_) => 0,
            GenSpec::HeaderFooter(_) => 1,
        };
        (g.z, over)
    });
    on
}

/// What sheaf draws over page `id`, which shows `def` with `geom` at
/// place `pos`, drawn into `page`, its dictionary as emitted: its layers,
/// its recognized text where the view draws it, and the generators on it.
pub(crate) fn page(
    cx: &mut Cx,
    id: PageId,
    def: &PageDef,
    geom: &Geom,
    pos: usize,
    page: &mut Dictionary,
) {
    let view = cx.overlay.map(|o| o.inputs);
    let ocr = view.is_none_or(|v| v.ocr_over);
    let draws = view.is_none_or(|v| v.generators);
    let layers: Vec<&Layer> = (def.content.over.iter())
        .filter(|l| match &l.body {
            Static::OcrText(text) => ocr && !text.is_empty(),
            Static::Placements(_) => true,
        })
        .collect();
    stack(cx, (id, pos), def, geom, (&layers, draws), page);
    // The page's own resources, when it still names them.
    if let Ok(&Object::Reference(r)) = page.get(b"Resources")
        && let Some(key @ ObjKey::PageRes { .. }) = cx.syms.key(r)
    {
        cx.push_once(key, |cx| {
            Object::Dictionary(cx.page_res.get(&key).cloned().unwrap_or_default())
        });
    }
}

/// Page `id`'s own content as a save writes it, which shows `def` with
/// `geom` at place `pos`, and the generators on it (`draws`: when the view
/// draws them) and its `layers` (those the view draws) around it,
/// isolated by its balance, drawn into `page`, with its resources a
/// [`ObjKey::PageRes`] when they change (see the module's doc). As it is
/// when its file holds no block of sheaf's and nothing covers it, or when
/// no layer goes on it and the blocks it holds are what's drawn there now
/// ([`intact`], which knows nothing of layers drawn anew). Others'
/// artifacts of the kinds the generators drawn on it replace are taken
/// out ([`replacing`]); those a splice refuses stay, counted in a note. A
/// page whose content can't be read is drawn as it is, and gets no
/// generator and no layer, said once in a note. A save notes the block of
/// each generator on it, kept or drawn anew ([`drew`]).
fn stack(
    cx: &mut Cx,
    (id, pos): (PageId, usize),
    def: &PageDef,
    geom: &Geom,
    (layers, draws): (&[&Layer], bool),
    page: &mut Dictionary,
) {
    let on = match draws {
        true => generators_on(cx.st, id),
        false => Vec::new(),
    };
    let (src, index) = (def.base.src.clone(), def.base.index);
    let marks = src.markers(index);
    let filters = &def.content.filters;
    if on.is_empty() && layers.is_empty() && marks.is_empty() && filters.is_empty() {
        return;
    }
    let st = cx.st;
    let contents = src.page(index).map_or(&[][..], |f| &f.contents);
    // What each generator reads of the page, and its layout here, as
    // `intact` digests it, which a rebuild doesn't digest again.
    let ctxs: Vec<PageCtx> = (on.iter())
        .map(|g| generate::page_ctx(st, g, id, pos, geom))
        .collect();
    let mut laid = Vec::new();
    let mut digest = |g: &Generator| {
        let at = on.iter().position(|o| o.id == g.id)?;
        let digest = digest_of(cx, (id, g), &ctxs[at]);
        laid.push((g.id, digest));
        Some(digest)
    };
    if filters.is_empty() && layers.is_empty() && intact(st, id, &marks, contents, &on, &mut digest)
    {
        if cx.overlay.is_none() {
            // Its blocks as `intact` found them, those under its content
            // first.
            let under = (on.iter())
                .take_while(|g| g.z == sheaf_edit::Z::Under)
                .count();
            let end = contents.len() - flats_last(&marks, contents);
            let over = end - (on.len() - under);
            let blocks = contents[..under].iter().chain(&contents[over..end]);
            for (&stream, g) in blocks.zip(&on) {
                let mark = (marks.iter()).find_map(|&(m, mark)| (m == stream).then_some(mark));
                keeps(cx, (id, g), &src, (stream, mark));
            }
        }
        return;
    }
    let replacing = replacing(&on, &src, index);
    let at = (id, &src, index);
    let written = match (marks.is_empty() && filters.is_empty(), replacing) {
        // A file whose catalog doesn't say it holds sheaf's blocks may all
        // the same, as one saved with recognized text alone does: its
        // blocks say so, and it's rebuilt, so nesting doesn't grow.
        (true, None) => match src.blocks(index) {
            Ok(blocks)
                if !blocks.isolation.is_empty()
                    || (blocks.sheaf.iter()).any(|b| matches!(b.at, BlockAt::Stream(_))) =>
            {
                rebuilt(cx, at, &[], (&ctxs, &laid), (&on, layers), page)
            }
            Ok(blocks) => {
                wrapped(cx, at, blocks.balance, &ctxs, (&on, layers), page);
                Some(Stayed::default())
            }
            Err(_) => None,
        },
        (_, replacing) => {
            let filters: Vec<Filter> = filters.iter().cloned().chain(replacing).collect();
            rebuilt(cx, at, &filters, (&ctxs, &laid), (&on, layers), page)
        }
    };
    let Some(stayed) = written else {
        match on.is_empty() && layers.is_empty() {
            true => log::warn!(
                "page {}'s content can't be read: it's kept as it is",
                index + 1
            ),
            false => cx.unread.push(id),
        }
        return;
    };
    if stayed.ranges > 0 {
        cx.stayed.push((id, stayed.ranges));
    }
    if stayed.text > 0 {
        cx.text_stayed.push(id);
    }
    if stayed.settings > 0 {
        cx.settings_kept.push((id, stayed.settings));
    }
    if !on.is_empty() {
        cx.needs(PdfVersion::ARTIFACTS);
    }
    for version in layers.iter().filter_map(|l| layer_needs(&l.body)) {
        cx.needs(version);
    }
}

/// What stays of what a page's filters select: others' ranges a splice
/// refuses to take out; the recognized text sheaf wrote that can't be,
/// a cut a splice refuses or a layer's stream without the `q` its first
/// `Q` ends; and the property lists of an older sheaf's that can't be
/// written again without what it kept in them.
#[derive(Default)]
struct Stayed {
    ranges: usize,
    text: usize,
    settings: usize,
}

/// What the generators `on` page `index` of `src` replace there (§11.4):
/// the pagination artifacts of their kinds that others drew, every tool
/// but the author ([`PagOrigin::REPLACED`]), taken out by a filter when the
/// page's blocks hold some. Read from its blocks, once a source, so a page
/// added later is covered too. None when there are none, or its content
/// can't be read, which the page's drawing finds.
fn replacing(on: &[&Arc<Generator>], src: &Src, index: u32) -> Option<Filter> {
    let kinds: BTreeSet<PaginationKind> = (on.iter())
        .flat_map(|g| g.replaces.iter().copied())
        .collect();
    if kinds.is_empty() {
        return None;
    }
    let origins = BTreeSet::from(PagOrigin::REPLACED);
    let blocks = src.blocks(index).ok()?;
    blocks.selected(&kinds, &origins).next()?;
    Some(Filter::DropMarked(MarkedSel::Pagination { kinds, origins }))
}

/// Whether page `page`, whose `/Contents` is `contents`, which `marks` says
/// sheaf marked, keeps its content as it is, the generators `on` it (in
/// [`generators_on`]'s order) laid out alike there: none of the blocks it
/// holds is another file's; it holds the block of each that's laid out
/// alike, whose digest `digest` gives (None when it can't tell), from the
/// recipe each draws from there now, in that order, those under its
/// content first and those over it after, and between them the streams
/// that isolate the rest, which is all its own, then the blocks of
/// recognized text it holds, if any, and after the generators over it the
/// blocks of flattened annotations it holds, if any (§11.4's
/// `[gen-under… QOpen inner… QClose text… gen-over… flat…]`); or with none
/// on it, it holds no block of the document's, and no isolation but
/// around another file's blocks or before the layers it holds. What the
/// emitter writes as the page has it, and what the realizer renders from
/// its base page.
pub(crate) fn intact(
    st: &DocState,
    page: PageId,
    marks: &[(ObjectId, Marker)],
    contents: &[ObjectId],
    on: &[&Arc<Generator>],
    mut digest: impl FnMut(&Generator) -> Option<u128>,
) -> bool {
    let unbacked =
        (marks.iter()).any(|(_, m)| matches!(m, Marker::Gen { id, .. } if !st.backs(*id, page)));
    if on.is_empty() {
        let backed =
            (marks.iter()).any(|(_, m)| matches!(m, Marker::Gen { id, .. } if st.backs(*id, page)));
        let isolates = (marks.iter()).any(|(_, m)| *m == Marker::Isolate);
        let layers = (marks.iter()).any(|(_, m)| static_block(Some(*m)));
        return !backed && (!isolates || unbacked || layers);
    }
    if unbacked {
        return false;
    }
    let mark = |stream: ObjectId| marks.iter().find(|(m, _)| *m == stream).map(|(_, m)| *m);
    let contents = &contents[..contents.len() - flats_last(marks, contents)];
    let under = on
        .iter()
        .take_while(|g| g.z == sheaf_edit::Z::Under)
        .count();
    let over = on.len() - under;
    if contents.len() < under + over + 2 {
        return false;
    }
    let (blocks_under, rest) = contents.split_at(under);
    let (middle, blocks_over) = rest.split_at(rest.len() - over);
    // The recognized text's blocks after the isolated content.
    let layers = (middle.iter().rev())
        .take_while(|&&s| matches!(mark(s), Some(Marker::Ocr(_))))
        .count();
    let Some(inner) = (middle.len() - layers).checked_sub(2) else {
        return false;
    };
    // A block of `g` laid out alike, from the recipe it draws from here
    // now: one that says another keeps what the record and an open read of
    // it (its recipe, its Bates number) from what's drawn.
    let mut alike = |stream: ObjectId, g: &Generator| match mark(stream) {
        Some(Marker::Gen {
            id,
            layout: Some(l),
            recipe: Some(r),
            ..
        }) => id == g.id && r == generate::recipe_digest(g, page) && digest(g) == Some(l),
        _ => false,
    };
    let blocks = blocks_under.iter().chain(blocks_over);
    blocks.zip(on).all(|(&stream, g)| alike(stream, g))
        && mark(middle[0]) == Some(Marker::Isolate)
        && mark(middle[inner + 1]) == Some(Marker::Isolate)
        && middle[1..=inner].iter().all(|&s| mark(s).is_none())
}

/// Whether `mark` marks a layer's block: static content drawn over the
/// page's own, kept where it is (§11.4).
fn static_block(mark: Option<Marker>) -> bool {
    matches!(mark, Some(Marker::Ocr(_) | Marker::Flat(_)))
}

/// How many of the last streams of `contents`, which `marks` says sheaf
/// marked, are flattened annotations' blocks: drawn last, over the
/// generators too, as the annotations showed (§11.4).
fn flats_last(marks: &[(ObjectId, Marker)], contents: &[ObjectId]) -> usize {
    let flat = |s: &ObjectId| (marks.iter()).any(|(m, k)| m == s && matches!(k, Marker::Flat(_)));
    contents.iter().rev().take_while(|s| flat(s)).count()
}

/// What generator `g` lays out on page `page` of `st`, of `geom`, as its
/// blocks say it (`/L`), read from the model alone, as [`digest_of`] reads
/// it: None for an image or a page a file in the store holds, which only
/// decoding it tells, and for a page `st` hasn't got. What the realizer
/// asks, which never reads a page's content.
pub(crate) fn model_layout_digest(
    st: &DocState,
    g: &Generator,
    page: PageId,
    geom: &Geom,
) -> Option<u128> {
    let ctx = generate::page_ctx(st, g, page, st.pos_of(page)?, geom);
    laid_digest(st, g, page, &ctx)
}

/// [`model_digest`] of `g` on page `page` of `st`, which `ctx` says: a
/// header's and footer's as `st`'s versions have laid it out
/// ([`header_digest`]).
pub(crate) fn laid_digest(
    st: &DocState,
    g: &Generator,
    page: PageId,
    ctx: &PageCtx,
) -> Option<u128> {
    match g.spec.header_footer() {
        Some(_) => Some(header_digest(st, g, page, ctx)),
        None => model_digest(g, ctx),
    }
}

/// Header and footer `g`'s layout's digest on page `page` of `st`, which
/// `ctx` says: laid out once for every version of `st`'s that lays the
/// page out alike, by what it's laid out from ([`laid_key`]), which the
/// tests' builds check by laying it out again.
pub(crate) fn header_digest(st: &DocState, g: &Generator, page: PageId, ctx: &PageCtx) -> u128 {
    let key = laid_key(g, page, ctx);
    if let Some(digest) = st.laid().get(&key) {
        debug_assert_eq!(digest, generate::layout_digest(g, ctx, None), "{key:?}");
        return digest;
    }
    let digest = generate::layout_digest(g, ctx, None);
    st.laid().put(key, digest);
    digest
}

/// What header and footer `g` lays out on page `page` from, which `ctx`
/// says: itself, its recipe there, which says its spec and what it fixed
/// for the page, and what its tokens read of the page, which with its id
/// is all its layout reads.
pub(crate) fn laid_key(g: &Generator, page: PageId, ctx: &PageCtx) -> crate::model::LaidKey {
    let reads = crate::realize::layout_reads(g.needs(), ctx);
    (g.id, generate::recipe_digest(g, page), reads)
}

/// [`model_layout_digest`] on the page `ctx` says.
pub(crate) fn model_digest(g: &Generator, ctx: &PageCtx) -> Option<u128> {
    let Some(wm) = g.spec.watermark() else {
        return Some(generate::layout_digest(g, ctx, None));
    };
    let art = match &wm.content {
        WatermarkContent::Text { .. } => None,
        WatermarkContent::Image {
            image: ImageRef::Placed(p),
            ..
        } => generate::placed_art(p, true, ResBody::Written(p.at.id)),
        WatermarkContent::Page {
            page: PageForm::Placed(p),
            ..
        } => generate::placed_art(p, false, ResBody::Written(p.at.id)),
        WatermarkContent::Image { .. } | WatermarkContent::Page { .. } => return None,
    };
    Some(generate::layout_digest(g, ctx, art.as_ref()))
}

/// What generator `g` lays out on page `id`, which `ctx` says, as its
/// blocks say it (`/L`), a watermark's image or page as this file has it,
/// and a header as the state's versions have laid it out
/// ([`header_digest`]).
fn digest_of(cx: &mut Cx, (id, g): (PageId, &Generator), ctx: &PageCtx) -> u128 {
    if g.spec.header_footer().is_some() {
        return header_digest(cx.st, g, id, ctx);
    }
    let art = g.spec.watermark().and_then(|wm| art(cx, &wm.content));
    generate::layout_digest(g, ctx, art.as_ref())
}

/// The generators `on` page `id`, page `index` of `src`, and its
/// `layers`, drawn into `page` around its own content, each generator on
/// the page `ctxs` says, which holds none of sheaf's blocks and no filter
/// changes: its streams referenced as they are, counted by their
/// `balance`.
fn wrapped(
    cx: &mut Cx,
    (id, src, index): (PageId, &Src, u32),
    balance: Balance,
    ctxs: &[PageCtx],
    (on, layers): (&[&Arc<Generator>], &[&Layer]),
    page: &mut Dictionary,
) {
    let base = sink::content_entries(cx, page);
    let mut reserved = reserved(cx, src, index, page, &NameSet::default());
    let mut needs = ResourceNeeds::default();
    let (mut under, mut over) = (Vec::new(), Vec::new());
    for (g, ctx) in on.iter().zip(ctxs) {
        let block = drawn(cx, (id, g), ctx, &mut reserved, &mut needs);
        match g.z {
            sheaf_edit::Z::Under => under.push(block),
            sheaf_edit::Z::Over => over.push(block),
        }
    }
    let (open, close) = isolated(balance);
    let mut contents = under;
    contents.push(isolation(cx, Shared::QOpen(open)));
    contents.extend(base);
    contents.push(isolation(cx, Shared::QClose(close)));
    let mut flats = Vec::new();
    for l in layers {
        let block = layer(cx, l, &mut reserved, &mut needs);
        match l.kind() {
            BlockKind::Ocr => contents.push(block),
            BlockKind::Flattened => flats.push(block),
        }
    }
    contents.extend(over);
    contents.extend(flats);
    page.set("Contents", Object::Array(contents));
    page_res(
        cx,
        (src, index),
        page,
        &NameSet::default(),
        (&needs, &ResStrip::default()),
    );
}

/// Page `id`'s own content, page `index` of `src`, whose streams sheaf
/// marked, or which `filters` change, rebuilt into `page` with the
/// generators `on` it and its `layers` around what's left (§11.4): each
/// stream's marker read from its dictionary, whatever the file's catalog
/// says, and one that says it isolates content but holds more than `q`s or
/// `Q`s the page's own content; the blocks of the document's generators set
/// aside, those laid out alike again from the same recipe kept as they are;
/// another file's blocks kept but where a filter takes them out, and the
/// streams that isolate them with them, else set aside too; the recognized
/// text sheaf wrote that a filter takes out set aside, found by the page's
/// blocks, the layer an older sheaf wrote with its `q` or not at all; the
/// layers' blocks it leaves kept as they are, in their order, the
/// recognized text's after the isolated content and before `layers`,
/// flattened annotations' after the generators over it, before those
/// `layers` adds, and the names they draw with kept; a stream a filter's
/// ranges come out of written as a copy ([`ObjKey::Filtered`]), each range
/// cut where a splice takes it ([`content::splice_each`]), and each range
/// kept whose list loses what an older sheaf kept in it written so
/// ([`content::without_keys`]). What's left is counted again, and the names
/// only what's set aside or cut used are dropped from the page's resources,
/// which a block drawn anew may name again. A stream the file has that
/// isolates as many as now is referenced as it is. What of what the filters
/// select stays, as a splice or a rewrite refuses it; None when its content
/// can't be read. Each of `on` is laid out on the page `ctxs` says; `laid`
/// is what of their layouts is digested already.
fn rebuilt(
    cx: &mut Cx,
    (id, src, index): (PageId, &Src, u32),
    filters: &[Filter],
    (ctxs, laid): (&[PageCtx], &[(GenId, u128)]),
    (on, layers): (&[&Arc<Generator>], &[&Layer]),
    page: &mut Dictionary,
) -> Option<Stayed> {
    let st = cx.st;
    let backs = |g| st.backs(g, id);
    let lim = src.limits();
    let facts = src.page(index)?;
    let content = PageContent::read(src.pdf(), &facts.contents, lim).ok()?;
    let blocks = match filters.is_empty() {
        true => None,
        false => Some(src.blocks(index).ok()?),
    };
    let resources = facts.resources.as_deref();
    let res = ResCx::read(src.pdf(), resources);
    let filtered = |dict: &Dictionary| {
        let Some(Marker::Gen { id, .. }) = Marker::of(dict) else {
            return false;
        };
        filters.contains(&Filter::DropStreams(StreamSel::SheafGen(id)))
    };
    // The pagination artifacts the filters select.
    let selects = |p: &Pagination| {
        (filters.iter()).any(|f| match f {
            Filter::DropMarked(MarkedSel::Pagination { kinds, origins }) => {
                p.selected_by(kinds, origins)
            }
            _ => false,
        })
    };
    let others: Vec<&Pagination> = (blocks.iter().flat_map(|b| b.pagination.iter()))
        .filter(|p| selects(p))
        .collect();
    let ids = content.ids();
    let dicts = content.dicts();
    let concat = content.concat();
    // The streams of recognized text sheaf wrote that the filters leave
    // out: its blocks, and the layer an older sheaf wrote with its `q`. A
    // layer's stream without one stays, its first `Q` the page's own then.
    let drops_text = filters.contains(&Filter::DropStreams(StreamSel::SheafOcr));
    let texts: Vec<ObjectId> = (blocks.as_deref().filter(|_| drops_text))
        .map(|blocks| {
            (blocks.sheaf.iter())
                .filter_map(|b| match (b.block, b.at) {
                    (SheafBlock::Ocr(_), BlockAt::Stream(stream)) => Some(stream),
                    _ => None,
                })
                .chain(blocks.legacy_ocr.iter().flat_map(|l| [l.q, l.layer]))
                .collect()
        })
        .unwrap_or_default();
    let mut text_stayed = (0..ids.len())
        .filter(|&i| {
            drops_text
                && dicts[i].has(LEGACY_OCR)
                && !texts.contains(&ids[i])
                && concat.stream(i).trim_ascii() != b"q"
        })
        .count();
    let marked: Vec<Option<Marker>> = (0..ids.len())
        .map(|i| match Marker::of(&dicts[i]) {
            Some(Marker::Isolate) if content::isolates(concat.stream(i)).is_none() => None,
            marker => marker,
        })
        .collect();
    let mark = |i: usize| marked[i];
    // Each stream: whether a filter leaves it out, as it does another
    // file's block of a kind it selects; and the block of the document's
    // each generator on the page lays out alike from the recipe it draws
    // from there now, kept.
    let dropped: Vec<bool> = (0..ids.len())
        .map(|i| {
            let theirs = matches!(mark(i), Some(Marker::Gen { id, .. }) if !backs(id));
            filtered(&dicts[i])
                || texts.contains(&ids[i])
                || (theirs && others.iter().any(|p| p.span.start.stream as usize == i))
        })
        .collect();
    let mut kept_block: Vec<Option<usize>> = Vec::new();
    for (g, ctx) in on.iter().zip(ctxs) {
        let backed = (0..ids.len()).filter(|&i| {
            !dropped[i] && matches!(mark(i), Some(Marker::Gen { id, .. }) if id == g.id)
        });
        let backed: Vec<usize> = backed.collect();
        let digest = match backed.is_empty() {
            true => None,
            false => match laid.iter().find(|(id, _)| *id == g.id) {
                Some(&(_, digest)) => Some(digest),
                None => Some(digest_of(cx, (id, g), ctx)),
            },
        };
        let recipe = generate::recipe_digest(g, id);
        kept_block.push(backed.into_iter().find(|&i| {
            matches!(mark(i), Some(Marker::Gen { layout: Some(l), recipe: Some(r), .. })
                if Some(l) == digest && r == recipe)
        }));
    }
    let foreign = (0..ids.len())
        .any(|i| !dropped[i] && matches!(mark(i), Some(Marker::Gen { id, .. }) if !backs(id)));
    let reused = |i: usize| kept_block.contains(&Some(i));
    // The layers' blocks the file holds that no filter leaves out, in
    // their order: kept as they are, after the page's isolated content.
    let saved_layers: Vec<usize> = (0..ids.len())
        .filter(|&i| !dropped[i] && static_block(mark(i)))
        .collect();
    let aside: Vec<bool> = (0..ids.len())
        .map(|i| {
            dropped[i]
                || match mark(i) {
                    Some(Marker::Gen { id, .. }) => backs(id) && !reused(i),
                    Some(Marker::Isolate) => !foreign,
                    Some(Marker::Ocr(_) | Marker::Flat(_)) | None => false,
                }
        })
        .collect();
    // The ranges the filters take out of streams kept, each the cuts that
    // take one out: sheaf's recognized text merged into a stream, first,
    // then a generator's, and the artifacts they select but in a block of
    // sheaf's of its own, set aside, left out or laid out alike. One a
    // splice refuses stays, counted.
    let merged = |block: &dyn Fn(SheafBlock) -> bool| {
        let found = blocks.iter().flat_map(|b| b.sheaf.iter());
        found
            .filter_map(|b| match b.at {
                BlockAt::Marked(span) if block(b.block) && !aside[span.start.stream as usize] => {
                    Some(vec![span])
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let mut groups: Vec<Vec<SpanAcross>> = Vec::new();
    if filters.contains(&Filter::DropMarked(MarkedSel::SheafOcr)) {
        groups.extend(merged(&|b| matches!(b, SheafBlock::Ocr(_))));
    }
    let texts_cut = groups.len();
    for f in filters {
        if let Filter::DropMarked(MarkedSel::SheafGen(g)) = f {
            groups.extend(merged(&|b| b == SheafBlock::Gen(*g)));
        }
    }
    let mut stayed = 0;
    for p in &others {
        let i = p.span.start.stream as usize;
        if aside[i] || matches!(mark(i), Some(Marker::Gen { .. })) || static_block(mark(i)) {
            continue;
        }
        match p.cuts(concat, &res, lim) {
            Ok(cuts) if groups.contains(&cuts) => {}
            Ok(cuts) => groups.push(cuts),
            Err(_) => stayed += 1,
        }
    }
    let spliced = content::splice_each(concat, &res, lim, &groups);
    let refused = |taken: &[bool]| taken.iter().filter(|t| !**t).count();
    text_stayed += refused(&spliced.taken[..texts_cut]);
    stayed += refused(&spliced.taken[texts_cut..]);
    let cuts: Vec<SpanAcross> = (groups.iter().zip(&spliced.taken))
        .filter(|(_, taken)| **taken)
        .flat_map(|(cuts, _)| cuts.iter().copied())
        .collect();
    let whole = |i: usize| {
        let at = |at: usize| Pos {
            stream: i as u32,
            at: at as u32,
        };
        SpanAcross {
            start: at(0),
            end: at(concat.stream(i).len()),
        }
    };
    // What's left of base′ is counted without the layers' blocks kept,
    // which are drawn after it, but the names they draw with stay the
    // page's.
    let moved = |i: usize| saved_layers.contains(&i);
    let mut out_of: Vec<SpanAcross> = (0..ids.len())
        .filter(|&i| aside[i] || moved(i))
        .map(whole)
        .collect();
    out_of.extend(&cuts);
    let mut kept = content::kept(concat, &res, lim, &out_of).ok()?;
    if !saved_layers.is_empty() {
        let streams: Vec<&[u8]> = saved_layers.iter().map(|&i| concat.stream(i)).collect();
        let drawn = content::names_used(&Concat::new(&streams), &res, lim, &[]).ok()?;
        for (kind, name) in drawn.iter() {
            kept.dropped.remove(kind, name);
        }
    }
    let strips = |sel: LegacySel| filters.contains(&Filter::StripLegacy(sel));
    let mut strip = ResStrip {
        renamed: Vec::new(),
        lists: strips(LegacySel::Resources),
    };
    // The lists written in the content of the ranges kept that lose
    // sheaf's private data, written again without it. One that can't be
    // keeps it, counted; a named one is the resources'.
    let mut rewrites = Vec::new();
    let mut settings = 0;
    for span in older_ranges(filters, blocks.as_deref()) {
        let named = || {
            (blocks.iter().flat_map(|b| b.marked.iter()))
                .any(|m| m.span == span && matches!(m.props, Some(Props::Named(..))))
        };
        if aside[span.start.stream as usize]
            || spliced.takes(concat, span)
            || (strip.lists && named())
        {
            continue;
        }
        match content::without_keys(concat, &res, lim, span, &private::LIST_KEYS) {
            Ok(rewrite) => rewrites.extend(rewrite),
            Err(_) => settings += 1,
        }
    }
    // The names an older sheaf gave the page's resources, given new ones
    // where the content draws with them, and kept clear of by what's drawn
    // anew. Only then are the names the resources bind read before the
    // streams kept are written; otherwise after, as they always were, so
    // the page is written as it was.
    let mut renaming = None;
    if strip.lists {
        let mut reserved = reserved(cx, src, index, page, &kept.dropped);
        strip.renamed = renamed(&mut reserved);
        rewrites.extend(content::renames(concat, &res, lim, &strip.renamed).ok()?);
        renaming = Some(reserved);
    }
    let spliced = spliced.streams(concat, &rewrites);
    // Each stream as written: a copy where it changed, or where its
    // dictionary loses what sheaf marked it with, else the file's.
    let unmarked = strips(LegacySel::Streams);
    let scope = Scope { src, copy: 0 };
    let entry = |cx: &mut Cx, i: usize| {
        let changed = spliced.as_ref().and_then(|streams| match &streams[i] {
            Cow::Owned(bytes) => Some(bytes.clone()),
            Cow::Borrowed(_) => None,
        });
        let marked = || (dicts[i].iter()).any(|(k, _)| private::STREAM_KEYS.contains(&&k[..]));
        let copy = changed.or_else(|| (unmarked && marked()).then(|| concat.stream(i).to_vec()));
        match copy {
            Some(bytes) => filtered_copy(cx, src.id(), ids[i], bytes),
            None => cx.sym(&Object::Reference(ids[i]), &scope),
        }
    };
    let mut base = Vec::new();
    for i in (0..ids.len()).filter(|&i| !aside[i] && !reused(i) && !moved(i)) {
        base.push(entry(cx, i));
    }
    let mut reserved = match renaming {
        Some(reserved) => reserved,
        None => reserved(cx, src, index, page, &kept.dropped),
    };
    let mut needs = ResourceNeeds::default();
    let contents = match on.is_empty() && layers.is_empty() && saved_layers.is_empty() {
        true => base,
        false => {
            let (mut under, mut over) = (Vec::new(), Vec::new());
            for ((g, kept_at), ctx) in on.iter().zip(&kept_block).zip(ctxs) {
                let block = match *kept_at {
                    Some(i) => {
                        keeps(cx, (id, g), src, (ids[i], mark(i)));
                        entry(cx, i)
                    }
                    None => drawn(cx, (id, g), ctx, &mut reserved, &mut needs),
                };
                match g.z {
                    sheaf_edit::Z::Under => under.push(block),
                    sheaf_edit::Z::Over => over.push(block),
                }
            }
            let (open, close) = isolated(kept.balance);
            // A stream of the file's that isolates as many.
            let theirs = |cx: &mut Cx, op: u8, n: u32| {
                let found = (0..ids.len()).find(|&i| {
                    aside[i]
                        && mark(i) == Some(Marker::Isolate)
                        && content::isolates(concat.stream(i)) == Some((op, n))
                });
                found.map(|i| entry(cx, i))
            };
            let mut contents = under;
            let q = theirs(cx, b'q', open).unwrap_or_else(|| isolation(cx, Shared::QOpen(open)));
            contents.push(q);
            contents.extend(base);
            let q = theirs(cx, b'Q', close).unwrap_or_else(|| isolation(cx, Shared::QClose(close)));
            contents.push(q);
            // Recognized text before the generators drawn over the page,
            // flattened annotations after them, as they showed.
            let mut flats = Vec::new();
            for &i in &saved_layers {
                let block = entry(cx, i);
                match mark(i) {
                    Some(Marker::Flat(_)) => flats.push(block),
                    _ => contents.push(block),
                }
            }
            for l in layers {
                let block = layer(cx, l, &mut reserved, &mut needs);
                match l.kind() {
                    BlockKind::Ocr => contents.push(block),
                    BlockKind::Flattened => flats.push(block),
                }
            }
            contents.extend(over);
            contents.extend(flats);
            contents
        }
    };
    page.set("Contents", Object::Array(contents));
    // A name dropped that a block drawn anew names the same resource by
    // stays as the page's resources bind it.
    let (mut dropped, mut added) = (kept.dropped, ResourceNeeds::default());
    let resources = sink::resources(cx, page);
    for need in needs.0 {
        let bound = (dropped.contains(need.kind, &need.name))
            .then(|| binding(cx, &resources, need.kind, &need.name))
            .flatten();
        match bound.is_some_and(|bound| alike(cx, &bound, &need.res)) {
            true => dropped.remove(need.kind, &need.name),
            false => added.0.push(need),
        }
    }
    page_res(cx, (src, index), page, &dropped, (&added, &strip));
    Some(Stayed {
        ranges: stayed,
        text: text_stayed,
        settings,
    })
}

/// The marked ranges of a page's `blocks` that `filters` take what an
/// older sheaf kept out of ([`Filter::StripLegacy`]), each once.
fn older_ranges(filters: &[Filter], blocks: Option<&PageBlocks>) -> Vec<SpanAcross> {
    let mut spans = Vec::new();
    for f in filters {
        let (Filter::StripLegacy(LegacySel::Marked(sel)), Some(blocks)) = (f, blocks) else {
            continue;
        };
        let found: Vec<SpanAcross> = match sel {
            MarkedSel::Pagination { kinds, origins } => {
                blocks.selected(kinds, origins).map(|p| p.span).collect()
            }
            MarkedSel::SheafGen(g) => (blocks.sheaf.iter())
                .filter_map(|b| match (b.block, b.at) {
                    (SheafBlock::Gen(id), BlockAt::Marked(span)) if id == *g => Some(span),
                    _ => None,
                })
                .collect(),
            MarkedSel::SheafOcr => (blocks.sheaf.iter())
                .filter_map(|b| match (b.block, b.at) {
                    (SheafBlock::Ocr(_), BlockAt::Marked(span)) => Some(span),
                    _ => None,
                })
                .collect(),
        };
        for span in found {
            if !spans.contains(&span) {
                spans.push(span);
            }
        }
    }
    spans
}

/// What `resources` binds `name` to in `kind`, as written.
fn binding(cx: &mut Cx, resources: &Dictionary, kind: ResKind, name: &[u8]) -> Option<Object> {
    let category = match resources.get(kind.key()).ok()? {
        Object::Reference(r) => cx.dict(*r)?,
        Object::Dictionary(d) => d.clone(),
        _ => return None,
    };
    category.get(name).ok().cloned()
}

/// Whether `bound`, what a page's resources bind a name to, is `res`: the
/// object it's written as, or one whose content is `res`'s one object.
fn alike(cx: &Cx, bound: &Object, res: &Res) -> bool {
    let Object::Reference(r) = bound else {
        return ResId::of(std::slice::from_ref(bound)) == res.id;
    };
    match &res.body {
        ResBody::Written(written) => written == r,
        ResBody::Objects(objects) => {
            objects.len() == 1
                && (cx.read(*r))
                    .is_some_and(|(o, ..)| ResId::of(std::slice::from_ref(&*o)) == res.id)
        }
    }
}

/// How many `q`s and `Q`s isolate content of `balance`: `open = 1 + d`
/// and `close = 1 + d + net`, `d = max(0, −min_prefix)` (§3.4).
fn isolated(balance: Balance) -> (u32, u32) {
    // `net` is never below `min_prefix`, so `close` is 1 at least.
    let d = i64::from(balance.min_prefix).min(0).unsigned_abs();
    let open = 1 + d;
    let close = (1 + d as i64 + i64::from(balance.net)) as u64;
    (count(open), count(close))
}

/// Generator `g` laid out on page `id`, as `ctx` says it is, its names
/// clear of `reserved` and kept there, what it draws with added to
/// `needs`: its block, marked `<< /K /Gen /G (g…) /L <…> /R <…> >>` and,
/// on a page the header numbers, `/N (…)`, written once a file.
fn drawn(
    cx: &mut Cx,
    (id, g): (PageId, &Generator),
    ctx: &PageCtx,
    reserved: &mut Reserved,
    needs: &mut ResourceNeeds,
) -> Object {
    let art = g.spec.watermark().and_then(|wm| art(cx, &wm.content));
    let drawn = generate::layout(g, ctx, art.as_ref(), reserved, cx.private);
    let recipe = generate::recipe_digest(g, id);
    // What a view or a save asks of the page next ([`header_digest`]): a
    // layout whose lists say it.
    if g.spec.header_footer().is_some() && !cx.strips() {
        let reads = crate::realize::layout_reads(g.needs(), ctx);
        debug_assert_eq!(drawn.layout_digest, generate::layout_digest(g, ctx, None));
        (cx.st.laid()).put((g.id, recipe, reads), drawn.layout_digest);
    }
    reserved.take(&drawn.res);
    add_needs(needs, drawn.res.clone());
    let mut marker = dictionary! {
        "K" => "Gen",
        "G" => Object::String(g.id.to_string().into_bytes(), StringFormat::Literal),
        "L" => digest_string(drawn.layout_digest),
        "R" => digest_string(recipe),
    };
    // Its Bates number, in decimal, which an open reads the numbers back
    // from.
    if let Some(n) = ctx.bates {
        marker.set("N", Object::string_literal(n.to_string()));
    }
    let (block, key) = block(cx, marker, drawn.ops.to_vec());
    let layout = drawn.layout_digest;
    drew(
        cx,
        (id, g),
        DrawnBlock {
            layout,
            recipe,
            key,
        },
    );
    block
}

/// `res` added to `needs`, each but a name it has already.
fn add_needs(needs: &mut ResourceNeeds, res: ResourceNeeds) {
    for need in res.0 {
        if !(needs.0.iter()).any(|n| n.kind == need.kind && n.name == need.name) {
            needs.0.push(need);
        }
    }
}

/// Notes generator `g`'s block on page `id` that the page's content, page
/// of `src`, holds and keeps as it is: `stream`, which `mark` marks.
/// Nothing for one that doesn't say both its digests.
fn keeps(
    cx: &mut Cx,
    (id, g): (PageId, &Generator),
    src: &Src,
    (stream, mark): (ObjectId, Option<Marker>),
) {
    let Some(Marker::Gen {
        layout: Some(layout),
        recipe: Some(recipe),
        ..
    }) = mark
    else {
        return;
    };
    let key = ObjKey::Base {
        blob: src.id(),
        obj: stream,
        copy: 0,
    };
    drew(
        cx,
        (id, g),
        DrawnBlock {
            layout,
            recipe,
            key,
        },
    );
}

/// Notes, for a save's record (§9.1's `drawn`), generator `g`'s `block` on
/// page `id`; and when the saved file's block of it there was drawn from
/// the recipe `g` draws from there now, that the page draws it again, and
/// when that block is another one, that it may go: a live redraw, the same
/// drawn otherwise only for where the page is, the count, its label or its
/// boxes (§3.5, §11.9's allowed drops, [`AllowedDrops`]). Nothing for an
/// overlay.
///
/// [`AllowedDrops`]: super::AllowedDrops
fn drew(cx: &mut Cx, (id, g): (PageId, &Generator), block: DrawnBlock) {
    if cx.overlay.is_some() {
        return;
    }
    let was = cx.prior.and_then(|drawn| drawn.get(&(id, g.id)));
    if let Some(was) = was
        && was.recipe == generate::recipe_digest(g, id)
    {
        cx.redrawn.insert((id, g.id));
        if was.key != block.key {
            cx.allowed.insert(was.key);
        }
    }
    cx.drawn.insert((id, g.id), block);
}

/// The names page `index` of `src`'s resources bind, as `page` names them,
/// but those `dropped`: what what's drawn on it anew keeps clear of. Kept
/// by what the resources are and what's dropped, so pages that share
/// them are read once.
fn reserved(cx: &mut Cx, src: &Src, index: u32, page: &Dictionary, dropped: &NameSet) -> Reserved {
    let base_res = src.page(index).and_then(|f| f.resources.clone());
    let identity = resources_identity(src.id(), base_res.as_deref());
    let key = (identity, names_of(dropped));
    if let Some(reserved) = cx.reserved_of.get(&key) {
        return reserved.clone();
    }
    let mut reserved = cx.reserved(page.get(b"Resources").ok());
    for (kind, name) in dropped.iter() {
        reserved.forget(kind, name);
    }
    cx.reserved_of.insert(key, reserved.clone());
    reserved
}

/// What a page's resources lose of sheaf's private data
/// ([`LegacySel::Resources`]): the names an older sheaf gave them, each
/// with its new one; and whether the property lists named there lose
/// [`private::LIST_KEYS`].
#[derive(Default)]
struct ResStrip {
    renamed: Vec<(ResKind, Vec<u8>, Vec<u8>)>,
    lists: bool,
}

impl ResStrip {
    fn is_empty(&self) -> bool {
        self.renamed.is_empty() && !self.lists
    }
}

/// `page`'s resources, page `index` of `src`'s, with the names `dropped`
/// taken out of their categories, those `needs` adds bound, and what
/// `strip` takes out of them: a dictionary of the page's own
/// ([`ObjKey::PageRes`]), shared by the pages whose base resources,
/// dropped names, added names and strip are the same. Nothing when it's
/// none of them.
fn page_res(
    cx: &mut Cx,
    (src, index): (&Src, u32),
    page: &mut Dictionary,
    dropped: &NameSet,
    (needs, strip): (&ResourceNeeds, &ResStrip),
) {
    if dropped.is_empty() && needs.0.is_empty() && strip.is_empty() {
        return;
    }
    let base_res = src.page(index).and_then(|f| f.resources.clone());
    let key = ObjKey::PageRes {
        base: resources_identity(src.id(), base_res.as_deref()),
        dropped: names_of(dropped),
        added: added(needs, strip),
    };
    if !cx.page_res.contains_key(&key) {
        let mut resources = sink::resources(cx, page);
        for kind in ResKind::ALL {
            let renamed: Vec<_> = (strip.renamed.iter())
                .filter(|(k, ..)| *k == kind)
                .collect();
            let lists = strip.lists && kind == ResKind::Properties;
            let mut names = dropped.of(kind).peekable();
            if names.peek().is_none() && renamed.is_empty() && !lists {
                continue;
            }
            let mut category = match resources.get(kind.key()) {
                Ok(Object::Reference(r)) => cx.dict(*r).unwrap_or_default(),
                Ok(Object::Dictionary(d)) => d.clone(),
                _ => continue,
            };
            for name in names {
                category.remove(name);
            }
            for (_, old, new) in renamed {
                if let Some(bound) = category.remove(old) {
                    category.set(new.clone(), bound);
                }
            }
            if lists {
                strip_lists(cx, &mut category);
            }
            resources.set(kind.key(), category);
        }
        sink::bind(cx, needs, &mut resources);
        cx.page_res.insert(key, resources);
    }
    let resources = cx.placeholder(key);
    page.set("Resources", resources);
}

/// Stream `obj` of the source in blob `blob` with ranges spliced out, as
/// `bytes` are left of it: a copy of its own, once a file, keyed by which
/// stream it is and what's left ([`ObjKey::Filtered`]).
fn filtered_copy(cx: &mut Cx, blob: BlobId, obj: ObjectId, bytes: Vec<u8>) -> Object {
    let base = ObjKey::Base { blob, obj, copy: 0 };
    let key = ObjKey::Filtered {
        stream: sheaf_util::checksum(&base.bytes()),
        filter: sheaf_util::checksum(&bytes) as u64,
    };
    let id = cx.push_once(key, |_| {
        let mut stream = Stream::new(Dictionary::new(), bytes);
        let _ = stream.compress();
        Object::Stream(stream)
    });
    Object::Reference(id)
}

/// `n`, as many `q`s or `Q`s as a stream holds: a page's content counts
/// them in an `i32`, so it's one.
fn count(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// What a page's resources are, as `blob`'s file writes them (`res`, or
/// none): the object they are, or what's written inline. Pages that share
/// one share it.
fn resources_identity(blob: BlobId, res: Option<&Object>) -> u128 {
    let mut bytes = blob.hash.to_be_bytes().to_vec();
    bytes.extend(blob.len.to_be_bytes());
    match res {
        None => bytes.push(0),
        Some(Object::Reference((number, generation))) => {
            bytes.push(1);
            bytes.extend(number.to_be_bytes());
            bytes.extend(generation.to_be_bytes());
        }
        Some(inline) => {
            bytes.push(2);
            bytes.extend(ResId::of(std::slice::from_ref(inline)).0.to_be_bytes());
        }
    }
    sheaf_util::checksum(&bytes)
}

/// The property lists named in `category`, a page's `/Properties`, as
/// written without [`private::LIST_KEYS`]: a list that has them, written
/// there in place of the one it named.
fn strip_lists(cx: &mut Cx, category: &mut Dictionary) {
    let names: Vec<Vec<u8>> = category.iter().map(|(name, _)| name.clone()).collect();
    for name in names {
        let list = match category.get(&name) {
            Ok(Object::Dictionary(d)) => Some(d.clone()),
            Ok(&Object::Reference(r)) => cx.dict(r),
            _ => None,
        };
        let Some(mut list) = list else {
            continue;
        };
        if !(list.iter()).any(|(k, _)| private::LIST_KEYS.contains(&&k[..])) {
            continue;
        }
        for key in private::LIST_KEYS {
            list.remove(key);
        }
        category.set(name, list);
    }
}

/// A digest of what a page's resources get besides their base's: the names
/// `needs` adds, each one's category, name and resource; and what `strip`
/// takes out of them.
fn added(needs: &ResourceNeeds, strip: &ResStrip) -> u64 {
    let mut bytes = Vec::new();
    for need in &needs.0 {
        bytes.extend(need.kind.key());
        bytes.push(0);
        bytes.extend(&need.name);
        bytes.push(0);
        bytes.extend(need.res.id.0.to_be_bytes());
    }
    if !strip.is_empty() {
        bytes.push(1);
        bytes.push(u8::from(strip.lists));
        for (kind, old, new) in &strip.renamed {
            bytes.extend(kind.key());
            bytes.push(0);
            bytes.extend(old);
            bytes.push(0);
            bytes.extend(new);
            bytes.push(0);
        }
    }
    names_digest(&bytes)
}

/// The names an older sheaf gave the resources `reserved` keeps
/// ([`private::is_resource_name`]), each given a new one, kept there in its
/// place.
fn renamed(reserved: &mut Reserved) -> Vec<(ResKind, Vec<u8>, Vec<u8>)> {
    let older: Vec<(ResKind, Vec<u8>)> = (reserved.names())
        .filter(|(_, name)| private::is_resource_name(name))
        .map(|(kind, name)| (kind, name.to_vec()))
        .collect();
    (older.into_iter())
        .filter_map(|(kind, old)| {
            let new = reserved.rename(kind, &old)?;
            Some((kind, old, new))
        })
        .collect()
}

/// A digest of the names `dropped` takes out of a page's resources: each
/// one's category and name.
fn names_of(dropped: &NameSet) -> u64 {
    let mut bytes = Vec::new();
    for (kind, name) in dropped.iter() {
        bytes.extend(kind.key());
        bytes.push(0);
        bytes.extend(name);
        bytes.push(0);
    }
    names_digest(&bytes)
}

/// `bytes`' digest, as a page's resources' key holds a set of names
/// ([`ObjKey::PageRes`]).
fn names_digest(bytes: &[u8]) -> u64 {
    sheaf_util::checksum(bytes) as u64
}

/// What a block's stream dictionary says: `/SheafBlock` and `marker`, or
/// nothing when the emission strips sheaf's private data (§15).
fn marked(cx: &Cx, marker: Dictionary) -> Dictionary {
    match cx.strips() {
        true => Dictionary::new(),
        false => dictionary! { "SheafBlock" => marker },
    }
}

/// A block of sheaf's drawing `ops`, marked `/SheafBlock` with `marker`
/// ([`marked`]), as a stream of its own, once a file: where it is, and its
/// key.
fn block(cx: &mut Cx, marker: Dictionary, ops: Vec<u8>) -> (Object, ObjKey) {
    let stream = Object::Stream(Stream::new(marked(cx, marker), ops));
    let key = ObjKey::Block(ResId::of(std::slice::from_ref(&stream)).0);
    let id = cx.push_once(key, |_| {
        let mut stream = stream;
        if let Object::Stream(s) = &mut stream {
            let _ = s.compress();
        }
        stream
    });
    (Object::Reference(id), key)
}

/// Layer `l` drawn on a page, its names clear of `reserved` and kept
/// there, what it draws with added to `needs`: its block.
fn layer(cx: &mut Cx, l: &Layer, reserved: &mut Reserved, needs: &mut ResourceNeeds) -> Object {
    match &l.body {
        Static::OcrText(text) => recognized(cx, (text, l.id), reserved, needs),
        Static::Placements(placed) => flattened(cx, (placed, l.id), reserved, needs),
    }
}

/// A block of sheaf's drawing `ops`, marked `/SheafBlock` with `marker`
/// ([`marked`]), as a stream of its own, once a file, its data deflated as
/// it's written ([`Deferred`]): where it is. Keyed as [`block`] keys it.
fn deferred_block(cx: &mut Cx, marker: Dictionary, ops: Vec<u8>) -> Object {
    let stream = Object::Stream(Stream::new(marked(cx, marker), ops));
    let key = ObjKey::Block(ResId::of(std::slice::from_ref(&stream)).0);
    let Object::Stream(Stream {
        mut dict, content, ..
    }) = stream
    else {
        unreachable!("made a stream");
    };
    let id = cx.push_deferred(key, |_| {
        dict.remove(b"Length");
        dict.set("Filter", "FlateDecode");
        let ops: Arc<[u8]> = content.into();
        let data = move || super::deflated(&ops);
        Deferred {
            dict,
            data: Arc::new(data),
        }
    });
    Object::Reference(id)
}

/// Recognized text `text`, its layer `id`'s, drawn on a page, its names
/// clear of `reserved` and kept there, what it draws with added to
/// `needs`: its block, marked `<< /K /Ocr /B (b…) >>`, written once a
/// file.
fn recognized(
    cx: &mut Cx,
    (text, id): (&TextLayer, BlockId),
    reserved: &mut Reserved,
    needs: &mut ResourceNeeds,
) -> Object {
    let (ops, res) = crate::text_layer::block(text, id, reserved, cx.private);
    reserved.take(&res);
    add_needs(needs, res);
    let marker = dictionary! { "K" => "Ocr", "B" => Object::string_literal(id.to_string()) };
    block(cx, marker, ops).0
}

/// The forms `placed`, layer `id`'s, drawn on a page in order, each in a
/// `q` of its own through its matrix and, when it says so, marked
/// `/Artifact`, as XObjects named clear of `reserved` and kept there, what
/// they draw with added to `needs`: its block, marked `<< /K /Flat /B
/// (b…) >>`, written once a file as it's serialized. A source's form is
/// copied with the page; another that can't be had ([`Cx::form_of`]) is
/// left out, said once.
fn flattened(
    cx: &mut Cx,
    (placed, id): (&[Placement], BlockId),
    reserved: &mut Reserved,
    needs: &mut ResourceNeeds,
) -> Object {
    let mut w = ContentWriter::new();
    for p in placed {
        let said = |e: &anyhow::Error| {
            log::warn!("a flattened annotation is drawn without its appearance: {e:#}");
        };
        // A source's is the copy of it the page's content reaches, its box
        // unread: the placement's matrix was worked out from it.
        let at = match &p.form {
            FormRef::Base(at) => placed_form(cx, at).as_reference().ok(),
            form => cx.form_of(form, said).map(|f| f.id),
        };
        let Some(at) = at else {
            continue;
        };
        // Named by which form it is, whichever number it's written at.
        let form = Res {
            id: ResId(p.form.digest()),
            body: ResBody::Written(at),
        };
        let artifact = p.tag == PlaceTag::Artifact;
        if artifact {
            w.begin_tagged("Artifact");
        }
        w.save();
        w.transform(p.matrix);
        w.draw(form);
        w.restore();
        if artifact {
            w.end_marked();
        }
    }
    let (ops, res) = w.finish(reserved);
    reserved.take(&res);
    add_needs(needs, res);
    let marker = dictionary! { "K" => "Flat", "B" => Object::string_literal(id.to_string()) };
    deferred_block(cx, marker, ops)
}

/// A digest as a block writes it: a string of its 16 bytes, big-endian.
fn digest_string(digest: u128) -> Object {
    Object::String(digest.to_be_bytes().to_vec(), StringFormat::Hexadecimal)
}

/// One of the shared streams that isolate a page's content, marked
/// ([`marked`]), once a file: where it is.
fn isolation(cx: &mut Cx, shared: Shared) -> Object {
    let id = cx.push_once(ObjKey::Shared(shared), |cx| {
        let (op, n) = match shared {
            Shared::QOpen(n) => (b"q\n", n),
            Shared::QClose(n) => (b"Q\n", n),
        };
        let marker = marked(cx, dictionary! { "K" => "Isolate" });
        let mut stream = Stream::new(marker, op.repeat(n as usize));
        let _ = stream.compress();
        Object::Stream(stream)
    });
    Object::Reference(id)
}

/// What watermark `content` draws that isn't its own, as the file being
/// written has it: its image or page, read once a file, and once a
/// realizer ([`Arts`]). None for text, and for an image or a page that
/// can't be had, which is drawn without it.
fn art(cx: &mut Cx, content: &WatermarkContent) -> Option<Art> {
    match content {
        WatermarkContent::Text { .. } => None,
        WatermarkContent::Image {
            image: ImageRef::Blob(blob),
            ..
        } => made(cx, (blob.id(), None), || {
            image_of(blob.bytes())
                .inspect_err(|e| log::warn!("a watermark is drawn without its image: {e:#}"))
                .ok()
        }),
        WatermarkContent::Page {
            page: PageForm::Blob { pdf, index },
            ..
        } => made(cx, (pdf.id(), Some(*index)), || {
            super::annots::blob_form(pdf.bytes(), *index)
                .map(|(objects, bbox, matrix)| Art::Form {
                    res: Res::of(objects),
                    bbox,
                    matrix,
                })
                .inspect_err(|e| log::warn!("a watermark is drawn without its page: {e:#}"))
                .ok()
        }),
        WatermarkContent::Image {
            image: ImageRef::Placed(p),
            ..
        } => {
            let body = placed(cx, &p.at);
            generate::placed_art(p, true, body)
        }
        WatermarkContent::Page {
            page: PageForm::Placed(p),
            ..
        } => {
            let body = placed(cx, &p.at);
            generate::placed_art(p, false, body)
        }
    }
}

/// What watermark `content`'s image or page is drawn as, as
/// `/SheafGenerators` says it: the digest of its resource, and the quarter
/// turns an image shows upright at. Nothing, for one that can't be had,
/// which draws nothing and isn't found again.
pub(crate) fn drawn_as(cx: &mut Cx, content: &WatermarkContent) -> (ResId, u8) {
    match art(cx, content) {
        Some(Art::Image { res, turns, .. }) => (res.id, turns),
        Some(Art::Form { res, .. }) => (res.id, 0),
        None => (ResId(0), 0),
    }
}

/// A watermark's image or page, by its blob and the page's index (None
/// for an image).
pub(crate) type ArtKey = (BlobId, Option<u32>);

/// Watermarks' images and pages kept across emissions: a realizer's,
/// whose groups emit each page alone, so a watermark of one image on every
/// page decodes it once. At most [`ARTS_KEPT`], the latest made first; each
/// is what its blob's bytes make, so a kept one is what making it again
/// would give.
#[derive(Default)]
pub(crate) struct Arts(Mutex<VecDeque<(ArtKey, Option<Art>)>>);

/// The images and pages [`Arts`] keeps: a document's watermark and
/// background, and a few more as their dialog tries others.
const ARTS_KEPT: usize = 4;

impl Arts {
    fn get(&self, key: ArtKey) -> Option<Option<Art>> {
        let kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, art)| art.clone())
    }

    fn keep(&self, key: ArtKey, art: Option<Art>) {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|(k, _)| *k != key);
        kept.push_front((key, art));
        kept.truncate(ARTS_KEPT);
    }
}

#[cfg(test)]
thread_local! {
    /// How many watermark images and pages this thread has made.
    pub(crate) static ARTS_MADE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The image or page `key` names, which `make` makes: once an emission,
/// and once its overlay's [`Arts`].
fn made(cx: &mut Cx, key: ArtKey, make: impl FnOnce() -> Option<Art>) -> Option<Art> {
    if let Some(made) = cx.arts.get(&key) {
        return made.clone();
    }
    let kept = cx.overlay.and_then(|o| o.arts);
    let made = match kept.and_then(|arts| arts.get(key)) {
        Some(made) => made,
        None => {
            #[cfg(test)]
            ARTS_MADE.set(ARTS_MADE.get() + 1);
            let made = make();
            if let Some(arts) = kept {
                arts.keep(key, made.clone());
            }
            made
        }
    };
    cx.arts.insert(key, made.clone());
    made
}

/// An image file's first image as an image XObject.
fn image_of(bytes: &[u8]) -> anyhow::Result<Art> {
    let image = (sheaf_image::io::read_bytes(bytes.to_vec())?
        .into_iter()
        .next())
    .ok_or_else(|| anyhow::anyhow!("the file holds no image"))?;
    let stream = crate::images::image_stream(&image)?;
    Ok(Art::Image {
        res: Res::of([Object::Stream(stream)]),
        width: image.width,
        height: image.height,
        turns: (image.rotate % 4) as u8,
    })
}

/// The XObject `at`, a saved block's, as a resource drawn: copy 0 of it,
/// where it's written ([`placed_form`]).
fn placed(cx: &mut Cx, at: &ObjRef) -> ResBody {
    let reference = placed_form(cx, at);
    ResBody::Written(reference.as_reference().expect("a reference stays one"))
}

/// Copy 0 of form `at`, a source's, as written: its placeholder.
fn placed_form(cx: &mut Cx, at: &ObjRef) -> Object {
    let scope = Scope {
        src: &at.src,
        copy: 0,
    };
    cx.sym(&Object::Reference(at.id), &scope)
}
