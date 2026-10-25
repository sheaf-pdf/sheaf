# M10.5 §3: The content model

Part of [M10.5: the design](README.md).


#### 3.1 The Store, blobs and sources

```rust
// sheaf-edit/src/ids.rs, re-exported by sheaf-doc (slice 3 starts it in sheaf-doc/src/store.rs; slice 11 moves it, §6.1)
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct BlobId { pub hash: u128 /* xxh3-128 */, pub len: u64 }      // serde: "b<32 hex>-<len>" (JSON-safe)
pub type SourceId = BlobId;                                             // a source IS a PDF blob

// sheaf-doc/src/store.rs
pub struct Store { blobs: RwLock<HashMap<BlobId, Weak<BlobInner>>> }   // per Document; outside undo
#[derive(Clone)] pub struct Blob(Arc<BlobInner>);
struct BlobInner { id: BlobId, bytes: Arc<[u8]>, kind: BlobKind, pdf: Mutex<Weak<Source>> }
pub enum BlobKind { Pdf { password: Option<Zeroizing<String>>, origin: Origin }, Image(ImageFormat),
                    Attachment { mime: Option<Arc<str>> } }
pub enum Origin { File(Arc<Path>), Memory(MemoryKind) }     // where it was FIRST interned: a hint, never a display name
pub enum MemoryKind { Blank, Images, Scan, Recovered, Library, Summary, Derived /* transitional, gone in slice 17 */ }

/// What records hold: pins the blob; Eq/Ord/Hash by id; serializes as its BlobId.
#[derive(Clone)] pub struct BlobRef(Blob);
/// A parsed PDF source: pins its Blob; Eq/Ord/Hash by id; serializes as its SourceId.
#[derive(Clone)] pub struct Src(Arc<Source>);

impl Store {
    /// On an id hit the bytes are COMPARED; a mismatch is Err(BlobCollision).
    pub fn intern(&self, bytes: Arc<[u8]>, kind: BlobKind) -> Result<Blob, BlobCollision>;
    pub fn get(&self, id: BlobId) -> Option<Blob>;           // None once nothing holds it
    pub fn source(&self, b: &Blob) -> Result<Src, OpenError>; // parses lazily; re-parses only if every Src died
}
```

A **blob** is a byte string the document uses (a PDF, an image, an
attached file), named by its content hash. A **source** is a PDF blob,
parsed.

**Liveness is the Arc.**
- A blob lives while a `Blob`, `BlobRef` or `Src` exists anywhere. That
  covers a kept snapshot, a step's prims, an open `Tx` (which pins the
  blobs its caller interned with `Tx::pin`), an engine, and the app's
  clipboard blob cache (§14.1).
- The `Store` keeps only `Weak` references, so there is no sweep to
  forget.
- `Source` holds its `Blob`, and `BlobInner` holds only a
  `Weak<Source>`, so there is no cycle.
- `HeapSize` counts each blob once, by id.

**Interning is outside undo** ("deleting a shape never deletes its
asset", tldraw's rule). Callers intern off-thread before lowering. Op
JSON names blobs with `BlobSel` (§6.6), which a resolve step turns into
a `BlobRef` before lowering. Lowering never sees bytes.

**One source per distinct file.** Inserting the same bytes twice gives
one `Source`. Identity on the page side comes from `PageDef.copy`, and
on the form side from `FormInst` (§3.7), never from the source. Display
names (rename stems, notices) come from the op's
`PageSource::File { name }`, the name the user picked, never from the
blob's first `Origin`. They are kept per form instance in
`FormDoc.names` (§3.7).

```rust
// sheaf-doc/src/source/
pub struct Source { blob: Blob, crypt: Option<Arc<Crypt>>, pdf: LazyPdf, facts: Facts }
impl Source {
    pub fn id(&self) -> SourceId;
    pub fn base_sound(&self) -> bool;                                 // false when lopdf rebuilt the xref
    pub fn xref(&self) -> &XrefFacts;                                 // kind, start, size, /ID, header offset, hybrid, linearized
    pub fn crypt(&self) -> Option<&Arc<Crypt>>;                       // StmF, StrF, EFF, key, /ID: kept, never stripped
    pub fn object(&self, id: ObjectId) -> Result<Arc<Object>>;
    pub fn raw_span(&self, id: ObjectId) -> Option<Range<usize>>;    // absolute in the buffer; None for ObjStm members
    pub fn page_count(&self) -> u32;
    pub fn page(&self, i: u32) -> Result<Arc<PageFacts>>;            // ObjectId, parent chain, inherited attrs, Geom, /Contents,
                                                                      // /Annots, /StructParents, repeat index
    pub fn annots(&self, i: u32) -> Result<Arc<[BaseAnnot]>>;        // mark (v2/v1/stale), foreign, link, widget
    pub fn blocks(&self, i: u32) -> Result<Arc<PageBlocks>>;         // lexical classification (§13); never at open
    pub fn form(&self) -> Result<Arc<SourceForm>>;                   // forms::read + node ids + /DR handles + /CO + XFA facts
    pub fn outline(&self) -> Result<Arc<[BaseBookmark]>>;
    pub fn labels(&self) -> Result<Option<Arc<[BaseLabel]>>>;
    pub fn generators(&self) -> Result<Arc<[Generator]>>;            // /SheafGenerators + /SheafBlock stream markers
    pub fn signatures(&self) -> Result<Arc<[SigFact]>>;              // field, byte range, DocMDP P, FieldMDP, UR3 TransformParams
    pub fn revisions(&self) -> Result<Arc<[RevisionInfo]>>;          // the /Prev chain (forms' EarlierValues)
    pub fn conformance(&self) -> Result<Conformance>;                // XMP pdfaid / pdfuaid claims
}
```

**`LazyPdf`** is a patch to vendored lopdf.
- It keeps the parsed xref and trailer, and serves `get(id)` through
  `Reader::get_object`, with cycle detection, per-object decryption
  (strings in stream dicts included, which closes lopdf #603) and the
  `object_streams` cache.
- It honors `/EFF` for `/Type /EmbeddedFile` streams.
- It records the header offset, and `raw_span` and xref offsets are
  translated by it.
- It runs over a byte source: an in-memory `Arc<[u8]>`, or a
  positional-read file handle. The positional handle is used only to
  read a saved file's old objects for the removal check (§11.9).
- One `Limits` value covers every reader: lazy objects, the page tree,
  forms, the content lexer and JSON.
- When lopdf has to rebuild the xref, the file falls back to an eager
  load with `base_sound == false`. Such a file is never appended to.
- `Object::Real` becomes `f64` (about 10 vendor sites and 46 sheaf
  sites).

**Facts are never history.** Whichever thread asks first computes a
fact. Expensive facts, such as classifying the content of 7,000 pages,
or an inserted file's annotations and form, are forced off the UI
thread: in the resolve phase of a job, in a background dialog preview,
or in the save planner.

**Open never lexes content.** `generators()` reads `/SheafGenerators`
and the `/SheafBlock` keys in content-stream *dicts* only, with no
decompression. Blocks that another tool merged into other streams are
found later, by the `blocks` fact.

#### 3.2 DocState

```rust
#[derive(Clone)]
pub struct DocState {
    order: im::Vector<PageId>,                                  // Key::Order
    pages: im::HashMap<PageId, Arc<PageDef>>,                   // Key::Page(id)
    geoms: im::HashMap<PageId, Arc<Geom>>,                      // Key::Geom(id)
    page_annots: im::HashMap<PageId, im::Vector<AnnotId>>,      // Key::PageAnnots(id): /Annots order
    annots: im::HashMap<AnnotId, AnnotRec>,                     // Key::Annot(id): marks, foreign, links, widgets
    fields: im::OrdMap<FieldId, Arc<FieldNode>>,                // Key::Field(id): every node of the tree
    form: Arc<FormDoc>,                                         // Key::Form
    bookmarks: im::HashMap<OutlineId, Arc<Bookmark>>,           // Key::Bookmark(id)
    outline_roots: im::Vector<OutlineId>,                       // Key::OutlineRoots
    labels: Option<Arc<[LabelSpec]>>,                           // Key::Labels
    generators: im::OrdMap<GenId, Arc<Generator>>,              // Key::Generator(id)
    idx: Indexes,   // derived, not part of Eq: position map per `order` Arc; /NM → AnnotId; full name → FieldId
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Key { Order, Page(PageId), Geom(PageId), PageAnnots(PageId), Annot(AnnotId), Field(FieldId),
               Form, Bookmark(OutlineId), OutlineRoots, Labels, Generator(GenId) }
```

**Why these keys.** A key is the unit of value validation, dirtiness and
invalidation.
- `Geom` is separate from `Page`, so a crop or rotate during an OCR run
  doesn't touch what the OCR flush validated.
- `PageAnnots` is separate from `Page`.
- Bookmarks and field nodes are keyed per item.
- `Labels` and `Form` are single records, edited as wholes.
- `Order` and `OutlineRoots` are single values. Work that only needs
  "insert somewhere" commits with `Relower` (§8.5) instead of
  conflicting on them.

**Construction and access.**
- DocState is built only through `DocState::builder()` (open,
  `ExportTx`) and `apply(&Prim)`.
- Reads go through accessors: `page_count`, `page_at`, `pos_of(id)`
  (O(1) after one O(n) build per `order` Arc), `def`, `geom`,
  `annots_on`, `annot`, `annot_by_nm(page, nm)`, `field`, `field_named`,
  `bookmarks_preorder` and `generators_on(page)`.
- About 160 sites read today's fields directly (`state.pages` 96,
  `outline` 25, `text_layers` 10, and about 30 others). They move to the
  accessors.

#### 3.3 Pages: `PageDef` and `Geom`

```rust
pub struct PageDef {
    pub base: PageBase,                   // blank/image pages are memory sources (§3.9)
    pub copy: u32,                        // 0 = the base page object itself; n ≥ 1 = the nth private copy
    pub content: Arc<ContentStack>,       // ContentStack::EMPTY (shared Arc) when untouched
    digest: OnceLock<u128>,
}
pub struct PageBase { pub src: Src, pub index: u32 /* 0-based, in src */ }
pub struct Geom {                         // PageSpace; crop clipped to media; a crop outside media is ignored
    pub media: Rect, pub crop: Rect, pub trim: Option<Rect>, pub bleed: Option<Rect>, pub art: Option<Rect>,
    pub rotate: Rotation,                 // ABSOLUTE, as it will be written
    pub user_unit: f32,
}
```

**An untouched page costs almost nothing.**
- Its `PageDef`, `Geom` and annotation list are `ptr_eq` to the root's,
  and its content is the shared `EMPTY`.
- The emitter writes nothing for it.
- The realizer renders it from the base engine.
- It takes about 200 bytes per page, about 1.4 MB at 7,000 pages, shared
  by every version.

**The copy index is fixed at creation.** Copy 0 always owns the base
page object. If copy 0 is deleted, copy 1 is still written as a new
object of its own. Moving pages never changes `/P`, links or the object
map. A page tree that lists one page object twice (invalid, but real)
opens as copy 0 and copy 1, with a notice.

**Other page-dict keys.** `/Tabs`, and other keys sheaf may set later,
arrive with their first producer (M10 slice 10). They come as
`KeyPatch(OrdMap<Name, Option<PatchValue>>)`, whose values are symbolic
(`PatchValue` references are `ObjRef | PageId | BlobRef | FormRef`,
never raw object numbers), plus a classify role row per patched key
(§20). No patch type ships in M10.5, because nothing would produce it.

#### 3.4 The content stack

A **content stack** is a page's content as a definition: filters applied
to the base page's `/Contents`, and layers drawn under and over it.

```rust
pub struct ContentStack {
    pub filters: im::Vector<Filter>,     // applied to the base /Contents, in order
    pub under: im::Vector<Layer>,        // drawn before the base, first = bottom
    pub over: im::Vector<Layer>,         // drawn after the base, isolated from it by q/Q
}
pub struct Layer { pub id: BlockId, pub kind: BlockKind, pub body: Static }
pub enum BlockKind { Ocr, Flattened }
pub enum Static {
    OcrText(Arc<TextLayer>),             // invisible text (Tr 3, one glyphless Type0 per file); words in PageSpace
    Placements(Arc<[Placement]>),        // Form XObjects placed by BBox/Matrix/Rect (flatten)
}
pub struct Placement { pub form: FormRef, pub matrix: Affine, pub tag: PlaceTag /* Artifact | Untagged */ }
pub enum FormRef {
    Base(ObjRef),                        // an appearance stream already in a source
    Blob(BlobRef),                       // a one-page PDF blob: its page as a form (library stamps)
    Pinned(Arc<PinnedAp>),               // a derived appearance, its inputs captured when flattened
}
pub struct PinnedAp { pub inputs: ApInputs /* the Mark; or Widget + terminal value/display + /AS + FormDoc /DA /Q /DR
                                               handles */, pub digest: u128 }
pub enum Filter {
    DropMarked(MarkedSel),               // remove marked-content ranges of the base (lexical splice, §13)
    DropStreams(StreamSel),              // drop whole base /Contents entries
    StripLegacy(LegacySel),              // RemovePrivateData on legacy sheaf content: drop private keys from inline
                                         // property lists; rename private resource names used by Tf/Do operands
}
pub enum LegacySel { Marked(MarkedSel), Stream(StreamSel) }
pub enum MarkedSel {
    Pagination { kinds: EnumSet<PaginationKind>, origins: EnumSet<PagOrigin> },
    SheafGen(GenId), SheafOcr,           // sheaf blocks inside streams another tool merged
}
pub enum PaginationKind { Header, Footer, Watermark, Background }
pub enum PagOrigin { Sheaf, SheafLegacy, Acrobat, Authored }      // §13 classifies
pub enum StreamSel { SheafBlock(BlockId), SheafGen(GenId), SheafOcr /* + legacy /SheafOcrLayer and its 'q' */, Isolation }
```

**A stack holds definitions, not bytes.**
- The content writer (§13) derives layer bytes deterministically and
  caches them by input digest.
- Additions never re-encode the original content, so an Append is still
  possible.
- `Pinned` captures the inputs, not the bytes. The emitter re-derives
  the identical form from the layer alone, so a flattened appearance
  never depends on a deleted annotation or on a cache.
- `check_invariants` asserts that no `AnnotId` or `FieldId` is
  referenced from a ContentStack.

**Drawing order is fixed:**

```
generators(Under) → static under → QOpen^open → base′ → QClose^close → static over → generators(Over)
open  = 1 + d          close = 1 + d + net          d = max(0, −min_prefix)
```

`Balance { net, min_prefix }` is base′'s lexical q/Q balance (§13): the
net count, and the lowest the running count goes. Some worked bases:

| Base | net | min_prefix | open | close |
|---|---|---|---|---|
| `q … cm` (no Q) | +1 | 0 | 1 | 2 |
| `Q q` | 0 | −1 | 2 | 2 |
| `Q` | −1 | −1 | 2 | 1 |

A base that can't be lexed fails the op that needs isolation. There is
no "unknown" count.

Isolation streams are written once per materialization, and each is
marked (`/SheafBlock << /K /Isolate >>`, §11.4). Before reassembling,
the emitter sets aside sheaf's own isolation streams and blocks from the
base, so nesting never grows across save, reopen and edit cycles.

**Growth.** M11 adds `ContentStack.rewrite`, "the new base", which
absorbs the filters present when it is set; later filters apply to its
output. It also adds `Static::Ops`, a definition evaluated per page at
emit (§20). Neither exists in M10.5.

#### 3.5 Generators: parametric, document-level content

A **generator** is content defined once for the document and laid out
per page at emit time: the header/footer (with Bates) and the watermark
or background.

```rust
pub struct Generator {
    pub id: GenId,                                   // random < 2^53; written as a PDF string
    pub spec: GenSpec,                               // HeaderFooter(Arc<HeaderFooterSpec>) | Watermark(Arc<WatermarkSpec>)
    pub z: Z,                                        // Under | Over  (background = watermark Under)
    pub scope: Scope,                                // All (includes pages added later) | Pages(im::OrdSet<PageId>)
    pub frozen: Frozen,                              // decided at apply: date and file tokens; Bates numbers
    pub replaces: EnumSet<PaginationKind>,           // Acrobat-ADDED artifacts of these kinds on covered pages are
                                                     // dropped at emit (never authored ones)
}
pub struct Frozen { pub date: Arc<str>, pub file: Arc<str>, pub bates: Option<Arc<im::OrdMap<PageId, u64>>> }
pub struct HeaderFooterSpec { pub texts: [Arc<str>; 6], pub font: FontChoice, pub size: f32, pub color: Color,
                              pub margins: Margins, pub first_number: u32, pub bates: Option<BatesSpec> }
pub struct BatesSpec { pub prefix: Arc<str>, pub suffix: Arc<str>, pub digits: u8, pub start: u64 }
pub struct WatermarkSpec { pub content: WatermarkContent, pub position: GridPos /* 3×3 grid, displayed frame */ }
pub enum WatermarkContent {
    Text { text: Arc<str>, font: FontChoice, size: f32, color: Color, opacity: f32, angle: f32 },
    Image { image: ImageRef, scale: f32, opacity: f32 },
    Page { page: PageForm, scale: f32, opacity: f32 },
}
pub enum ImageRef { Blob(BlobRef), Placed(ObjRef) }        // Placed: the XObject a saved block draws (rebound at open)
pub enum PageForm { Blob { pdf: BlobRef, index: u32 }, Placed(ObjRef) }
impl Generator {
    pub fn needs(&self) -> CtxNeeds;                          // which PageCtx fields its tokens read
    pub fn layout(&self, page: &PageCtx) -> Drawn;            // pure, deterministic; may be empty
    pub fn recipe_digest(&self, page: PageId) -> u128;        // spec + z + this page's frozen values
}
pub struct PageCtx { pub geom: Geom, pub position: u32, pub count: u32, pub label: Arc<str>, pub bates: Option<u64> }
pub struct Drawn { pub ops: Arc<[u8]>, pub res: Arc<[(Name, Res)]>, pub layout_digest: u128 }
```

**Live and frozen tokens** (proposal 8).
- Page-number, count and label tokens (`<<1>>`, `<<N>>`, `<<label>>`)
  are **live**: they are evaluated against the document being written,
  whether that is the binder or an extract, and against the binder for a
  print branch, which prints pages of the document ("Page 5 of 10", as
  print preview shows it: Greg, 2026-10-22; `KeepPages`' `numbering`,
  §12.3).
- Bates numbers, `<<date>>` and `<<file>>` are **frozen at apply**.
  Reapplying the header is how to renumber.
- `needs()` says which `PageCtx` fields a spec's tokens read. A
  watermark without tokens needs only geometry, so a reorder neither
  re-renders nor re-emits it.

**Orientation.** Layout happens in the page's displayed frame (the
visible box and absolute `/Rotate`) at emit time. Headers and watermarks
stay upright after a rotate, and a dialog's preview equals the result.

**Recognition after a save or reopen.**
- Each drawn block is its own stream. Its dict carries
  `/SheafBlock << /K /Gen /G (g<hex>) /L <layout hex> /R <recipe hex> /N (000123) >>`
  (§11.4 has the dict for every kind of block). `/G`, `/L`, `/R` and
  `/N` are strings, so no integer goes past ISO 32000-1 Annex C's ±2³¹
  and a PDFium or Acrobat re-save can't round them. `/N` is present only
  with Bates.
- Identical layouts share one block stream: a block is keyed by its
  layout digest.
- The content is wrapped as `/Artifact /ShP<n> BDC … EMC`. `/ShP<n>` is
  a **named** property list in the page's `/Resources /Properties`:
  `<< /Type /Pagination /Subtype /Header|/Footer|/Watermark|/Background /SheafGen (g<hex>) >>`.
  Acrobat, pdf.js and sheaf's classifier recognize it even after another
  tool merges `/Contents`, and `RemovePrivateData` strips `/SheafGen`
  with a dict edit, never a content rewrite.
- A covered page with nothing to draw gets an empty marked block, so a
  `Pages` scope rebuilds exactly.
- On open, `Source::generators()` reads the specs from
  `/SheafGenerators` (scope `all` or `pages`). It rebuilds `Pages`
  scopes and the frozen Bates map from each page's block (`/N`), keyed
  by that session's PageIds.
- It rebinds `ImageRef::Placed` and `PageForm::Placed` to the XObject in
  the recognized block's resources, never to object numbers in the JSON.
- If another tool merged a block's stream, its Bates number can't be
  recovered. The plan phase then reports it, and Update re-lays that
  page from the spec, with a notice.

**The emitter compares before writing.**
- If a page's layout digest equals the base block's `/L`, the base
  stream is referenced and nothing is written.
- If it differs, the old block stream is set aside and the new one
  drawn: a reorder that changed "Page n of N", a crop that moved the
  edge, or a rotate that turned the frame.
- If the recipe digest `/R` is unchanged, that difference is a live
  redraw (`LIVE_REDRAW`, §7.1).

**Unbacked blocks are kept.** A sheaf block in the base that no
generator in the model backs is kept as foreign content.
`RemoveHeaderFooter` and `RemoveWatermark` remove it through explicit
filters.

**Unencodable text** draws `'?'` with a one-time notice, for marks and
generators alike, until M10 slice 7's fallback font (§13). Emit is
total: a later label change that makes a live token unencodable can't
make the document unsaveable.

**Scope.** `Scope::All` covers pages inserted later. `Pages(OrdSet)`
gives an O(log n) `covers`.

#### 3.6 Annotations: one collection, one id space

```rust
pub struct AnnotRec { pub page: PageId, pub body: AnnotBody }
pub enum AnnotBody { Mark(Arc<Mark>), Foreign(Arc<Foreign>), Widget(Arc<Widget>) }
pub struct Mark { pub kind: AnnotKind, pub style: Style, pub contents: Arc<str>, pub author: Arc<str>,
                  pub created: PdfDate, pub modified: PdfDate,
                  pub nm: Arc<str> /* "sheaf:<id>" for marks sheaf made; the source's /NM for adopted and imported ones */,
                  pub reply_to: Option<AnnotId>, pub partner: Option<AnnotId>, pub turn: Rotation,
                  pub file: Option<BlobRef> /* attachment */, pub stamp: Option<FormRef> /* page stamp */,
                  pub src: Option<ObjRef> /* identity across saves; source keys kept (below) */ }
pub struct Foreign { pub src: ObjRef, pub copy: u32, pub subtype: Name, pub rect: Rect, pub flags: AnnotFlags,
                     pub nm: Option<Arc<str>>, pub reply_to: Option<AnnotId>, pub summary: Arc<ForeignSummary> }
pub struct Widget { pub field: FieldId, pub src: Option<ObjRef>, pub inst: FormInst,
                    pub merged: bool,      // its dict IS its terminal field's dict (one object)
                    pub rect: Rect, pub flags: AnnotFlags, pub mk: Arc<Mk>, pub da: Option<Arc<str>>,
                    pub state: Option<Name> /* /AS */, pub states: Arc<[Name]>, pub props: Arc<WidgetProps> }
pub struct WidgetProps { pub border: Option<Border>, pub actions: Arc<[(Trigger, Action)]>, pub has_appearance: bool }
```

A **mark** is an annotation sheaf made and owns. A **foreign**
annotation is one another tool made.

**Other tools' annotations are first-class records,** read into the root
at open.
- Deleting one is `Put Annot(id) → None`, and its replies cascade by id.
- Links are `Foreign` records with subtype Link. The emitter remaps or
  drops them (§11.3). A modeled `Link` body arrives with M13's first
  link-editing op.

**Widgets are annotations.** There is one `AnnotId` space, and `/Annots`
order is one `Vector<AnnotId>`. PDFium slots and the tab-order fallback
are derived from it. `WidgetProps` carries `/BS` or `/Border`, `/A` and
`/AA`, and whether an appearance exists. `Field::has_scripts`, M10 slice
6's borders and the action-driven slices read it.

**A mark with `src` keeps the source's other keys.**
- The emitter starts from the source dict and overwrites the keys sheaf
  owns for that kind:
  - `/Rect`, `/Contents`, `/C`, `/CA` and the geometry keys;
  - `/M`, `/T`, `/NM`, `/IRT` and `/RT`;
  - `/AP` and `/SheafAnnot`.
- It keeps `/DS`, `/RD`, `/IT`, `/Measure`, `/Popup` and unknown keys.
- When `/Contents` differs from the source's, it drops `/RC` (rich
  text), so Acrobat can't keep showing the old text and the old text
  doesn't survive in the new emission.
- Adopting another tool's comment (`AdoptAnnotations`) gives a mark with
  `src` under the same rule. It keeps the comment's `/NM`, so on reopen
  it is recognized as a mark by its `/SheafAnnot` v2, whose JSON carries
  its id, as v1's does.

**`/SheafAnnot` v2** holds the mark's JSON in PageSpace, with `"v":2`
and a staleness digest `"std"`.
- **The digest is canonical.** It covers the standard keys sheaf wrote,
  normalized:
  - numbers rounded to 1/64 pt and colors to 1/1000;
  - text strings decoded to NFC Unicode, whatever their encoding or
    encryption;
  - `/Rect` with its corners normalized.

  So a mere re-serialization by Acrobat or Preview (`100` → `100.0`, f32
  rounding, a UTF-16BE `/Contents`) keeps a sheaf mark a sheaf mark.
- **A mismatch on read** means another tool edited the mark after sheaf
  wrote it. The mark opens as **Foreign**: visible, deletable and
  adoptable. sheaf never reverts the other tool's edit.
- **v1 payloads** are converted on read (proposal 21). Geometry comes
  from the standard keys, which are already PageSpace; the JSON supplies
  only what PDF can't hold. Staleness compares the JSON's `modified`
  with `/M`.
- **Ticks** keep their identity in the v2 JSON (`"tick": id`). Their
  stamp `/Name` is `/Tick`, not `/SheafTick<id>`.
- **Duplicate `sheaf:<id>`.** `/NM` is unique only per page, and
  Acrobat's Insert Pages duplicates it. The first occurrence in document
  order keeps its id. Later ones get fresh `AnnotId`s and a new `/NM` on
  their next write, with one notice.

#### 3.7 Forms: three layers, explicit instances

1. **`SourceForm`** is an immutable fact per source: the `forms::read`
   output plus node ids, `/DR` font handles (`ObjRef`), `/CO`, XFA
   facts, earlier-revision values, signatures and template pages.
2. **`FieldNode` records** live in DocState. One `FieldId` space covers
   every node.
3. **Values** live inside those records.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct FormInst { pub src: SourceId, pub copy: u32 }   // copy 0 = the source's first instance (for the primary, its
                                                           // own objects in place); every other instance, and any
                                                           // instance of a non-primary source, is written as new objects
pub struct FieldNode {
    pub parent: Option<FieldId>, pub partial: Arc<str>,      // /T as it will be written; full names derived
    pub src: Option<ObjRef>, pub inst: FormInst,             // which instance of which source's form
    pub kids: im::Vector<FieldId>,
    pub term: Option<Arc<Terminal>>,
}
pub struct Terminal { pub kind: FieldKind, pub flags: FieldFlags, pub value: Value, pub default: Value,
                      pub display: Option<Arc<str>>, pub props: Arc<FieldProps>,
                      pub widgets: im::Vector<AnnotId>,
                      pub outside: u16 }   // widgets on page templates: not modeled, kept by keep_only, never cascaded
pub struct FormDoc { pub roots: im::Vector<FieldId>, pub co: im::Vector<FieldId>, pub da: Option<Arc<str>>,
                     pub q: Option<u8>, pub dr: Arc<[(Name, FontHandle)]>, pub need_appearances: bool,
                     pub sig_flags: u32, pub xfa: Option<XfaState>, pub scripts: ScriptState, pub damaged: bool,
                     pub names: im::OrdMap<FormInst, Arc<str>> }   // the file name each instance came from (the open's
                                                                   // for the primary, the op's `name` for inserts; a
                                                                   // duplicate inherits its original's): rename stems, notices
pub enum ScriptState { On, Off { instances: Arc<[FormInst]> } }   // off when scripts came from > 1 instance
```

A **form instance** (`FormInst`) is one copy of one source's form in the
document.

**A form instance is explicit.**
- `InsertPages`, `ReplacePages` and `DuplicatePages` create one new
  `FormInst` per op and source instance. Its `copy` is the next unused
  copy number for that source in the state.
- A file's non-contiguous page selection is one
  `InsertPages { pages: RangeSet }` (§6.1), so its widgets join one
  instance. Inserting the same bytes again is another op and another
  instance, renamed apart.
- `Join` (adding pages to an existing instance) has no M10.5 producer
  once page sets exist. It can be added later.

**Owned AcroForm keys** are `/Fields`, `/CO`, `/DA`, `/Q`, `/DR`,
`/NeedAppearances` and `/SigFlags`. They are always emitted from
`FormDoc` (§11.3). `/XFA` and `/Perms` are governed by save transforms
(§11.8).

**Page templates** (`/Names /Templates`) are pages outside the page
tree. They aren't modeled: `keep_only` keeps them, and their widgets
count in `Terminal.outside`. Spawning a page from a template arrives
with M10's script slices (`PageSource::Template`).

**M10 slices 4–5's mechanisms, re-expressed:**

| Today | Becomes |
|---|---|
| `Field.copy` | `FieldNode.inst` / `Widget.inst` |
| `Field.under` | a real parent node |
| `Document.forms`, `set_source_form` | lowering reads `Src::form()` |
| `take_in_fields`' join branch | one op per file selection (`pages: RangeSet`) |
| `name_apart`, `bring_fields`, `take_in_fields` | lowering helpers; renames recorded as prims; stem from `FormDoc.names` (the op's `name` for inserts) |
| `scripts_mixed`, `script_sources` | `FormDoc.scripts` |
| `form_notice`, `EditOutcome.forms_renamed/signatures_in` | `Notice::{FormRenamed, CopiesRenamed, SignatureNotValidHere, ScriptsOff}` (§6.5) |
| `signed_from`, `next_copy` | lowering helpers over `FormDoc` and `SourceForm` |

#### 3.8 Outline and labels

```rust
pub struct Bookmark { pub title: Arc<str>, pub target: Option<Target>, pub color: Option<Color>, pub style: TextStyle,
                      pub open: bool, pub kids: im::Vector<OutlineId>,
                      pub src: Option<ObjRef> /* original item: /A, /SE and unknown keys survive */ }
pub enum Target { Page { page: PageId, view: Option<DestView /* PageSpace */> }, Kept /* an /A or named dest as read */ }
pub struct LabelSpec { pub start: PageId, pub style: LabelStyle, pub prefix: Arc<str>, pub first: u32 }
```

Labels are read in full at open. An Append re-emits `/PageLabels`
whenever `Order` changes and labels exist (`emit_deps`, §7.2).

#### 3.9 Memory sources, cost and accounting

**Blank and image pages are memory sources.** Blank pages,
images-to-PDF, scans, placed library stamps and the comment summary are
small PDFs built in memory, interned as `Memory(kind)` blobs and
inserted with `InsertPages`. Nothing is written to a cache.

**Cost at 7,000 pages:**

| Operation | DocState cost | Append cost |
|---|---|---|
| Open | 7,000 PageDef + Geom (≈2 MB) | — (page tree walk, no content parsed) |
| Reorder all | one `Order` value | changed `/Pages` nodes, plus `/PageLabels` if labels exist, plus, for each live-token generator, every covered page whose layout changed (its block stream + page dict) |
| Rotate all | 7,000 `Geom` | 7,000 page dicts, plus generator blocks re-laid in the turned frame |
| Crop 10 pages | 10 `Geom` | 10 page dicts (+ their generator blocks if the visible edge moved) |
| Watermark all | one `Generator` | one block stream per distinct layout, one font/ExtGState/image per file, one Resources dict per distinct base Resources, 7,000 page dicts |
| OCR all | 7,000 PageDef with `Arc<TextLayer>` | one block per page, one glyphless font per file |
| Fill one of 20,000 fields (M10) | one `FieldNode` + widget | field dict + widget + AP (one object when merged) |

**Accounting (§8.7)** splits history memory into what trimming frees and
what only pruning frees:
- `snapshot_bytes` is the im nodes and indexes of a kept snapshot not
  shared with its neighbors.
- `step_bytes` is the deep bytes uniquely retained by a version's prims,
  Arc-deduplicated by pointer.
- `blob_bytes` counts each blob once.
- `HistoryStats` reports `snapshot_bytes`, `step_bytes`, `blob_bytes`,
  `undone_source_bytes` (reachable only from undone branches) and
  `engine_bytes`.
