//! What sheaf draws on a page (M10.5 §11.4).
//!
//! - **Watermarks and backgrounds**, the generators on the page, laid out
//!   on it ([`generate::layout`]) and assembled around its own content:
//!   `/Contents [under… QOpen^open base′… QClose^close over…]`, a
//!   background under it and a watermark over it. Each block is a stream
//!   of its own marked `/SheafBlock << /K /Gen /G (g…) /L <…> /R <…> >>`,
//!   its generator, its layout's digest and its recipe's; identical blocks
//!   are one object ([`ObjKey::Block`]), so a watermark on 7,000 pages
//!   alike is one. A page with nothing to draw for one gets an empty block,
//!   so its scope can be read back. The page's own content is isolated by
//!   `q`s and `Q`s, shared marked streams ([`Shared::QOpen`],
//!   [`Shared::QClose`]) counted by its balance (§3.4): `open = 1 + d` and
//!   `close = 1 + d + net`, `d = max(0, −min_prefix)`, so no `Q` of its
//!   undoes theirs, and nothing it leaves set reaches what's drawn over it.
//!   Its streams are referenced, never re-encoded. A page whose content
//!   can't be read gets none, said once in a note, and the save goes on.
//!   The page's resources are a new dictionary ([`ObjKey::PageRes`]): its
//!   base page's, the categories it adds to copied one level down, the
//!   names it draws with added clear of the names they bind; pages that
//!   share base resources and additions share one, written only when the
//!   page still names it once the legacy installers are done.
//! - **base′**, the page's own content as a save writes it, when its file
//!   holds sheaf's blocks (by its streams' markers, [`Source::markers`]) or
//!   a filter changes it. The blocks of the document's generators
//!   ([`DocState::backs`]) are set aside, so one gone or changed leaves no
//!   trace and nesting never grows; another file's are kept, with the
//!   streams that isolate them, until a filter takes them out; isolating
//!   streams are otherwise set aside too, and so is an OCR layer sheaf
//!   wrote before M10.5 that a new one replaces. A block laid out alike
//!   again (its `/L`) is the file's stream, and a stream of the file's
//!   that isolates as many as base′ needs is used again; a page whose
//!   blocks are all laid out alike, in place around its isolated content,
//!   is written as it is ([`intact`], which the realizer asks too). A
//!   filter's ranges are spliced out of the streams they're in, each a
//!   copy of its own ([`ObjKey::Filtered`]), so a stream pages share stays
//!   whole on the others: another file's blocks merged into a stream, and
//!   others' pagination artifacts, as Remove Watermark selects them and as
//!   the generators drawn on a page replace those of their kinds
//!   ([`replacing`]), every tool's but the author's (§11.4). One a splice
//!   refuses stays, counted in a note; another file's block of a kind
//!   replaced, a stream of its own, is left out whole. A range kept whose
//!   property list a filter takes what an older sheaf kept in it out of
//!   (Clean Copy's [`Filter::StripLegacy`]) has the list written again
//!   without it, in the same copy; one that can't be keeps it, counted in
//!   a note. What's left is counted again, and the names only what's set
//!   aside or cut used are dropped from the page's resources (one level
//!   down), so the form an Acrobat watermark draws is no longer the page's,
//!   but those a block drawn anew binds to the same resource.
//! - **OCR text**, through `text_layer::install`: `/Contents` becomes `[q,
//!   …, layer]`, the layer's streams marked `/SheafOcrLayer` and its text
//!   in `/SheafOcr`. A layer the page's content holds already (a re-OCR
//!   over a saved one) is taken out first, so the page has one.
//! - **The header and footer** through `stamps::install`, on each page it
//!   covers whose base doesn't draw it already (`stamps::drawn_over`): a
//!   page whose file sheaf saved with it isn't drawn on twice. Bates
//!   numbers count the pages it covers, in order.
//!
//! The OCR text and header and footer go over the rest, in their legacy
//! shape until slices 17 and 18. An overlay draws each only when its view
//! asks for it, the watermarks and backgrounds as a save draws them, and
//! finds where a page is, whether the header and footer is on it and its
//! Bates number from the state's own tables, never by a walk of the
//! pages.
//!
//! What's drawn adds resources keyed by digest: a resource
//! ([`ObjKey::Res`], its other objects [`ObjKey::ResPart`]) is written once
//! a file, so every OCR page draws in one font, and every watermark in one
//! font, graphics state and image.
//!
//! [`ObjKey::Res`]: crate::materialize::ObjKey::Res
//! [`ObjKey::ResPart`]: crate::materialize::ObjKey::ResPart
//! [`ObjKey::Block`]: crate::materialize::ObjKey::Block
//! [`ObjKey::PageRes`]: crate::materialize::ObjKey::PageRes
//! [`ObjKey::Filtered`]: crate::materialize::ObjKey::Filtered
//! [`Shared::QOpen`]: crate::materialize::key::Shared::QOpen
//! [`Shared::QClose`]: crate::materialize::key::Shared::QClose
//! [`Source::markers`]: crate::source::Source::markers
//! [`DocState::backs`]: crate::model::DocState::backs

use std::borrow::Cow;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat, dictionary};

use super::{Cx, PdfVersion};
use crate::content::sink::{self, Sink as _};
use crate::content::{
    self, Balance, BlockAt, ContentError, GenId, Marker, NameSet, PagOrigin, PageBlocks,
    PageContent, Pagination, PaginationKind, Pos, Res, ResBody, ResCx, ResId, ResKind, Reserved,
    ResourceNeeds, SheafBlock, SpanAcross,
};
use crate::generate::{self, Art};
use crate::geom::Geom;
use crate::materialize::copy::Scope;
use crate::materialize::key::{ObjKey, Shared};
use crate::model::{
    DocState, Filter, Generator, ImageRef, LEGACY_KEYS, LegacySel, MarkedSel, PageDef, PageForm,
    PageId, StreamSel, WatermarkContent,
};
use crate::source::ObjRef;
use crate::stamps;
use crate::store::{BlobId, Src};

/// The PDF version what sheaf draws over a page showing `def` needs, the
/// header and footer's and the generators' aside: its OCR text's fonts.
pub(crate) fn needs(def: &PageDef) -> Option<PdfVersion> {
    def.content.ocr_text().map(|_| PdfVersion::CID_FONTS)
}

/// Whether what sheaf draws on `st`'s pages needs a later PDF version than
/// `version`: a watermark's marked content and transparency, or OCR text's
/// fonts ([`needs`]); the header and footer's aside, which only a Rewrite
/// draws until slice 17. The planner reads it, as an Append that needs a
/// later version than its file says so in the catalog (§11.7), which a
/// signature counts as any other change (§9.3).
pub(crate) fn needs_after(st: &DocState, version: PdfVersion) -> bool {
    let watermarked = || (st.generators().values()).any(|g| g.spec.watermark().is_some());
    (version < PdfVersion::ARTIFACTS && watermarked())
        || (version < PdfVersion::CID_FONTS
            && (st.pages()).any(|(_, def, _)| needs(def).is_some_and(|v| v > version)))
}

/// The watermarks and backgrounds on page `id` of `st`, backgrounds first:
/// what a save draws on it, and an overlay whose view draws generators.
pub(crate) fn generators_on(st: &DocState, id: PageId) -> Vec<&Arc<Generator>> {
    if st.generators().is_empty() {
        return Vec::new();
    }
    let mut on: Vec<&Arc<Generator>> = (st.generators().values())
        .filter(|g| g.spec.watermark().is_some() && g.covers(id))
        .collect();
    on.sort_by_key(|g| g.z());
    on
}

/// What sheaf draws over page `id`, which shows `def` with `geom` at
/// place `pos`, drawn into `page`, its dictionary as emitted.
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
    stack(cx, id, def, geom, ocr, draws, page);
    if ocr {
        if let Some(layer) = def.content.ocr_text() {
            crate::text_layer::install(cx, page, layer);
        }
        if let Some(version) = needs(def) {
            cx.needs(version);
        }
    }
    header_footer(cx, id, def, geom, pos, page);
    // The page's own resources, when it still names them.
    if let Ok(&Object::Reference(r)) = page.get(b"Resources")
        && let Some(key @ ObjKey::PageRes { .. }) = cx.syms.key(r)
    {
        cx.push_once(key, |cx| {
            Object::Dictionary(cx.page_res.get(&key).cloned().unwrap_or_default())
        });
    }
}

/// The header and footer over page `id`, as [`page`] draws it.
fn header_footer(
    cx: &mut Cx,
    id: PageId,
    def: &PageDef,
    geom: &Geom,
    pos: usize,
    page: &mut Dictionary,
) {
    let st = cx.st;
    // A derived copy's page draws what the copy says it drew.
    let (hf, bates) = match cx.overlay.map(|o| o.inputs) {
        None => (
            stamps::drawn_over(st, id),
            cx.bates.get(pos).copied().flatten(),
        ),
        Some(v) => {
            let hf = (st.header_footer()).filter(|hf| {
                v.generators
                    && st.header_covers(id)
                    && !stamps::base_draws(&def.base.src, def.base.index, hf)
            });
            (hf, st.bates_of(id))
        }
    };
    let Some(hf) = hf else {
        return;
    };
    let facts = stamps::PageFacts {
        index: pos,
        count: st.page_count(),
        bates,
    };
    let placed = hf.layout(geom.shown(), &facts);
    if !placed.is_empty() {
        stamps::install(cx, page, &placed, hf.color, geom);
        cx.needs(PdfVersion::ARTIFACTS);
    }
}

/// Page `id`'s own content as a save writes it, which shows `def` with
/// `geom`, and the generators on it (`draws`: when the view draws them)
/// around it, isolated by its balance, drawn into `page`, with its
/// resources a [`ObjKey::PageRes`] when they change (see the module's
/// doc); `ocr` when its recognized text is drawn too. As it is when its
/// file holds no block of sheaf's and nothing covers it, or when the
/// blocks it holds are what's drawn there now ([`intact`]) and no OCR layer
/// of its own goes. Others' artifacts of the kinds the generators drawn on
/// it replace are taken out ([`replacing`]); those a splice refuses stay,
/// counted in a note. A page whose content can't be read is drawn as it
/// is, and gets no generator, said once in a note.
fn stack(
    cx: &mut Cx,
    id: PageId,
    def: &PageDef,
    geom: &Geom,
    ocr: bool,
    draws: bool,
    page: &mut Dictionary,
) {
    let on = match draws {
        true => generators_on(cx.st, id),
        false => Vec::new(),
    };
    let (src, index) = (def.base.src.clone(), def.base.index);
    let marks = src.markers(index);
    let filters = &def.content.filters;
    if on.is_empty() && marks.is_empty() && filters.is_empty() {
        return;
    }
    // An OCR layer sheaf wrote before, which the new one replaces, isn't
    // the page's own content.
    let replaced =
        ocr && def.content.ocr_text().is_some() && crate::text_layer::has_layer(cx, page);
    let st = cx.st;
    let contents = src.page(index).map_or(&[][..], |f| &f.contents);
    // Each generator's layout here, as `intact` digests it, which a rebuild
    // doesn't digest again.
    let mut laid = Vec::new();
    let mut digest = |g: &Generator| {
        let digest = digest_of(cx, g, geom);
        laid.push((g.id, digest));
        Some(digest)
    };
    if filters.is_empty() && !replaced && intact(st, id, &marks, contents, &on, &mut digest) {
        return;
    }
    let replacing = replacing(&on, &src, index);
    let written = match (marks.is_empty() && filters.is_empty(), replacing) {
        (true, None) => {
            wrapped(cx, &src, index, geom, &on, replaced, page).then_some(Stayed::default())
        }
        (_, replacing) => {
            let filters: Vec<Filter> = filters.iter().cloned().chain(replacing).collect();
            let at = (id, &src, index);
            rebuilt(cx, at, &filters, (geom, &laid), &on, replaced, page)
        }
    };
    let Some(stayed) = written else {
        match on.is_empty() {
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
    if stayed.settings > 0 {
        cx.settings_kept.push((id, stayed.settings));
    }
    if !on.is_empty() {
        cx.needs(PdfVersion::ARTIFACTS);
    }
}

/// What stays of what a page's filters select: the ranges a splice
/// refuses to take out, and the property lists of an older sheaf's that
/// can't be written again without what it kept in them.
#[derive(Default)]
struct Stayed {
    ranges: usize,
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
        .flat_map(|g| g.replaces().iter().copied())
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
/// sheaf marked, keeps its content as it is, the generators `on` it (backgrounds
/// first) laid out alike there: none of the blocks it holds is another
/// file's; it holds the block of each that's laid out alike, whose digest
/// `digest` gives (None when it can't tell), backgrounds' first and
/// watermarks' last, and between them the streams that isolate the rest,
/// which is all its own; or with none on it, it holds no block of the
/// document's, and no isolation but around another file's blocks. What
/// the emitter writes as the page has it, and what the realizer renders
/// from its base page.
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
        return !backed && (!isolates || unbacked);
    }
    if unbacked {
        return false;
    }
    let mark = |stream: ObjectId| marks.iter().find(|(m, _)| *m == stream).map(|(_, m)| *m);
    let under = on
        .iter()
        .take_while(|g| g.z() == sheaf_edit::Z::Under)
        .count();
    let over = on.len() - under;
    let Some(inner) = contents.len().checked_sub(under + over + 2) else {
        return false;
    };
    let (blocks_under, rest) = contents.split_at(under);
    let (middle, blocks_over) = rest.split_at(inner + 2);
    // A block of `g` laid out alike.
    let mut alike = |stream: ObjectId, g: &Generator| match mark(stream) {
        Some(Marker::Gen {
            id,
            layout: Some(l),
        }) => id == g.id && digest(g) == Some(l),
        _ => false,
    };
    let blocks = blocks_under.iter().chain(blocks_over);
    blocks.zip(on).all(|(&stream, g)| alike(stream, g))
        && mark(middle[0]) == Some(Marker::Isolate)
        && mark(middle[inner + 1]) == Some(Marker::Isolate)
        && middle[1..=inner].iter().all(|&s| mark(s).is_none())
}

/// What generator `g` lays out on a page of `geom`, as its blocks say it
/// (`/L`), read from the model alone, as [`digest_of`] reads it: None for
/// an image or a page a file in the store holds, which only decoding it
/// tells. What the realizer asks, which never reads a page's content.
pub(crate) fn model_layout_digest(g: &Generator, geom: &Geom) -> Option<u128> {
    let wm = g.spec.watermark()?;
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
    Some(generate::layout_digest(g.id, wm, geom, art.as_ref()))
}

/// What generator `g` lays out on a page of `geom`, as its blocks say it
/// (`/L`), its image or page as this file has it.
fn digest_of(cx: &mut Cx, g: &Generator, geom: &Geom) -> u128 {
    let wm = g.spec.watermark().expect("a watermark");
    let art = art(cx, &wm.content);
    generate::layout_digest(g.id, wm, geom, art.as_ref())
}

/// The generators `on` page `index` of `src` drawn into `page` around its
/// own content, which holds none of sheaf's blocks and no filter changes:
/// its streams referenced as they are, counted by its blocks' balance, or
/// but for an OCR layer sheaf wrote before that a new one `replaced`,
/// counted again without it. False when its content can't be read.
fn wrapped(
    cx: &mut Cx,
    src: &Src,
    index: u32,
    geom: &Geom,
    on: &[&Arc<Generator>],
    replaced: bool,
    page: &mut Dictionary,
) -> bool {
    if replaced {
        crate::text_layer::strip(cx, page);
    }
    let base = sink::content_entries(cx, page);
    let balance = match replaced {
        false => src.blocks(index).map(|blocks| blocks.balance),
        true => fresh_balance(cx, src, index, &base),
    };
    let Ok(balance) = balance else {
        return false;
    };
    let mut reserved = reserved(cx, src, index, page, &NameSet::default());
    let mut needs = ResourceNeeds::default();
    let (mut under, mut over) = (Vec::new(), Vec::new());
    for g in on {
        let block = drawn(cx, g, geom, &mut reserved, &mut needs);
        match g.z() {
            sheaf_edit::Z::Under => under.push(block),
            sheaf_edit::Z::Over => over.push(block),
        }
    }
    let (open, close) = isolated(balance);
    let mut contents = under;
    contents.push(isolation(cx, Shared::QOpen(open)));
    contents.extend(base);
    contents.push(isolation(cx, Shared::QClose(close)));
    contents.extend(over);
    page.set("Contents", Object::Array(contents));
    page_res(cx, src, index, page, &NameSet::default(), &needs);
    true
}

/// Page `id`'s own content, page `index` of `src`, whose streams sheaf
/// marked, or which `filters` change, rebuilt into `page` with the
/// generators `on` it around what's left (§11.4): each stream's marker
/// read from its dictionary, whatever the file's catalog says, and one
/// that says it isolates content but holds more than `q`s or `Q`s the
/// page's own content; the blocks of the
/// document's generators set aside, those laid out alike again kept as
/// they are; another file's blocks kept but where a filter takes them out,
/// and the streams that isolate them with them, else set aside too; an
/// OCR layer sheaf wrote before that a new one `replaced` set aside; a
/// stream a filter's ranges come out of written as a copy
/// ([`ObjKey::Filtered`]), each range cut where a splice takes it
/// ([`content::splice_each`]), and each range kept whose list loses what an
/// older sheaf kept in it written so ([`content::without_keys`]). What's
/// left is counted again, and the names only what's set aside or cut used
/// are dropped from the page's resources, which a block drawn anew may
/// name again. A stream the file has that isolates as many as now is
/// referenced as it is. What of what the filters select stays, as a splice
/// or a rewrite refuses it; None when its content can't be read. `laid` is
/// what of the generators' layouts on `geom` is digested already.
#[allow(clippy::too_many_arguments)]
fn rebuilt(
    cx: &mut Cx,
    (id, src, index): (PageId, &Src, u32),
    filters: &[Filter],
    (geom, laid): (&Geom, &[(GenId, u128)]),
    on: &[&Arc<Generator>],
    replaced: bool,
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
    let marked: Vec<Option<Marker>> = (0..ids.len())
        .map(|i| match Marker::of(&dicts[i]) {
            Some(Marker::Isolate) if content::isolates(concat.stream(i)).is_none() => None,
            marker => marker,
        })
        .collect();
    let mark = |i: usize| marked[i];
    // Each stream: whether a filter leaves it out, as it does another
    // file's block of a kind it selects; and the block of the document's
    // each generator on the page lays out alike, kept.
    let dropped: Vec<bool> = (0..ids.len())
        .map(|i| {
            let theirs = matches!(mark(i), Some(Marker::Gen { id, .. }) if !backs(id));
            filtered(&dicts[i])
                || (theirs && others.iter().any(|p| p.span.start.stream as usize == i))
        })
        .collect();
    let mut kept_block: Vec<Option<usize>> = Vec::new();
    for g in on {
        let backed = (0..ids.len()).filter(|&i| {
            !dropped[i] && matches!(mark(i), Some(Marker::Gen { id, .. }) if id == g.id)
        });
        let backed: Vec<usize> = backed.collect();
        let digest = match backed.is_empty() {
            true => None,
            false => match laid.iter().find(|(id, _)| *id == g.id) {
                Some(&(_, digest)) => Some(digest),
                None => Some(digest_of(cx, g, geom)),
            },
        };
        kept_block.push(backed.into_iter().find(
            |&i| matches!(mark(i), Some(Marker::Gen { layout: Some(l), .. }) if Some(l) == digest),
        ));
    }
    let foreign = (0..ids.len())
        .any(|i| !dropped[i] && matches!(mark(i), Some(Marker::Gen { id, .. }) if !backs(id)));
    let reused = |i: usize| kept_block.contains(&Some(i));
    let aside: Vec<bool> = (0..ids.len())
        .map(|i| {
            dropped[i]
                || (replaced && dicts[i].has(crate::text_layer::MARKER.as_bytes()))
                || match mark(i) {
                    Some(Marker::Gen { id, .. }) => backs(id) && !reused(i),
                    Some(Marker::Isolate) => !foreign,
                    None => false,
                }
        })
        .collect();
    // The ranges the filters take out of streams kept, each the cuts that
    // take one out: a generator's merged into a stream, and the artifacts
    // they select but in a block of sheaf's of its own, set aside, left out
    // or laid out alike. One a splice refuses stays, counted.
    let mut groups: Vec<Vec<SpanAcross>> = Vec::new();
    let mut stayed = 0;
    for f in filters {
        let Filter::DropMarked(MarkedSel::SheafGen(g)) = f else {
            continue;
        };
        let found = blocks.iter().flat_map(|b| b.sheaf.iter());
        groups.extend(found.filter_map(|b| match (b.block, b.at) {
            (SheafBlock::Gen(id), BlockAt::Marked(span))
                if id == *g && !aside[span.start.stream as usize] =>
            {
                Some(vec![span])
            }
            _ => None,
        }));
    }
    for p in &others {
        let i = p.span.start.stream as usize;
        if aside[i] || matches!(mark(i), Some(Marker::Gen { .. })) {
            continue;
        }
        match p.cuts(concat, &res, lim) {
            Ok(cuts) if groups.contains(&cuts) => {}
            Ok(cuts) => groups.push(cuts),
            Err(_) => stayed += 1,
        }
    }
    let spliced = content::splice_each(concat, &res, lim, &groups);
    stayed += spliced.taken.iter().filter(|t| !**t).count();
    let cuts: Vec<SpanAcross> = (groups.iter().zip(&spliced.taken))
        .filter(|(_, taken)| **taken)
        .flat_map(|(cuts, _)| cuts.iter().copied())
        .collect();
    // The lists of the ranges kept that lose what an older sheaf kept in
    // them, written again without it. One that can't be keeps it, counted.
    let mut rewrites = Vec::new();
    let mut settings = 0;
    for span in older_ranges(filters, blocks.as_deref()) {
        if aside[span.start.stream as usize] || spliced.takes(concat, span) {
            continue;
        }
        match content::without_keys(concat, &res, lim, span, &LEGACY_KEYS) {
            Ok(rewrite) => rewrites.extend(rewrite),
            Err(_) => settings += 1,
        }
    }
    let spliced = spliced.streams(concat, &rewrites);
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
    let mut out_of: Vec<SpanAcross> = (0..ids.len()).filter(|&i| aside[i]).map(whole).collect();
    out_of.extend(&cuts);
    let kept = content::kept(concat, &res, lim, &out_of).ok()?;
    let scope = Scope { src, copy: 0 };
    let entry = |cx: &mut Cx, i: usize| cx.sym(&Object::Reference(ids[i]), &scope);
    let mut base = Vec::new();
    for i in (0..ids.len()).filter(|&i| !aside[i] && !reused(i)) {
        let copy = spliced.as_ref().and_then(|streams| match &streams[i] {
            Cow::Owned(bytes) => Some(bytes.clone()),
            Cow::Borrowed(_) => None,
        });
        base.push(match copy {
            Some(bytes) => filtered_copy(cx, src.id(), ids[i], bytes),
            None => entry(cx, i),
        });
    }
    let mut reserved = reserved(cx, src, index, page, &kept.dropped);
    let mut needs = ResourceNeeds::default();
    let contents = match on.is_empty() {
        true => base,
        false => {
            let (mut under, mut over) = (Vec::new(), Vec::new());
            for (g, kept) in on.iter().zip(&kept_block) {
                let block = match *kept {
                    Some(i) => entry(cx, i),
                    None => drawn(cx, g, geom, &mut reserved, &mut needs),
                };
                match g.z() {
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
            contents.extend(over);
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
    page_res(cx, src, index, page, &dropped, &added);
    Some(Stayed {
        ranges: stayed,
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

/// Generator `g` laid out on a page of `geom`, its names clear of
/// `reserved` and kept there, what it draws with added to `needs`: its
/// block, written once a file.
fn drawn(
    cx: &mut Cx,
    g: &Generator,
    geom: &Geom,
    reserved: &mut Reserved,
    needs: &mut ResourceNeeds,
) -> Object {
    let wm = g.spec.watermark().expect("a watermark");
    let art = art(cx, &wm.content);
    let drawn = generate::layout(g.id, wm, geom, art.as_ref(), reserved);
    reserved.take(&drawn.res);
    for need in &drawn.res.0 {
        if !(needs.0.iter()).any(|n| n.kind == need.kind && n.name == need.name) {
            needs.0.push(need.clone());
        }
    }
    let recipe = generate::recipe_digest(wm);
    block(cx, g.id.to_string(), &drawn, recipe)
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

/// `page`'s resources, page `index` of `src`'s, with the names `dropped`
/// taken out of their categories and those `needs` adds bound: a
/// dictionary of the page's own ([`ObjKey::PageRes`]), shared by the pages
/// whose base resources, dropped names and added names are the same.
/// Nothing when it's neither.
fn page_res(
    cx: &mut Cx,
    src: &Src,
    index: u32,
    page: &mut Dictionary,
    dropped: &NameSet,
    needs: &ResourceNeeds,
) {
    if dropped.is_empty() && needs.0.is_empty() {
        return;
    }
    let base_res = src.page(index).and_then(|f| f.resources.clone());
    let key = ObjKey::PageRes {
        base: resources_identity(src.id(), base_res.as_deref()),
        dropped: names_of(dropped),
        added: added(needs),
    };
    if !cx.page_res.contains_key(&key) {
        let mut resources = sink::resources(cx, page);
        for kind in ResKind::ALL {
            let mut names = dropped.of(kind).peekable();
            if names.peek().is_none() {
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
            resources.set(kind.key(), category);
        }
        sink::bind(cx, needs, &mut resources);
        cx.page_res.insert(key, resources);
    }
    page.set("Resources", cx.placeholder(key));
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

/// The balance of `entries`, page `index` of `src`'s content but what
/// [`wrapped`] took out: read again, as the page's blocks counted all of
/// it.
fn fresh_balance(
    cx: &Cx,
    src: &crate::store::Src,
    index: u32,
    entries: &[Object],
) -> Result<Balance, ContentError> {
    let ids: Vec<ObjectId> = (entries.iter())
        .filter_map(|e| match cx.syms.key(e.as_reference().ok()?)? {
            ObjKey::Base { obj, .. } => Some(obj),
            _ => None,
        })
        .collect();
    let lim = src.limits();
    let read = |fault| ContentError::Read { page: index, fault };
    let content = PageContent::read(src.pdf(), &ids, lim).map_err(read)?;
    let resources = src.page(index).and_then(|f| f.resources.clone());
    let res = ResCx::read(src.pdf(), resources.as_deref());
    crate::content::balance(content.concat(), &res, lim).map_err(|e| read(content.fault(e)))
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

/// A digest of the names `needs` adds to a page's resources: each one's
/// category, name and resource.
fn added(needs: &ResourceNeeds) -> u64 {
    let mut bytes = Vec::new();
    for need in &needs.0 {
        bytes.extend(need.kind.key());
        bytes.push(0);
        bytes.extend(&need.name);
        bytes.push(0);
        bytes.extend(need.res.id.0.to_be_bytes());
    }
    names_digest(&bytes)
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

/// `drawn`, generator `id`'s block (`id` as blocks name it), its recipe's
/// digest `recipe`, as a stream of its own, once a file: where it is.
fn block(cx: &mut Cx, id: String, drawn: &generate::Drawn, recipe: u128) -> Object {
    let marker = dictionary! {
        "K" => "Gen",
        "G" => Object::String(id.into_bytes(), StringFormat::Literal),
        "L" => digest_string(drawn.layout_digest),
        "R" => digest_string(recipe),
    };
    let stream = Object::Stream(Stream::new(
        dictionary! { "SheafBlock" => marker },
        drawn.ops.to_vec(),
    ));
    let key = ObjKey::Block(ResId::of(std::slice::from_ref(&stream)).0);
    let id = cx.push_once(key, |_| {
        let mut stream = stream;
        if let Object::Stream(s) = &mut stream {
            let _ = s.compress();
        }
        stream
    });
    Object::Reference(id)
}

/// A digest as a block writes it: a string of its 16 bytes, big-endian.
fn digest_string(digest: u128) -> Object {
    Object::String(digest.to_be_bytes().to_vec(), StringFormat::Hexadecimal)
}

/// One of the shared streams that isolate a page's content, once a file:
/// where it is.
fn isolation(cx: &mut Cx, shared: Shared) -> Object {
    let id = cx.push_once(ObjKey::Shared(shared), |_| {
        let (op, n) = match shared {
            Shared::QOpen(n) => (b"q\n", n),
            Shared::QClose(n) => (b"Q\n", n),
        };
        let marker = dictionary! { "SheafBlock" => dictionary! { "K" => "Isolate" } };
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
/// where it's written.
fn placed(cx: &mut Cx, at: &ObjRef) -> ResBody {
    let scope = Scope {
        src: &at.src,
        copy: 0,
    };
    let reference = cx.sym(&Object::Reference(at.id), &scope);
    ResBody::Written(reference.as_reference().expect("a reference stays one"))
}
