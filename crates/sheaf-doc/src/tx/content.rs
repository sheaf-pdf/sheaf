//! What sheaf draws on pages, lowered: recognized text over a page, in
//! place of what sheaf wrote in its file, and the watermark, the
//! background and the header and footer, each drawn by a generator (M10.5
//! §3.4–3.5).

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use sheaf_edit::{GridPos, HeaderFooterSpec, Z};

use super::{EditError, Lower, Notice, OthersFound};
use crate::content::{
    self, BlockAt, ContentError, Font, GenId, PagOrigin, PageContent, Pagination, PaginationKind,
    ResCx, SheafBlock, SpanAcross, Std14,
};
use crate::generate::{self, header};
use crate::model::{
    DocState, Filter, Frozen, GenSpec, Generator, Layer, MarkedSel, PageDef, PageId, Scope, Static,
    StreamSel, WatermarkContent, WatermarkGen,
};
use crate::prim::{Key, Val};
use crate::store::Src;
use crate::text_layer::TextLayer;

/// Page `page`'s recognized text: `layer`, in place of any it has, or
/// none when it has no words ([`put_ocr`]).
pub(crate) fn set_ocr(
    lw: &mut Lower,
    page: PageId,
    layer: &Arc<TextLayer>,
) -> Result<(), EditError> {
    put_ocr(lw, page, (!layer.is_empty()).then(|| layer.clone()))
}

/// Page `page`'s recognized text taken off, its stack's and its file's
/// ([`put_ocr`]).
pub(crate) fn clear_ocr(lw: &mut Lower, page: PageId) -> Result<(), EditError> {
    put_ocr(lw, page, None)
}

/// Page `page`'s recognized text: `text` in place of what it has, or none.
/// A layer it replaces on the page's stack, saved or not, keeps its
/// block's id, and a new one draws one; what its file holds is filtered
/// out ([`saved_ocr`]). Nothing is put when that leaves the page as it is.
fn put_ocr(lw: &mut Lower, page: PageId, text: Option<Arc<TextLayer>>) -> Result<(), EditError> {
    lw.read(Key::Page(page));
    let def = lw.rec.def(page).expect("resolved").clone();
    let filters = saved_ocr(lw, page, &def)?;
    let kept = def.content.ocr_layer().map(|l| l.id);
    let layer = text.map(|text| Layer {
        id: kept.unwrap_or_else(|| lw.block_id()),
        body: Static::OcrText(text),
    });
    let mut content = def.content.with_ocr(layer);
    if !filters.is_empty() {
        content = content.with_filters(filters);
    }
    if content == def.content {
        return Ok(());
    }
    let def = PageDef::new(def.base.clone(), def.copy, content);
    lw.rec.put(Key::Page(page), Some(Val::Page(Arc::new(def))));
    Ok(())
}

/// The filters that take the recognized text sheaf wrote in the file of
/// page `page`, shown as `def`, out of it (§3.4): its OCR blocks, each a
/// stream of its own, and the layer an older sheaf wrote, with its `q`
/// ([`StreamSel::SheafOcr`]); and the blocks another tool merged into a
/// stream ([`MarkedSel::SheafOcr`]), once a splice is shown to take them
/// out as a save cuts them. None when it holds none. Read from the page's
/// blocks, a fact of its source read once; a page whose content can't be
/// read, or whose merged blocks a splice refuses, is
/// [`EditError::Content`].
fn saved_ocr(lw: &Lower, page: PageId, def: &PageDef) -> Result<Vec<Filter>, EditError> {
    let (src, index) = (&def.base.src, def.base.index);
    let blocks = src.blocks(index).map_err(|e| lw.content(e, page))?;
    let ocr = (blocks.sheaf.iter()).filter(|b| matches!(b.block, SheafBlock::Ocr(_)));
    let streams = ocr.clone().any(|b| matches!(b.at, BlockAt::Stream(_)));
    let merged: Vec<SpanAcross> = ocr
        .filter_map(|b| match b.at {
            BlockAt::Marked(span) => Some(span),
            BlockAt::Stream(_) => None,
        })
        .collect();
    let mut filters = Vec::new();
    if streams || blocks.legacy_ocr.is_some() {
        filters.push(Filter::DropStreams(StreamSel::SheafOcr));
    }
    if !merged.is_empty() {
        cut_together(src, index, &merged).map_err(|e| lw.content(e, page))?;
        filters.push(Filter::DropMarked(MarkedSel::SheafOcr));
    }
    Ok(filters)
}

/// Nothing when a save can cut `spans`, ranges of page `index` of `src`,
/// out of its content as [`content::splice_each`] cuts them, each a range
/// of its own: all of them only when a splice takes them together. Else
/// why not.
fn cut_together(src: &Src, index: u32, spans: &[SpanAcross]) -> Result<(), ContentError> {
    let (Some(facts), lim) = (src.page(index), src.limits()) else {
        return Ok(());
    };
    let read = |fault| ContentError::Read { page: index, fault };
    let content = PageContent::read(src.pdf(), &facts.contents, lim).map_err(read)?;
    let res = ResCx::read(src.pdf(), facts.resources.as_deref());
    match content::splice(content.concat(), &res, lim, spans) {
        Ok(_) => Ok(()),
        Err(error) => Err(ContentError::Unsafe { page: index, error }),
    }
}

/// The watermark (`z` over) or background (under) drawing `content` at
/// `position`, on the pages of `scope` (every page, those added later too,
/// when None), in place of the one of that `z` there is, whose generator it
/// keeps. It replaces the artifacts of its kind others drew on its pages
/// as a save writes them (§6.5). Reads nothing but the generators: what it
/// draws on each page is laid out as a save writes it.
pub(crate) fn set_watermark(
    lw: &mut Lower,
    content: &WatermarkContent,
    position: GridPos,
    z: Z,
    scope: Option<&im::OrdSet<PageId>>,
) -> Result<(), EditError> {
    drawable(lw, content)?;
    let scope = match scope {
        None => Scope::All,
        Some(pages) if pages.is_empty() => {
            return Err(lw.invalid("the watermark is on no page"));
        }
        Some(pages) => Scope::Pages(pages.clone()),
    };
    if let WatermarkContent::Text { text, font, .. } = content
        && Std14::from(*font).encode(text).1.0 > 0
    {
        lw.notices.push(Notice::Unencodable);
    }
    let kept = lw.rec.watermark(z).map(|g| g.id);
    let id = kept.unwrap_or_else(|| lw.gen_id());
    let wm = WatermarkGen {
        content: content.clone(),
        position,
    };
    let g = Arc::new(Generator {
        id,
        spec: GenSpec::Watermark(Arc::new(wm)),
        z,
        scope,
        frozen: Frozen::default(),
        replaces: BTreeSet::from([WatermarkGen::kind_at(z)]),
    });
    lw.rec.put(Key::Generator(id), Some(Val::Generator(g)));
    Ok(())
}

/// Whether a watermark can draw `content` ([`generate::unfit`]).
fn drawable(lw: &Lower, content: &WatermarkContent) -> Result<(), EditError> {
    match generate::unfit(content) {
        Some(why) => Err(lw.invalid(why)),
        None => Ok(()),
    }
}

/// The watermark (`z` over), the background (under) or both (`z` None)
/// taken off `pages`, with those of their kinds others drew there
/// ([`remove_generated`]).
pub(crate) fn remove_watermark(
    lw: &mut Lower,
    z: Option<Z>,
    pages: &[PageId],
    include_authored: bool,
) -> Result<OthersFound, EditError> {
    let kinds = match z {
        Some(z) => BTreeSet::from([WatermarkGen::kind_at(z)]),
        None => BTreeSet::from([PaginationKind::Watermark, PaginationKind::Background]),
    };
    remove_generated(lw, &kinds, pages, include_authored)
}

/// The header and footer taken off `pages`, with the headers and footers
/// others drew there ([`remove_generated`]).
pub(crate) fn remove_header_footer(
    lw: &mut Lower,
    pages: &[PageId],
    include_authored: bool,
) -> Result<OthersFound, EditError> {
    remove_generated(lw, &header::kinds(), pages, include_authored)
}

/// The generators drawing pagination artifacts of `kinds` taken off
/// `pages` ([`narrow`]), and the artifacts of those kinds others drew there
/// (§6.5), each page's read as [`others_on`] reads it, one rule: their
/// blocks the pages' files hold go with them ([`DocState::backs`]), and
/// others' are filtered out where a splice takes them out. What it found of
/// others', counted. Each page's blocks are read, a page at a time, with
/// progress and a way to cancel (§6.3).
///
/// [`DocState::backs`]: crate::model::DocState::backs
fn remove_generated(
    lw: &mut Lower,
    kinds: &BTreeSet<PaginationKind>,
    pages: &[PageId],
    include_authored: bool,
) -> Result<OthersFound, EditError> {
    narrow(lw, kinds, pages);
    let mut found = OthersFound::default();
    for (i, &page) in pages.iter().enumerate() {
        lw.pages_done(i, pages.len())?;
        lw.read(Key::Page(page));
        let (filters, on) = found_on(&lw.rec, page, kinds, include_authored);
        found = found.plus(on);
        let def = lw.rec.def(page).expect("resolved");
        let content = def.content.with_filters(filters);
        if content != def.content {
            let def = PageDef::new(def.base.clone(), def.copy, content);
            lw.rec.put(Key::Page(page), Some(Val::Page(Arc::new(def))));
        }
    }
    Ok(found)
}

/// What taking the artifacts of `kinds` off `pages` of `st` would find of
/// others' there (§6.5, D61): what [`remove_generated`] counts, read as it
/// reads it, with the generators drawing them taken off those pages first,
/// and nothing changed. What a dialog says Remove would take and Set would
/// replace (proposal 31). It reads each page's blocks, so it's for off the
/// UI thread. A page `st` hasn't got isn't counted.
pub fn others_on(
    st: &DocState,
    kinds: &BTreeSet<PaginationKind>,
    pages: &[PageId],
    include_authored: bool,
) -> OthersFound {
    let mut st = st.clone();
    for (id, g) in narrowed(&st, kinds, pages).left {
        (st.set(&Key::Generator(id), g.map(Val::Generator)))
            .expect("a generator's slot holds a generator");
    }
    (pages.iter())
        .filter(|&&p| st.def(p).is_some())
        .map(|&p| found_on(&st, p, kinds, include_authored).1)
        .fold(OthersFound::default(), OthersFound::plus)
}

/// What taking the artifacts of `kinds` off page `page` of `st`, whose
/// generators drawing them are off it now, finds (§6.5): the filters that
/// take out those others drew, and what it found of others', counted
/// ([`OthersFound`]). Taken: another file's blocks, those another tool
/// merged into a stream of its own, and the artifacts Acrobat and sheaf
/// before M10.5 drew, and with `include_authored` the author's own, each
/// where a splice takes it out. One a splice refuses, an author's it isn't
/// asked to take, one inside a form and a page that can't be read are left
/// as they are; one the page's filters take out already isn't counted.
fn found_on(
    st: &DocState,
    page: PageId,
    kinds: &BTreeSet<PaginationKind>,
    include_authored: bool,
) -> (Vec<Filter>, OthersFound) {
    let mut found = OthersFound::default();
    let mut filters = Vec::new();
    let def = st.def(page).expect("a page of the state's");
    let (src, index) = (&def.base.src, def.base.index);
    let Ok(blocks) = src.blocks(index) else {
        found.unreadable += 1;
        return (filters, found);
    };
    found.in_forms += usize::from(blocks.nested_in_forms);
    let had = |f: &Filter| def.content.filters.contains(f);
    // Each range to cut, with who drew it, None for a block of the
    // document's own generators, which isn't counted; and whether the
    // page's filters take it out already.
    let mut ranges: Vec<(Option<PagOrigin>, &Pagination, bool)> = Vec::new();
    for b in blocks.sheaf.iter() {
        let SheafBlock::Gen(g) = b.block else {
            continue;
        };
        if !blocks.kind_of(g).is_some_and(|k| kinds.contains(&k)) {
            continue;
        }
        let backed = st.backs(g, page);
        match b.at {
            BlockAt::Stream(_) if backed => {}
            BlockAt::Stream(_) => {
                let drop = Filter::DropStreams(StreamSel::SheafGen(g));
                found.other_files += usize::from(!had(&drop));
                filters.push(drop);
            }
            BlockAt::Marked(span) => {
                let drop = Filter::DropMarked(MarkedSel::SheafGen(g));
                let merged = blocks.pagination.iter().find(|p| p.span == span);
                let origin = (!backed).then_some(PagOrigin::Sheaf);
                ranges.extend(merged.map(|p| (origin, p, had(&drop))));
                filters.push(drop);
            }
        }
    }
    let filtered = |p: &Pagination| {
        (def.content.filters.iter()).any(|f| match f {
            Filter::DropMarked(MarkedSel::Pagination { kinds, origins }) => {
                p.selected_by(kinds, origins)
            }
            _ => false,
        })
    };
    // An author's it isn't asked to take is cut with the rest only when
    // the page's filters take it already, to tell whether it's out.
    for p in blocks.pagination.iter().filter(|p| kinds.contains(&p.kind)) {
        let asked = include_authored || p.origin != PagOrigin::Authored;
        match p.origin {
            PagOrigin::Sheaf => {}
            origin if asked || filtered(p) => ranges.push((Some(origin), p, filtered(p))),
            _ => found.authored_left += 1,
        }
    }
    let taken = cut_alone(src, index, &ranges);
    let mut origins = BTreeSet::new();
    for (&(origin, _, gone), taken) in ranges.iter().zip(taken) {
        let Some(origin) = origin else {
            continue;
        };
        // Out already.
        if gone && taken {
            continue;
        }
        let count = match (origin, taken) {
            (PagOrigin::Acrobat, true) => &mut found.acrobat,
            (PagOrigin::Sheaf, true) => &mut found.other_files,
            (PagOrigin::SheafLegacy, true) => &mut found.older_sheaf,
            (PagOrigin::Authored, true) => &mut found.authored,
            (PagOrigin::Authored, false) => &mut found.authored_left,
            (_, false) => &mut found.refused,
        };
        *count += 1;
        if taken && origin != PagOrigin::Sheaf {
            origins.insert(origin);
        }
    }
    if !origins.is_empty() {
        let kinds = kinds.clone();
        filters.push(Filter::DropMarked(MarkedSel::Pagination { kinds, origins }));
    }
    (filters, found)
}

/// Which of `ranges`, pagination artifacts of page `index` of `src`, a
/// save can take out, as it cuts them ([`content::splice_each`]): none
/// when the page's content can't be read.
fn cut_alone<T, U>(src: &Src, index: u32, ranges: &[(T, &Pagination, U)]) -> Vec<bool> {
    let none = || vec![false; ranges.len()];
    if ranges.is_empty() {
        return Vec::new();
    }
    let (Some(facts), lim) = (src.page(index), src.limits()) else {
        return none();
    };
    let Ok(content) = PageContent::read(src.pdf(), &facts.contents, lim) else {
        return none();
    };
    let res = ResCx::read(src.pdf(), facts.resources.as_deref());
    let concat = content.concat();
    let cuts: Vec<Option<Vec<SpanAcross>>> = (ranges.iter())
        .map(|(_, p, _)| p.cuts(concat, &res, lim).ok())
        .collect();
    let groups: Vec<Vec<SpanAcross>> = cuts.iter().flatten().cloned().collect();
    let mut taken = (content::splice_each(concat, &res, lim, &groups).taken).into_iter();
    cuts.iter()
        .map(|c| c.is_some() && taken.next().unwrap_or(false))
        .collect()
}

/// The generators drawing pagination artifacts of `kinds` taken off
/// `pages` ([`narrowed`]).
fn narrow(lw: &mut Lower, kinds: &BTreeSet<PaginationKind>, pages: &[PageId]) {
    let narrowed = narrowed(&lw.rec, kinds, pages);
    if narrowed.ordered {
        lw.read(Key::Order);
    }
    for (id, g) in narrowed.left {
        lw.rec.put(Key::Generator(id), g.map(Val::Generator));
    }
}

/// Generators taken off pages ([`narrowed`]).
struct Narrowed {
    /// Each one it changed, as it's left: None when it's gone.
    left: Vec<(GenId, Option<Arc<Generator>>)>,
    /// Whether one was on every page, which reads the order.
    ordered: bool,
}

/// The generators of `st` drawing pagination artifacts of `kinds`, each as
/// it's left once taken off `pages`: narrowed to the pages it's left on, or
/// gone when that's none; one on none of them isn't changed. One on every
/// page is left on the others there are now. The Bates numbers of the
/// pages it's taken off go with them, never to be given again.
fn narrowed(st: &DocState, kinds: &BTreeSet<PaginationKind>, pages: &[PageId]) -> Narrowed {
    let picked = (st.generators().values()).filter(|g| !g.kinds().is_disjoint(kinds));
    let gone: HashSet<PageId> = pages.iter().copied().collect();
    let (mut left, mut ordered) = (Vec::new(), false);
    for g in picked {
        let g = match &g.scope {
            Scope::All if pages.is_empty() => continue,
            Scope::All => {
                ordered = true;
                let every = Scope::Pages(st.order().iter().copied().collect());
                Arc::new(Generator {
                    scope: every,
                    ..Generator::clone(g)
                })
            }
            Scope::Pages(_) => g.clone(),
        };
        let (on, went) = g.on_pages_left(|p| !gone.contains(&p));
        if went > 0 {
            left.push((g.id, on));
        }
    }
    Narrowed { left, ordered }
}

/// The header and footer `spec` says, on the pages of `scope` (every page,
/// those added later too, when None), in place of the one there is, whose
/// generator, date and file it keeps; a new one's `<<date>>` and `<<file>>`
/// say now and the document's name (§3.5). Its Bates numbers count the
/// pages it covers in order from the spec's start, which reads the order,
/// fixed now: setting it again is how to renumber. Numbers that would run
/// past `u64::MAX` are refused. It replaces the headers
/// and footers others drew on its pages as a save writes them (§6.5).
/// With no text, there's none. Text its face can't draw, a page label
/// included, is said ([`Notice::Unencodable`]).
pub(crate) fn set_header_footer(
    lw: &mut Lower,
    spec: &HeaderFooterSpec,
    scope: Option<&im::OrdSet<PageId>>,
) -> Result<(), EditError> {
    let was = lw.rec.header_footer().cloned();
    if header::is_empty(spec) {
        if let Some(was) = was {
            lw.rec.put(Key::Generator(was.id), None);
        }
        return Ok(());
    }
    if let Some(why) = header::unfit(spec) {
        return Err(lw.invalid(why));
    }
    let scope = match scope {
        None => Scope::All,
        Some(pages) if pages.is_empty() => {
            return Err(lw.invalid("the header and footer is on no page"));
        }
        Some(pages) => Scope::Pages(pages.clone()),
    };
    let host = &lw.cx.host;
    let (date, file) = match &was {
        Some(was) if !was.frozen.date.is_empty() => {
            (was.frozen.date.clone(), was.frozen.file.clone())
        }
        _ => (host.date.clone(), host.file.clone()),
    };
    let bates = match &spec.bates {
        Some(b) => {
            lw.read(Key::Order);
            let on = lw.rec.order().iter().copied().filter(|&p| scope.covers(p));
            match header::numbered(on, b.start) {
                Some(numbers) => Some(numbers),
                None => {
                    return Err(lw.invalid(format!(
                        "Bates numbers from {} run past the largest there is",
                        b.start
                    )));
                }
            }
        }
        None => None,
    };
    let id = was.map_or_else(|| lw.gen_id(), |was| was.id);
    let g = Generator {
        id,
        spec: GenSpec::HeaderFooter(Arc::new(spec.clone())),
        z: Z::Over,
        scope,
        frozen: Frozen { date, file, bates },
        replaces: header::kinds(),
    };
    let face = Std14::from(spec.font);
    if header::unencodable(spec, &g.frozen, &face) || label_unencodable(&lw.rec, &g) {
        lw.notices.push(Notice::Unencodable);
    }
    lw.rec
        .put(Key::Generator(id), Some(Val::Generator(Arc::new(g))));
    Ok(())
}

/// Whether header and footer `g` draws, on a page of `st` it covers, a
/// label its face can't draw all of (`<<label>>`). A label's number is
/// digits or letters, which every face has, so only a range's prefix can
/// make one: the pages are read only when one does.
pub(crate) fn label_unencodable(st: &DocState, g: &Generator) -> bool {
    let Some(spec) = g.spec.header_footer() else {
        return false;
    };
    let face = Std14::from(spec.font);
    let mut prefixes = st.labels().into_iter().flat_map(|l| l.iter());
    if !prefixes.any(|l| header::label_unencodable(spec, &l.prefix, &face)) {
        return false;
    }
    (st.order().iter().filter(|&&p| g.covers(p))).any(|&p| {
        st.label_of(p)
            .is_some_and(|l| header::label_unencodable(spec, &l, &face))
    })
}
