//! The generators a file says sheaf drew on it (M10.5 §3.5), as an open
//! finds them ([`read`]): a fact of its source
//! ([`Source::generators`]), read from the catalog's `/SheafGenerators`
//! and the pages' blocks. It names what it read as the file numbers it:
//! the stream each block is in, and the XObject an image's or a page's
//! block draws, which a generator made of it binds to its source.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use lopdf::{LazyPdf, Object, ObjectId};
use serde::Deserialize;
use sheaf_edit::Z;

use super::{
    STORED_VERSION, StoredArt, StoredContent, StoredGen, StoredKind, StoredScope, StoredSpec,
    StoredWatermark, header, unfit_values,
};
use crate::content::{self, BlockAt, Marker, ResCx, SheafBlock, Tok};
use crate::materialize::emit::catalog::GENERATORS;
use crate::model::{
    Frozen, GenId, GenSpec, Generator, ImageRef, PageForm, PageId, Placed, Scope, WatermarkContent,
    WatermarkGen,
};
use crate::source::{ObjRef, Source};
use crate::store::Src;

/// A generator a file's `/SheafGenerators` and its pages' blocks say sheaf
/// drew on it (§3.5), as [`read`] finds it: what an open makes a generator
/// of ([`Saved::generator`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub id: GenId,
    /// The pages holding a block of it, by index, in order: its scope,
    /// with those it draws nothing on. None when it's on every page, those
    /// added later too.
    pub pages: Option<Vec<u32>>,
    stored: StoredGen,
    /// The XObject its image or page is: the one its first block that
    /// draws one draws.
    placed: Option<ObjectId>,
    /// A header's Bates numbers, by page index: its blocks' (`/N`).
    numbers: BTreeMap<u32, u64>,
    /// Its blocks in streams of their own that say both digests, in order:
    /// each one's page, by index, its stream, and its layout's and recipe's
    /// digests (`/L`, `/R`).
    blocks: Vec<(u32, ObjectId, u128, u128)>,
    /// The pages a header with Bates numbers is on whose blocks another
    /// tool merged into a stream of the page's, which keeps no number.
    unnumbered: Vec<u32>,
}

impl Saved {
    /// It as a generator of a document whose pages of `src`, its file, are
    /// `page` by index: None when none of the pages it's on is one. A
    /// header's Bates numbers are on the pages its blocks number.
    pub fn generator(&self, src: &Src, page: impl Fn(u32) -> Option<PageId>) -> Option<Generator> {
        let scope = match &self.pages {
            None => Scope::All,
            Some(pages) => {
                let on: im::OrdSet<PageId> = pages.iter().filter_map(|&i| page(i)).collect();
                if on.is_empty() {
                    return None;
                }
                Scope::Pages(on)
            }
        };
        let stored = &self.stored;
        let (spec, frozen) = match &stored.spec {
            StoredSpec::Watermark(wm) => {
                let spec = WatermarkGen {
                    content: self.content(src, &wm.content)?,
                    position: wm.position,
                };
                (GenSpec::Watermark(Arc::new(spec)), Frozen::default())
            }
            StoredSpec::HeaderFooter(hf) => {
                let bates = hf.bates.as_ref().map(|_| {
                    let numbers = self.numbers.iter();
                    Arc::new(numbers.filter_map(|(&i, &n)| Some((page(i)?, n))).collect())
                });
                let frozen = stored.frozen.as_ref();
                let frozen = Frozen {
                    date: frozen.map_or("", |f| &f.date).into(),
                    file: frozen.map_or("", |f| &f.file).into(),
                    bates,
                };
                (GenSpec::HeaderFooter(Arc::new(hf.clone())), frozen)
            }
        };
        // What [`read`] checks it says.
        let replaces = match &spec {
            GenSpec::Watermark(_) => BTreeSet::from([WatermarkGen::kind_at(stored.z)]),
            GenSpec::HeaderFooter(_) => header::kinds(),
        };
        Some(Generator {
            id: self.id,
            spec,
            z: stored.z,
            scope,
            frozen,
            replaces,
        })
    }

    /// The pages of its file, by index and in order, that a header with
    /// Bates numbers is on but whose numbers can't be read: another tool
    /// merged their blocks into a stream of the page's (§3.5).
    pub fn unnumbered(&self) -> &[u32] {
        &self.unnumbered
    }

    /// Its blocks in streams of their own that say both their digests: each
    /// one's page, by index, its stream, and its layout's and recipe's
    /// digests. What the opened file's record says it drew
    /// ([`drawn_of`](crate::materialize::record::drawn_of)).
    pub fn blocks(&self) -> &[(u32, ObjectId, u128, u128)] {
        &self.blocks
    }

    /// A watermark's content as `stored` says it, its image or page the
    /// XObject a block of it draws in `src`.
    fn content(&self, src: &Src, stored: &StoredContent) -> Option<WatermarkContent> {
        let placed = |art: &StoredArt| {
            Some(Placed {
                at: ObjRef {
                    src: src.clone(),
                    id: self.placed?,
                },
                res: art.res()?,
                turns: art.turns % 4,
            })
        };
        Some(match stored {
            StoredContent::Text {
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
            StoredContent::Image {
                image,
                scale,
                opacity,
            } => WatermarkContent::Image {
                image: ImageRef::Placed(placed(image)?),
                scale: *scale,
                opacity: *opacity,
            },
            StoredContent::Page {
                pdf,
                scale,
                opacity,
            } => WatermarkContent::Page {
                page: PageForm::Placed(placed(pdf)?),
                scale: *scale,
                opacity: *opacity,
            },
        })
    }
}

/// The generators sheaf drew on `src` (§3.5): what its catalog's
/// `/SheafGenerators` says each is, on the pages whose blocks name it: by
/// their streams' markers ([`Source::markers`]), and on a page holding no
/// block in a stream of its own whose resources name a list of sheaf's,
/// by the ranges its content marks, where another tool merged a block into
/// a stream of its own. One said to be on every page that a page whose
/// content can be read holds no block of, as when another tool added the
/// page, is on the pages that hold one. An image or a page is the XObject
/// a block of it draws in a stream of its own, named in its page's
/// resources. A header's Bates numbers are its blocks' (`/N`); a page whose
/// block another tool merged has none, which is logged and which a save
/// notes. Passed over, each with a log line: JSON that isn't the version
/// sheaf writes; a generator that can't be read, whose id isn't one, that
/// says what it draws, its z or what it replaces in a way sheaf wouldn't,
/// is on some pages but none holds a block of it, or whose image or page
/// no block draws; a second of an id, or of its kind at its z (a second
/// header). Never at open: a file's whole page table is read here, in
/// [`crate::open::finish`].
pub fn read(src: &Source) -> Vec<Saved> {
    let pdf = src.pdf();
    let Some(json) = stored_json(pdf) else {
        return Vec::new();
    };
    // Each generator read on its own, so one that can't be is passed over
    // alone.
    #[derive(Deserialize)]
    struct Listed {
        v: u32,
        generators: Vec<serde_json::Value>,
    }
    let stored = match serde_json::from_slice::<Listed>(&json) {
        Ok(stored) if stored.v == STORED_VERSION => stored,
        Ok(stored) => {
            log::warn!(
                "/SheafGenerators is version {}, not one sheaf reads",
                stored.v
            );
            return Vec::new();
        }
        Err(e) => {
            log::warn!("/SheafGenerators can't be read: {e}");
            return Vec::new();
        }
    };
    // Each generator's blocks in streams of their own; and the pages each
    // holds a block on, merged too, and those only merged.
    let mut blocks: BTreeMap<GenId, Vec<Block>> = BTreeMap::new();
    let mut holding: BTreeMap<GenId, BTreeSet<u32>> = BTreeMap::new();
    let mut merged_on: BTreeMap<GenId, Vec<u32>> = BTreeMap::new();
    for index in 0..src.page_count() {
        let markers = src.markers(index);
        for &(stream, marker) in markers.iter() {
            if let Marker::Gen {
                id,
                layout,
                recipe,
                number,
            } = marker
            {
                let block = Block {
                    index,
                    stream,
                    number,
                    digests: layout.zip(recipe),
                };
                blocks.entry(id).or_default().push(block);
                holding.entry(id).or_default().insert(index);
            }
        }
        let streams = markers.iter().any(|(_, m)| matches!(m, Marker::Gen { .. }));
        if !streams && names_a_list(src, index) {
            for id in merged(src, index) {
                holding.entry(id).or_default().insert(index);
                merged_on.entry(id).or_default().push(index);
            }
        }
    }
    let mut kinds = BTreeSet::new();
    let mut found = Vec::new();
    for g in stored.generators {
        let g: StoredGen = match serde_json::from_value(g) {
            Ok(g) => g,
            Err(e) => {
                log::warn!("a generator /SheafGenerators says can't be read: {e}");
                continue;
            }
        };
        let Some(id) = GenId::parse(g.id.as_bytes()) else {
            log::warn!("/SheafGenerators names a generator {:?}", g.id);
            continue;
        };
        // A header and footer is over the page's content, and says what it
        // fixed.
        let fits = match (&g.kind, &g.spec) {
            (StoredKind::Watermark, StoredSpec::Watermark(_)) => true,
            (StoredKind::HeaderFooter, StoredSpec::HeaderFooter(_)) => {
                g.z == Z::Over && g.frozen.is_some()
            }
            _ => false,
        };
        if !fits {
            log::warn!(
                "generator {id} isn't a {:?} as sheaf writes one at {:?}",
                g.kind,
                g.z
            );
            continue;
        }
        if g.replaces != g.replacing() {
            log::warn!("generator {id} replaces {:?}", g.replaces);
            continue;
        }
        let on = blocks.get(&id).map_or(&[][..], Vec::as_slice);
        let held = holding.get(&id);
        let holds = |index: u32| held.is_some_and(|h| h.contains(&index));
        // A page added since that it isn't on: one whose content can be
        // read, as each it was drawn on was.
        let added = || (0..src.page_count()).find(|&i| !holds(i) && src.blocks(i).is_ok());
        let pages = match g.scope {
            StoredScope::All => match added() {
                None => None,
                Some(index) => {
                    log::warn!(
                        "generator {id} is on every page, but page {} holds no block of it: \
                         it's on the pages that do",
                        index + 1
                    );
                    Some(held.map_or_else(Vec::new, |h| h.iter().copied().collect()))
                }
            },
            StoredScope::Pages => Some(held.map_or_else(Vec::new, |h| h.iter().copied().collect())),
        };
        if pages.as_ref().is_some_and(Vec::is_empty) {
            log::warn!("generator {id} is on no page");
            continue;
        }
        let placed = match &g.spec {
            StoredSpec::Watermark(StoredWatermark {
                content: StoredContent::Text { .. },
                ..
            })
            | StoredSpec::HeaderFooter(_) => None,
            StoredSpec::Watermark(StoredWatermark {
                content:
                    content @ (StoredContent::Image { image: art, .. }
                    | StoredContent::Page { pdf: art, .. }),
                ..
            }) => {
                if art.res().is_none() {
                    log::warn!("generator {id}'s image or page says {:?}", art.placed);
                    continue;
                }
                let subtype = match content {
                    StoredContent::Image { .. } => b"Image".as_slice(),
                    _ => b"Form",
                };
                Some(rebind(src, on, subtype))
            }
        };
        if placed == Some(None) {
            log::warn!("generator {id}'s image or page isn't drawn by a block of it");
            continue;
        }
        let unfit = match &g.spec {
            StoredSpec::Watermark(wm) => unfit_values(wm.content.values()),
            StoredSpec::HeaderFooter(hf) => header::unfit(hf),
        };
        if let Some(why) = unfit {
            log::warn!("generator {id} can't be drawn: {why}");
            continue;
        }
        if found.iter().any(|f: &Saved| f.id == id) || !kinds.insert((g.kind, g.z)) {
            log::warn!("generator {id} is a second of its id, or of its kind at its z");
            continue;
        }
        let numbered = match &g.spec {
            StoredSpec::HeaderFooter(hf) => hf.bates.is_some(),
            StoredSpec::Watermark(_) => false,
        };
        let numbers: BTreeMap<u32, u64> = (on.iter())
            .filter(|_| numbered)
            .filter_map(|b| Some((b.index, b.number?)))
            .collect();
        let unnumbered: Vec<u32> = match numbered {
            true => merged_on.remove(&id).unwrap_or_default(),
            false => Vec::new(),
        };
        if let Some(&first) = unnumbered.first() {
            log::warn!(
                "{} pages of header {id}, from page {}, hold its blocks merged into their \
                 content, whose Bates numbers can't be read",
                unnumbered.len(),
                first + 1
            );
        }
        let blocks = (on.iter())
            .filter_map(|b| {
                let (layout, recipe) = b.digests?;
                Some((b.index, b.stream, layout, recipe))
            })
            .collect();
        found.push(Saved {
            id,
            pages,
            stored: g,
            placed: placed.flatten(),
            numbers,
            unnumbered,
            blocks,
        });
    }
    found
}

/// A generator's block in a stream of its own, as its marker says it.
struct Block {
    /// Its page, by index.
    index: u32,
    stream: ObjectId,
    /// Its Bates number (`/N`).
    number: Option<u64>,
    /// Its layout's and recipe's digests (`/L`, `/R`), when it says both.
    digests: Option<(u128, u128)>,
}

/// Whether page `index` of `src` names a list of sheaf's generators' in
/// its resources' `/Properties`: where a block another tool merged into a
/// stream of its own may be.
fn names_a_list(src: &Source, index: u32) -> bool {
    let pdf = src.pdf();
    let dict = |obj| pdf.dereference(obj).ok()?.1.as_dict().ok();
    let Some(resources) = src.page(index).and_then(|f| f.resources.as_deref()) else {
        return false;
    };
    let lists = dict(resources).and_then(|res| dict(res.get(b"Properties").ok()?));
    lists.is_some_and(|lists| {
        (lists.iter()).any(|(_, list)| dict(list).is_some_and(|l| l.has(b"SheafGen")))
    })
}

/// The generators whose blocks page `index` of `src` holds merged into a
/// stream of its own, as the ranges its content marks name them; none when
/// it can't be read.
fn merged(src: &Source, index: u32) -> Vec<GenId> {
    let Ok(blocks) = src.blocks(index) else {
        return Vec::new();
    };
    (blocks.sheaf.iter())
        .filter_map(|b| match (b.block, b.at) {
            (SheafBlock::Gen(id), BlockAt::Marked(_)) => Some(id),
            _ => None,
        })
        .collect()
}

/// The catalog's `/SheafGenerators` string, when it has one.
fn stored_json(pdf: &LazyPdf) -> Option<Vec<u8>> {
    let catalog = pdf.catalog().ok()?;
    let (_, json) = pdf
        .dereference(catalog.get(GENERATORS.as_bytes()).ok()?)
        .ok()?;
    Some(json.as_str().ok()?.to_vec())
}

/// The XObject the first of `blocks` that draws one draws, when its page's
/// resources name it and it's of `subtype` (`/Image` or `/Form`).
fn rebind(src: &Source, blocks: &[Block], subtype: &[u8]) -> Option<ObjectId> {
    let lim = src.limits();
    let pdf = src.pdf();
    blocks.iter().find_map(|&Block { index, stream, .. }| {
        let stream = pdf.object_unkept(stream).ok()?;
        let data = (stream.as_stream().ok()?)
            .decompressed_content_with_limit(lim.content.decompressed)
            .ok()?;
        let mut operand = None;
        let mut name = None;
        for token in content::lex(&data, &ResCx::none(), lim) {
            match token.ok()?.1 {
                Tok::Operand(o) => operand = Some(o),
                Tok::Op(b"Do") => {
                    name = operand.and_then(content::name);
                    break;
                }
                _ => operand = None,
            }
        }
        let name = name?;
        let resources = src.page(index)?.resources.as_deref()?;
        let resources = pdf.dereference(resources).ok()?.1.as_dict().ok()?;
        let xobjects = pdf.dereference(resources.get(b"XObject").ok()?).ok()?.1;
        let id = xobjects
            .as_dict()
            .ok()?
            .get(&name)
            .ok()?
            .as_reference()
            .ok()?;
        let dict = &pdf.get(id).ok()?.as_stream().ok()?.dict;
        (dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(subtype)).then_some(id)
    })
}
