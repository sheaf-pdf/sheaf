# M10.5 slice 3: Lazy sources and the Store

Status: done · Part of [M10.5: the edit API](README.md)

**Lazy sources and the Store** (L; bugs 4 and 9).
- **LazyPdf** in vendored lopdf:
  - the xref and trailer at open, `get_object` on demand, the ObjStm
    cache;
  - per-object decryption, stream-dict strings included, and `/EFF`;
  - the header offset in `XrefFacts`;
  - one `Limits`;
  - an eager fallback with `base_sound = false`;
  - a positional-read byte source.
- `Object::Real` becomes `f64`.
- `Store`, `Blob`, `BlobRef` and `Src`, with byte-compared interning;
  `Source` holds its `Blob`.
- **`SourceInfo` shrinks to a registration.** Its bytes, page sizes
  and password move into the `Blob` and `Src`, and a registration
  keeps only `{ src: Src, name, derived }`.
  - `SourceId(u32)` stays as the registration handle until slice 11.
    Today's form identity is per registration: every insert registers
    anew, and `take_in_fields`' join applies only within one
    registration's runs. Switching to `SourceId = BlobId` before form
    instances exist would merge two inserts of the same bytes into
    one form.
  - Derived copies and generated-cache files become interned blobs. A
    derived copy's registration keeps its `derived` record.
- **Facts:** the page table with inherited attributes and `Geom`,
  repeated page objects, per-page `/Annots` lists, conformance.
- `forms::read` and the layout sizes read `LazyPdf`.
- `HistoryStats.blob_bytes`, and a real `undone_source_bytes`.
- **Deletes:**
  - `SourceInfo`'s `data`, `bytes`, `page_sizes`, `page_count`,
    `first_page` and `password` (moved into `Blob` and `Src`), and
    `set_source_sizes`;
  - the PDFium size walk for layout;
  - the decrypt-and-strip in the read-only paths that moved;
  - the hard-coded `undone_source_bytes`.
- **Checks:**
  - Every corpus object read lazily equals lopdf's eager load
    (skipped without the corpus).
  - The RC4-128, AES-128 and AES-256 fixtures decrypt per object
    identically.
  - An attachment-only `/EFF` fixture decrypts its attachment.
  - A file with a 100-byte prefix reads identically.
  - Fuzz targets cover the lazy reader and the page tree.
  - Cyclic `/Kids`, a `/Parent` loop, a 10,000-deep chain, an ObjStm
    bomb and a 4,096-section chain each give typed errors within the
    caps.
  - `Geom`'s displayed sizes equal PDFium's on every corpus page.
  - The geometry walk takes under 100 ms on the synthetic 7k and
    under 250 ms on the heavy one.
  - A forced blob collision is refused.
  - Inserting the same file twice still gives two registrations and
    two forms.
  - The save suites pass with f64.
  - Inserting a 400-page scan, undoing and pruning frees its bytes
    (the Store's counter drops).

## Notes

Done. Sources are blobs named by their content and read lazily:
vendored lopdf's `LazyPdf` reads a file's cross-reference sections and
trailer when it opens and each object when it's asked for, decrypted
on its own, under one set of caps; the `Store` holds each file's bytes
once, and a `Source` adds the page table, whose `Geom` gives the layout
PDFium's sizes from the first paint. A registration is
`{ src, name, derived }`; the PDFium size walk, `set_source_sizes` and
the read-only paths' decrypt-and-strip are gone; and history counts
the sources only undone versions hold and frees them with those
versions (bugs 4 and 9).
- **f64 reals** (vendored lopdf): `Object::Real(f64)`, parsed as an
  f64 in objects and in content. `From<f64>` keeps the value;
  `From<f32>` gives the f64 nearest the f32's shortest decimal, so
  `0.1f32` still writes `0.1`. `as_float` returns an f64, and `as_f32`
  is `as_f64`. sheaf makes a real from an f32 with `Object::from` and
  reads one back `as f32`, so page geometry is still computed in f32,
  as PDFium computes it.
- **`LazyPdf`** (`vendor/lopdf/src/lazy.rs`):
  `open(bytes, password, &ReadLimits)` finds the header, reads the
  `/Prev` chain and the trailer, keeping `/Encrypt`, and
  authenticates the empty password, then the one given
  (`InvalidPassword` otherwise). Objects go into `OnceLock` slots, in
  chunks made on first use, with no lock held while parsing (the
  first value kept wins), and so does a failure the bytes alone
  decide (a parse, an offset, a decryption), as the error to give
  again. An object stream is decoded once and its members moved into
  their slots as `Arc`s; a stream whose indirect `/Length` the parser
  couldn't read is filled as the eager loader fills it. Each read
  carries a context through the parser, so a cycle or a broken cap on
  its resolution path fails the read, nothing on the path is kept,
  and later reads on it fail at once. The API is `Document`'s for
  reading (`get_object`, `get_dictionary`, `dereference`, `catalog`,
  `trailer`, `version`, `is_encrypted`) with `object` (an `Arc`),
  `raw_span`, `ids` and `xref()`, the `XrefFacts { kind, start, size,
  max_stream, id, header_offset, hybrid, linearized, sections }`.
  When the sections can't be read, the table is rebuilt by scanning
  and every object read at once (`base_sound()` is false); a broken
  cap never falls back. With no `trailer` keyword, the rebuilt
  table's trailer is the newest cross-reference stream that decodes.
  The bytes are a `ByteSource`: an `Arc<[u8]>`, or `FileBytes`, a
  file read by position, whose window doubles while a parse fails
  (but for a broken cap) or reaches its end. `ReadLimits::DEFAULT`
  caps what a stream decompresses to (256 MiB), the sections (1,024),
  object numbers (8,388,607) and depth (64), each an
  `Error::Limit(Limit::…)`; a cross-reference stream lists no number
  past the object cap, and the chain's streams twice as many entries
  at most, refused from their dictionaries before they decompress; a
  `/Length` cycle is `ReferenceCycle`. The eager reader was
  refactored to share it, not copied: the parser asks a `Resolve`
  trait for strictness, the decompression limit and indirect lengths,
  `XrefReader` reads the sections from any byte source, and the
  header offset, an object's bound, the choice of password and the
  integral-real `/Length` are shared helpers.
- **Encryption, per object, both ways** (`encryption.rs`): a stream's
  dictionary is walked like any other, so its strings take the string
  filter (lopdf #603); its data takes its `/Crypt` filter, else
  `/EFF`'s for `/Type /EmbeddedFile`, else `StmF`'s. `EncryptionState`
  reads and writes `/EFF` (`set_embedded_file_filter`,
  `default_embedded_file_filter`, `get_embedded_file_filter`). From V4
  a filter's name is `/Identity` or one of `/CF`'s, and an absent
  `StmF` or `StrF` is `/Identity`; any other name is
  `DecryptionError::UndefinedCryptFilter` at the open, and
  `get_stream_filter` and `get_string_filter` return a `Result` (they
  fell back to RC4, so `StmF /Identity` read as noise). Left alone:
  cross-reference streams, the encryption dictionary (an earlier
  revision's too), metadata when `/EncryptMetadata` is false, and a
  signature's `/Contents`, kept as stored both ways, as PDFium keeps
  it. A `Crypt` filter with no `/Name` or no `DecodeParms` is
  Identity.
- **`Store`, `Blob` and `Src`** (`store.rs`): `BlobId { hash, len }`,
  `sheaf_core::checksum` and the length. `Store::intern` hands back
  the live blob of the same id after comparing the bytes, outside the
  lock (a mismatch is `BlobCollision`), and keeps the first blob's
  origin and password. The map holds `Weak`s, swept when it would
  grow, and a blob's `Drop` only lowers the live count (`bytes()`).
  `Store::source` parses a blob once while a `Src` of it lives;
  `open` interns and parses; `holds` backs a debug assert that every
  registration's blob is the document's store's.
  `BlobKind::Pdf { password: Option<Zeroizing<String>>, origin }`
  with `Origin::{File(Arc<Path>), Memory(MemoryKind::Derived)}`, and
  `BlobKind::file(path, password)` a file's. `Src` is `Arc<Source>`,
  equal, hashed and ordered by `BlobId`. `sheaf_core::read_file`
  reads a file straight into one `Arc<[u8]>`.
- **`Source` and its facts** (`source/`): the blob, its `LazyPdf`,
  its `Limits`, the page table, and conformance when first asked. The
  page table is an iterative walk of `/Kids` in PDFium's order: a kid
  with `/Kids` is a node and any other a page, a page listed twice is
  two entries (`repeat` 0 and 1) sharing its facts, built once, and a
  kid that names no dictionary is a Letter stand-in, as PDFium and
  lopdf's `get_pages` keep it. It stops at the root's `/Count` when
  PDFium counts by it (1..0xFFFFE), since PDFium loads no page past
  it. The inheritable keys walk `/Parent` as PDFium's `GetPageAttr`
  does, memoized per node. `OpenError` is `Password`, `Pdf`,
  `PageTree(Cycle | ParentLoop | TooDeep | TooManyPages | NoPages)`
  or `Collision`. `PageFacts { id, parent, resources, geom, contents,
  annots, struct_parents, repeat }` keeps `/Resources`, `/Contents`
  and `/Annots` as written (an annotation's slot is its index), each
  an `Arc` that repeats share, and an `/Annots` array pages name by
  reference read once and shared; the `/Parent` chain is a shared
  list. `conformance()` reads the XMP's PDF/A and PDF/UA claims with
  quick-xml, under any prefix.
- **`Geom`** (`geom.rs`): normalized f32 `Rect`s and
  `Geom { media, crop, trim, bleed, art, rotate, user_unit }`, read as
  PDFium sizes a page. An empty media box is Letter; an empty crop box
  is the media box and any other is clipped to it (one outside it is
  0×0, PDFium's `Intersect`); `/Rotate` is `(degrees / 90) mod 4`
  (135 is 90, −90 is 270); `/UserUnit` is kept but not applied.
  `visible()` and `shown()`; `Source::page_size(i)` is `shown()`, and
  Letter past the table.
- **One `Limits`** (`source/limits.rs`): `reader` (lopdf's
  `ReadLimits`), `page_tree` (1,024 deep; 0xFFFFF − 1 pages, which
  caps node visits too) and `forms` (`FormLimits`: forms::read's caps,
  which the form check and the XFA reader had copies of). `MAX_COORD`
  and `MAX_LAYOUT_LEN` stay constants.
- **Registrations** (`model.rs`): `SourceInfo { src, name: PathBuf,
  derived }`. `SourceInfo::read(store, path, password)`, the one way
  to register a file, reads it once, interns it (`Origin::File`) and
  parses it; `SourceInfo::blank(store, pages)` (tests and `fixtures`)
  makes blank Letter pages. `Document::new(store, primary, outline,
  policy)` takes the store, as §14.2's
  `open::start(bytes, password, store)` will. Layout sizes,
  `removes_content`, `name_apart`, `form_notice`, save's same-file
  and unchanged checks, recovery and exchange read `src` and `name`.
- **Layout sizes from `Geom`, from the first paint:** the app reads,
  interns and parses a file off the UI thread, opens PDFium on the
  same `Arc<[u8]>` (`RenderHandle::open_bytes`, which replaced
  `open(path)`; `PdfiumEngine` holds an `Arc<[u8]>`), and installs
  both. Insert, replace and save (which reads the saved file back
  into the store before `rebase_to_saved`) do the same in their
  background tasks, and recovery opens the same way.
- **Derived copies are memory blobs:** crop, `strip_stamps` and
  `watermark` take the `&Store` and intern their copies as
  `Memory(Derived)` through `derive::copy_of`, keeping the original's
  name and the `derived` record. No cache file is written, and their
  engines open from the copies' bytes. Blank pages, image pages and
  Combine's image PDFs are read back from the generated cache once,
  into `Origin::File` blobs. `blank::find` reads `/Annots` from the
  page table.
- **The read-only paths read the Source:** `forms::read(src)` (its
  `Reader` generic over `attachments::Objects`, which lopdf's
  `Document` and `LazyPdf` implement; widgets from the page table, so
  a page listed twice has its widgets at both indexes, each shared
  `/Annots` array looked through once; the caps from
  `src.limits().forms`), `properties::read(src)` (Info, the versions
  and MarkInfo over the `LazyPdf`, fonts from the page table's
  inherited resources, security from the kept `/Encrypt`, Fast Web
  View from `XrefFacts.linearized`) and `signatures_broken(src)`
  (`encrypted` from `is_encrypted()`). `read_open(src)` reads the form
  from the source and the rest from one eager load of the blob's
  bytes, no longer of the path again. `forms::read::fields_of` serves
  `forms::rules`' reads of the eager document they write. The layers
  panel opens its re-saved copy from memory, and `read_attachment`
  takes the file's bytes.
- **History counts and frees sources** (`model.rs`, `history.rs`): a
  version owns the registrations it lists. `rebase_to_saved` puts the
  saved file's on the version it makes; when `prune` moves the first
  version forward, the old first's registrations go to the new one,
  ahead of its own; `remove_version` drops the removed version's
  through `unregister`, never the primary. `HistoryStats.blob_bytes`
  is `store.bytes()`, and `undone_source_bytes` counts, once by
  `BlobId`, the blobs only versions off the current line register;
  `total_bytes()` counts the second, not the first.
- **Fuzzing:** `fuzz/` (cargo-fuzz's layout, outside the workspace,
  with its own `[patch.crates-io]` for lopdf and a lock file) has the
  `lazy_reader` and `page_tree` targets, which call
  `sheaf_doc::source::fuzz`'s bodies, and `pdf.dict`;
  `.github/workflows/fuzz.yml` runs each for 300 s nightly from the
  corpus the last night's run grew (kept with `actions/cache`), seeded
  with the six fixtures by the ignored `write_fuzz_seeds`, and keeps a
  crash's input. The stable `fuzz_bodies_survive_mutated_fixtures`
  runs the bodies on 3,600 seeded mutations of the fixtures, and keeps
  a failing case's input in the temp directory, since the AES
  fixtures' IVs are drawn afresh each run.
- **The gate harness:** the geometry walk row times
  `store.source(&blob)` on bytes already interned (the
  cross-reference sections and the page table) and asserts < 100 ms
  on the synthetic and < 250 ms on the heavy, after the table prints.
  The fixtures open through `SourceInfo::read`, and the crop row's
  engines open from the copies' bytes, writing nothing.
- **The ledger:** 42 rows for slice 3. `SourceInfo`'s six fields,
  `path` (now `name`) and `page_size`, `SourceData`,
  `set_source_sizes`, `file::source_bytes`; the size walk
  (`RenderHandle::{page_sizes, open, data}`, `Renderer::page_sizes`,
  `pdfium::PdfiumEngine::bytes`, `Msg::PageSizes`, `SizeWalk`,
  `SIZE_CHUNK`, `DocInfo::{page_count, first_page}`,
  `DocView::{fetch_page_sizes, fetch_source_sizes}`); the form caps
  that moved into `FormLimits` (`forms::read`'s eleven, `check`'s
  `MAX_DA` and `MAX_XFA`, `xfa::MAX_XFA`);
  `properties::{properties_of, page_resources, linearized}`; and
  `PropertiesContext::{password, bytes}`. `QUALIFIED_ONLY` gains
  `data`, `bytes`, `page_count`, `first_page`, `password`,
  `page_size`, `open`, `linearized` and `path`. Run over `crates/` as
  of b840e10 (slice 2), every row hits.
- **Docs:** `docs/document-model.md`'s Core idea sketches the Store,
  Blob, Src, Source and SourceInfo and gains "Sources are read
  lazily" (the open and its kept failures, the page table and `Geom`,
  what repeats share and the root's `/Count`, `Limits` and the files
  they keep from opening, the rebuilt table and its stream trailer,
  bytes in memory, what still loads eagerly, the geometry-walk gate);
  "What the viewer changes" and Saving say sizes come from `Geom` and
  old sources stay registered until their versions are pruned;
  Undo/redo's sources bullet and the Decisions bullet on
  `HistoryStats` describe `blob_bytes`, `undone_source_bytes` and
  freeing at pruning. `vendor/lopdf/SHEAF.md` has each patch (f64
  reals, `LazyPdf` and `ByteSource`, the reading and encryption
  fixes) with what covers it, and says the way back to crates.io is
  now upstreaming the patch, as the root `Cargo.toml`'s comment does.
  `bench/README.md` names the walk row, and `bench.py`'s docstring
  says the PDFium size walk is no longer the layout's.

Deviations from the plan:
- **Line references had moved:** the hard-coded `undone_source_bytes`
  was in `stats_for` (model.rs:1841, not :1715), and
  `set_source_sizes` at model.rs:953.
- **"The PDFium size walk for layout" named no symbols.** It was
  `RenderHandle::page_sizes`, `Msg::PageSizes` and the worker's
  `SizeWalk`, `Renderer::page_sizes` and its PDFium impl,
  `DocInfo.first_page` (the layout's estimate),
  `DocView::fetch_page_sizes` and `fetch_source_sizes`, and the size
  receivers after insert, replace and save; all are deleted. So are
  `RenderHandle::open(path)`, `RenderHandle::data`,
  `PdfiumEngine::bytes` and `DocInfo.page_count`, which had no users
  once every engine opened from bytes (the layout's count is the
  Src's).
- **The decrypt-and-strip went from the read-only paths only:**
  `file::load_pdf` and `load_pdf_mem` stay for the old writers, crop,
  derive and exchange (slices 7 and 9). What moved to the Source is
  `signatures_broken` (its `load_source`), `properties::read` (its
  `load_pdf`) and `read_open`'s form. `read_open`'s marks, labels,
  header/footer, attachments, layers and styles, and `describe`'s
  outline styles, load eagerly until slice 4, so the scanned 7k's
  ready RSS didn't drop (Measured).
- **The tests live in the workspace:** `LazyPdf`'s in sheaf-doc
  (`source::lazy_tests`), since vendor/lopdf is outside the workspace
  and lopdf's own tests aren't vendored; the Geom-equals-PDFium corpus
  check in sheaf-ops (`tests/source_corpus.rs`), since sheaf-doc
  doesn't link PDFium.
- **One `Limits` over two crates:** sheaf-doc's `Limits` owns the
  value, and its `reader` part is lopdf's `ReadLimits`, which
  `LazyPdf` takes. `max_depth` is 64, not 256: a resolution path
  recurses through the parser, and at opt-level 3 a 2 MiB thread
  overflowed between 512 and 1,024 levels, while real paths are about
  6 deep. The section cap is 1,024, below the check's 4,096 and above
  the 1,002-section test's floor.
- **`BlobRef` comes with its first holder, in slice 8,** which
  completes the scope bullet "`Store`, `Blob`, `BlobRef` and `Src`".
  It's a choice, not the rule's: the slice ships other facts before
  their consumers (`raw_span`, `base_sound`, `Source::object`,
  `PageFacts`' `struct_parents`, `contents` and `parents()`, `Geom`'s
  `trim`, `bleed`, `art` and `user_unit`, `conformance`),
  since its bullets and Checks name them, while a `BlobRef(Blob)` with
  no record to hold it would be `Blob` again under another name.
- **Nothing ships unproduced:** `BlobKind` is `Pdf` only and
  `MemoryKind` `Derived` only; `BlobId`'s `b<hex>-<len>` form waits
  for Op JSON (11); `Source::id()` is the `BlobId`, since
  `model::SourceId` stays the registration until 11. There's no
  `Source::crypt()`, nor an `EncryptionState` accessor on `LazyPdf`,
  until slice 7's serializer reads them. `conformance()` ships as a
  fact with no consumer before slice 16's PDF/A note.
- **Generated-cache files:** only those registered as sources are
  interned, by reading the cache file once (`Origin::File`): blank
  pages, image pages and Combine's image PDFs. Stamps and attachments
  are cache stems marks refer to, which nothing holds a blob for
  before slices 6 and 8, so they stay stems. Derived copies are
  memory blobs and aren't written at all.
- **A crop box outside the media box shows nothing** (0×0, PDFium's
  `Intersect`), not "ignored" as §3.3 has it. The corpus can't decide
  it (no page has one; the 8 crops that reach past their media boxes
  matched PDFium exactly), so PDFium's rule won: the layout equals
  what the engine renders until slice 13, as the app's PDFium sizes
  did. `/Rotate` is PDFium's `(degrees / 90) mod 4` (135 gives 90,
  where `Rotation::from_degrees` gives 0), and `/UserUnit` doesn't
  scale sizes, as PDFium's don't.
- **The page table numbers pages as PDFium does,** since PDFium
  renders by index until slice 13; lopdf's `get_pages`, which the old
  writers use, can disagree on a damaged tree. The walk stops at the
  root's `/Count` when PDFium counts by it (an integer, a real's whole
  part or a reference to one, in 1..0xFFFFE): with `/Count 2` over
  three kids, the layout showed a third page PDFium can't load, which
  rendered as an error, and search never reached it. One past the
  tree leaves the tree's pages, where PDFium's extra indexes load
  nothing. PDFium also takes an inner node's `/Count` when the root's
  isn't usable; that isn't modelled, and no corpus file needs it (the
  counts agree on every one). A dictionary written in place in
  `/Kids` is skipped, as `get_pages` skips it (PDFium would make it a
  page). `PageFacts.parent` is a shared chain, not a Vec a page, so
  memory stays linear in nodes (Vec chains of 1,024 a page would let
  a 40 MB file reach gigabytes); `contents` keeps a reference to an
  array as that reference, since telling an array from a stream reads
  it. During the walk only a broken cap, a `ReferenceCycle` or a
  failed decryption fails the open; any other read failure counts as
  an absent object, as PDFium's `GetDirectObjectFor` has it.
- **Files PDFium shows that sheaf won't open (rule 12).** The app
  opens a file through its source now, before PDFium is asked, so a
  file whose source doesn't open no longer opens, where b840e10
  opened it through PDFium alone and showed it, with its marks and
  form unread (`load_sheaf_annots` only logged "reading
  annotations"). That's a `/Parent` loop or a node listing itself (a
  typed error where PDFium stops quietly), a page-tree object that
  doesn't decrypt (where PDFium reads what it can), and a broken cap.
  It was also the 3 corpus forms lopdf couldn't read (1819139,
  5208359, 6614490), which the rebuild below opens now: every corpus
  file PDFium opens opens as a source, and
  `every_corpus_page_s_geom_is_pdfium_s_size` fails on one that
  doesn't, with the error the app would show. No corpus file has the
  others. Treating a page dictionary that doesn't decrypt as absent
  wasn't taken: the layout would show a Letter stand-in where PDFium
  renders the page.
- **`Source::page(i)` is `Option<&PageFacts>`,** not §3.1's
  `Result<Arc<PageFacts>>`, and `pages()` is beside it: the table is
  built when the source opens, so a page is there or past the end,
  and nothing about it can fail later. `Source` is `{ blob, pdf,
  limits, pages, conformance }`, without §3.1's `crypt` (slice 7).
- **Hostile shapes the plan's Checks didn't name,** found by the
  review and fixed with a test each (numbers under Measured): the
  table copied a page's `/Annots` array, `/Contents` array and
  `/Resources` dictionary into every listing, so pages sharing one
  referenced array, or a page a DAG lists thousands of times, took
  gigabytes (now `Arc`s, shared); a failed read wasn't kept, so a
  broken object every page names was parsed once a page (now kept
  unless fatal); a cross-reference stream's entries were capped only
  after the whole chain decoded, and twenty sections relisting one
  range opened after half a minute (now from each stream's
  dictionary, and twice the object cap in all); a `startxref` a few
  bytes off read a positional file to its end (the probe reads 64
  bytes); and `raw_span` ended a stream whose `/Length` the parser
  couldn't read at its `stream` keyword (now after `endobj`, by the
  length `read_content` used). `forms::read` looks through a shared
  `/Annots` array once, which the sharing would otherwise leave as
  pages × entries of lookups.
- **`name` is a `PathBuf`,** for the recovery sidecar, save's
  same-file check, notices and `name_apart`'s stems; it's never read
  for bytes. `Document::new` takes the store explicitly.
- **`forms::read` finds widgets through the page table, over a
  crate-private `Objects` trait** rather than `&LazyPdf`:
  `forms::rules` (`drop_xfa`, `drop_rights`) also reads the form of
  the eager document it writes, through the transitional `fields_of`
  (a transitional ledger row now, 3 to 9). `forms::xfa::read` takes
  the caps as a parameter, so a source's own limits reach its XFA.
  `FormLimits` holds `xfa` too, and `MAX_SCRIPT_ENTRIES` is `scripts`.
  `drop_rights` now keeps `/SigFlags` when the tree is damaged, rather
  than counting the signed fields read before the damage. The
  manifest counts didn't move.
- **Six reading fixes in shared lopdf code change the eager reader
  too,** each found by the corpus comparison or the review and each
  turning a wrong or failed eager read into a right one:
  `transform_object`'s exemptions above (126 encrypted corpus forms
  had a catalog, a signature or an earlier `/Encrypt` that didn't
  decrypt, kept half decrypted by the eager reader); `/Encrypt null`
  is no encryption (eager read no object of 7 corpus forms or of pool
  `0676477`); `xref_start` takes any white-space around the offset (2
  forms and pool `2289457` load, and 2 forms are no longer rebuilt);
  and PDFium's `VerifyCrossRefTable`, a classic table whose first
  object in use isn't at its entry's offset being rebuilt by scanning
  (`XrefError::Offsets`), for `forms/safedocs/0290608.pdf` (57 junk
  bytes before `%PDF-`, offsets from the file's start), whose catalog
  read neither way; a rebuilt file with no `trailer` keyword taking
  the newest cross-reference stream that decodes as its trailer, as
  PDFium does, for the 3 forms above (damaged, with only
  cross-reference streams; two linearized and 23 and 35 bytes shorter
  than `/L`; a stream that doesn't decode is no trailer, so lopdf's
  tests of malformed streams still fail their loads); and, in a
  rebuilt file, a member that object streams the table doesn't place
  hold coming from the stream latest in the file, as incremental
  updates append, where the eager reader took whichever a thread read
  first (1819139, whose updates move fields between object streams,
  read a field's old value in 2 eager loads of 8). lopdf's 321 tests
  pass with each.
- **The lazy reader parts from the eager one where the eager one is
  wrong:** it reads only what the table lists, under the number the
  table gives, and an object that doesn't decrypt is
  `Error::Decryption`, not kept half decrypted. The eager reader's
  header keying and unlisted object-stream members were left alone,
  since the old writers load through it, so the corpus check allows
  12 listed files, each with its listed number of differences of its
  listed kind (a Shadowed one only the trailer's `/Info`), and fails
  when a listed file stops differing.
- **`LazyPdf` details the plan left open:** `XrefFacts.start` is
  where the newest section really is, counted from the header (0 when
  rebuilt), not a miswritten `startxref`; `/ID` is
  `Option<[Vec<u8>; 2]>`, and the largest xref-stream number is
  `max_stream`. Each object stream is decoded once with lopdf's
  `ObjectStream::new_with_limit` and only a state per container kept,
  rather than reusing `Reader`'s `object_streams` map, the eager
  reader's own field. A rebuilt table adds the object-stream members
  its scan can't list, as the eager loader does. `FileBytes::new(File)`
  records the length once, and a read asks for a byte past the
  object's bound, so a well-formed object takes one read. `ids()` (for
  the fuzz body) and `is_encrypted()` are extra; `ByteSource`'s
  `#[allow(clippy::len_without_is_empty)]`, with its reason, keeps
  clippy on lopdf itself clean.
- **Encryption choices:** `/EFF` is named on a state with
  `set_embedded_file_filter`, not by a new field on
  `EncryptionVersion::V4`/`V5`, which would break lopdf's API and
  tests. An undefined filter name fails the open (all three defaults,
  though PDFium ignores `/EFF`), and a stream's `/Crypt /Name` follows
  the same rule when it's read (no corpus stream has one). An embedded
  file is known by `/Type /EmbeddedFile`, which 3 corpus embedded files
  lack; no corpus file has `/EFF`. The fixtures are written with a
  table and with a stream, to check the xref stream's `/ID` stays as
  it is now that stream dictionaries are walked. A signature's
  `/Contents` stays as stored both ways, for slices 10 and 12.
- **f64 choices:** `measure_dict` writes `/C` as its full f64
  (0.013888888888888888), since it's computed in f64 and narrowing
  made `c * 72` 1.0000000074505806; `anon_name`'s `{:.2}` fingerprint
  formats each real as an f32, so annotations without `/NM` keep their
  names; the colour readers that make a u8 compute in f64 (differing
  only at .5 boundaries). `incremental.rs`'s `{:.6}` now writes an
  f32's shortest digits (561.4, not 561.400024).
  `reals_keep_their_digits` is in file.rs's tests, through
  `Content::encode`, since lopdf's `Writer` isn't public. `From<f32>`
  skips finding the shortest decimal for a whole number below 2^24,
  its own shortest decimal (7 ns, not 96); the heap allocation the
  review named was 10–15% of the rest (about 200 ns a fraction),
  which the rule needs, so a stack buffer wasn't kept.
- **Engines open from the blob's bytes:** `RenderHandle::open_bytes`
  replaced `open` rather than sitting beside it. `PdfiumEngine` takes
  an `Arc<[u8]>`, and `sheaf_core::read_file` reads into one through
  `Arc::new_zeroed_slice` (`new_uninit_slice` would hand out a
  `&mut [u8]` over uninitialized memory) with no second copy:
  `Arc::from(Vec)` copied, and the scanned 7k's cold render went from
  365 to 748 ms. With the size walk gone, PDFium's page list was cold
  when the outline asked each destination's page index: the heavy's
  outline took 1.18 s, and ready 1,790 ms. `PdfiumEngine::outline`
  now loads every page dictionary first, under the lock a page at a
  time and only when there are bookmarks (60 ms at 7k), a lookup PDFium
  needs and the layout doesn't: a transitional ledger row, 3 to 4,
  since slice 4's outline comes from the source.
- **New API, each with live users:** `SourceInfo::read`, `Store::open`
  and `holds`, `OpenError::{Collision, is_password}`,
  `docview::needs_password` (lopdf's error and PDFium's; the combine
  dialog's inspect uses it), `Source::pages()`, and
  `forms::fixture::source` and `Form::source` for tests.
  `read_attachment` takes the file's bytes and password;
  `Incoming::read` takes the `&SourceInfo` it warns about, and insert
  and replace intern first, then read the form from the same Src.
  `signatures_broken` returns an `Option` and `properties::read` a
  `Properties`: neither can fail now, and the dialog's error branch is
  gone. `PropertiesContext` is `{ path, source: Src, page_size, dirty }`.
  The workspace's Replace-with dialog counts pages with
  `SourceInfo::read` on a throwaway store, not PDFium.
- **Fixed on the way:** `WouldBreakSignatures.encrypted` was in effect
  `password.is_some()` (the eager load strips the encryption state),
  so a signed file encrypted with the empty user password offered
  Append, which the incremental writer then refused. File > Properties
  said "No Security" for every such file (573 in the corpus). Save's
  unchanged check no longer skips a source whose length wasn't
  recorded (`bytes == 0`). `forms/safedocs/1263925.pdf`'s signature
  dictionary doesn't decrypt, so under rule 12 its field reads as
  unsigned (signed 1 → 0), and `signatures_broken` doesn't ask before
  rewriting it.
- **History beyond the bullets:** the primary a save replaces is
  dropped in `rebase_to_saved` when no version lists it; otherwise a
  save's file that pruning left behind (`remove_subtree` taking a saved
  branch) stayed registered, by nothing, for the session. A job that
  nets to nothing puts its version's registrations back to wait for the
  next version instead of losing them (only OCR uses jobs, and it
  registers nothing). The replay proptest reused registrations across
  branches, which the app never does (it registers each insert anew),
  so its builder now draws from `other` or the opened file, and
  `check()` asserts each registration is listed once (by a version or
  as waiting), the registered set is those plus the primary, `forms`
  holds only registered ids, and every recorded state's pages and
  fields name registered sources. `droppable` and `stats_for` take the
  line, worked out once a trim. Steps name sources by `SourceId`, so
  source bytes are never step bytes, which settles slice 1's "source
  bytes in step accounting"; `total_bytes()` includes
  `undone_source_bytes` and leaves out `blob_bytes`, so a large undone
  insert makes `MemoryBudget` drop every snapshot it may until its
  branch is pruned, by count until slice 12, as slice 1 noted for step
  bytes.
- **The app keeps an engine per registration** (`Doc.handles`, each
  holding the blob's `Arc<[u8]>`) and `form_bar.incoming` per
  `SourceId`, so pruning lowers the store's count but not the app's
  RSS until slice 13's pool. `doc.model.source(sid)` can now be None
  for a pruned branch's id; every caller already handles it.
- **Fuzzing ran here:** nightly was installed, contrary to the
  planner's note, and cargo-fuzz 0.13.2 went into `~/.cargo/bin`. The
  targets build with `--target x86_64-unknown-linux-gnu` (cargo-fuzz's
  musl binary defaults to musl), as the workflow does; the workflow
  itself can't run locally.
- **The plan's ledgers changed:** the transitional ledger's
  generated-cache row says blank and image pages come in slice 3 and
  stamps in 6, and a row for the `Objects` seam and `fields_of` is
  added (3 to 9), and one for `PdfiumEngine::outline`'s page
  dictionaries (3 to 4); the lint ledger's `temp_dir` row has the layers
  panel's temp file gone in slice 3 (its decrypted in-memory copy stays
  until slice 13). `SizeWalk` and `SIZE_CHUNK` got rows at the end of
  the slice; the slice's other private helpers became methods or
  changed in place, and `docs/document-model.md`'s claim that a blob
  lives while a snapshot or an engine holds it was corrected (only
  registrations hold blobs; an engine holds the bytes).
- **Tests adapted:** about 70 `SourceInfo` literals became
  `SourceInfo::blank` (model only) or `SourceInfo::read` through
  small local wrappers (tests that need a real file's content); the
  save tests' `kept()` is gone, since every source keeps its bytes.
  The form-read tests can't write `INFINITY` or NaN through a file
  (lopdf writes `inf`, which it can't parse), so they swap a
  placeholder, at the same length, for a real past f64's range; NaN
  can't come from a file at all now, and the proptest's floats are a
  `Num` strategy. The damaged-tree tests read under small limits
  through `Store::with_limits` (`pub(crate)`). Files open through
  `BlobKind::file(path, password)`, `Store::open` and
  `SourceInfo::read`, the tests' too. The lazy-against-eager
  comparison goes both ways (an object only the lazy read has is
  `OnlyLazy`), and the encryption tests compare the lazy read with
  the eager one, which they compare with the plain file. The gate
  harness still sets `SHEAF_CACHE_DIR`, since `read_open`'s
  `recover_cached` can write there. A proptest regression seed a
  deliberately broken build wrote was removed.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The 7k harness,** one full run after the review's fixes (step 9;
  ms unless marked; slice 2's baselines in brackets). Step 8's run,
  with the heavy twice more, is in parentheses where it differs by
  more than noise:

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | geometry walk (asserted) | 48.2 [46.6] | 80.0 (72.2–80.4) [59.4] | 61.8 (79.7) [53.0] |
  | ready | 77.7 [77.1] | 965 (982–1,090) [724] | 592 [853] |
  | ready, peak RSS / added | 59.6 / 52.4 MB [47.2 / 40.4] | 449 / 436 MB [320 / 312] | 1,039 / 1,032 MB [1,011 / 1,005] |
  | render 1,024 px, cold | 29.7 [30.0] | 728 [733] | 382 [365] |
  | render 1,024 px, warm | 5.26 [5.26] | 5.05 [4.95] | 22.2 [16.5] |
  | 200 previews at 320 px | 538/s [522] | 168/s [171] | 151/s [152] |
  | incremental save, one mark | 55.1 [56.6] | 166 (172–179) [166] | 813 (891) [742] |
  | incremental save, peak RSS / added | 79.1 / 31.0 MB [61.5 / 35.5] | 509 / 164 MB [374 / 216] | 1,545 / 1,004 MB [1,032 / 1,004] |
  | Rewrite (the bytes) | 295 (332) [300] | 3,109 (3,555) [3,082] | 934 [1,201] |
  | Rewrite, peak RSS / added | 99.6 / 49.1 MB [78.0 / 50.5] | 558 / 195 MB [417 / 248] | 1,598 / 1,022 MB [1,078 / 1,021] |
  | crop all pages (no copy written) | 461 (526) [445] | 2,348 (2,721–2,745) [2,326] | 1,461 (1,696) [2,256] |
  | crop, the edit alone | 1.63 [1.65] | 1.59 [1.70] | 1.54 [1.71] |
  | Bates apply | 109 [114] | 263 [270] | 63.5 [428] |
  | Bates, the Rewrite | 745 (823) [750] | 3,938 (4,502) [3,979] | 1,330 (1,492) [1,708] |
  | flatten | 340 (369) [336] | 3,556 (4,072) [3,489] | 1,301 (1,521) [1,495] |
  | flatten, peak RSS / added | 95.1 / 62.2 MB [74.2 / 63.3] | 570 / 231 MB [420 / 281] | 1,562 / 1,039 MB [1,057 / 1,041] |

  History: AnnotAdd 0.674 ms [0.686] and a full reorder 0.958 ms
  [0.945], both asserted; the 200 OCR versions as in slice 2 (201
  versions, 2 snapshots, 4,076 MB of steps, 4,738 MB of RSS growth).
  The walk read 47.9 and 74.0 ms in step 4 and 47.4 and 71.7 ms in
  step 5. What moved, and why:
  - **The walk** is the source's own open now (the lazy sections and
    the page table), where slice 2's was PDFium's `page_sizes`; both
    gates pass at about half and a third of their bounds. The
    scanned's isn't gated: 7,000 page dictionaries in one flat tree,
    each parsed when the walk reaches it.
  - **The heavy's ready is 965 ms here and 982–1,090 ms in step 8,**
    at and over slice 4's < 1 s gate (827 ms after step 5): step 6
    reads the form over the source, where step 5 read it from
    `read_open`'s eager document, which stays for the rest of the
    open, beside `describe`'s. A probe of the pieces on the heavy:
    reading, interning and the walk 85–148 ms; `forms::read` over the
    source 128–204 ms; each eager load 40–158 ms (`describe`'s for
    outline styles, and `read_open`'s); `facts_in` 20 ms; `read_open`
    in all 400–536 ms. Its peak RSS, 448–450 MB [320], is the source
    keeping the objects the form read (7,000 merged fields and
    widgets and what they name) beside the eager loads.
  - **The scanned's ready RSS didn't drop,** as expected: 1,039 MB
    [1,011]. The session holds the file's 474 MB as a blob (the
    renderer's shared buffer held it before), and `read_open`'s eager
    parse of it is the other 500 MB until slice 4. It was 1,512 MB
    after step 5, when `read_open` still read the path again, and
    ready itself is faster (581 ms [853]).
  - **Peak RSS rises by what the open holds** on every later row,
    while what each operation adds is the same or less (the heavy's
    Rewrite adds 194 MB [248], its incremental save 161 MB [216]).
  - **Crop was 1.17–1.18× on the synthetic and heavy in step 8,** and
    is 1.04× and 1.01× in this run, though no copy is written: each
    copy's page-table walk (50–70 ms at 7k) took the cache write's
    place. The
    scanned's crop (1,461 [2,256]), Rewrite (934 [1,201]) and Bates
    apply (63.5 [428]) are faster, reading the blob instead of writing
    or reading 474 MB.
  - **The scanned's incremental save** read 822–891 ms over steps 5–8
    [742]; it writes the whole file (slice 10's gate is < 50 ms).
  - **The scanned's warm render** read 21.8–23.0 ms in steps 5–8
    [16.5]. It isn't this slice's: a probe renders the same page in
    23.2 ms through b840e10's sheaf-render on this machine now, and in
    23.3 ms through the slice's, whichever way its buffer is read.
  - **The writer's rows** (Rewrite, Bates' Rewrite, flatten) were
    1.10–1.17× in step 8 and are 0.98–1.02× on the synthetic and heavy
    now. It isn't `From<f32>`'s whole-number path (step 9): with it
    turned off, the heavy's Rewrite read 3,086 ms and flatten 3,577.
    Slice 2 measured these rows moving 0.8–1.2× with code placement
    alone.
- **`LazyPdf` alone** (step 2, a scratch program): the heavy 7k's
  eager load takes 205 MB in 155 ms, its lazy open 18 MB in 7 ms, and
  every object read lazily 187 MB (52,392 objects in 0.50 s, one
  thread); the synthetic 35 MB in 35 ms, 6 MB in 3.7 ms and 28 MB; the
  scanned's eager load 1,004 MB and its lazy open 477 MB (its bytes).
- **The app** (Xwayland, `--timing`, the dev profile; b840e10 against
  step 5): first pixels on the synthetic 7k 1.365–1.380 s before and
  0.510–0.529 s after (insert and save runs), the corpus f1040
  1.36–1.48 s and 1.35–1.41 s; inserting f1040s1 into the 7k 1.0 s
  and 41–45 ms. The walk takes 121 ms in the dev profile at 7k. A
  scripted crop of f1040 said "Cropped 1 page", the preview changed
  shape (320×414 to 320×434) as it appeared, and the in-place save, a
  Rewrite, kept it. The f1040 and synthetic 7k selftests (insert and
  save) passed again after step 6.
- **The corpus** (`../pdf-corpus`, 4,392 manifest files):
  - **Lazy against eager:** 4,389 files and 2,989,321 objects, each
    read from memory and through `FileBytes`, both ways; 573 decrypted
    with the empty password, 30 rebuilt, none needing a password, and
    3 that neither reader loads (1252465, 2615560, 6220579). No object
    reads only lazily. 12 files differ, each
    by its listed count and kind: Unlisted, eager keeping object-stream
    members the table doesn't list (0845477 70, 1156137 1, 1530261 86,
    2280585 80, 3401937 6); Shadowed, eager keeping an earlier
    revision's Info that another entry points at, the one object that
    differs being the trailer's `/Info` (0054508, 0078699, 0155165,
    0270171, 0372984); Undecryptable (0169467's XMP in the clear though
    `/EncryptMetadata` is true, and 1263925's `/SigRef /DigestValue`).
    Before the shared fixes: 4,377 files, 28 rebuilt, 7 "needing a
    password" (each `/Encrypt null`) and 8 unloadable; at step 8, 4,386
    files, 27 rebuilt and 6 unloadable, before the stream trailer.
  - **Encryption:** 53 streams in 29 encrypted forms read otherwise
    once stream dictionaries are walked (17 `/Indexed` palettes, 31
    `/PieceInfo` and `/LastModified` dates, 3 embedded files'
    `/Params`, 2 `/ADBE_FillSign`), left as ciphertext before in eager
    loads and in what the old writers wrote.
  - **Geom against PDFium:** 4,388 files PDFium opens, every one with
    a source (4,385 at step 8), 24,251 pages, no count and no size
    differences (exact f32). What the rules met
    (24,253 pages lopdf lists): 678 inherit `/MediaBox`, 5 `/CropBox`
    and 88 `/Rotate`; 426 crop boxes differ from the media box, 8
    reaching past it; 107 quarter turns, and 66 negative or ≥ 360. No
    page has a crop outside its media box, an off-quarter `/Rotate`,
    an empty media box or a `/UserUnit`.
  - **Forms:** a dump of every file's fields and widgets, hashed, is
    identical before and after but 1263925 (signed 1 → 0) and the 6
    unloadable files' error text, and every widget's page index is
    unchanged. `forms_corpus`: 4,392 files, 4,388 opened, 4,389
    checked, 3,511 clean; 4,362 read, 4,349 as their manifest rows, 1
    damaged, 0 unread, 12 differing, all as before but the 3 forms the
    stream trailer opens: they check clean, and 1819139 reads as its
    row (61 fields), where it was the one unread (the other two are
    `unparsed` rows). Step 8's note had 4,386 checked, 3,508 clean,
    4,361 read, 4,348 as their rows and 1 unread; its "11 differing"
    was 12.
    `corpus_hybrid_reference_files_read_their_newest_revision`: 172
    files, none differ; `corpus_hybrid_forms_bind_their_xfa`: 71.
  - **Properties:** 584 of 4,392 read otherwise at step 7: 573
    encrypted files now show their method and permissions, 5 Shadowed
    ones their newest `/Info`, and 6 fail both ways, 3 of which read
    now through the stream trailer. Pages, fonts, version, tags
    and Fast Web View are identical on all 4,386 readable files.
  - **The two pool files lopdf couldn't load** (slice 2): `0676477`
    opens with 47 pages and `2289457` with 1 (linearized, 2
    sections), as PDFium counts them, through the Store and eagerly.
  - **Times** (the test profile, optimized): lazy against eager
    12.7 s, Geom against PDFium 4.5 s, `forms_corpus` 13.0 s,
    sheaf-doc's unit tests, the corpus's among them, 13.4 s.
- **The review's hostile shapes** (step 9, its scratch probes, release,
  peak RSS including building the file), before and after:
  - every page's `/MediaBox` one array that never closes: 20,000 pages
    of 20,000 numbers (1.96 MB) opened in 49.4 s, now 106 ms (the eager
    load 53 ms); 1,000 pages of a million (2.09 MB) in 214.6 s, now
    241 ms;
  - 1,000 pages naming one `/Annots` array of 100,000 references
    (691 KB): 7.6 s and 11.7 GB, now 32 ms and 28 MB;
  - a node listed 300 times, listing its page 300 times, whose own
    `/Annots` has 1,000 entries (10 KB): 6.6 s and 10.6 GB, now 32 ms
    and 34 MB, 90,000 pages;
  - a cross-reference stream of 50 million entries (146 KB):
    `Limit(Objects)` after 6.6 s and 1.56 GB, now after 0.14 ms, before
    it decompresses; four such sections, 40.4 s and 5.8 GB, now
    0.5 ms; 20 sections relisting 8 million numbers (470 KB) opened
    after 30.2 s, and now fail after 2.45 s, the two sections the
    budget allows decoded;
  - a 50 MB file whose only section is at its start, `startxref` 5
    bytes off, read through a counting `ByteSource` with a 4 KiB first
    window: 114 MB read, now under 0.1 MB.
  - `From<f32>`, a million conversions: fractions 202 ms (the heap
    allocation 10–15% of it), whole numbers 96 ms, now 7 ms.
- **f64 reals** (step 1, a scratch program writing the every_kind
  fixture with 17 kinds of marks, and what's made from it): the
  cropped copy, both watermark copies, the blank page and the fixture
  are byte-identical; the rewrites differ only in the Measure
  annotation's `/C` (9 bytes, and the offsets after it); the
  incremental outputs write 161 numbers each in shorter digits, every
  one equal as an f32, 533 bytes smaller. lopdf's own round trip of
  the corpus: 578 of 4,392 outputs differ, only in reals (narrowed to
  f32 all 4,392 match), and 55,861 of their 476,763 reals now keep
  digits f32 dropped (595.32001 came back as 595.32).
- **Fuzzing** (nightly, ASan, the dictionary, seeded with 158 small
  corpus files kept outside the repo): in step 4, 180 s a target,
  5,904 runs of `lazy_reader` and 7,214 of `page_tree`; at step 8,
  120 s each from the grown seeds, 3,191 runs (coverage 5,270) and
  4,048 (6,988), peak RSS 882 and 873 MB; at step 9, 150 s each from
  those and the six fixtures, 3,374 runs (5,632) and 4,327 (7,445),
  885 and 861 MB. No crash. The stable test's 3,600 mutations take
  about 1.8 s.
- **Suites:** `cargo test --workspace` 594 passed, 17 ignored (554 at
  slice 2), in 67–89 s; lopdf's 321 tests pass in a checkout of
  39492cb with the patch, and clippy on lopdf itself is clean. The
  ledger walks 238 files in 1.1 s.

Found:
- **lopdf's rebuild couldn't find a trailer in a file with only
  cross-reference streams:** it looked for a `trailer` keyword, where
  PDFium takes the newest `/XRef` stream's dictionary. 3 corpus forms
  (1819139, 5208359, 6614490, damaged; two are linearized and 23 and
  35 bytes shorter than `/L`) opened in PDFium and in neither lopdf
  reader, and the app, which opens through the source, refused them
  where b840e10 showed them. Fixed in step 9 (deviations).
- **lopdf's eager load of a rebuilt file wasn't deterministic:** a
  member several object streams held came from whichever a thread
  read first. Fixed in step 9.
- **PDFium counts pages by the root's `/Count`** when it's in
  1..0xFFFFE, so a tree listing more showed pages the engine can't
  load. The table stops there now (deviations).
- **lopdf writes an integral real past i64's range** (1e20) as digits
  without a point, which its parser can't read back (an integer
  overflow, and `real()` needs the point). f32 reals had it too.
- **`Document::encrypt` then `save_modern` is wrong in lopdf:** object
  stream members are encrypted under their own ids inside a container
  that isn't encrypted. The fixtures use `save_to`. lopdf's V5 encode
  still leaves out `/Length` (slice 2).
- **PDFium and poppler ignore `/EFF`,** so an attachment-only file
  shows its attachment as ciphertext there, while lopdf, sheaf,
  Acrobat and qpdf decrypt it.
- **The size walk had been warming PDFium's page list** for the
  outline's destination lookups (above), and **a lazily read form
  costs more than an eager load** on the heavy (128–204 ms against
  40–158 ms for every object), its objects kept for the session.
- rustdoc warns about ambiguous links in `forms/mod.rs` and
  `field.rs` (`read` and `check` are each a function and a module) and
  broken links to the cfg-gated `fixture` and `corpus` modules; they
  predate the slice.

Checked by tests:
- every corpus object read lazily equals the eager load:
  `corpus_objects_read_lazily_equal_the_eager_load` (from memory and
  through `FileBytes`, both ways; the 12 listed files by kind and
  count, a Shadowed difference only the trailer's `/Info`), and on built
  files (every_kind, a classic table, object streams,
  hybrid_reference and hybrid_update, from memory and from a file,
  four threads at once) `lazy_objects_equal_the_eager_load`;
- the RC4-128, AES-128 and AES-256 fixtures:
  `rc4_128_aes_128_and_aes_256_decrypt_per_object_identically` (built
  in code: strings in page and stream dictionaries, content, XMP, an
  embedded file and a signature, written with a table and with a
  stream; no plaintext in the bytes but the signature's `/Contents`;
  every object equal, lazily and eagerly; no password is
  `InvalidPassword`), and `an_undefined_crypt_filter_is_a_typed_error`;
- the `/EFF` fixture:
  `an_attachment_only_eff_file_decrypts_its_attachment` (V4,
  `StmF`/`StrF` `/Identity`, `EFF /StdCF`, found through the name
  tree);
- the 100-byte prefix:
  `a_file_with_a_100_byte_prefix_reads_identically` (both xref kinds;
  spans shift by 100, `header_offset` is 100);
- fuzzing: `fuzz_bodies_survive_mutated_fixtures`, with `fuzz/`'s
  targets and `fuzz.yml`, seeded by `write_fuzz_seeds`;
- hostile files, each typed and within 5 s:
  `an_objstm_bomb_is_a_typed_error` (and a cross-reference stream
  bomb),
  `a_cross_reference_stream_bomb_is_refused_from_its_dictionary` (50
  million entries in one section or four, refused before they
  decompress, data that doesn't decompress alike; sections relisting
  one range within the chain's budget),
  `a_4096_section_chain_is_a_typed_error`,
  `a_10000_deep_reference_chain_is_a_typed_error` (by `dereference`
  and by `/Length`), `a_length_cycle_is_a_typed_error` (a 60-stream
  cycle, 64 threads); from `Store::source`, with no stack overflow,
  `cyclic_kids_are_a_typed_error`, `a_parent_loop_is_a_typed_error`,
  `a_10000_deep_page_tree_is_a_typed_error` (1,024 levels open, 1,025
  don't, a 10,000-long `/Parent` chain fails) and
  `too_many_pages_or_none_are_typed_errors` (`TooManyPages`, a DAG
  bomb, `NoPages` twice);
  `pages_share_what_they_hold_however_often_it_s_listed` (1,000 pages
  naming one 10,000-entry `/Annots`, and a DAG of 90,000 listings of
  one page, each holding one `Arc`), and
  `a_broken_object_every_page_names_is_read_once` (1,000 pages, an
  array of 300,000 numbers that never closes, the same error twice);
  `a_broken_xref_falls_back_to_an_eager_load` (with 0290608's shape,
  and a file of cross-reference streams only, whose trailer is its
  newest stream's dictionary);
  `a_slightly_wrong_startxref_reads_little_of_a_file` (under 64 KiB
  of 8 MB);
  `a_stream_whose_length_the_parser_cant_read_spans_to_endobj` (a
  chain and an integral real, and no span for a length that names
  nothing); and the 1,003-section
  `a_cross_reference_stream_many_sections_share_is_read_once`, now
  lazily too, under 1 s;
- Geom equals PDFium: `every_corpus_page_s_geom_is_pdfium_s_size`
  (sheaf-ops; every file PDFium opens opens as a source), and on built
  trees `inherited_boxes_and_rotation_read_as_pdfium_reads_them`,
  `a_page_listed_twice_is_two_pages`, `per_page_annots_are_listed` and
  `the_root_s_count_ends_the_table_as_it_ends_pdfium_s` (a `/Count`
  below the tree's, as a real and a reference, ones PDFium doesn't
  count by, and a cycle past it unmet);
  conformance: `conformance_reads_pdfa_and_pdfua_claims`;
- the geometry walk: gate_7k's walk rows assert (48.2 and 80.0 ms at
  the end);
- a forced collision: `a_forced_blob_collision_is_refused`, beside
  `interning_the_same_bytes_twice_gives_one_blob`,
  `the_store_counts_live_bytes_once_and_forgets_dead_blobs` and
  `a_source_is_parsed_once_while_a_src_lives`;
- two inserts of one file:
  `inserting_the_same_file_twice_gives_two_registrations_and_two_forms`
  (one Src under two SourceIds, two forms with fields renamed apart
  and their own `Field.source`, read from the Src);
- the save suites with f64: `reals_keep_their_digits` (whole numbers
  too), and the sheaf-doc and sheaf-ops save, kept, recovery and forms
  suites;
- freeing a scan: `pruning_an_undone_400_page_scan_frees_its_bytes`
  (the registration gone, `store.bytes()` down by the scan's length,
  `store.get(id)` None), `an_undone_insert_counts_its_source_until_redo`
  (the store's bytes against the primary's and the insert's lengths),
  `the_root_advancing_keeps_its_sources`,
  `a_pruned_save_s_file_goes_when_the_next_save_replaces_it`,
  `memory_budget_counts_undone_sources_but_not_blobs`,
  `a_job_that_nets_to_nothing_leaves_no_version`'s waiting
  registration, and the replay proptest's registration checks (64
  cases, 4,096 in release, the past-max run); seven mutations each
  failed one of them;
- the read-only paths:
  `a_page_listed_twice_has_its_widgets_at_both_indexes`,
  `reads_info_version_and_tags`,
  `lists_fonts_including_forms_and_inherited_ones`,
  `an_encrypted_file_reads_decrypted_with_its_permissions` (V5
  AES-256, the empty user password) and sheaf-ops'
  `an_encrypted_signed_file_asks_with_no_append`, which fails if
  `encrypted` goes back to the password;
- the ledger: `nothing_retired_is_back` with the 42 rows;
- mutations: leaving stream dictionaries out of the walk fails the
  RC4/AES test, putting the RC4 fallback back fails the `/EFF` and
  undefined-filter tests, and ignoring `/EFF` fails the `/EFF` test;
  not keeping failures, not sharing listings and arrays, dropping the
  stream cap and growing the section probe each fail their step 9
  test.

Left for later slices:
- **Slice 4:** open from the Source's facts. `read_open`'s marks,
  other tools' comments, labels, header/footer, attachments,
  collection, layers and outline styles, `describe`'s outline styles
  (`load_source`) and `attachments_panel::refresh_facts`' `facts_in`
  still load eagerly. The heavy's ready is 965–1,090 ms against the
  < 1 s gate, with the pieces measured above; the form read over the
  source (128–204 ms) stays, and `LazyPdf` keeps what it read for the
  session. The scanned's ready RSS (1,039 MB) is the 474 MB blob and
  the eager parse, which goes with `read_open`. Keep first paint O(1)
  after the source opens: the layout's sizes are the page table's,
  built by `Store::source` off the UI thread (47–49 ms release,
  121 ms dev, at 7k), so `open::start` must add no walk of its own
  before it. With the outline from the source, `PdfiumEngine::outline`
  no longer loads every page dictionary before it reads destinations
  (the transitional ledger's row, 3 to 4).
- **Slice 5:** `Geom::{to_view, from_view}`; `save::geom_of` and
  `file::page_geom` over `Geom`; exchange's `Geoms` still loads its
  sources eagerly for `page_geom`. A reader that must match PDFium's
  f32 arithmetic casts as it reads, since `as_float` is f64.
- **Slice 6:** stamps interned as blobs (cache stems today) and
  `recover_cached` gone; the content toolkit's blocks read streams
  through the source without memoizing them: `LazyPdf::get` keeps
  every object it reads, and `raw_span` parses the object again,
  copying its stream.
- **Slice 7:** the encrypt side: `serialize/crypt.rs` over
  `encrypt_object`, which already encrypts stream-dictionary strings,
  applies `/EFF` and keeps a signature's `/Contents` as stored;
  `Source::crypt()` and a `LazyPdf` accessor for its
  `EncryptionState` (an absent `StmF` or `StrF` is stored as
  Identity, so re-encoding writes `/StmF /Identity`); the rewrite's
  `/Encrypt` strip (bug 4's other half); the header offset (bug 5)
  from `XrefFacts.header_offset`.
- **Slice 8:** `BlobRef` in records, which completes slice 3's
  "`Store`, `Blob`, `BlobRef` and `Src`" (deviations), and attachments
  as blobs (`BlobKind::Attachment`) rather than cache stems.
- **Slice 9:** the `Objects` seam (`attachments::Objects`, the generic
  `resolve` and `xfa::read`) and `forms::read::fields_of` go with the
  old writers, and `load_source` with them.
- **Slice 10:** the removal check reads a source by position
  (`FileBytes`); an Append needs `base_sound()`, numbers its objects
  above `XrefFacts.size` and every xref stream's number
  (`max_stream`, slice 2's 5801985), and takes `/Prev` from
  `XrefFacts.start`. Decide how an unreadable signature counts
  (1263925).
- **Slice 11:** `SourceId = BlobId`, deleting the `SourceId(u32)`
  registrations (`SourceInfo`, `next_source`, `unregister`);
  `BlobId`'s `b<hex>-<len>` serde form for Op JSON; `Tx::pin` holding
  `BlobRef`s.
- **Slice 12:** prune by bytes, `step_bytes` and `blob_bytes`, so an
  undone large insert stops making `MemoryBudget` drop every snapshot
  until its branch goes by count.
- **Slice 13:** the engine pool frees bytes: `Doc.handles` keeps an
  engine per registration for the session, holding the blob's
  `Arc<[u8]>` after pruning frees the registration. The layers
  panel's temp file is gone; its re-saved copy is still decrypted, in
  memory.
- **Slice 16:** `conformance()`'s first consumer, the PDF/A note.
- **Slice 17:** `MemoryKind::Derived` and the `derived` record go with
  derived sources.
- **Slice 19:** the generated cache: blank pages, images and Combine's
  image PDFs are read from it into `Origin::File` blobs today, and
  become memory sources when generated.rs goes.
- **Any slice:** PDFium's count by an inner node's `/Count` when the
  root's isn't usable, if a damaged tree needs it; upstreaming the
  lopdf patch (SHEAF.md).
