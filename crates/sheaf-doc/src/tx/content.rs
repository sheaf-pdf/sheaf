//! What sheaf draws on pages, lowered: recognized text over a page, and
//! the watermark, the background and the header and footer, each drawn by
//! a generator (M10.5 §3.4–3.5).

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use sheaf_edit::{GridPos, HeaderFooterSpec, Z};

use super::{EditError, Lower, Notice, OthersFound};
use crate::content::{
    self, BlockAt, Font, PagOrigin, PageContent, Pagination, PaginationKind, ResCx, SheafBlock,
    SpanAcross, Std14,
};
use crate::generate;
use crate::model::{
    Filter, GenId, GenSpec, Generator, MarkedSel, PageDef, PageId, Scope, StreamSel,
    WatermarkContent, WatermarkGen,
};
use crate::prim::{Key, Val};
use crate::stamps::HeaderFooter;
use crate::store::Src;
use crate::text_layer::TextLayer;

/// Page `page`'s recognized text: `layer`, in place of any it has, or
/// none.
pub(crate) fn set_ocr(lw: &mut Lower, page: PageId, layer: Option<Arc<TextLayer>>) {
    let def = lw.rec.def(page).expect("resolved");
    if layer.is_none() && def.content.ocr_text().is_none() {
        return;
    }
    let content = def.content.with_ocr_text(layer);
    let def = PageDef::new(def.base.clone(), def.copy, content);
    lw.rec.put(Key::Page(page), Some(Val::Page(Arc::new(def))));
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
        z,
        scope,
        replaces: BTreeSet::from([WatermarkGen::kind_at(z)]),
    };
    let spec = GenSpec::Watermark(Arc::new(wm));
    let g = Arc::new(Generator { id, spec });
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
/// taken off `pages`: each narrowed to the pages it's left on, or gone when
/// that's none; as it is when it's on none of them. One on every page is
/// left on the others there are now, which reads the order. Its blocks
/// the pages' files hold go with it ([`DocState::backs`]); others of its
/// kind there are filtered out (§6.5), which reads each page's blocks:
/// another file's blocks, those another tool merged into a stream of its
/// own, and the artifacts Acrobat and sheaf before M10.5 drew, and with
/// `include_authored` the author's own, each where a splice takes it out.
/// What it found of others', counted ([`OthersFound`]): one a splice
/// refuses, an author's it isn't asked to take, one inside a form and a
/// page that can't be read are left as they are, never failing it.
///
/// [`DocState::backs`]: crate::model::DocState::backs
pub(crate) fn remove_watermark(
    lw: &mut Lower,
    z: Option<Z>,
    pages: &[PageId],
    include_authored: bool,
) -> OthersFound {
    narrow_watermarks(lw, z, pages);
    let kinds: BTreeSet<PaginationKind> = match z {
        Some(z) => BTreeSet::from([WatermarkGen::kind_at(z)]),
        None => BTreeSet::from([PaginationKind::Watermark, PaginationKind::Background]),
    };
    let mut found = OthersFound::default();
    for &page in pages {
        lw.read(Key::Page(page));
        let def = lw.rec.def(page).expect("resolved").clone();
        let (src, index) = (&def.base.src, def.base.index);
        let Ok(blocks) = src.blocks(index) else {
            found.unreadable += 1;
            continue;
        };
        found.in_forms += usize::from(blocks.nested_in_forms);
        let mut content = def.content.clone();
        // Each range to cut, with who drew it: None for a block of the
        // document's own generators, which isn't counted.
        let mut ranges: Vec<(Option<PagOrigin>, &Pagination)> = Vec::new();
        for b in blocks.sheaf.iter() {
            let SheafBlock::Gen(g) = b.block else {
                continue;
            };
            if !blocks.kind_of(g).is_some_and(|k| kinds.contains(&k)) {
                continue;
            }
            let backed = lw.rec.backs(g, page);
            match b.at {
                BlockAt::Stream(_) if backed => {}
                BlockAt::Stream(_) => {
                    content = content.with_filter(Filter::DropStreams(StreamSel::SheafGen(g)));
                    found.other_files += 1;
                }
                BlockAt::Marked(span) => {
                    content = content.with_filter(Filter::DropMarked(MarkedSel::SheafGen(g)));
                    let merged = blocks.pagination.iter().find(|p| p.span == span);
                    let origin = (!backed).then_some(PagOrigin::Sheaf);
                    ranges.extend(merged.map(|p| (origin, p)));
                }
            }
        }
        for p in blocks.pagination.iter().filter(|p| kinds.contains(&p.kind)) {
            match p.origin {
                PagOrigin::Sheaf => {}
                PagOrigin::Authored if !include_authored => found.authored_left += 1,
                origin => ranges.push((Some(origin), p)),
            }
        }
        let taken = cut_alone(src, index, &ranges);
        let mut origins = BTreeSet::new();
        for (&(origin, _), taken) in ranges.iter().zip(taken) {
            let Some(origin) = origin else {
                continue;
            };
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
            content =
                content.with_filter(Filter::DropMarked(MarkedSel::Pagination { kinds, origins }));
        }
        if content != def.content {
            let def = PageDef::new(def.base.clone(), def.copy, content);
            lw.rec.put(Key::Page(page), Some(Val::Page(Arc::new(def))));
        }
    }
    found
}

/// Which of `ranges`, pagination artifacts of page `index` of `src`, a
/// save can take out, as it cuts them ([`content::splice_each`]): none
/// when the page's content can't be read.
fn cut_alone<T>(src: &Src, index: u32, ranges: &[(T, &Pagination)]) -> Vec<bool> {
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
        .map(|(_, p)| p.cuts(concat, &res, lim).ok())
        .collect();
    let groups: Vec<Vec<SpanAcross>> = cuts.iter().flatten().cloned().collect();
    let mut taken = (content::splice_each(concat, &res, lim, &groups).taken).into_iter();
    cuts.iter()
        .map(|c| c.is_some() && taken.next().unwrap_or(false))
        .collect()
}

/// [`remove_watermark`]'s generators, narrowed or gone.
fn narrow_watermarks(lw: &mut Lower, z: Option<Z>, pages: &[PageId]) {
    let drawn: Vec<(GenId, Arc<WatermarkGen>)> = (lw.rec.generators().values())
        .filter_map(|g| Some((g.id, g.spec.watermark()?.clone())))
        .filter(|(_, wm)| z.is_none_or(|z| wm.z == z))
        .collect();
    for (id, wm) in drawn {
        let left = match &wm.scope {
            Scope::Pages(on) => {
                let mut left = on.clone();
                for p in pages {
                    left.remove(p);
                }
                if left.len() == on.len() {
                    continue;
                }
                left
            }
            Scope::All if pages.is_empty() => continue,
            Scope::All => {
                lw.read(Key::Order);
                let gone: HashSet<PageId> = pages.iter().copied().collect();
                (lw.rec.order().iter().copied())
                    .filter(|p| !gone.contains(p))
                    .collect()
            }
        };
        if left.is_empty() {
            lw.rec.put(Key::Generator(id), None);
            continue;
        }
        let wm = WatermarkGen {
            scope: Scope::Pages(left),
            ..WatermarkGen::clone(&wm)
        };
        let spec = GenSpec::Watermark(Arc::new(wm));
        let g = Arc::new(Generator { id, spec });
        lw.rec.put(Key::Generator(id), Some(Val::Generator(g)));
    }
}

/// The header and footer `spec` says, on the pages of `scope` (every page,
/// those added later too, when None), in place of the one there is, whose
/// generator it keeps. Its `<<date>>` and `<<file>>` are fixed when it's
/// added: now and the document's name, or the one's it replaces. With no
/// text, there's none.
pub(crate) fn set_header_footer(
    lw: &mut Lower,
    spec: &HeaderFooterSpec,
    scope: Option<Vec<PageId>>,
) {
    let drawn =
        (lw.rec.generators().values()).find_map(|g| Some((g.id, g.spec.header_footer()?.clone())));
    let host = &lw.cx.host;
    let (date, file) = match &drawn {
        Some((_, was)) if !was.date.is_empty() => (was.date.clone(), was.file.clone()),
        _ => (host.date.to_string(), host.file.to_string()),
    };
    let hf = HeaderFooter {
        texts: spec.texts.clone(),
        font_size: spec.size,
        color: spec.color,
        top: spec.margins.top,
        bottom: spec.margins.bottom,
        left: spec.margins.left,
        right: spec.margins.right,
        first_page_number: spec.first_number,
        pages: scope,
        date,
        file,
        bates: spec.bates.clone(),
    };
    if hf.is_empty() {
        if let Some((id, _)) = drawn {
            lw.rec.put(Key::Generator(id), None);
        }
        return;
    }
    if hf.unencodable() {
        lw.notices.push(Notice::Unencodable);
    }
    let id = drawn.map_or_else(|| lw.gen_id(), |(id, _)| id);
    let spec = GenSpec::HeaderFooter(Arc::new(hf));
    let g = Arc::new(Generator { id, spec });
    lw.rec.put(Key::Generator(id), Some(Val::Generator(g)));
}

/// The header and footer taken off `pages`: the generator narrowed to the
/// pages it's left on, or gone when that's none; as it is when it's on
/// none of them.
pub(crate) fn remove_header_footer(lw: &mut Lower, pages: &[PageId]) {
    let drawn =
        (lw.rec.generators().values()).find_map(|g| Some((g.id, g.spec.header_footer()?.clone())));
    let Some((id, hf)) = drawn else {
        return;
    };
    let on: Vec<PageId> = match &hf.pages {
        Some(scope) => scope.clone(),
        None => lw.rec.order().iter().copied().collect(),
    };
    let gone: HashSet<PageId> = pages.iter().copied().collect();
    let left: Vec<PageId> = on.iter().copied().filter(|p| !gone.contains(p)).collect();
    if left.len() == on.len() {
        return;
    }
    if left.is_empty() {
        lw.rec.put(Key::Generator(id), None);
        return;
    }
    let hf = HeaderFooter {
        pages: Some(left),
        ..HeaderFooter::clone(&hf)
    };
    let g = Arc::new(Generator {
        id,
        spec: GenSpec::HeaderFooter(Arc::new(hf)),
    });
    lw.rec.put(Key::Generator(id), Some(Val::Generator(g)));
}
