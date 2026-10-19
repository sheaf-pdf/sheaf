//! What sheaf draws on pages, lowered: recognized text over a page, and
//! the header and footer, drawn by a generator (M10.5 §3.4–3.5).

use std::collections::HashSet;
use std::sync::Arc;

use sheaf_edit::HeaderFooterSpec;

use super::{Lower, Notice};
use crate::model::{GenId, GenSpec, Generator, PageDef, PageId};
use crate::prim::{Key, Val};
use crate::stamps::HeaderFooter;
use crate::text_layer::TextLayer;

/// Page `page`'s recognized text: `layer`, in place of any it has, or
/// none.
pub(crate) fn set_ocr(lw: &mut Lower, page: PageId, layer: Option<Arc<TextLayer>>) {
    let def = lw.rec.def(page).expect("resolved");
    if layer.is_none() && def.content.ocr_text().is_none() {
        return;
    }
    let def = PageDef {
        content: def.content.with_ocr_text(layer),
        ..PageDef::clone(def)
    };
    lw.rec.put(Key::Page(page), Some(Val::Page(Arc::new(def))));
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
    let id = drawn.map(|(id, _)| id).unwrap_or_else(|| {
        let generators = lw.rec.generators();
        GenId::draw(&mut fastrand::Rng::new(), |id| generators.contains_key(&id))
    });
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
