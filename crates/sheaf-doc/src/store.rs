//! The bytes a document uses, named by their content (M10.5 §3.1): a
//! [`Blob`] is a byte string, a [`Src`] a PDF blob parsed. The [`Store`]
//! hands out one blob for the same bytes, however often they're interned,
//! and one source for a blob while anything holds it.
//!
//! Liveness is the `Arc`: a blob lives while a [`Blob`], [`BlobRef`] or
//! [`Src`] holds it, and the store keeps only weak references, so nothing
//! has to be freed by hand. A [`Source`] holds its blob, and the blob only a weak
//! reference to its source, so there's no cycle. Interning is outside
//! undo: undoing an insert leaves its file interned, and the bytes go when
//! the last version that holds them does.

use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, Weak};

use image::ImageFormat;
use zeroize::Zeroizing;

use crate::serialize::durable::FileStamp;
use crate::source::{Limits, OpenError, Source};

pub use sheaf_edit::BlobId;

/// The id of the blob `bytes` are: [`sheaf_util::checksum`] of them, and
/// their length.
pub fn blob_id(bytes: &[u8]) -> BlobId {
    BlobId {
        hash: sheaf_util::checksum(bytes),
        len: bytes.len() as u64,
    }
}

/// What a blob is: a PDF, an image a watermark draws, or a file a mark
/// attaches.
pub enum BlobKind {
    Pdf {
        /// What it was opened with, if it needed one.
        password: Option<Zeroizing<String>>,
        origin: Origin,
    },
    /// An image file, of the type its bytes say it is.
    Image(ImageFormat),
    /// The bytes of a file an attachment mark holds.
    Attachment,
}

impl BlobKind {
    /// An image file's, by what its bytes say they are: None when they're
    /// no image sheaf reads.
    pub fn image(bytes: &[u8]) -> Option<BlobKind> {
        sheaf_image::io::sniff(bytes).map(BlobKind::Image)
    }

    /// A PDF sheaf made in memory, as `kind` says.
    pub fn memory(kind: MemoryKind) -> BlobKind {
        BlobKind::Pdf {
            password: None,
            origin: Origin::Memory(kind),
        }
    }

    /// A page stamp's one-page PDF: a library stamp's, or one made from a
    /// placed stamp's page.
    pub fn stamp() -> BlobKind {
        BlobKind::memory(MemoryKind::Library)
    }

    /// Comments to read or write as data (`exchange::Comments`), opened
    /// with `password`: another PDF's, or a PDF of no pages made of a data
    /// file's or of a document's.
    pub fn comments(password: Option<&str>) -> BlobKind {
        BlobKind::Pdf {
            password: password.map(|p| Zeroizing::new(p.to_owned())),
            origin: Origin::Memory(MemoryKind::Comments),
        }
    }

    /// A file restored after a crash, from its recovery sidecar, opened
    /// with `password`.
    pub fn recovered(password: Option<&str>) -> BlobKind {
        BlobKind::Pdf {
            password: password.map(|p| Zeroizing::new(p.to_owned())),
            origin: Origin::Memory(MemoryKind::Recovered),
        }
    }

    /// A PDF read from the file at `path`, opened with `password`.
    pub fn file(path: &Path, password: Option<&str>) -> BlobKind {
        BlobKind::Pdf {
            password: password.map(|p| Zeroizing::new(p.to_owned())),
            origin: Origin::File(path.into()),
        }
    }
}

/// Where a blob was first interned from: a hint, never a name to show.
/// Interning the same bytes again keeps the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    File(Arc<Path>),
    Memory(MemoryKind),
}

/// What made a blob in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    /// A page stamp's one-page PDF, from the stamp library or made from a
    /// placed stamp's page, as one placed or pasted is.
    Library,
    /// A file restored after a crash from its recovery sidecar (M10.5
    /// §11.10): its bytes on disk with the sidecar's sections after them,
    /// or a Full sidecar's whole document.
    Recovered,
    /// Blank pages an insert brings ([`memory::blank`]).
    ///
    /// [`memory::blank`]: crate::memory::blank
    Blank,
    /// Images as pages, which an insert brings ([`memory::images`]).
    ///
    /// [`memory::images`]: crate::memory::images
    Images,
    /// A comment summary's pages (sheaf-ops' `comments::summary`).
    Summary,
    /// Comments as dictionaries, read or gathered to be written as data
    /// ([`exchange::Comments`]): a PDF of no pages made of an FDF or XFDF
    /// file's, or of a document's, or another PDF, read for its comments.
    /// Opened in a store of its own, and never a document's pages.
    ///
    /// [`exchange::Comments`]: crate::exchange::Comments
    Comments,
}

/// Two different byte strings with one [`BlobId`]: the second isn't
/// interned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("two different files have the same checksum ({id:?})")]
pub struct BlobCollision {
    pub id: BlobId,
}

/// The blobs of one document, and the caps its sources are read under.
pub struct Store {
    blobs: RwLock<HashMap<BlobId, Weak<BlobInner>>>,
    /// The bytes of the live blobs, each counted once.
    live: Arc<AtomicUsize>,
    limits: Limits,
}

/// A byte string the document uses. Clones share it.
#[derive(Clone)]
pub struct Blob(Arc<BlobInner>);

struct BlobInner {
    id: BlobId,
    bytes: Arc<[u8]>,
    kind: BlobKind,
    /// Its source while one lives.
    pdf: Mutex<Weak<Source>>,
    /// The store's count of live bytes.
    live: Arc<AtomicUsize>,
}

impl Drop for BlobInner {
    /// Never touches the store's map: a blob's last handle can drop while
    /// the map is locked. Its dead entry goes when the bytes are interned
    /// again, or when the map would grow.
    fn drop(&mut self) {
        self.live.fetch_sub(self.bytes.len(), Ordering::Relaxed);
    }
}

impl Store {
    /// An empty store, reading sources under [`Limits::DEFAULT`].
    pub fn new() -> Arc<Store> {
        Store::with_limits(Limits::DEFAULT)
    }

    pub(crate) fn with_limits(limits: Limits) -> Arc<Store> {
        Arc::new(Store {
            blobs: RwLock::default(),
            live: Arc::default(),
            limits,
        })
    }

    /// The blob of `bytes`: the one already interned when it's alive, and
    /// its bytes are these, compared byte for byte, with the origin and
    /// password it was first given; otherwise a new one of `kind`.
    /// Different bytes under a live blob's id are a collision.
    pub fn intern(&self, bytes: Arc<[u8]>, kind: BlobKind) -> Result<Blob, BlobCollision> {
        let id = blob_id(&bytes);
        let same = |blob: Arc<BlobInner>| {
            if Arc::ptr_eq(&blob.bytes, &bytes) || blob.bytes == bytes {
                Ok(Blob(blob))
            } else {
                Err(BlobCollision { id })
            }
        };
        // Compared outside the lock: the bytes never change.
        let found = self.read().get(&id).and_then(Weak::upgrade);
        if let Some(blob) = found {
            return same(blob);
        }
        let mut blobs = self.blobs.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(blob) = blobs.get(&id).and_then(Weak::upgrade) {
            drop(blobs);
            return same(blob);
        }
        if blobs.len() == blobs.capacity() {
            // Before the map grows, the dead go.
            blobs.retain(|_, blob| blob.strong_count() > 0);
        }
        self.live.fetch_add(bytes.len(), Ordering::Relaxed);
        let blob = Arc::new(BlobInner {
            id,
            bytes,
            kind,
            pdf: Mutex::default(),
            live: self.live.clone(),
        });
        blobs.insert(id, Arc::downgrade(&blob));
        Ok(Blob(blob))
    }

    /// The blob `id`, while anything holds it.
    pub fn get(&self, id: BlobId) -> Option<Blob> {
        self.read().get(&id).and_then(Weak::upgrade).map(Blob)
    }

    /// `blob` parsed: its cross-reference sections and trailer, and its
    /// page table. Parsed once while a [`Src`] of it lives, and again only
    /// after every one has dropped. The page table walks the whole tree,
    /// so this runs off the UI thread.
    pub fn source(&self, blob: &Blob) -> Result<Src, OpenError> {
        blob.source(&self.limits)
    }

    /// `bytes` interned as a blob of `kind`, as [`intern`](Self::intern)
    /// does, and parsed, as [`source`](Self::source) does.
    pub fn open(&self, bytes: Arc<[u8]>, kind: BlobKind) -> Result<Src, OpenError> {
        let blob = self.intern(bytes, kind)?;
        self.source(&blob)
    }

    /// The file at `path`, read once, opened with `password` and parsed,
    /// and the file as it was read: what the record of a document opened
    /// on it starts from ([`SaveRecord::opened`]). A wrong or missing
    /// password is [`OpenError::Password`] in the chain.
    ///
    /// [`SaveRecord::opened`]: crate::materialize::SaveRecord::opened
    pub fn read_file(
        &self,
        path: &Path,
        password: Option<&str>,
    ) -> anyhow::Result<(Src, FileStamp)> {
        use anyhow::Context as _;
        let reading = || format!("reading {}", path.display());
        let (bytes, metadata) = sheaf_util::read_file_and_metadata(path).with_context(reading)?;
        let stamp = FileStamp::of(&metadata, &bytes).with_context(reading)?;
        let src = (self.open(bytes, BlobKind::file(path, password)))
            .with_context(|| format!("opening {}", path.display()))?;
        Ok((src, stamp))
    }

    /// The image file at `path`, read once and interned: an error when
    /// it isn't an image sheaf reads.
    pub fn read_image(&self, path: &Path) -> anyhow::Result<Blob> {
        use anyhow::Context as _;
        let reading = || format!("reading {}", path.display());
        let bytes = sheaf_util::read_file(path).with_context(reading)?;
        let kind = BlobKind::image(&bytes)
            .with_context(|| format!("{} isn't an image sheaf reads", path.display()))?;
        Ok(self.intern(bytes, kind)?)
    }

    /// The sources parsed now, by their blobs' ids: what a mark naming a
    /// stamp or an attachment by stem is looked for in. None is parsed for
    /// this.
    pub fn sources(&self) -> Vec<Src> {
        let blobs: Vec<Arc<BlobInner>> = self.read().values().filter_map(Weak::upgrade).collect();
        let mut out: Vec<Src> = (blobs.iter())
            .filter_map(|blob| {
                blob.pdf
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .upgrade()
            })
            .map(Src)
            .collect();
        out.sort_unstable();
        out
    }

    /// Whether `blob` is this store's.
    pub fn holds(&self, blob: &Blob) -> bool {
        self.get(blob.id())
            .is_some_and(|ours| Arc::ptr_eq(&ours.0, &blob.0))
    }

    /// The bytes of every live blob, each counted once.
    pub fn bytes(&self) -> usize {
        self.live.load(Ordering::Relaxed)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<BlobId, Weak<BlobInner>>> {
        self.blobs.read().unwrap_or_else(PoisonError::into_inner)
    }
}

/// For tests: `pages` blank Letter pages, as though read from `a.pdf`,
/// parsed into `store`.
#[cfg(any(test, feature = "fixtures"))]
pub fn blank(store: &Store, pages: u32) -> Src {
    let bytes = crate::memory::blank_pages(612.0, 792.0, pages).expect("blank pages are written");
    let kind = BlobKind::file(Path::new("a.pdf"), None);
    store.open(bytes.into(), kind).expect("blank pages open")
}

/// The name a file goes by where the reader sees it (a form instance's,
/// a notice's): its path's last part.
pub fn file_name(path: &Path) -> Arc<str> {
    path.file_name()
        .map_or_else(|| "".into(), |n| n.to_string_lossy().into())
}

impl Blob {
    pub fn id(&self) -> BlobId {
        self.0.id
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.0.bytes
    }

    pub fn len(&self) -> usize {
        self.0.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.bytes.is_empty()
    }

    pub fn kind(&self) -> &BlobKind {
        &self.0.kind
    }

    /// The name of the file it was first interned from, when it was read
    /// from one: a hint where nothing better names it, never a form
    /// instance's name (those come from the edit that brought it).
    pub fn origin_name(&self) -> Option<Arc<str>> {
        match &self.0.kind {
            BlobKind::Pdf {
                origin: Origin::File(path),
                ..
            } => Some(file_name(path)),
            _ => None,
        }
    }

    /// It parsed under `limits`, as [`Store::source`] parses it, which
    /// reads under its store's: the source that's live of it, else a new
    /// one. What reads a blob's pages where its store isn't to hand (a
    /// stamp's page, a watermark's) reads under [`Limits::DEFAULT`], every
    /// document's.
    pub(crate) fn source(&self, limits: &Limits) -> Result<Src, OpenError> {
        let mut pdf = self.0.pdf.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(source) = pdf.upgrade() {
            return Ok(Src(source));
        }
        let source = Arc::new(Source::open(self.clone(), limits)?);
        *pdf = Arc::downgrade(&source);
        Ok(Src(source))
    }

    /// The password a PDF blob was opened with.
    pub fn password(&self) -> Option<&str> {
        match &self.0.kind {
            BlobKind::Pdf { password, .. } => password.as_deref().map(String::as_str),
            BlobKind::Image(_) | BlobKind::Attachment => None,
        }
    }
}

impl fmt::Debug for Blob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Blob").field(&self.0.id).finish()
    }
}

/// A blob as a record holds it: pins it. Equal, ordered and hashed by its
/// id.
#[derive(Clone)]
pub struct BlobRef(Blob);

impl BlobRef {
    pub fn blob(&self) -> &Blob {
        &self.0
    }

    pub fn id(&self) -> BlobId {
        self.0.id()
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        self.0.bytes()
    }
}

impl From<Blob> for BlobRef {
    fn from(blob: Blob) -> BlobRef {
        BlobRef(blob)
    }
}

impl PartialEq for BlobRef {
    fn eq(&self, other: &BlobRef) -> bool {
        self.id() == other.id()
    }
}

impl Eq for BlobRef {}

impl std::hash::Hash for BlobRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl PartialOrd for BlobRef {
    fn partial_cmp(&self, other: &BlobRef) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BlobRef {
    fn cmp(&self, other: &BlobRef) -> std::cmp::Ordering {
        self.id().cmp(&other.id())
    }
}

impl fmt::Debug for BlobRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BlobRef").field(&self.id()).finish()
    }
}

/// A PDF source: pins its blob. Equal, ordered and hashed by its blob's
/// id.
#[derive(Clone)]
pub struct Src(Arc<Source>);

impl Src {
    /// A reference that doesn't pin it: how an engine opened on its bytes
    /// finds it gone (M10.5 §10.3).
    pub fn downgrade(&self) -> Weak<Source> {
        Arc::downgrade(&self.0)
    }

    /// The source `weak` refers to, while anything pins it.
    pub fn upgrade(weak: &Weak<Source>) -> Option<Src> {
        weak.upgrade().map(Src)
    }
}

impl Deref for Src {
    type Target = Source;

    fn deref(&self) -> &Source {
        &self.0
    }
}

impl PartialEq for Src {
    fn eq(&self, other: &Src) -> bool {
        self.id() == other.id()
    }
}

impl Eq for Src {}

impl std::hash::Hash for Src {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

impl PartialOrd for Src {
    fn partial_cmp(&self, other: &Src) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Src {
    fn cmp(&self, other: &Src) -> std::cmp::Ordering {
        self.id().cmp(&other.id())
    }
}

impl fmt::Debug for Src {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Src").field(&self.id()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::fixture::Form;

    fn from_file(name: &str) -> BlobKind {
        BlobKind::file(Path::new(name), None)
    }

    fn intern(store: &Store, bytes: &[u8]) -> Blob {
        store.intern(bytes.into(), from_file("a.pdf")).unwrap()
    }

    #[test]
    fn interning_the_same_bytes_twice_gives_one_blob() {
        let store = Store::new();
        let bytes = Form::new(2).bytes();
        let first = store
            .intern(bytes.as_slice().into(), from_file("a.pdf"))
            .unwrap();
        // Another buffer of the same bytes, from another file.
        let second = store.intern(bytes.into(), from_file("b.pdf")).unwrap();
        assert!(Arc::ptr_eq(&first.0, &second.0));
        assert_eq!(store.bytes(), first.len());
        let BlobKind::Pdf { origin, .. } = second.kind() else {
            panic!("a PDF")
        };
        assert_eq!(origin, &Origin::File(Path::new("a.pdf").into()));
        assert!(Arc::ptr_eq(&store.get(first.id()).unwrap().0, &first.0));
        let other = intern(&store, b"other bytes");
        assert_ne!(other.id(), first.id());
        assert_eq!(store.bytes(), first.len() + other.len());
    }

    #[test]
    fn a_stem_names_its_blob() {
        let id = blob_id(b"notes");
        let stem = id.stem();
        assert_eq!(stem, format!("{}-5", sheaf_util::checksum_hex(b"notes")));
        assert_eq!(BlobId::from_stem(&stem), Some(id));
        let (hash, _) = stem.split_once('-').unwrap();
        for other in [
            // Named by FNV-1a, before xxh3.
            "cbf29ce484222325-0".to_string(),
            String::new(),
            "fixture-file".into(),
            stem.to_uppercase(),
            format!("+{stem}"),
            format!("{hash}-05"),
            format!("{hash}-"),
            format!("{hash}-5.pdf"),
        ] {
            assert_eq!(BlobId::from_stem(&other), None, "{other:?}");
        }
    }

    #[test]
    fn a_forced_blob_collision_is_refused() {
        let store = Store::new();
        let held = intern(&store, b"a blob");
        let bytes: Arc<[u8]> = Arc::from(&b"other bytes"[..]);
        let id = blob_id(&bytes);
        // As if `held`'s bytes had the other bytes' checksum and length.
        store
            .blobs
            .write()
            .unwrap()
            .insert(id, Arc::downgrade(&held.0));
        let refused = store.intern(bytes, from_file("b.pdf")).unwrap_err();
        assert_eq!(refused, BlobCollision { id });
        assert_eq!(store.bytes(), held.len());
        assert!(Arc::ptr_eq(&store.get(id).unwrap().0, &held.0));
    }

    #[test]
    fn the_store_counts_live_bytes_once_and_forgets_dead_blobs() {
        let store = Store::new();
        let a = intern(&store, b"four");
        let again = intern(&store, b"four");
        let b = intern(&store, b"eight by");
        assert_eq!(store.bytes(), 12);
        let id = b.id();
        drop(b);
        assert_eq!(store.bytes(), 4);
        assert!(store.get(id).is_none());
        drop(a);
        assert_eq!(store.bytes(), 4);
        drop(again);
        assert_eq!(store.bytes(), 0);

        // A source holds its blob.
        let blob = intern(&store, &Form::new(1).bytes());
        let (id, len) = (blob.id(), blob.len());
        let src = store.source(&blob).unwrap();
        drop(blob);
        assert_eq!(store.bytes(), len);
        assert_eq!(store.get(id).unwrap().id(), id);
        drop(src);
        assert_eq!(store.bytes(), 0);
        assert!(store.get(id).is_none());

        // Interned again, the dead are replaced; and they go before the
        // map grows.
        let blob = intern(&store, b"four");
        assert_eq!(store.bytes(), 4);
        drop(blob);
        for i in 0..1000u32 {
            drop(intern(&store, &i.to_le_bytes()));
        }
        assert!(store.blobs.read().unwrap().len() < 100);
        assert_eq!(store.bytes(), 0);
    }

    #[test]
    fn a_source_is_parsed_once_while_a_src_lives() {
        let store = Store::new();
        let blob = intern(&store, &Form::new(3).bytes());
        let first = store.source(&blob).unwrap();
        let second = store.source(&blob).unwrap();
        let elsewhere =
            std::thread::scope(|s| s.spawn(|| store.source(&blob).unwrap()).join()).unwrap();
        assert!(std::ptr::eq(&*first, &*second));
        assert!(std::ptr::eq(&*first, &*elsewhere));
        assert_eq!(first, elsewhere);
        assert_eq!(first.page_count(), 3);
        let source = Arc::downgrade(&first.0);
        drop((first, second, elsewhere));
        assert!(source.upgrade().is_none());
        let parsed_again = store.source(&blob).unwrap();
        assert_eq!(parsed_again.id(), blob.id());
        assert_eq!(parsed_again.page_count(), 3);
    }
}
