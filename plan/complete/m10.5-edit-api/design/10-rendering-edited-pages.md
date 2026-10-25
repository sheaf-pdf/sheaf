# M10.5 §10: Rendering edited pages

Part of [M10.5: the design](README.md).


#### 10.1 The realizer

The **realizer** turns a page of the model into something PDFium can
render: the base engine for an untouched page, or an overlay for an
edited one.

```rust
pub struct ViewInputs {
    pub marks: MarkDraw,                          // ByHost (GPUI draws sheaf marks; PDFium must not) | ByEngine | Off
    pub generators: bool,                         // false for analysis (OCR, Trim White, blank detection)
    pub ocr_over: bool,                           // draw OCR text layers (they're invisible; matters to text reads)
    pub widgets: bool,
    pub hidden: Arc<im::HashSet<AnnotId>>,        // comment filter, Hide Comments
    pub dragging: Arc<im::HashSet<AnnotId>>,
    pub ocg_off: Option<Arc<OcgOverride>>,        // layers panel: one catalog-level patch per source
    pub form_draw: FormDraw,                      // Appearances | Live (FFLDraw) until M10 decides
}
impl ViewInputs {
    pub fn screen() -> Self;
    pub fn print(opts: &PrintOpts) -> Self;
    pub fn content_only() -> Self;                // base + filters + static under-layers over the whole MediaBox:
}                                                 // no generators, marks, widgets or OCR; its content reads Page(p) only
pub struct GroupKey { pub source: SourceId, pub chunk: u32 /* base index / GROUP */ }   // GROUP from slice 2's spike
pub struct PageView { pub engine: EngineKey, pub index: PageIndex, pub key: RenderKey, pub job: JobXform }
impl Realizer {
    pub fn view(&self, st: &DocState, page: PageId, inputs: &ViewInputs) -> Result<PageView>;
}
```

**Geometry-only pages need no overlay** if spike C passes (slice 2). A
page whose only change is its `Geom` renders from the base engine with a
job transform:
- the rotation delta;
- a matrix plus a clip, for a visible box inside the base's visible box.

A crop, a rotate or a whole-document crop of 7,000 pages then opens no
group engines. A geometry change that needs pixels outside the base's
box realizes like any other edit.

**An overlay is an in-memory append section over the source's own
bytes.** The same `emit` as saves produces it, with
`EmitScope::Pages(group pages)` and `RotatePolicy::KeepBase`: the
overlay page keeps the base's `/Rotate` and the job carries the delta,
so nothing is rotated twice. The same append serializer writes it.
- **Edited pages are replaced in place** under their own object numbers.
  Their page index, AcroForm, OCProperties, inherited attributes and
  shared resources keep their context.
- **Duplicates (`copy ≥ 1`)** are appended as extra page objects under
  one new kid of the root `/Pages`, inside their original's group
  section, with inheritable keys written explicitly (§11.6).

**Overlay assembly.**
- **One allocator per source,** session-wide, maps an `ObjKey` to a
  number above the source's `/Size`. A number is assigned on first
  emission and never reused.
- **Per-page deltas are cached by `RenderKey` as symbolic objects:**
  unencrypted, with shared keys (`Res`, `GenForm`, `Shared`, `PageRes`,
  the root `/Pages`, AcroForm) left as references.
- **A group's section** is the deduplicated union of its pages' deltas,
  plus one root `/Pages` and one AcroForm built for the group, plus a
  small xref. It is encrypted **at assembly** with the source's `Crypt`,
  since RC4 and AES-128 keys depend on the final number.
- **Cost.** An edit reopens only its group's engine, about 2 ms at 7,000
  pages (measured). Other pages keep their engines and warm caches.

**Other bases and patches:**
- **Untouched pages** render from the source's base engine.
- **A source-level view patch** (OCG visibility) is one catalog-only
  overlay that serves every untouched page of that source with one
  engine.
- **Loading** uses `FPDF_LoadCustomDocument` (pdfium-render's
  `load_pdf_from_reader`) over two segments, `[base Arc, section Arc]`,
  with no O(file) copy.
- **Encrypted bases (spike A).** Sections are encrypted with the source
  key. If PDFium rejects that, the fallback, for that source only, is a
  decrypted in-memory image, built lazily and dropped with the source.
  Nothing reaches disk.
- **Damaged bases, and any base class spike B fails,** realize as a
  standalone in-memory PDF per group.

**Rejected:**
- PDFium's `GenerateContent`: lossy, about 95 ms per change, and it
  mutates the live document.
- Per-page scratch PDFs.
- Per-source generations.
- Per-page overlays.

#### 10.2 Render keys

```rust
pub struct RenderKey(u128);  // xxh3 over: base (source, index); PageDef digest minus `copy`; Geom digest minus rotate
                             // (box-only edits rendered by job transform contribute their box instead); digests of the
                             // annotations the realization DRAWS under these inputs (others' annotations, widgets; sheaf
                             // marks only under ByEngine); widget and field digests on the page (+ FormDoc digest if
                             // any widget); per covering generator: (id, recipe digest, the PageCtx fields it needs());
                             // the ViewInputs digest restricted to what the realization draws on this page
```

- Digests are cached per Arc (`OnceLock<u128>`), so a key costs
  O(annotations on the page) hash combines.
- **What it keys:** tiles by `(RenderKey, Job)`, previews by
  `(RenderKey, width, rotation)`, chars and search hits by `RenderKey`.
- **Consequences:**
  - Duplicates share tiles, and an undo back to an identical Arc hits
    the cache.
  - A content edit that keeps the source changes the key.
  - A save changes nothing, so tiles survive.
  - Dragging a sheaf mark under `ByHost` changes no key.
  - A reorder changes only the keys of pages covered by generators whose
    tokens need position or count.
- **Deleted:** `touched()`'s per-edit layout clear, `drop_stale_images`
  and the global `invalidate_tiles`. Layout rebuilds only on
  `ChangeSet.order` or `geometry`, from the first changed position.

#### 10.3 The engine pool

- **One worker thread** in sheaf-render owns `EngineKey → PdfiumEngine`,
  where
  `EngineKey = Base(SourceId) | Group(GroupKey, section digest) | SourcePatch(SourceId, digest)`.
- **Budgets.**
  - Engines serving visible pages are pinned.
  - Others are LRU, capped by count and by bytes (PDFium's RSS per
    engine, measured in slice 2).
  - **Background consumers** (OCR, Trim White, plugin page scans) have
    their own small LRU budget, so they can't evict the screen's
    engines.
- **Request semantics are kept:**
  - priorities: 0 for previews, 1 for tiles, 2–3 for prefetch, `u8::MAX`
    for background;
  - coalescing by `(RenderKey, Job)`;
  - a dropped receiver skips its render;
  - size walks and search are chunked between renders.
- **Liveness.** The pool holds the bytes `Arc` for open engines and a
  `Weak` to the `Source`, and closes engines whose source is gone.
- **Deleted:** `RenderHandle`'s thread per source,
  `Renderer::hide_annots` and `set_annots_hidden`, and their replays
  (docview.rs:298, :1722, :1901; layers_panel.rs:110;
  comments_panel.rs:621).

#### 10.4 View inputs and marks on screen

- **Hiding is data.** A hidden annotation becomes `/F |= Hidden` in the
  overlay's copy of the dict, or is left out of the realized `/Annots`.
- **Each consumer passes its own inputs:**
  - the screen passes its filters;
  - print preview passes its options;
  - OCR, Trim White and blank detection pass `content_only()`, so an
    unsaved header or watermark is never read as page content;
  - plugin renders pass `ByEngine`, with an optional "without markup".
- **Sheaf marks stay drawn by GPUI in M10.5,** with `MarkDraw::ByHost`
  telling the realizer to leave them out. The switch to PDFium drawing
  their `/AP` lands in M10 slice 12. It deletes the GPUI mark painter
  (`annot_elements`, docview.rs:5329), `stamp_preview.rs` and the tape
  pictures.
- **Header/footer, watermarks, OCR and changed fields are drawn by
  PDFium** from the realization. The GPUI header/footer overlay
  (docview.rs:5196) is deleted, so an unsaved header now shows under
  view rotation too.

#### 10.5 The text service

```rust
pub trait TextService {
    fn chars(&self, snap: &Snapshot, page: PageId, o: TextOpts) -> Result<Arc<PageChars>>;  // PageSpace boxes, 1/16 pt
    fn search(&self, snap: &Snapshot, q: &Query, pages: PageSel, o: TextOpts, p: &Progress) -> Hits;
}
pub struct TextOpts { pub fields: bool /* merge widget display text (M10 slice 14); false for plugins until 0.5 */ }
```

- **Where text comes from.** Untouched pages are read from the base
  engine and cached per `(source, index)`. Edited pages are read from
  their realization and cached per `RenderKey`. The screen, the file and
  the text agree by construction.
- **Callers:** Find, Patterns, selection, calculator picks, plugin text
  and blank detection. They walk `DocState` order in `SEARCH_CHUNK`
  steps between renders.
- **What becomes searchable:** inserted, blank, cropped, duplicated and
  unsaved-OCR pages (M11 bug 6).
- **Deleted:**
  - `layer_hits`, `merge_source_hits` and `index_of_source_page`;
  - `CharsKey::{Engine, Layer}` and `PageChars::from_layer`;
  - `text_layer::search` and `pattern_search` as a parallel path;
  - the plugin API's OCR special cases.

#### 10.6 Nothing on disk

Every temporary file goes:
- the layers panel's decrypted temp copy, which becomes a view input;
- the recovery temp primary, which is now opened from memory;
- the print temp file: bytes go to GDI from memory, and CUPS gets `lp -`
  on stdin;
- the export temp file;
- stamp and tape pictures, which are rendered from a one-annotation
  in-memory document;
- the generated cache.

The cache directory holds no document content. Besides the user's files,
sheaf writes only the recovery sidecar, the short-lived append journal
and short-lived same-directory temp files for atomic writes (§11.7).
Outside document content, it also writes the plugin installer's unpack
folder and the instance socket. An attachment the user opens in an
external app is the one user-requested file, and M11 slice 1 deletes it
at close.
