//! What sheaf knows about the file it last wrote (M10.5 §9.1): a
//! [`SaveRecord`], attached to the version a save wrote, which drives the
//! next save. The opened file's is synthesized at open
//! ([`SaveRecord::opened`]); an older record shrinks to its [`SaveNote`],
//! what the History panel shows of a save. And a save's input
//! ([`SaveInput`]): the state it writes, and the saved side it writes
//! after.
//!
//! Slice 9 builds what slice 9 produces: the whole file's hash, a restored
//! document's pending tail and the keys a path changed are slice 10's, and
//! what generators drew (`drawn`) stays empty until slices 16 and 17.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use lopdf::{Dictionary, LazyPdf, Object, ObjectId, XrefFacts};

use super::key::ObjKey;
use super::plan::{SaveTransform, StrategyKind};
use crate::attachments::Objects;
use crate::model::{DocState, GenId, PageId, Snapshot, SourceId, SourceInfo, VersionId};
use crate::serialize::crypt::Crypt;
use crate::serialize::durable::FileStamp;
use crate::store::BlobId;

/// What sheaf knows about a file it wrote, or opened.
#[derive(Clone)]
pub struct SaveRecord {
    /// The version the file holds.
    pub version: VersionId,
    pub path: Arc<Path>,
    /// When it was written, or read: a PDF date string.
    pub at: String,
    pub strategy: StrategyKind,
    /// The file as it was written or read: a save to it writes only while
    /// it's still that file (M10.5 proposal 33).
    pub stamp: FileStamp,
    pub xref: XrefFacts,
    /// How it's encrypted, which saves keep. None when it isn't.
    pub crypt: Option<Arc<Crypt>>,
    /// Where each object sheaf wrote sits.
    pub map: Arc<ObjMap>,
    /// The primary's object numbers live in the file: a Rewrite's
    /// `keep_only` prunes the rest.
    pub present: Arc<BitSet>,
    /// What the file's root objects hold that the state doesn't own.
    pub roots: Arc<RootValues>,
    pub tree: Arc<TreeShape>,
    /// What each generator drew on each page, by its layout's and recipe's
    /// digests. Empty until slices 16 and 17.
    pub drawn: Arc<HashMap<(PageId, GenId), (u128, u128)>>,
    /// What save policy already did to the file (§11.8), so a later save
    /// never repeats it.
    pub transforms: Arc<[SaveTransform]>,
}

impl SaveRecord {
    /// The record of `info`, the file a document opened at `version`, as
    /// `stamp` found it: what the file says of itself, since sheaf wrote
    /// none of it. Its objects are at their own numbers (the map's
    /// identity), every number in use is present, and the root objects'
    /// keys and page tree are the file's.
    pub fn opened(version: VersionId, info: &SourceInfo, stamp: FileStamp) -> SaveRecord {
        let src = &info.src;
        let pdf = src.pdf();
        let tree = TreeShape::of(pdf);
        SaveRecord {
            version,
            path: info.name.as_path().into(),
            at: crate::annot::pdf_date_now(),
            strategy: StrategyKind::Opened,
            stamp,
            xref: src.xref().clone(),
            crypt: src.crypt().cloned(),
            map: Arc::new(ObjMap::opened(pdf, src.id(), &tree)),
            present: Arc::new(pdf.ids().map(|(n, _)| n).collect()),
            roots: Arc::new(RootValues::of(pdf)),
            tree: Arc::new(tree),
            drawn: Arc::default(),
            transforms: Arc::from([]),
        }
    }

    /// What the History panel shows of the save that wrote it.
    pub fn note(&self) -> SaveNote {
        SaveNote {
            path: self.path.clone(),
            at: self.at.clone(),
            strategy: self.strategy,
        }
    }
}

/// What a write says of the file it made, before the file is on disk: its
/// [`SaveRecord`] but for the file's stamp, which only the write to disk
/// gives.
#[derive(Clone)]
pub struct OutputRecord {
    pub version: VersionId,
    pub path: Arc<Path>,
    /// When it was made: a PDF date string.
    pub at: String,
    pub strategy: StrategyKind,
    pub xref: XrefFacts,
    pub crypt: Option<Arc<Crypt>>,
    pub map: Arc<ObjMap>,
    pub present: Arc<BitSet>,
    pub roots: Arc<RootValues>,
    pub tree: Arc<TreeShape>,
    pub transforms: Arc<[SaveTransform]>,
}

impl OutputRecord {
    /// The record of the file, once written, as `stamp` found it.
    pub fn stamped(self, stamp: FileStamp) -> SaveRecord {
        SaveRecord {
            version: self.version,
            path: self.path,
            at: self.at,
            strategy: self.strategy,
            stamp,
            xref: self.xref,
            crypt: self.crypt,
            map: self.map,
            present: self.present,
            roots: self.roots,
            tree: self.tree,
            drawn: Arc::default(),
            transforms: self.transforms,
        }
    }
}

/// A save as the History panel shows it: what an older record shrinks to.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveNote {
    pub path: Arc<Path>,
    /// A PDF date string.
    pub at: String,
    pub strategy: StrategyKind,
}

/// Where the objects of a file sheaf wrote sit, by key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjMap {
    /// The blob whose objects are at their own numbers: the primary's,
    /// which stable numbering keeps where they were. The map lists none of
    /// them.
    pub identity: Option<BlobId>,
    /// Every other key, at its number.
    pub keys: BTreeMap<ObjKey, ObjectId>,
}

impl ObjMap {
    /// The map of `pdf`, the file in blob `blob`, as sheaf opened it: its
    /// objects at their own numbers, and its root objects' keys, its page
    /// tree's (`tree`) among them, at theirs.
    pub fn opened(pdf: &LazyPdf, blob: BlobId, tree: &TreeShape) -> ObjMap {
        let catalog = pdf.catalog().ok();
        let reference = |o: Option<&Object>| o.and_then(|o| o.as_reference().ok());
        let mut keys = BTreeMap::new();
        let trailer = pdf.trailer();
        let roots = [
            (ObjKey::Catalog, reference(trailer.get(b"Root").ok())),
            (ObjKey::Info, reference(trailer.get(b"Info").ok())),
        ];
        let in_catalog = [
            (ObjKey::AcroForm, b"AcroForm".as_slice()),
            (ObjKey::OutlinesRoot, b"Outlines"),
            (ObjKey::Labels, b"PageLabels"),
        ]
        .map(|(key, name)| (key, reference(catalog.and_then(|c| c.get(name).ok()))));
        for (key, id) in roots.into_iter().chain(in_catalog) {
            if let Some(id) = id {
                keys.insert(key, id);
            }
        }
        for (n, node) in (0..).zip(&tree.nodes) {
            keys.insert(ObjKey::PagesNode(n), node.id);
        }
        ObjMap {
            identity: Some(blob),
            keys,
        }
    }

    /// Where `key`'s object sits: its number in [`keys`](Self::keys), or
    /// its own for copy 0 of an object of the [`identity`](Self::identity)
    /// blob. Whether that object is still in the file is
    /// [`SaveRecord::present`]'s to say.
    pub fn get(&self, key: &ObjKey) -> Option<ObjectId> {
        if let Some(&id) = self.keys.get(key) {
            return Some(id);
        }
        match *key {
            ObjKey::Base { blob, obj, copy: 0 } if self.identity == Some(blob) => Some(obj),
            _ => None,
        }
    }
}

/// A set of object numbers, a bit each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    pub fn insert(&mut self, n: u32) {
        let (word, bit) = (n as usize / 64, n % 64);
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        self.words[word] |= 1 << bit;
    }

    pub fn contains(&self, n: u32) -> bool {
        let (word, bit) = (n as usize / 64, n % 64);
        self.words.get(word).is_some_and(|w| w & (1 << bit) != 0)
    }

    pub fn len(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// The numbers, in order.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        (0u32..).zip(&self.words).flat_map(|(i, &w)| {
            (0..64)
                .filter(move |b| w & (1 << b) != 0)
                .map(move |b| i * 64 + b)
        })
    }
}

impl FromIterator<u32> for BitSet {
    fn from_iter<I: IntoIterator<Item = u32>>(iter: I) -> BitSet {
        let mut set = BitSet::default();
        for n in iter {
            set.insert(n);
        }
        set
    }
}

/// The keys of each root object a save emits from the state, never from
/// what the file holds (§9.1): the same keys always, so set, save, undo,
/// save gives back each one as it was.
pub const CATALOG_OWNED: &[&[u8]] = &[
    b"Pages",
    b"Outlines",
    b"PageLabels",
    b"AcroForm",
    b"Version",
    b"SheafGenerators",
    // The header and footer's settings, until slice 17's generators.
    b"SheafHeaderFooter",
];
pub const ACROFORM_OWNED: &[&[u8]] = &[
    b"Fields",
    b"CO",
    b"DA",
    b"Q",
    b"DR",
    b"NeedAppearances",
    b"SigFlags",
];
pub const OUTLINES_OWNED: &[&[u8]] = &[b"First", b"Last", b"Count"];

/// What a file's root objects hold that a save doesn't emit from the
/// state: the values written, so a key that went (an `/OpenAction` to a
/// deleted page, a dropped `/XFA`) stays gone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RootValues {
    /// The catalog's keys but [`CATALOG_OWNED`].
    pub catalog: Dictionary,
    /// The form's but [`ACROFORM_OWNED`], when there's a form.
    pub acroform: Option<Dictionary>,
    /// The outline root's but [`OUTLINES_OWNED`], when there's one.
    pub outlines: Option<Dictionary>,
    /// The name dictionary's, every one, when there's one.
    pub names: Option<Dictionary>,
}

impl RootValues {
    /// `pdf`'s, as it holds them.
    pub fn of<P: Objects + ?Sized>(pdf: &P) -> RootValues {
        let catalog = pdf.catalog().ok();
        let dict = |key: &[u8]| {
            let at = catalog?.get(key).ok()?;
            pdf.dereference(at).ok()?.1.as_dict().ok()
        };
        let unowned = |d: &Dictionary, owned: &[&[u8]]| -> Dictionary {
            (d.iter())
                .filter(|(k, _)| !owned.contains(&k.as_slice()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        RootValues {
            catalog: catalog.map_or_else(Dictionary::new, |c| unowned(c, CATALOG_OWNED)),
            acroform: dict(b"AcroForm").map(|d| unowned(d, ACROFORM_OWNED)),
            outlines: dict(b"Outlines").map(|d| unowned(d, OUTLINES_OWNED)),
            names: dict(b"Names").cloned(),
        }
    }
}

/// A file's page tree: its nodes, the root first, each before the nodes
/// under it, as the page table walks them (a kid with `/Kids` is a node,
/// whatever its `/Type`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeShape {
    pub nodes: Vec<TreeNode>,
}

/// A node of the page tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub id: ObjectId,
    /// Its `/Kids`, nodes and pages, in order.
    pub kids: Vec<ObjectId>,
    /// How many kids it had when the file was opened: an Append splits it
    /// only once it's grown past max(128, this) (§11.6).
    pub size: usize,
}

impl TreeShape {
    /// `pdf`'s, from the catalog's `/Pages`. A node met again is listed
    /// once.
    pub fn of<P: Objects + ?Sized>(pdf: &P) -> TreeShape {
        let root = pdf
            .catalog()
            .ok()
            .and_then(|c| c.get(b"Pages").ok())
            .and_then(|p| p.as_reference().ok());
        let mut nodes = Vec::new();
        let mut seen = HashSet::new();
        let mut stack: Vec<ObjectId> = root.into_iter().collect();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Ok(dict) = pdf.get_dictionary(id) else {
                continue;
            };
            let kids: Vec<ObjectId> = (dict.get(b"Kids").ok())
                .and_then(|k| pdf.dereference(k).ok())
                .and_then(|(_, k)| k.as_array().ok())
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_reference().ok())
                .collect();
            let under = kids
                .iter()
                .copied()
                .filter(|&k| pdf.get_dictionary(k).is_ok_and(|d| d.has(b"Kids")));
            let under: Vec<ObjectId> = under.collect();
            stack.extend(under.into_iter().rev());
            nodes.push(TreeNode {
                id,
                size: kids.len(),
                kids,
            });
        }
        TreeShape { nodes }
    }
}

/// What a save writes from: the state of the version it writes, the
/// registrations its pages and forms come from, and the saved side it
/// writes after, when there's one.
#[derive(Clone)]
pub struct SaveInput {
    pub version: VersionId,
    pub target: DocState,
    /// The primary's registration: the file a Rewrite writes onto.
    pub primary: SourceId,
    /// The registrations, until slice 11 makes a source its blob: a
    /// derived copy's says which pages it was made from.
    pub sources: HashMap<SourceId, Arc<SourceInfo>>,
    pub saved: Option<SavedSide>,
}

/// The saved version's state, and its record.
#[derive(Clone)]
pub struct SavedSide {
    pub state: DocState,
    pub record: Arc<SaveRecord>,
}

impl SaveInput {
    /// `snap` with no saved side: nothing is sealed, and nothing is known
    /// of a file it writes after. What exports and the other writers from
    /// a [`Snapshot`] take.
    pub fn detached(snap: &Snapshot) -> SaveInput {
        SaveInput {
            version: snap.version,
            target: snap.state.clone(),
            primary: snap.primary,
            sources: snap.sources.clone(),
            saved: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bit_set_holds_what_was_put_in() {
        let set: BitSet = [0, 63, 64, 1000, 64].into_iter().collect();
        assert_eq!(set.iter().collect::<Vec<_>>(), [0, 63, 64, 1000]);
        assert_eq!(set.len(), 4);
        assert!(set.contains(1000) && !set.contains(999) && !set.contains(100_000));
        assert!(BitSet::default().is_empty() && !set.is_empty());
    }

    /// A key the map lists sits where it says, even an identity blob's
    /// object written elsewhere; otherwise copy 0 of the identity blob's
    /// objects sits at its own number, and nothing else sits anywhere.
    #[test]
    fn a_map_lists_keys_and_holds_its_blob_s_objects_where_they_were() {
        let blob = BlobId { hash: 1, len: 2 };
        let base = |obj, copy| ObjKey::Base { blob, obj, copy };
        let map = ObjMap {
            identity: Some(blob),
            keys: BTreeMap::from([(ObjKey::Catalog, (9, 0)), (base((5, 0), 0), (12, 0))]),
        };
        assert_eq!(map.get(&ObjKey::Catalog), Some((9, 0)));
        assert_eq!(map.get(&base((5, 0), 0)), Some((12, 0)));
        assert_eq!(map.get(&base((4, 1), 0)), Some((4, 1)));
        assert_eq!(map.get(&base((4, 1), 1)), None);
        let other = ObjKey::Base {
            blob: BlobId { hash: 3, len: 2 },
            obj: (4, 1),
            copy: 0,
        };
        assert_eq!(map.get(&other), None);
        assert_eq!(map.get(&ObjKey::Info), None);
    }
}
