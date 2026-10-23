//! What a source's pages hold beside their content, read once a source
//! ([`Source::annots`]): sheaf's marks, with what they hold, and other
//! tools' comments and links, each with the object it is and an id (M10.5
//! §4). Marks another program copied, which say an id an earlier mark
//! has, are given new ids; other tools' annotations are given ids no mark
//! has. Both are drawn from an rng seeded with the file's hash, so the
//! same bytes give the same ids, however often and wherever they're read.
//! A connector keeps its partner only when the two name each other.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use in_progress::Progress;
use lopdf::ObjectId;

use super::Renamed;
use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::file::{ForeignAnnot, Found, InFile};
use crate::source::Source;

/// What a source's pages hold beside their content.
#[derive(Debug, Default)]
pub struct FileAnnots {
    /// Each page's that has any, by index, in `/Annots` order.
    pub pages: HashMap<u32, Vec<FileAnnot>>,
    /// The marks given new ids whose `/NM`s change (see `fresh_ids`), in
    /// page order and then `/Annots` order.
    pub renamed: Vec<Renamed>,
    /// Every mark given a new id, named or not, by page index and its slot
    /// in `/Annots`: its object says the id it was copied with, so a save
    /// writes it anew.
    pub fresh: HashSet<(u32, usize)>,
    /// What the marks hold, by the stems they name it by, found the first
    /// time one is asked for.
    held: OnceLock<Held>,
}

/// The page stamps' page forms and the attachments' embedded files a
/// source's marks hold, and the forms other tools' stamps are, which the
/// marks made of them hold, by stem: where two name one, the first's, in
/// page order and then `/Annots` order.
#[derive(Debug, Default)]
struct Held {
    stamps: HashMap<String, ObjectId>,
    files: HashMap<String, ObjectId>,
}

/// An annotation on a page of a source, and the object it is: None for a
/// dictionary written into the page's `/Annots`.
#[derive(Debug, Clone, PartialEq)]
pub struct FileAnnot {
    pub object: Option<ObjectId>,
    /// Its place in the page's `/Annots`, among the form's widgets and
    /// what sheaf passes over (popups, say).
    pub slot: usize,
    pub body: FileBody,
}

/// What an annotation on a page of a source is.
#[derive(Debug, Clone, PartialEq)]
pub enum FileBody {
    /// One of sheaf's marks, under its id, and what it holds of its own:
    /// its page stamp's page form or its attachment's embedded file.
    Mark {
        annot: Box<Annot>,
        held: Option<ObjectId>,
    },
    /// Another tool's comment or link, under the id drawn for it.
    Foreign {
        id: AnnotId,
        summary: Arc<ForeignAnnot>,
    },
}

impl FileAnnot {
    pub fn id(&self) -> AnnotId {
        match &self.body {
            FileBody::Mark { annot, .. } => annot.id,
            FileBody::Foreign { id, .. } => *id,
        }
    }
}

impl FileAnnots {
    /// Page `index`'s, in `/Annots` order.
    pub fn on(&self, index: u32) -> &[FileAnnot] {
        self.pages.get(&index).map_or(&[], Vec::as_slice)
    }

    /// sheaf's marks on page `index`.
    pub fn marks_on(&self, index: u32) -> impl Iterator<Item = &Annot> {
        self.on(index).iter().filter_map(|a| match &a.body {
            FileBody::Mark { annot, .. } => Some(&**annot),
            FileBody::Foreign { .. } => None,
        })
    }

    /// sheaf's marks, by page index: what pages inserted from the file
    /// bring of it.
    pub fn marks(&self) -> HashMap<u32, Vec<Annot>> {
        (self.pages.keys())
            .map(|&index| (index, self.marks_on(index).cloned().collect::<Vec<_>>()))
            .filter(|(_, marks)| !marks.is_empty())
            .collect()
    }

    /// Other tools' comments and links on page `index`, in `/Annots`
    /// order, with their ids and objects.
    pub fn foreign_on(
        &self,
        index: u32,
    ) -> impl Iterator<Item = (AnnotId, Option<ObjectId>, &Arc<ForeignAnnot>)> {
        self.on(index).iter().filter_map(|a| match &a.body {
            FileBody::Foreign { id, summary } => Some((*id, a.object, summary)),
            FileBody::Mark { .. } => None,
        })
    }

    /// The page form a page stamp naming `stem` draws: a mark's, or
    /// another tool's stamp's that a mark is made of.
    pub fn stamp(&self, stem: &str) -> Option<ObjectId> {
        self.held().stamps.get(stem).copied()
    }

    /// The embedded file an attachment naming `stem` holds.
    pub fn file(&self, stem: &str) -> Option<ObjectId> {
        self.held().files.get(stem).copied()
    }

    fn held(&self) -> &Held {
        self.held.get_or_init(|| {
            let mut held = Held::default();
            let mut order: Vec<u32> = self.pages.keys().copied().collect();
            order.sort_unstable();
            for a in order.into_iter().flat_map(|index| self.on(index)) {
                let (annot, object) = match &a.body {
                    FileBody::Mark {
                        annot,
                        held: Some(object),
                    } => (&**annot, *object),
                    FileBody::Foreign { summary, .. } => match (&summary.mark, summary.stamp) {
                        (Some(mark), Some(form)) => (&**mark, form),
                        _ => continue,
                    },
                    FileBody::Mark { held: None, .. } => continue,
                };
                let (by_stem, stem) = match &annot.kind {
                    AnnotKind::PageStamp { stamp, .. } => (&mut held.stamps, stamp),
                    AnnotKind::FileAttachment { file, .. } => (&mut held.files, file),
                    _ => continue,
                };
                by_stem.entry(stem.clone()).or_insert(object);
            }
            held
        })
    }
}

/// What `src`'s pages hold, read through its [`LazyPdf`](lopdf::LazyPdf)
/// in one pass over the page table's annotations; `p` counts its pages.
pub(crate) fn read(src: &Source, p: &dyn Progress) -> FileAnnots {
    let pages = (src.pages().iter())
        .inspect(|_| p.inc(1))
        .map(|page| (page.id, page.geom.rotate));
    let found = crate::file::annots_in(src.pdf(), pages);
    let hash = src.id().hash;
    let mut rng = fastrand::Rng::with_seed(hash as u64 ^ (hash >> 64) as u64);
    let mut pages = found.pages;
    let (renamed, fresh) = fresh_ids(&mut pages, &mut rng);
    follow_renamed(&mut pages, &renamed);
    pair_connectors(&mut pages);
    // Other tools' ids, drawn after the marks' new ones, none a mark's.
    let mut taken: HashSet<AnnotId> = marks(&pages).map(|a| a.id).collect();
    let mut order: Vec<u32> = pages.keys().copied().collect();
    order.sort_unstable();
    let pages = order
        .into_iter()
        .map(|index| {
            let list = pages.remove(&index).expect("a page with annotations");
            let list = list
                .into_iter()
                .map(
                    |InFile {
                         object,
                         slot,
                         found,
                     }| {
                        let body = match found {
                            Found::Mark { annot, held } => FileBody::Mark {
                                annot: Box::new(annot),
                                held,
                            },
                            Found::Foreign(f) => FileBody::Foreign {
                                id: draw(&mut rng, &mut taken),
                                summary: Arc::new(f),
                            },
                        };
                        FileAnnot { object, slot, body }
                    },
                )
                .collect();
            (index, list)
        })
        .collect();
    FileAnnots {
        pages,
        renamed,
        fresh,
        held: OnceLock::new(),
    }
}

/// Every mark among `pages`.
fn marks(pages: &HashMap<u32, Vec<InFile>>) -> impl Iterator<Item = &Annot> {
    pages.values().flatten().filter_map(|a| match &a.found {
        Found::Mark { annot, .. } => Some(annot),
        Found::Foreign(_) => None,
    })
}

/// An id from `rng` below 2^53, so a JSON reader holds it exactly, that
/// `taken` hasn't got; now it has.
pub(crate) fn draw(rng: &mut fastrand::Rng, taken: &mut HashSet<AnnotId>) -> AnnotId {
    loop {
        let id = AnnotId(rng.u64(1..1 << 53));
        if taken.insert(id) {
            return id;
        }
    }
}

/// New ids for the marks whose id an earlier mark has, in page order and
/// then `/Annots` order (M10.5 §3.6): copies another program made, which
/// say the `/NM sheaf:<id>` they were copied with (Acrobat's Insert Pages
/// makes them, since `/NM` need only be unique on its page). The first
/// keeps its id. Each new one is drawn from `rng` ([`draw`]), taken by no
/// mark in the file. A mark sheaf made has no name of its own, so its next
/// write gives it the new id's `/NM`, and a reply copied with it stays in
/// its thread; those are the ones given back. One that keeps another
/// tool's `/NM` keeps it, and only its id changes. Every one given a new
/// id, by page and slot, too.
fn fresh_ids(
    pages: &mut HashMap<u32, Vec<InFile>>,
    rng: &mut fastrand::Rng,
) -> (Vec<Renamed>, HashSet<(u32, usize)>) {
    let mut taken: HashSet<AnnotId> = marks(pages).map(|a| a.id).collect();
    let mut seen = HashSet::new();
    let mut renamed = Vec::new();
    let mut fresh = HashSet::new();
    let mut order: Vec<u32> = pages.keys().copied().collect();
    order.sort_unstable();
    for page in order {
        let list = pages.get_mut(&page).expect("a page with annotations");
        // The names the copies on this page had, and have now.
        let mut names = HashMap::new();
        let mut copies = Vec::new();
        for (i, entry) in list.iter_mut().enumerate() {
            let Found::Mark { annot: a, .. } = &mut entry.found else {
                continue;
            };
            if seen.insert(a.id) {
                continue;
            }
            let old = a.id;
            a.id = draw(rng, &mut taken);
            if a.name.is_empty() {
                let [was, _] = old.sheaf_names();
                names.insert(was, a.name());
                renamed.push(Renamed {
                    page,
                    was: old,
                    id: a.id,
                });
            }
            fresh.insert((page, entry.slot));
            copies.push(i);
        }
        for i in copies {
            if let Found::Mark { annot: a, .. } = &mut list[i].found
                && let AnnotKind::Reply { parent } | AnnotKind::Status { parent, .. } = &mut a.kind
                && let Some(new) = names.get(parent)
            {
                *parent = new.clone();
            }
        }
    }
    (renamed, fresh)
}

/// Each connector's partner kept only when it's a connector of the file's
/// whose partner is that one: a pair's ends name each other (M10.5 §6.4).
/// One whose other end another tool deleted, or a copy of an end another
/// program made (which names the end it was copied from), has none.
fn pair_connectors(pages: &mut HashMap<u32, Vec<InFile>>) {
    let partners: HashMap<AnnotId, Option<AnnotId>> = (marks(pages))
        .filter_map(|a| match a.kind {
            AnnotKind::Connector { partner, .. } => Some((a.id, partner)),
            _ => None,
        })
        .collect();
    for entry in pages.values_mut().flatten() {
        if let Found::Mark { annot, .. } = &mut entry.found {
            let id = annot.id;
            if let AnnotKind::Connector { partner, .. } = &mut annot.kind
                && partner.is_some_and(|p| partners.get(&p) != Some(&Some(id)))
            {
                *partner = None;
            }
        }
    }
}

/// Other tools' comments in the thread of a mark `fresh_ids` renamed, as
/// a reply to a copy is: they name it as the file does, by its old id, and
/// now name it by its new one, on its page. Where a mark on that page
/// still has the old name, they're left naming that one.
fn follow_renamed(pages: &mut HashMap<u32, Vec<InFile>>, renamed: &[Renamed]) {
    if renamed.is_empty() {
        return;
    }
    let names: HashMap<(u32, String), String> = renamed
        .iter()
        .map(|r| {
            let ([was, _], [now, _]) = (r.was.sheaf_names(), r.id.sheaf_names());
            ((r.page, was), now)
        })
        .collect();
    for (&page, list) in pages.iter_mut() {
        let kept: HashSet<String> = list
            .iter()
            .filter_map(|a| match &a.found {
                Found::Mark { annot, .. } => Some(annot.name()),
                Found::Foreign(_) => None,
            })
            .collect();
        for entry in list.iter_mut() {
            let Found::Foreign(f) = &mut entry.found else {
                continue;
            };
            let Some(parent) = &mut f.parent else {
                continue;
            };
            if let Some(new) = names.get(&(page, parent.clone()))
                && !kept.contains(parent.as_str())
            {
                *parent = new.clone();
            }
        }
    }
}
