# M10.5 §6: Ops and primitives

Part of [M10.5: the design](README.md).


#### 6.1 Ops: public, semantic, one schema

An **op** is a semantic edit as a caller states it ("crop pages 3–5").
Ops live in the new **`sheaf-edit`** crate. It holds types only (serde
and schemars, no other sheaf crate) and builds for wasm32-wasip2.
sheaf-doc, sheaf-render, sheaf-plugin and sheaf-plugin-api depend on it.

**The types ops carry live in sheaf-edit too,** since it can't depend on
sheaf-doc. Slice 11 moves them out of sheaf-doc, and sheaf-doc
re-exports them:
- the id newtypes `PageId`, `OutlineId` (model.rs:23, :26), `AnnotId`
  (annot.rs:25) and `FieldId` (forms/field.rs:19), with `BlobId` (§3.1)
  and `Local`;
- every payload spec: the mark kinds (from `AnnotKind`, annot.rs:72)
  and `Style`, `TextLayer` and `OcrWord` (text_layer.rs:44, :34),
  `HeaderFooterSpec` and `BatesSpec` (from stamps.rs:46),
  `WatermarkSpec` and `WatermarkContent` (from watermark.rs:41),
  `TextStyle` (model.rs:107), colors, boxes, label ranges and the rest;
- `Rotation`, `PageSize` (in `PageSource::Blank`) and `DestView`, from
  sheaf-core, with their tests and a `JsonSchema` derive like the rest.

Inherent methods that need sheaf-doc become free functions there.

**sheaf-core is split and renamed `sheaf-util`** (Greg, 2026-10-19).
sheaf-edit is published with the plugin SDK, so it can't depend on an
unpublished crate, and `sheaf-core` is taken on crates.io (see
[`plugins/cargo-sheaf-plugin.md`](../../../topics/plugins/cargo-sheaf-plugin.md)).
- Its types move out:
  - `Rotation`, `PageSize` and `DestView` go to sheaf-edit, as above.
    sheaf-render, which doesn't depend on sheaf-doc, imports them from
    there.
  - `PageIndex` goes to sheaf-render. sheaf-doc never uses it, and every
    crate that does (app, ops, ocr, print) already depends on
    sheaf-render.
- **What stays is `sheaf-util`:** `checksum`, `checksum_hex`,
  `Checksum`, `read_file` and `read_file_and_metadata`. Its doc comment
  states its charter: byte helpers shared by sheaf-doc and sheaf-render,
  with no types and no sheaf dependencies. twox-hash is its only
  dependency. It is never published.
- **Every crate outside the published set gets `publish = false`.** The
  set is sheaf-edit, sheaf-plugin-api, sheaf-plugin-manifest and
  cargo-sheaf-plugin.
- sheaf-ui's `sheaf-core` dependency goes; nothing in sheaf-ui uses it.

**Conventions:**
- Names are VerbNoun.
- Ops address things by reference only (§6.6).
- Patches are tri-state: `Patch<T> = Keep | Clear | Set(T)`.
- Every spec is `#[serde(deny_unknown_fields)]`.
- **Versioning.** Persisted JSON (`sheaf edit` files and plugin
  submissions) is one envelope:
  `{ "v": "1.0", "label"?: string, "ops": [...] }`.
  - `label` names the version a `sheaf edit` file makes. A plugin's
    envelope omits it, since `submit` takes the label separately; one
    that has it is `Invalid`.
  - A reader refuses an unknown major or a newer minor.
  - Every field added after 1.0 is `#[serde(default)]`; a committed
    schemars snapshot test enforces it.

```rust
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)] #[non_exhaustive]
pub enum Op {
    // pages
    MovePages      { pages: PageSel, before: Option<PageRef> },
    RotatePages    { pages: PageSel, by: Turn },                       // relative here; absolute in the prim
    DeletePages    { pages: PageSel },
    DuplicatePages { pages: PageSel, #[serde(default)] locals: Vec<Local> },
    InsertPages    { from: PageSource, #[serde(default)] pages: Option<RangeSet<u32>> /* None = all */,
                     before: Option<PageRef>, #[serde(default)] bring: Bring, #[serde(default)] locals: Vec<Local> },
    ReplacePages   { pages: Vec<PageRef>, from: PageSource, #[serde(default)] with: Option<RangeSet<u32>>,
                     #[serde(default)] bring: Bring },
    SetPageBoxes   { pages: PageSel, crop: Patch<BoxSpec>, trim: Patch<BoxSpec>, bleed: Patch<BoxSpec>, art: Patch<BoxSpec> },
    SetPageLabels  { ranges: Option<Vec<LabelRange>> },
    // bookmarks
    AddBookmark    { parent: Option<BookmarkRef>, at: Sibling, title: String, target: Option<PageRef>,
                     view: Option<DestView>, color: Option<Color>, style: TextStyle, local: Option<Local> },
    PatchBookmarks { bookmarks: Vec<BookmarkRef>, patch: BookmarkPatch },
    MoveBookmarks  { bookmarks: Vec<BookmarkRef>, parent: Option<BookmarkRef>, at: Sibling, with_pages: bool },
    DeleteBookmarks{ bookmarks: Vec<BookmarkRef> },
    // annotations
    AddMark        { page: PageRef, mark: NewMark, reply_to: Option<AnnotRef>, local: Option<Local>,
                     #[serde(default)] imported: Option<Imported> },  // Cap::User (§6.8)
    AddConnector   { a: Anchor, b: Anchor, style: Style, #[serde(default)] locals: Vec<Local> },
    PatchMarks     { marks: Vec<AnnotRef>, patch: MarkPatch, #[serde(default)] imported: Option<Imported> },
    MoveMarks      { marks: Vec<AnnotRef>, by: Delta, to_page: Option<PageRef> },
    DeleteAnnotations { annots: Vec<AnnotRef> },                       // marks, others' comments, links — not widgets
    AdoptAnnotations  { annots: Vec<AnnotRef> },                       // others' comment → mark, same id and /NM
    // page content
    SetOcrText     { page: PageRef, layer: TextLayer },
    ClearOcrText   { pages: PageSel },                                // unsaved AND saved layers
    SetWatermark   { spec: WatermarkSpec, z: Z, scope: ScopeSel },     // replaces ours of the same z, and Acrobat-added
    RemoveWatermark{ z: Option<Z>, pages: PageSel, #[serde(default)] include_authored: bool },
    SetHeaderFooter{ spec: HeaderFooterSpec, scope: ScopeSel },
    RemoveHeaderFooter { pages: PageSel, #[serde(default)] include_authored: bool },
    FlattenAnnotations { pages: PageSel, which: FlattenSet },          // binder edit AND export step
}
#[derive(Serialize, Deserialize, JsonSchema)] #[serde(deny_unknown_fields)]
pub enum Transform {                  // accepted only by ExportTx (§6.3): never committable
    Op(Op),
    KeepPages { pages: PageSel, outline: OutlinePolicy /* Headings | Promote | Drop */, labels: bool },
    RemoveMarkup { keep: MarkupKeep /* links, widgets */ },
    RemovePrivateData,                // every key in private::KEYS (§15), legacy inline markers included
}
pub enum PageSource { File { file: BlobSel, name: String /* the file name the user picked: stems and notices */ },
                      Blank { size: PageSize, count: u32 }, Images { images: Vec<BlobSel>, fit: Fit } }
pub struct Bring { pub marks: bool, pub comments: bool, pub links: bool, pub form: FormBring /* New | Drop */,
                   pub outline: OutlineBring /* None | Append | MergeByTitle */, pub labels: bool }   // Default: all, New
pub struct NewMark { pub kind: MarkKind, pub geometry: MarkGeometry, pub style: Style, pub contents: String,
                     pub author: Option<String> /* None → host */, pub asset: Option<BlobSel> /* stamp, attachment */ }
                     // NO id, dates, /NM or partner
pub struct Imported { pub nm: String, pub created: PdfDate, pub modified: PdfDate, pub author: String }
                     // an imported comment's own /NM, dates and /T, kept as today (import.rs:274–286)
pub enum Delta { Page(Vec2), View(Vec2) }
```

**The 21 old `Edit` variants map as follows:**

| Old variant | New op |
|---|---|
| Move, Rotate, Delete, Duplicate, Insert, Replace | same names; Insert and Replace take page sets |
| Outline Insert, InsertTree, Move, Edit, Delete | `AddBookmark` (trees use locals), `MoveBookmarks`, `PatchBookmarks`, `DeleteBookmarks` |
| `AnnotAdd` | `AddMark` |
| `AnnotUpdate` | `PatchMarks` / `MoveMarks` |
| `AnnotDelete`, `CommentsDelete` | `DeleteAnnotations` |
| `ConnectorAdd` | `AddConnector` |
| `Set/ClearTextLayers` | `SetOcrText` / `ClearOcrText` |
| `SetHeaderFooter` | `SetHeaderFooter` / `RemoveHeaderFooter` |
| `SetPageLabels` | `SetPageLabels` |
| `RebasePages` | **deleted**: its callers say what they mean |

UI macros stay macros that emit batches with locals: Set Status, Color
by Level, Bookmarks from Pages, calculator ticks and paste. They read
the model through `Tx::reader()` (§6.3), so what they computed from is
recorded.

#### 6.2 The primitive

```rust
pub struct Prim { pub key: Key, pub before: Option<Val>, pub after: Option<Val> }
#[derive(Clone)]
pub enum Val { Order(im::Vector<PageId>), Page(Arc<PageDef>), Geom(Arc<Geom>), PageAnnots(im::Vector<AnnotId>),
               Annot(AnnotRec), Field(Arc<FieldNode>), Form(Arc<FormDoc>), Bookmark(Arc<Bookmark>),
               OutlineRoots(im::Vector<OutlineId>), Labels(Option<Arc<[LabelSpec]>>), Generator(Arc<Generator>) }
impl DocState {
    /// Sets the slot; asserts `before` (ptr_eq, else Eq); keeps the derived indexes current.
    pub fn apply(&mut self, p: &Prim) -> Result<(), Mismatch>;
}
```

- **Before-values cost nothing.** They are the parent's Arcs, and an im
  clone is O(1).
- **What before-values buy:** replay checking, `classify` from the step
  alone, the commit-time value check, and describe text for pruned
  parents.
- **Coalescing.** At commit and at amend, the prims are coalesced per
  key into one prim from the first `before` to the last `after`, and
  identity prims are dropped. A batch that sets `a` from x to y and back
  to x therefore leaves nothing (§8.5).

#### 6.3 Transactions, lowering and reads

A **transaction** (`Tx`) collects ops against a base version.
**Lowering** turns each op into prims inside it.

```rust
pub struct Tx {                                  // binder transaction: Op only; committable. Send + 'static.
    base: VersionId, base_state: DocState,       // what the caller saw (VersionId names that exact state)
    state: DocState,                             // working copy: read-your-writes
    ops: Vec<ResolvedOp>, prims: Vec<Prim>,
    seen: BTreeMap<Key, Option<Val>>,            // lowering reads: the value at first read
    decided: BTreeMap<Key, Option<Val>>,         // decision reads: what the caller based its ops on
    steps: Vec<StepDraft>, locals: LocalMap, created: Created, handed_out: bool,
    notices: Vec<Notice>, pins: Vec<Blob>, cx: Arc<LowerCx>,
}
pub struct ExportTx { /* the same machinery; accepts Transform; consumed only by export::run; never committed */ }
pub struct LowerCx { store: Arc<Store>, alloc: Arc<IdAlloc>, host: HostAttrs, settings: Arc<EditSettings>,
                     actor: Actor, grants: Grants /* plugin asset handles, CLI --with names */ }
impl Document { pub fn lower_cx(&self, actor: Actor) -> Arc<LowerCx>; }   // the only constructor: actors can't be forged
pub struct HostAttrs { pub person: Arc<str>, pub initials: Arc<str>, pub now: PdfDate }
impl Tx {
    pub fn apply(&mut self, op: Op, p: &Progress) -> Result<OpResult, EditError>;          // atomic per op
    pub fn apply_batch(&mut self, ops: Vec<Op>, p: &Progress) -> Result<Vec<OpResult>, EditError>;  // atomic
    pub fn check(&self, op: &Op) -> Result<(), EditError>;     // resolve refs + caps only: no puts, cascades or ids
    pub fn read(&mut self, key: Key);                           // decision read of the base value
    pub fn read_render(&mut self, page: PageId, inputs: &ViewInputs);  // the keys feeding what that page draws
    pub fn reader(&mut self) -> Reader<'_>;                     // model reads that record decision reads
    pub fn pin(&mut self, b: Blob);
    pub fn mark_handed_out(&mut self);                          // created ids were returned to a caller (plugins)
    pub fn state(&self) -> &DocState;
    pub fn preview(&self) -> Preview;                           // lines, effects, consequences vs saved, created, caps
}
```

**`Tx::apply` runs in three stages:**
1. **Resolve.** `BlobSel` becomes a `BlobRef` through the Store, the
   CLI's `--with` names or the plugin's asset grants. A raw
   `BlobSel::Id` from a plugin actor is `Refused`.
2. **Check capabilities.** The cap of the resolved op and its targets is
   checked against `cx.actor` (§6.7) **before** any lowering.
3. **Lower.**

**Lowering reads only** the `Tx` state, immutable `Src` facts, the
Store, `HostAttrs` (captured once per `Tx`) and the allocator. It never
reads Document side tables, paths, caches or engines. Replay doesn't
re-run it, so purity is a hygiene rule; the prims are what replay
trusts.

**An op reads only the keys whose values it uses.** A target's existence
is checked by the before-value of what the op writes, not by a separate
read. `SetPageBoxes` reads and writes `Geom(p)` alone, so an OCR flush,
which writes `Page(p)`, never conflicts with a crop dialog.

**Two kinds of reads are recorded, both by value:**
- **Lowering reads** come from the accessors, cascades included.
  Resolving `PageSel::Positions` or `PageRef::Pos` reads `Order`.
- **Decision reads** are what the caller based its ops on. Each records
  the value it saw:
  - `Tx::read(key)`;
  - `Tx::reader()`, used by UI macros and the plugin host. A key the Tx
    already wrote is its own value and isn't recorded;
  - host-resolved payloads: M10's calculated values, OCR words, trim
    boxes. `FormsHost::resolve` returns `(payload, reads)`;
  - `Tx::read_render(page, inputs)`, for anything derived from pixels or
    page text. It records the keys that feed what is drawn under those
    inputs. The box and rotation the render used are not reads: a
    consumer that converts pixels keeps the render transform with its
    result (§5). Under `ViewInputs::content_only()` that is `Page(p)`
    alone.

**The host resolve stage** is the general pattern for work that needs
engines before lowering:

```
resolve(&Snapshot, &EnginePool, &Progress) -> (resolved ops, decision reads)  →  pure lowering in a Tx
```

OCR, Trim White, form calculation and M11's partial-image redaction all
follow it.

**Progress and cancel** reach lowering, because 7,000-page ops
(`FlattenAnnotations`, `RemoveHeaderFooter`'s filters) report per page.
A canceled `apply` leaves the Tx as it was before that op.

#### 6.4 Validation: fail, never repair

- **Every reference must resolve:** pages (including Move, Rotate,
  Delete, Duplicate and `ClearOcrText`, which ignore unknown ids today),
  `before`, bookmark targets, label anchors, generator scopes, reply
  parents, connector partners, locals and blobs.
- **Errors are typed:**

  ```
  EditError::{ NotFound{op, what}, Invalid{op, why}, Ambiguous{op, what}, Content{op, page, why},
               Conflict{since, keys}, Refused(Cap), GroupEnded, NotReady, Cancelled, Mismatch{version},
               Internal{what} }
  ```

  `Internal` is a bug in sheaf, such as a cascade that doesn't settle
  (§6.5). It panics in debug builds. M10 slice 9 adds
  `Locked { field }` for fields a signature locks.

- There is no lenient mode. The app prunes its own selections on every
  ChangeSet.
- **`DocState::check_invariants()`** runs after every op in tests, debug
  builds and the corpus run. It checks:
  - `order` ⇔ `pages` ⇔ `geoms` keys agree;
  - every id in `page_annots[p]` has `annots[id].page == p`;
  - widget ↔ field ↔ terminal lists agree;
  - no cycles or orphans in outline or field kids;
  - scopes and frozen maps name live pages;
  - at most one sheaf generator per kind and z, `/NM` unique per page,
    and field full names unique;
  - every `BlobRef` and `Src` resolves;
  - no ContentStack references an `AnnotId` or `FieldId`.

#### 6.5 Normalization: cascades declared once

A **cascade** is a named rule that keeps the state consistent after an
op (deleting a page deletes its annotations). Rules are pure, idempotent
and named. They run on the keys an op wrote, to a fixed point (more than
8 rounds is `EditError::Internal`). Their output is ordinary prims in
the same step. Each is reported as `Cascade { rule, objects }`, which
feeds notices such as "Deleted 3 pages and 2 bookmarks — Undo".

| Rule | Trigger | Effect |
|---|---|---|
| `page.annots` | page removed | its annotations removed (marks, others', links, widgets) |
| `annot.thread` | annotation removed | replies and statuses whose `reply_to` is it, transitively |
| `annot.partner` | connector removed | partner's `partner` cleared |
| `widget.field` | widget removed | a terminal whose last modeled widget went (and `outside == 0`) is removed; empty parents pruned; `/CO` and `roots` pruned (**deliberate change**: the model test `form_fields_follow_their_pages` (model.rs:3055; the assert at :3080) inverts, so the model matches what `keep_only` writes) |
| `page.bookmarks` | page removed | bookmarks targeting it removed, children promoted |
| `page.labels` | page removed | a range anchored there re-anchors to the next surviving page of its range, else is dropped (**deliberate change**) |
| `page.generators` | page removed | dropped from `Pages` scopes, with its frozen Bates entry; numbers are never reused |

**Op-specific lowering:**
- **DuplicatePages.**
  - The copy gets `copy + 1` and joins every `Pages` scope its original
    is in. It gets no frozen Bates number, and its OCR layers are shared
    by Arc.
  - Marks get fresh ids, with threads remapped inside the copied set.
  - Others' annotations get fresh ids and `copy + 1`. A deleted comment
    isn't in the list, so it can't reappear.
  - Widgets get fresh ids, and their fields get a new `FormInst` per
    source instance. The new instance inherits its original's name in
    `FormDoc.names`, and its fields are renamed apart under that name's
    stem, with `Notice::CopiesRenamed`. A merged widget stays merged in
    its copy.
  - The `/StructParents` and `/StructParent` rule is the emitter's
    (§11.3).
- **ReplacePages.**
  - The slot keeps its PageId, and `Geom` comes from the new base.
  - The old base's others' annotations and widgets go through the rules.
    Marks stay, in PageSpace. Filters and static layers are dropped.
  - Extra originals are deleted and extra replacements inserted.
    Incoming fields follow InsertPages.
- **InsertPages.**
  - Facts are read from the source, for the selected pages only.
  - Incoming marks get fresh ids. A connector keeps its partner only if
    the partner came too.
  - The outline is merged by title or appended.
  - **Fields.** One new `FormInst` per op, with `PageSource::File.name`
    recorded in `FormDoc.names`. If any incoming node's name clashes,
    the whole instance goes under a new parent named for that name's
    stem (`stem`, `stem_2`, …), and the decision is recorded in the
    prims. `/DR` font renames are recorded in `FormDoc`.
  - **Notices** go into `Commit.notices`, with the exact texts ported
    from M10 slices 4–5 (`form_notice`, model.rs:845):
    - `FormRenamed { under, file, scripted }`: "fields from {file} are
      under “{under}”, since their names were taken", plus "; its
      scripts still use the old names" when scripted;
    - `CopiesRenamed { under, scripted }`, from DuplicatePages: "the
      copies' fields are under “{under}”, apart from the originals",
      with the same suffix;
    - `SignatureNotValidHere { file }`: "the signature from {file} is no
      longer valid here" (the signed fields are kept with their values);
    - `ScriptsOff`: "scripts are off: fields from more than one form
      have them".

    One commit's notices are capitalized and joined with ". ", as
    today. Tests pin them: forms_merge.rs:196–210 and :298–311, and
    forms_saves.rs:306–336 (sheaf-ops).
- **KeepPages (Headings).** Before the `Order` put, bookmarks whose
  target goes but which keep surviving descendants are retargeted to
  `None`. That is extract's "parent becomes heading"; DeletePages keeps
  "promote".
- **SetWatermark and SetHeaderFooter.**
  - They replace an existing sheaf generator of the same kind and z in
    the same step.
  - They set `replaces` to their kind, so Acrobat-added artifacts of
    that kind on covered pages are dropped at emit (§11.4), pages
    inserted later included.
  - Lowering is O(1) and reads no content facts.
  - Authored pagination artifacts (a Word or InDesign running head) are
    never touched.
- **RemoveWatermark and RemoveHeaderFooter.**
  - They remove our generator (or narrow its scope).
  - They add `DropMarked`/`DropStreams` filters for unbacked sheaf
    blocks and Acrobat-added blocks on the selected pages.
  - With `include_authored`, they also remove authored artifacts.
  - The command runs as a job whose resolve phase forces the `blocks`
    facts, and its describe line counts each origin: "Remove the header
    and footer from pages 1–20 (2 made in Acrobat; 12 the author's left
    in place)".

#### 6.6 References and selectors

```rust
pub enum PageRef     { Id(PageId), New(Local), Pos(u32) }                   // 17 | "$3" | {"pos": 3}
pub enum BookmarkRef { Id(OutlineId), New(Local), Path(Vec<String>) }       // 9 | "$1" | {"path": ["Part I", "Ch. 2"]}
pub enum AnnotRef    { Id(AnnotId), New(Local), Nm { nm: String, page: Option<PageRef> } }   // {"nm": "…", "page": {...}}
pub enum BlobSel     { Id(BlobId), With(String), Asset(u32) }               // {"blob": "b…"} | {"with": "logo"} | {"asset": 2}
pub struct Local(pub u16);
pub enum PageSel { Ids(Vec<PageRef>), All, Span { first: PageRef, last: PageRef },
                   Positions(RangeSet<u32>), Bookmark(BookmarkRef) }        // #[non_exhaustive]
pub enum ScopeSel { All /* includes pages added later */, Pages(PageSel) }
```

**Per-kind references** have a custom deserializer with clear errors. A
misspelled local is an error, not a fallthrough to another variant. An
ambiguous `Path` or `Nm` (two matches) is `Ambiguous`.

**Positions are 1-based,** as the UI shows them: `pos` and `Positions`,
and the source pages of `InsertPages.pages` and `ReplacePages.with`.
The schemars docs say so, and a CLI golden pins it. (`PageBase.index`
is internal and 0-based.)

**Positions resolve against the pre-op state,** meaning the Tx state
when that op runs. This matches reads, describe lines and the intuition
of a sequential script: `[delete-pages 1, rotate-pages 2]` rotates the
page now in position 2. The step stores the resolved ids. The app never
sends positions: dialogs resolve indexes to ids against the version they
opened on, so their transactions don't read `Order` at all.

**Locals.** A **local** names something an op creates, for later ops in
the same transaction. A creating op names its results with
`local`/`locals`, and a later op in the same `Tx` uses `New(n)`. An
undeclared, duplicate or wrong-kind local is `Invalid`. `Commit.created`
maps every local, and every implicit creation in order, to real ids.

**Id guessing this deletes:**
- "the last mark on the page" (docview.rs:1965, :2476);
- `skip(before)` for paste (markup_tools.rs:1684);
- by `/NM` for adoption (adopt.rs:87);
- the next bookmark in pre-order (outline_panel.rs:206);
- pages after the old count in combine;
- `reveal_new_bookmarks`' outline diff (docview.rs:5876).

**`BlobSel::With` and `Asset`** exist because the CLI
(`--with NAME=PATH`) and plugins (`assets.upload`) are their producers.
They are resolved before lowering, and the step stores the `BlobRef`.

#### 6.7 Capabilities and actors

```rust
pub enum Cap { Plugin, Approval, User }
pub enum Actor { User, Plugin { id: Arc<str>, version: Arc<str> }, Cli, Job(JobKind /* Ocr | Trim | Import | Insert */) }
```

**The cap is computed from the resolved op and the kinds of its
targets,** not from the op's name alone. A test checks that the cap
table is total over (op, target kind), and that the actor table covers
every actor.

| Cap | Ops and targets |
|---|---|
| `Plugin` | Move, Rotate, Delete, Duplicate pages; `SetPageLabels`; all bookmark ops; `AddMark` (assets only through grants), `PatchMarks`, `MoveMarks`, `AddConnector`; `DeleteAnnotations` whose targets are all sheaf marks |
| `Approval` | `SetPageBoxes`, `SetWatermark`, `RemoveWatermark`, `SetHeaderFooter`, `RemoveHeaderFooter`, `ClearOcrText`, `AdoptAnnotations`, `FlattenAnnotations`; `DeleteAnnotations` touching others' comments or links |
| `User` | `InsertPages`, `ReplacePages` (host-prepared sources), `SetOcrText` (the OCR job); `AddMark` and `PatchMarks` with `imported` |

**What each actor may submit:**

| Actor | May submit |
|---|---|
| `User`, `Cli` | every op |
| `Job(Import)`, `Job(Insert)` | every op (they hold `User`) |
| `Job(Ocr)` | `SetOcrText` and `ClearOcrText` only |
| `Job(Trim)` | `SetPageBoxes` only |
| `Plugin` | `Plugin` ops, plus what its manifest grants (nothing in 0.4) |

- **Only `ExportTx` accepts `Transform`, so exports are export-only by
  type.** `Document::commit` takes a `Tx`, which can't hold one. M11's
  "plugins never apply redactions" is therefore enforced by the types.
  If M11 later moves an apply-in-place into `Op`, that op gets
  `Cap::User`.
- **Plugins in 0.4 get `Plugin` caps only.** `Approval` ops need a
  manifest permission, and none is granted in 0.4. The agent is a later
  milestone (§20).
- **Provenance is session-only.** History shows the actor ("Plugin Tax
  Bookmarks: …"). Nothing goes into the file except a mark's `/T`, which
  comes from the host.

#### 6.8 Host-owned attributes and identity

**Lowering sets these identically for every surface,** and never takes
them from callers:
- `AnnotId` and `/NM`;
- `created` and `modified`, from `HostAttrs.now` (one date per `Tx`);
- the default author;
- the tokens `<<name>>`, `<<initials>>`, `<<date>>` and `<<time>>` in
  contents and stamp text;
- clamps (opacity, stroke) and kind filters (fill only on fillable
  kinds, cloudy only on cloudable kinds);
- tape defaults and connector partners.

`NewMark` and `MarkPatch` have no fields for any of these. `sign_marks`,
`keep_unseen` and `annot_from_wit`'s repairs are deleted. `PatchMarks`
and `MoveMarks` apply only the fields they name, to the record as it is
when lowered.

**Imported comments are the one exception.** Comment import keeps each
comment's `/NM`, dates and author, as today, so a second import of the
same XFDF finds its marks by `AnnotRef::Nm` and replies still thread by
name. `AddMark` and `PatchMarks` take them as `imported: Imported`
(§6.1), which needs `Cap::User`, so only the import job and the CLI can
pass it. On `PatchMarks`, `imported.nm` must equal the mark's `/NM`. A
comment whose `/NM` names another tool's comment here is
`AdoptAnnotations` followed by that `PatchMarks`.

**A plugin never learns the user's identity** (proposal 20).
- When the actor is a plugin without the `identity` manifest permission,
  `<<name>>` and `<<initials>>` stay literal.
- Marks a plugin creates are authored by the plugin's name.
- `annot-view.author` reports a role (`you`, `other`, `plugin <id>`)
  instead of a name.
- The `identity` permission exists, but no bundled plugin asks for it.
