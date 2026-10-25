//! What sheaf knows about the file it last wrote (M10.5 §9.1): a
//! [`SaveRecord`], attached to the version a save wrote, which drives the
//! next save. The opened file's is synthesized at open
//! ([`SaveRecord::opened`]), and a document restored from its file's
//! recovery sidecar gets one of the file with the sidecar's sections
//! pending ([`SaveRecord::restored`], §11.10); an older record shrinks to
//! its [`SaveNote`], what the History panel shows of a save. And a save's
//! input ([`SaveInput`]): the state it writes, and the saved side it
//! writes after.
//!
//! What the generators drew on each page (`drawn`) is what a reorder or a
//! label change after a save is judged against: a page whose block a
//! generator's live tokens lay out otherwise now is written again, and
//! the old block, drawn from the same recipe, may go (§7.2, §11.9).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use lopdf::{Dictionary, LazyPdf, Object, ObjectId, XrefFacts};
use sheaf_util::Checksum;

use super::emit::PdfVersion;
use super::key::ObjKey;
use super::plan::{SaveTransform, StrategyKind};
use crate::attachments::Objects;
use crate::effects::SigClass;
use crate::model::{DocState, GenId, PageId, Snapshot, VersionId};
use crate::prim::Key;
use crate::serialize::crypt::Crypt;
use crate::serialize::durable::FileStamp;
use crate::store::{BlobId, Src};

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
    /// The checksum of the whole file, so far: made as a Rewrite writes
    /// it, a copy's as it's copied, and carried on over each section
    /// appended. None for a file as opened, and an Append onto one: a sum
    /// taken later would say nothing of the file being the one sheaf read.
    /// A copy of the file is checked against it (`Onto::CopyOf`).
    pub content_hash: Option<Checksum>,
    /// Whether the file reads as its cross-reference sections say (its
    /// base is sound), which an update after it needs: as opened, the
    /// source's say; a file a Rewrite wrote is.
    pub sound: bool,
    /// Whether the file's signatures, when it has any, still hold as they
    /// were signed: as opened, and after an update or a copy of the file,
    /// but not once a Rewrite wrote it.
    pub signatures_hold: bool,
    /// The PDF version it says it is: its header's, or its catalog's
    /// `/Version` when that's later. An Append that needs a later one says
    /// so in the catalog (§11.7).
    pub pdf_version: PdfVersion,
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
    /// What each generator's block on each page is: as opened, what the
    /// blocks the open recognized say ([`drawn_of`]); after a save, what it
    /// drew on the pages it wrote, and the rest as they were.
    pub drawn: Arc<DrawnBlocks>,
    /// What save policy already did to the file (§11.8), so a later save
    /// never repeats it.
    pub transforms: Arc<[SaveTransform]>,
    /// What a document restored from the file's recovery sidecar holds
    /// that the file hasn't got, until a save writes it (§11.10): the
    /// document isn't what the file holds while there's one.
    pub pending_tail: Option<PendingTail>,
}

/// What a recovery sidecar holds that its file hasn't got (M10.5 §11.10),
/// which a document restored from it carries until its first save.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingTail {
    /// The sidecar's sections, which go after the file's bytes before the
    /// next Append's own: None when it held the whole document (Full),
    /// which no section follows, so nothing says what the file holds and
    /// the next save is a Rewrite.
    pub bytes: Option<Arc<[u8]>>,
    pub flags: TailFlags,
}

/// What sections a file hasn't got yet do to it, as a recovery sidecar's
/// flags say (§11.10): what the save that writes them judges beside its
/// own path, since the saved state a restored document opened as holds
/// them already.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TailFlags {
    /// They remove or replace text the file has (strict removal, §9.2), or
    /// a save transform in them would need a Rewrite, or the removal check
    /// found text they leave readable: the save is a Rewrite, or asks.
    pub removes: bool,
    /// Their classes of change, for the file's signatures (§9.3).
    pub sig: SigClass,
    /// The full names of the fields they touch, for FieldMDP.
    pub fields: BTreeSet<String>,
}

/// What a document knows of its file as it opens (§9.1): the file at
/// `path` as it was read; or, restored from the file's recovery sidecar
/// (§11.10), the file as the sidecar was written against it, `base`, and
/// what the sidecar holds that the file hasn't got.
#[derive(Debug, Clone, PartialEq)]
pub enum Came {
    Read {
        path: Arc<Path>,
        stamp: FileStamp,
    },
    Restored {
        path: Arc<Path>,
        base: FileStamp,
        pending: PendingTail,
    },
}

impl SaveRecord {
    /// The record of `src`, the file at `path` a document opened on at
    /// `version`, as `stamp` found it: what the file says of itself, since
    /// sheaf wrote none of it. Its objects are at their own numbers (the
    /// map's identity), every number in use is present, and the root
    /// objects' keys and page tree are the file's.
    pub fn opened(version: VersionId, src: &Src, path: Arc<Path>, stamp: FileStamp) -> SaveRecord {
        let pdf = src.pdf();
        let tree = TreeShape::of(pdf);
        SaveRecord {
            version,
            path,
            at: crate::annot::pdf_date_now(),
            strategy: StrategyKind::Opened,
            stamp,
            content_hash: None,
            sound: src.base_sound(),
            signatures_hold: true,
            pdf_version: PdfVersion::of(src),
            xref: src.xref().clone(),
            crypt: src.crypt().cloned(),
            map: Arc::new(ObjMap::opened(pdf, src.id(), &tree)),
            present: Arc::new(pdf.ids().map(|(n, _)| n).collect()),
            roots: Arc::new(RootValues {
                info: pdf
                    .trailer()
                    .get(b"Info")
                    .and_then(Object::as_reference)
                    .ok(),
                ..RootValues::of(pdf)
            }),
            tree: Arc::new(tree),
            drawn: Arc::default(),
            transforms: Arc::from([]),
            pending_tail: None,
        }
    }

    /// The record of a document restored from its file's recovery sidecar
    /// (§11.10) at `version`: `src` is what the sidecar recovers, the
    /// file's bytes and its sections after them read as one (or its whole
    /// document), for the file at `path`; `base` is the file as the sidecar
    /// was written against it; and `pending`, what the file hasn't got. The
    /// record of `src` as opened, but the file's stamp, so its first save
    /// writes `pending` after the file's bytes, while they're still those.
    pub fn restored(
        version: VersionId,
        src: &Src,
        path: Arc<Path>,
        base: FileStamp,
        pending: PendingTail,
    ) -> SaveRecord {
        SaveRecord {
            pending_tail: Some(pending),
            ..SaveRecord::opened(version, src, path, base)
        }
    }

    /// Whether it says what its file holds: not when a recovery sidecar
    /// restored the whole document in its place (§11.10).
    pub fn knows_file(&self) -> bool {
        (self.pending_tail.as_ref()).is_none_or(|tail| tail.bytes.is_some())
    }

    /// Whether an update can follow the file: it says what the file holds,
    /// which reads as its cross-reference sections say, and has some.
    pub fn takes_updates(&self) -> bool {
        self.knows_file() && self.sound && self.xref.sections > 0
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

/// A generator's block on a page of a file (§3.5): the digests its
/// `/SHEF_Block` says, of its layout (`/L`) and of the recipe it was drawn
/// from (`/R`), and its object, by the key the removal check finds it by in
/// that file: the record's map's, or the opened file's own object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawnBlock {
    pub layout: u128,
    pub recipe: u128,
    pub key: ObjKey,
}

/// The blocks generators drew on a file's pages, by page and generator.
pub type DrawnBlocks = HashMap<(PageId, GenId), DrawnBlock>;

/// The blocks `found`, the generators an open of `blob`'s file recognized
/// ([`Source::generators`]), drew on its pages, each the page `page` says
/// of its index there: the first block of each on a page, in a stream of
/// its own that says both digests, the file's object at its own number.
/// Read from what recognition read of the streams' markers, so a file no
/// generator was found in is never walked.
///
/// [`Source::generators`]: crate::source::Source::generators
pub fn drawn_of(
    found: &[crate::generate::Saved],
    blob: BlobId,
    page: impl Fn(u32) -> Option<PageId>,
) -> DrawnBlocks {
    let mut drawn = HashMap::new();
    for saved in found {
        for &(index, obj, layout, recipe) in saved.blocks() {
            let Some(page) = page(index) else {
                continue;
            };
            let key = ObjKey::Base { blob, obj, copy: 0 };
            let block = DrawnBlock {
                layout,
                recipe,
                key,
            };
            drawn.entry((page, saved.id)).or_insert(block);
        }
    }
    drawn
}

/// What a write says of the file it made, before the file is on disk: its
/// [`SaveRecord`] but for the file's stamp and checksum, which the write to
/// disk gives.
#[derive(Clone)]
pub struct OutputRecord {
    pub version: VersionId,
    pub path: Arc<Path>,
    /// When it was made: a PDF date string.
    pub at: String,
    pub strategy: StrategyKind,
    pub sound: bool,
    pub signatures_hold: bool,
    pub pdf_version: PdfVersion,
    pub xref: XrefFacts,
    pub crypt: Option<Arc<Crypt>>,
    pub map: Arc<ObjMap>,
    pub present: Arc<BitSet>,
    pub roots: Arc<RootValues>,
    pub tree: Arc<TreeShape>,
    pub drawn: Arc<DrawnBlocks>,
    pub transforms: Arc<[SaveTransform]>,
}

impl OutputRecord {
    /// The record of the file, once written, as `stamp` found it, its
    /// checksum `content_hash` when that's known.
    pub fn stamped(self, stamp: FileStamp, content_hash: Option<Checksum>) -> SaveRecord {
        SaveRecord {
            version: self.version,
            path: self.path,
            at: self.at,
            strategy: self.strategy,
            stamp,
            content_hash,
            sound: self.sound,
            signatures_hold: self.signatures_hold,
            pdf_version: self.pdf_version,
            xref: self.xref,
            crypt: self.crypt,
            map: self.map,
            present: self.present,
            roots: self.roots,
            tree: self.tree,
            drawn: self.drawn,
            transforms: self.transforms,
            pending_tail: None,
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
            (ObjKey::Generators, b"SHEF_Generators"),
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

    /// The largest number, when there's one.
    pub fn last(&self) -> Option<u32> {
        let (i, w) = self.words.iter().enumerate().rfind(|(_, w)| **w != 0)?;
        Some(i as u32 * 64 + 63 - w.leading_zeros())
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
    b"SHEF_Generators",
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
    /// The trailer's `/Info`, which a save keeps as it is: what an Append's
    /// trailer names.
    pub info: Option<ObjectId>,
}

impl RootValues {
    /// `pdf`'s, as it holds them, but the trailer's.
    pub fn of<P: Objects + ?Sized>(pdf: &P) -> RootValues {
        let catalog = pdf.catalog().ok();
        let dict = |key: &[u8]| {
            let at = catalog?.get(key).ok()?;
            pdf.dereference(at).ok()?.1.as_dict().ok()
        };
        RootValues {
            catalog: catalog.map_or_else(Dictionary::new, |c| unowned(c, CATALOG_OWNED)),
            acroform: dict(b"AcroForm").map(|d| unowned(d, ACROFORM_OWNED)),
            outlines: dict(b"Outlines").map(|d| unowned(d, OUTLINES_OWNED)),
            names: dict(b"Names").cloned(),
            info: None,
        }
    }
}

/// `d`'s keys but `owned`.
pub(crate) fn unowned(d: &Dictionary, owned: &[&[u8]]) -> Dictionary {
    (d.iter())
        .filter(|(k, _)| !owned.contains(&k.as_slice()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
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
    /// The primary's node whose keys but its place in the tree it holds,
    /// its inheritable ones among them: its own in the file as opened, the
    /// node it was split from for one an Append split off, and none in a
    /// tree a Rewrite wrote, whose nodes hold nothing else.
    pub from: Option<ObjectId>,
}

impl TreeShape {
    /// `pdf`'s, from the catalog's `/Pages`, each node holding its own
    /// keys. A node met again is listed once.
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
                from: Some(id),
            });
        }
        TreeShape { nodes }
    }
}

/// What a save writes from: the state of the version it writes, the
/// source it opened on, and the saved side it writes after, when there's
/// one, with the keys changed on the way.
#[derive(Clone)]
pub struct SaveInput {
    pub version: VersionId,
    pub target: DocState,
    /// The primary: the file a Rewrite writes onto.
    pub primary: Src,
    pub saved: Option<SavedSide>,
    /// The keys any step changed on the history tree's path from the saved
    /// version to this one (M10.5 §7.2), which the planner judges by value
    /// ([`crate::effects::judge`]). Empty with no saved side.
    pub path_keys: Arc<BTreeSet<Key>>,
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
            primary: snap.primary.clone(),
            saved: None,
            path_keys: Arc::default(),
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
        assert_eq!(set.last(), Some(1000));
        assert_eq!(BitSet::from_iter([63]).last(), Some(63));
        assert_eq!(BitSet::default().last(), None);
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
