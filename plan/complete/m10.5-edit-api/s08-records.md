# M10.5 slice 8: Records

Status: done · Part of [M10.5: the edit API](README.md)

**Records** (XL; proposal 14; bugs 12, 15, 28 and 29).
- **The new DocState tables and final keys:**
  `PageDef { base, copy, content }`, `Geom`, `PageAnnots`,
  `AnnotRec { Mark, Foreign, Widget { inst, merged, props } }`,
  `FieldNode` with `FormInst`, `FormDoc` with `ScriptState`,
  `Bookmark`, `OutlineRoots`, `Labels` and `Generator`. Today's
  header/footer becomes a `Generator` with legacy semantics.
- **Liveness.** Records hold `BlobRef` and `Src`.
- **Content.** OCR text layers become `OcrText` layers.
- **Others' comments and links** become `Foreign` records.
- **Template widgets** count in `Terminal.outside`.
- **Open** builds the legacy header/footer `Generator` from
  `/SheafHeaderFooter`, without lexing. `/SheafGenerators` and
  `/SheafBlock` recognition arrive in slices 16–17.
- **Attachments** become `Attachment` blobs in the Store: attach.rs:40
  (`store_blob`) and import.rs:391 intern the file, and
  attachments.rs:106, :117 (`blob_path`) read it through the mark's
  `BlobRef`.
- **Indexes:** position, `/NM`, full name.
- **The registry rule's destructures.**
- **`compute`** is adapted mechanically. Its insert path gives each
  registration one `FormInst` (whose `src` is the registration's
  `SourceId` until slice 11) and keeps today's join for that
  registration's later runs.
- **The legacy projection feeds the old writers.**
- **`rebase_to_saved` is adapted to records,** for this slice only.
  After a save it re-points the cursor's `PageBase` to the saved
  file's `Src` by position, with copy 0, drops the OCR layers the
  file now holds, and rebinds provenance (`Mark.src`, `Foreign.src`,
  `FieldNode.src`, `Widget.src`, `Bookmark.src`) from the saved
  file's facts. It matches the way the app's re-read after a save
  matches today: marks by `/NM`, others' annotations by page and
  position, fields by full name, bookmarks by pre-order.
- **Deletes:**
  - `removed_comments` and `saved_removed`;
  - `Doc.foreign_annots` and its `(SourceId, index)` keying;
  - `Document.forms` and `set_source_form`;
  - `fresh_annot_id`'s scan, `find_annot`'s scan, the linear
    `index_of`, and `page_positions` per call;
  - the DocState struct literals;
  - today's per-page widget map, and the `text_layers` and
    `header_footer` fields in DocState (they survive only in the
    projection's types);
  - the attachment cache paths (`store_blob` and `blob_path` in
    attach.rs, attachments.rs and import.rs);
  - slice 1's transitional keys.
- **Checks:**
  - The model tests are ported.
  - The comments panel lists the same comments over the corpus.
  - Deleting another tool's comment and saving by both strategies
    leaves its string absent.
  - Duplicating a page whose comment was deleted doesn't bring the
    comment back.
  - M10 slice 4's merge and rename tests and M10 bugs 2–4 pass by
    both strategies, with `forms::check` clean.
  - `page_templates_stay` passes.
  - `a_files_pages_in_runs_join_one_form` and
    `corpus_a_form_combined_with_itself_is_two_forms` pass on records
    (one instance per registration).
  - A save, then an edit, then a second save by each old strategy
    keeps every mark, comment, field and bookmark (the adapted
    rebase).
  - A bench: 1,000 mark adds on the heavy 7k with no scan per mark.
  - The replay proptest passes on the new keys.
  - **Blob liveness:** add an attachment mark and a page stamp, drop
    every UI handle, then undo, redo, save and reopen; the bytes are
    intact.

## Notes

Done. The document is records. DocState's tables are the eleven final
keys' (`Order`, `Page`, `Geom`, `Bookmark`, `OutlineRoots`,
`PageAnnots`, `Annot`, `Field`, `Form`, `Labels`, `Generator`), and
slice 1's nine transitional keys are gone. A page is a `PageDef` over
its source page, with its copy number and its `ContentStack` (OCR text
is an `OcrText` layer), and a `Geom` of its own; sheaf's marks, other
tools' comments and links and the form's widgets are `AnnotRec`s in
one `AnnotId` space, in each page's `/Annots` order; the field tree is
`FieldNode` records in explicit `FormInst`s under a `FormDoc`; the
outline is `Bookmark` records under `OutlineRoots`; and the header and
footer is a `Generator` with its old semantics. Records hold `Src`,
`ObjRef`, `BlobRef` and `FormRef`, so a version pins the bytes it
shows, and a mark holds its stamp's page and its attachment's file:
`MarkAssets` and the attachment cache paths are gone. A page's
position, an annotation's `/NM` and a field's full name are indexes,
so no edit scans. `compute` is adapted mechanically, the old writers
read a legacy projection built the first time one asks,
`rebase_to_saved` re-points and rebinds the records after a save, and
one baseline state replaces the dirty flags' six copies.
`check_invariants` holds after every edit, at open and after a save,
and the replay property runs on the new keys (proposal 14; bugs 12,
15, 28 and 29).
- **Pages** (`model/records.rs`, `model/state.rs`): `PageDef { base:
  PageBase { source, src, index }, copy, content: Arc<ContentStack> }`
  and `Geom` per page, beside `order: im::Vector<PageId>`.
  `ContentStack { over: Vector<Layer { body: Static }> }` with
  `Static::OcrText(Arc<TextLayer>)`; every page with nothing over it
  shares one empty stack (`ContentStack::empty()`, a `LazyLock`
  static; `is_shared_empty` for accounting). A new page's `copy` is
  one past the highest copy any page of its (blob, index) holds
  (`NextCopy`, one pass an edit); a page the tree lists twice opens as
  copies 0 and 1. Accessors: `page_count`, `order`, `page_at`,
  `pos_of` (O(1)), `def`, `geom`, `ocr_text`, `pages()` (id, def,
  geom in order) and `render_delta`. `model::PageRef` is gone:
  `Document::page(i)` gives a `PageId`, and `geom`, `render_delta`
  and `page_size` take one. The app draws through `pane::Shown { id,
  source, index, geom, render }` (the pane's rotate view baked in);
  sheaf-plugin's `OcrSource::recognize` takes the `PageId` and its
  `PageBase`, `HostState::page_base` replaced `page_ref`, and
  `PageRenderer` is `{ page, base, size }`.
- **Bookmarks, labels, the header and footer:** `Bookmark { title:
  Arc<str>, target: Option<Target::{Page { page, view }, Kept}>,
  color, style, open, kids, src: Option<ObjRef> }` by `OutlineId`,
  and `outline_roots`; `bookmarks_preorder`, `bookmark_place`
  (parent and previous sibling), `bookmark_pages` and
  `bookmark_count`. Edits put only the bookmarks whose fields or kids
  changed (`kids_of`, `put_kids`, `outline_insert`, `outline_unlink`,
  `subtree`, `cascade_delete`, `merge_outline`, `bookmark_span`,
  `span_at`): renaming one of 2,000 costs 942 step bytes, against
  375,627 before. `open::outline`'s `FlatOutlineItem` has the item's
  `object` and `kept` (a `/Dest` or `/A` with no page here), so
  `ready` gives each bookmark its `src` and `Target::Kept`. `Labels`
  holds `Arc<[LabelSpec]>`. `Generator { id: GenId, spec:
  GenSpec::HeaderFooter(Arc<HeaderFooter>) }` in an `OrdMap`, built
  at open from `/SheafHeaderFooter` (no lexing) under a `GenId` drawn
  from the file's hash; `DocState::header_footer()` reads it, as the
  overlay does. `OutlineNode`, `outline_flat` and the nested helpers
  are gone.
- **Annotations:** `AnnotRec { page, body: Mark | Foreign | Widget }`
  in `annots`, and `page_annots: PageId → Vector<AnnotId>` (no entry
  for a page with none). `Mark { annot, src: Option<ObjRef>, file:
  Option<BlobRef>, stamp: Option<FormRef> }`; `Foreign { src:
  Option<ObjRef>, copy, summary: Arc<ForeignAnnot> }`, other tools'
  comments and links; `Widget { field, src, inst, merged, rect, flags,
  mk, da, state, states, props }`. Accessors: `annot`, `has_annot`,
  `annots_on`, `annot_ids_on`, `marks_on`, `mark_records_on`,
  `foreign_on`, `widgets_on`, `annots()`, `marks()`, `annot_count`,
  `annot_by_nm`, `annots_named` and `find_annot` (O(1)). `fresh_annot_id` asks
  `has_annot`. `removed_comments`, `Doc.foreign_annots` and its
  `(SourceId, index)` keying are gone: the comments panel, adoption
  and comment exports name others' comments by `AnnotId`
  (`comments::Item::Foreign(AnnotId)`, `adopt::foreign_at`,
  `exchange::gather`),
  and a deleted one is simply no record, so a duplicate can't bring it
  back (bug 12). A stale sheaf mark, or one whose stamp or file its
  object doesn't hold, opens as a `Foreign` record (the 5→8 row).
- **Handles:** `store::BlobRef(Blob)` (equal, ordered and hashed by
  id; `blob()`, `id()`, `bytes()`); `source::ObjRef { src, id }`;
  `FormRef::{Base(ObjRef), Blob(BlobRef)}` with `to_pdf()`;
  `BlobId::{from_stem, stem}` (only the canonical 32-hex stem);
  `BlobKind::stamp()`. A mark's stem resolves when it's added, when
  its stem changes and for an insert's marks the fact doesn't match:
  the Store's blob, then a registered source's fact (the primary
  first), then for stamps only the generated cache (`model::find_stamp`,
  until 19), else the edit fails naming it. Callers that intern
  (attach, the stamps panel, paste, comment import) hold the blob
  until the edit applies (`import::Interned`, `Imported.held`).
  `AttachmentRef::Mark(AnnotId)` replaced `Cached(stem)`. `WriteCx`
  caches by `FormRef` and `BlobId` and uses in place what the file it
  writes over holds. `assets.rs`, `generated::{store_blob, blob_path}`,
  `annot::copy_stream`, `Document::{assets, set_source_assets,
  set_saved_assets}` and `Snapshot.assets` are gone.
- **Forms:** `FieldNode { parent, partial, src, inst: FormInst { src:
  SourceId, copy }, kids, term: Option<Arc<Terminal>> }` by `FieldId`;
  `Terminal { kind, flags, value, default, props: Arc<FieldProps>,
  widgets: Vector<AnnotId>, outside }`; `form: Arc<FormDoc { roots,
  names: OrdMap<FormInst, Arc<str>>, scripts: ScriptState::{On, Off {
  instances }} }>`. `forms::read` lists every node in tree order
  (`FileForm.nodes`; a terminal's id is its node's), each widget's
  `merged`, and each terminal's `outside`, its widgets no page lists
  (page templates, which the old writers keep through `keep_only`).
  An insert gives each registration one `FormInst` (copy the next
  unused) and joins its later runs to it (`take_in_fields`); a name
  clash puts the instance under a parent sheaf makes, named from the
  file (`FormDoc.names`); a duplicate is a new instance renamed apart
  (`copy_fields`). Accessors: `field`, `fields`, `field_count`,
  `terminals`, `field_named`, `full_name`, `form`. `Document.forms`,
  `set_source_form`, the per-page widget map and the sticky
  `scripts_mixed` field are gone.
- **Indexes** (`state.rs`, never compared): `pos`, each page's
  position, built the first time one is asked and shared by the
  clones of one order (an `Arc<OnceLock>`; a new order starts
  another); `nm: OrdSet<(PageId, hash, AnnotId)>`, every named
  annotation; `names: OrdSet<(hash, FieldId)>` with its reverse map,
  every node by full name, re-found under a node whose partial, parent
  or kids change. `DocState::set` keeps them current, so a replay
  does too.
- **Facts:** `Source::annots()` (`open::FileAnnots`: each page's
  annotations in `/Annots` order, `FileAnnot { object, slot, body:
  FileBody::{Mark { annot, held }, Foreign { id, summary }} }`, the
  marks given new ids, and by-stem lookups of what marks hold) and
  `Source::form()` (`FileForm`), each read once a source. Other tools'
  ids come from the source-seeded rng after the marks' renames, so the
  same bytes open with the same ids. `file::annots_in` lists links.
  `open::Facts.annots: Arc<FileAnnots>` and `Facts.form:
  Arc<FileForm>`; `ViewFacts.foreign` is gone.
- **Construction:** `DocState::builder()` (`pages`, `outline`,
  `annots`, `labels`, `generators`, `form`, `build`); every struct
  literal outside it is gone. `get` and `set` live in `state.rs` with
  the private fields; `apply` and `Rec` stay in `prim.rs`.
- **Invariants** (`DocState::check_invariants() -> Result<(),
  Invariant>`, `Invariant::{Pages, Annots, Form, Outline, Generators,
  Index}`): order, pages and geoms name the same ids once each;
  (blob, index, copy) is unique and each index is within its source;
  each annotation is listed once, on its own page, and no list is
  empty or on a dead page; widgets and their terminals list each
  other and share an instance; every node is reached once from the
  roots, its kids name it as parent, and its instance has a name;
  every bookmark is reached once from the roots and a page target is
  live; generators are stored under their ids, with at most one
  header and footer; and each index finds what it should and no more.
  A debug build asserts it in `Document::step`, `opened` (so `ready`
  and `Document::new`) and `rebase_to_saved`; the history property
  asserts it after every action and on every rebuilt state, and the
  corpus open test on every file.
- **Accounting:** one exhaustive match on `Key` for the steps
  (`accounting`), each `Arc` once a version by pointer; the snapshot
  half is `DocState::snapshot_bytes(keys)`, which destructures the
  state and counts the path copies of each map (HAMT 32, B-tree 64),
  the index paths and the position estimate. The whole-value
  over-count is now `Order`, `OutlineRoots` and `Form` (a page's
  annotation list counts whole too, but is small).
- **The legacy projection** (`model/legacy.rs`, `Snapshot::legacy()`,
  a `OnceLock` built on first use, never in `snapshot()`):
  `LegacyDocState { text_layers, header_footer, removed_comments,
  fields }`, `removed_comments` the base page's fact's names (through
  `save::origin` for a derived page) less the page's records', and
  `fields` legacy `forms::Field`s from the records (`legacy_field`:
  the instance's registration and copy, the object read, the parent
  sheaf made as `under`). Page tree, marks and bookmarks the old
  writers read from the state itself: `save::write_outline` walks the
  records, copying `/SE` from a bookmark's item in the primary, and a
  `Kept` one's `/A` and `/Dest`; `forms::merge::page_copy` reads a
  page's first widget's `FormInst`.
- **The baseline:** `Document.baseline: DocState`, the opened state
  and then each save's, replaces `saved_outline`, `saved_labels`,
  `saved_annots`, `saved_fields`, `saved_removed` and
  `baked_header_footer`. `outline_dirty`, `labels_dirty`,
  `header_footer_dirty` (by spec), `annots_dirty`
  (`annots_changed_from`, pointers first), `fields_dirty` (legacy
  projections compared), `form_changed` and `removes_content`
  (`keeps_annots_of`, `keeps_bookmarks_of`) compare the cursor's
  records with it. The opened baseline leaves out the pages of marks
  given new ids at open, so the next save writes them.
- **`rebase_to_saved`, adapted:** each page re-pointed by position to
  `PageBase { saved registration, saved Src, i }`, copy 0, the empty
  stack and the saved page's `Geom`; `Mark.src` and `Mark.stamp`
  rebound by `/NM` to the saved file's objects; `Foreign` records by
  name and subtype on the page, the k-th of them the k-th there (copy
  0, the saved slot), and one the saved page hasn't got removed, with
  a log line; field nodes by full name to `saved.src.form()`, walked
  in tree order so the k-th node of a name is the k-th saved one,
  every node in instance `(saved, 0)`; widgets by their place among
  their field's saved node's on the page; bookmarks by pre-order, one
  deeper than `open::outline` reads (`MAX_DEPTH`, 32) from no item,
  and from the first whose item's depth or title differs, none;
  `FormDoc.names` the saved file's name, scripts kept. The baseline becomes the rebased
  state, and the snapshot recount goes through `snapshot_bytes`. The
  app reads the saved source's annotations and form on its save
  thread, so the rebase reads nothing on the UI thread, and its
  re-listing of others' comments after a save is gone.
- **`compute`, adapted:** each arm puts the keys it changes. MovePages
  puts `Order`; RotatePages `Geom`; SetTextLayers and ClearTextLayers
  `Page`; RebasePages `Page` (new base, next copy, content kept) and
  `Geom` (new base, old rotate); DeletePages removes the pages'
  records, annotations and widgets, keeping the terminals (with fewer
  widgets); InsertPages, DuplicatePages and ReplacePages splice
  `Order` and put `Page`, `Geom`, annotations and fields
  (`bring_annots`, `copy_annots`, `drop_annots`, `take_in_fields`,
  `copy_fields`, `scripts_apart`): a duplicate shares its original's
  content and `Geom` Arcs, copies its marks with fresh ids and its
  `Foreign` records with fresh ids and the copy's number, and makes a
  new form instance; an insert takes others' comments, links, widgets
  and fields from the source's facts with fresh ids, and sheaf's
  marks from the `Edit`, matched to the fact for their `src`. The
  annotation edits put `Annot` and `PageAnnots`; CommentsDelete
  removes the named `Foreign` records and their threads; AnnotDelete
  takes a mark or a `Foreign` id and refuses a widget. OutlineInsert,
  OutlineInsertTree and OutlineEdit refuse a page that isn't there.
- **Ledgers:** the retired ledger has 92 rows for slice 8 (`PageRef`,
  `OutlineNode`, DocState's fields that went, named at
  `model::state::DocState` where its struct is now, and its accessors
  that went, there and at `model::DocState` and `prim::DocState`, the
  other files with an `impl DocState`, the six baselines,
  `Document::{forms, set_source_form, assets, set_source_assets,
  set_saved_assets}`, `Snapshot::assets`, the outline helpers,
  `import_marks`, the nine keys' `Key` and `Val` variants, `Facts`'
  and `ViewFacts`' retired fields (`Facts::renamed` among them),
  `AnnotsInFile::assets`, `ForeignAnnot::page`, `is_comment_subtype`,
  `assets::{MarkAssets, SourceAssets, Stamp, File}`,
  `generated::{store_blob, blob_path}`, `copy_stream`,
  `AttachmentRef::Cached`, `Field::{source_name, has_scripts}`,
  `fixture::mark_assets`, `HostState::page_ref`, and the app's
  `Doc::{page_ref, foreign_annots, foreign_generation,
  index_of_page_in}`, `Incoming::{marks, renamed, assets, foreign,
  form}`, `reread_foreign`, `Forms::incoming`, `outline_panel::locate`,
  `stamp_preview::{Tried, Previews::missing}`), and `QUALIFIED_ONLY`
  gains the 20 last segments still in use elsewhere (`index_of`,
  `text_layers`, `removed_comments`, `widgets`, `forms`, `assets`,
  `marks`, `foreign`, `renamed`, `form`, `TextLayer`, `HeaderFooter`,
  `Stamp`, `File`, `locate`, `missing`, `Pages`, `Annots`,
  `incoming`, `has_scripts`). Not listed, as the names live on:
  `DocState::{annots, fields, header_footer, labels}` (accessors
  now), `Builder`'s setters and `annotated_pages` (never committed),
  `Document::scripts_mixed` (a method still) and `model::geom_in`
  (new signature). The transitional ledger's rows
  are updated: the registrations row names `PageBase.source` and
  `FormInst.src`; the generated cache keeps only stamps, through
  `model::find_stamp`; `MarkAssets` went; the legacy projection has
  `fields` and no widget map; a new row for the baseline (8→9); the
  adapted rebase and compute rows say what they do. Slice 9's and
  slice 11's Deletes, §7's list of what the effects replace, §17 and
  §19 name the baseline, `bring_annots` and `copy_fields` where they
  named the six copies, `import_marks` and `bring_fields`. No lint
  ban lands.
- **Docs:** `docs/document-model.md`'s Core idea sketches the records
  (`PageDef`/`PageBase` with its registration until 11, `Geom`,
  `ContentStack`, `AnnotRec`, `FieldNode`/`FormInst`/`FormDoc`,
  `Bookmark`/`Target`, `LabelSpec`, `Generator`), the indexes and
  invariants, the baseline and the `Snapshot`'s lazy projection;
  Opening a file builds records, seeds the ids of others'
  annotations, links and widgets and the `GenId`, and reads
  `Source::annots()`/`form()`; Undo/redo has the eleven keys, the
  indexes, the new costs and the narrowed over-count; Page content
  has marks holding their stamps and files, the stamp fallback until
  19; Saving has the projection and the adapted rebase; Text layers
  are page content, still unsaved by presence until slice 9 (how,
  and what changes then). The 7k harness's module doc has the new
  row, and interop.rs's the new test.

Deviations from the plan:
- **Line numbers had moved since a0955fc;** the work followed the
  symbols (the DocState literals at model.rs:929, :1049, :2802,
  save.rs:121 and prim.rs:542; import.rs's `store_blob` at :370;
  attachments.rs read through `MarkAssets`, not `blob_path`).
- **`PageBase` carries `source: SourceId` beside `src` until slice
  11** (§3.3 has `{ src, index }`): render handles, `OcrPage.source`,
  `SourceInfo.derived` and the old writers' source maps key by
  registration, and two inserts of the same bytes are two
  registrations of one `Src`. `FormInst.src` is the registration too,
  as the slice said. The registrations row says so.
- **Records and fields left for their producers:** `ContentStack` has
  only `over`, `Layer` only `body`, `Static` only `OcrText` (no
  filters, `under`, `Layer.id`/`kind`, Placements or
  `FormRef::Pinned`: slices 16–19); `PageDef` has no `digest` (slice
  13's render key is its consumer); `GenSpec` has only `HeaderFooter`,
  wrapping `stamps::HeaderFooter` unchanged (its own page scope, date
  and file; Bates numbers counted at each Rewrite), with no `Scope`,
  `Frozen`, `z` or `replaces` (16–17); `FormDoc` has no owned
  AcroForm keys (`/CO`, `/DA`, `/Q`, `/DR`, `/NeedAppearances`,
  `/SigFlags`), XFA state or `damaged`, which the old writers still
  take from the primary's `/AcroForm` until slice 9's emitter;
  `Terminal` has no `display`, its `value` and `default` stay
  `Option<Value>` and `FieldProps` keeps `Field`'s types; `LabelSpec`
  is `labels::LabelSpec` unchanged (`prefix: String`).
- **`Val::Labels` holds `Arc<[LabelSpec]>`,** no labels being the
  prim's None (§6.2's `Option<Arc<..>>` would say "none" two ways).
- **The baseline is one `DocState`, here, not six copies deleted in
  9:** every copy changed type with its table, so `Document.baseline`
  replaced them all (the planner's call); slice 9 deletes it with the
  dirty flags, and its Deletes say so. `header_footer_dirty` compares
  the header's spec, so a header removed and set again with equal
  settings stays clean whatever its new `GenId`; `fields_dirty`
  compares the legacy projections, terminals only (no slice-8 edit
  renames an existing node); `form_changed` compares with the
  baseline, so a recovered document compares with what it opened
  with.
- **The re-read after a save went in 8, not 9** (slice 4's notes put
  `reread_foreign` in 9): with `Foreign` records rebound by the
  rebase and marks holding their handles, it had nothing left to do.
  `set_saved_assets` went with it. What's left of the read is the
  saved file's annotations and form on the save thread.
- **The projection is lazy:** building it in `snapshot()` would add
  O(document) to every snapshot (plugins, autosave, print, saves), so
  an old writer builds it the first time it asks. It has `fields` and
  no widgets-by-page map or legacy `Widget { object, slot }`:
  `page_copy` reads the widget records' `FormInst`, and `slot` has no
  reader outside the form reader. The dirty flags come from the
  records and the baseline, not from it.
- **`Source::annots()` and `Source::form()` are new facts** (§3.1's
  facts, which the plan assumed): `open::finish` and the app's
  `Incoming::read` called `forms::read` and `file::annots_in`
  directly. Widget ids aren't in the form fact (that would make
  `form()` depend on `annots()`): `Document::opened` draws them from
  the source-seeded rng after the `GenId`, avoiding every id in the
  annotation fact. `ObjRef` (step 2), `BlobRef`/`FormRef` (step 4)
  arrived with their first holders.
- **Links are `Foreign` records; other unmodeled subtypes aren't.**
  `file::annots_in` lists Link annotations (`is_listed_subtype`, was
  `is_comment_subtype`) without converting them to marks, and the
  comments panel still lists only comments. Popup, Screen, Widget
  outside the form and the rest stay in the page's file, passed
  through by the old writers; slice 9's emitter decides their
  records.
- **`Foreign.src` is `Option<ObjRef>`:** an annotation written as a
  dictionary straight into `/Annots` has no object (16 entries in 5
  corpus files); None means the dictionary at `summary.slot`, which
  `exchange::gather` reads from the page base by slot. `Foreign.copy`
  is the copy number of its page (at open, duplicate, insert and
  replace), not the record's copy + 1, so two duplicates of one page
  stay distinct. `Mark.src` is None for marks on a copy (a page the
  tree lists twice, the same file inserted twice, a duplicate).
- **The fact's shapes:** `file::annots_in` returns `AnnotsInFile {
  pages }` of `InFile { object, slot, found: Found::{Mark { annot,
  held }, Foreign} }` with `sheaf()` and `foreign()` helpers; marks
  needed their object and their order among other tools'
  annotations. `ForeignAnnot` lost `page`, gained `is_link()`.
  `FileAnnots::stamp(stem)`/`file(stem)` are by-stem lookups built
  lazily from the marks' held objects (first in page, then `/Annots`
  order), read by compute's resolver and `WriteCx`.
- **Adoption replaces the record:** an `AnnotAdd` of a mark named as
  another tool's comment on its page removes that `Foreign` record
  and the parts grouped under it and takes its `src` (§3.6's "a mark
  with src"); the projection's removed names subtract mark names, so
  the old writers' "takes its place" output is unchanged. Deleting an
  adopted mark now deletes the comment with its thread, where it used
  to bring the original back until a save; undo restores it. Of
  `Foreign` records named alike (files repeat `/NM`s), the one taken
  is the one whose id the incoming mark's `id` is, which the app sets
  to the record it made the mark from (`adopt_foreign`), else the
  first by id; a mark of that name is never taken. `AnnotAdd` draws
  the mark's id afresh, so its incoming `id` meant nothing before;
  slice 11's `AdoptAnnotations { annots: Vec<AnnotRef> }` replaces
  this. `removes_content` counts an adopted comment as kept (a mark
  on its page is its object, or, for one written into `/Annots`, has
  its name), and its grouped parts too (a mark has their parent's
  name), so adopting one still saves in place, as before slice 8.
- **CommentsDelete scans the page's `Foreign` records** rather than
  ask the `/NM` index: files repeat a name on a page, and every
  comment with a deleted name goes, as before. The index serves
  single lookups (adoption).
- **The `/NM` index holds every named annotation** (`OrdSet<(PageId,
  hash, AnnotId)>`), not the last one set: with one entry per name,
  "the index equals a rebuild" fails once a file repeats a name and
  the indexed one is deleted. `annot_by_nm` gives the first match by
  id, `annots_named` every one. The full-name index holds every node
  of a name likewise. Neither name is asserted unique (real files
  break both; slice 11).
- **Inserts keep the `Edit`'s marks** (compute adapted mechanically):
  `InsertPages` and `ReplacePages` still carry sheaf's marks, matched
  to the source's fact by original id for their `src` and handles;
  everything else comes from the facts. `import_marks` became
  `bring_annots`, which also re-points replies (ours and others') to
  a mark whose name changed with its fresh id; §17, §19 and slice 11
  say `bring_annots`. `bring_fields` was folded into
  `take_in_fields`.
- **Copies:** keyed by (blob id, page index), one past the highest
  copy held (not the lowest unused). A page the tree lists twice is
  (index 0, copy 0) and (index 1, copy 1): two indexes of one
  object, so slice 9's emitter identifies a page by (src, index,
  copy), not (object, copy). RebasePages onto a page's own base is
  skipped (no copy bump); a rebased page keeps its content and
  rotation. ReplacePages puts `Order` only when it adds or removes
  pages.
- **`/SE` is kept for every bookmark read from the primary,** not
  only `Kept` ones (§3.8's src keeps "/A, /SE and unknown keys"; bug
  15 lists `/SE` as lost on every rewrite). The old writers write a
  `Kept` bookmark whose src isn't the primary (an inserted file's
  link bookmarks, or the old primary after undo across a save) as a
  plain heading, as before slice 8 (which lost every `/A`); slice 9's
  emitter copies them. Undo across a save makes `outline_dirty` true,
  since the old src differs from the baseline. `subset`'s
  `prune_outline` treats `Kept` as no page: such a bookmark survives
  an extract only as a heading over kids that do, then plain.
- **Bookmark targets are validated now** (bug 11's bookmark part,
  from slice 11): OutlineInsert, OutlineInsertTree and OutlineEdit
  refuse a page that isn't there, since the invariant that page
  targets are live needs edits never to make one. Label anchors and
  the header's page scope stay unvalidated.
- **Ids at open:** the `GenId` and the widgets' ids come from the rng
  the primary's `BlobId` hash seeds (the same seed as the open's
  renames; GenIds and AnnotIds are separate spaces), so the app's
  open and the command line's give equal states
  (`the_app_opens_as_the_command_line_does` compares them whole).
  compute draws a `GenId` from `fastrand::Rng::new()`, a fork of the
  thread's rng, deterministic under `fastrand::seed`.
- **Handles past the plan:** attachments are decoded with
  `attachments::mark_file_bytes(stream, size)`, never past the mark's
  declared size, so a small stream can't swell at open; one that
  can't be decoded within it opens as a mark holding no file
  (logged), saved without it. The app decodes at ready and at an
  insert on the UI thread (slice 11's resolve phase can move it).
  Unresolvable stems are errors at apply (slice 6 logged them at
  write). `WriteCx` uses in place a `FormRef::Base` of the file it
  writes over, or any stem that file's fact holds, trusting a stem to
  name the same bytes as slice 6 did, so an incremental save still
  appends no second copy; a file from another source is embedded
  from its `BlobRef` and recompressed (`copy_stream` went). The
  rebase rebinds `Mark.stamp` to the saved file's form, so marks
  don't pin the old primary or the library blob after a save.
  Import and paste keep a mark's stem when the document can already
  find it, and otherwise intern the bytes and rename the mark to the
  blob's stem (a `FormRef::Base` made a one-page PDF with
  `form_to_pdf`). `exchange::import` and
  `sheaf_ops::comments::import_data` take the `&Store`, and
  `Imported.held`/`import::Interned` hold the blobs until the edits
  apply (slice 11's `Tx::pin`). The stamp preview no longer retries
  by `MarkAssets` generation: a picture that failed isn't asked for
  again.
- **Forms past the plan:** full names pass over an empty `/T` as
  well as a missing one, as PDFium does (`GetFullNameForDict`), so
  `/T ()` under `up` is `up`, not `up.`; nodes are numbered in tree
  order, so every_kind's terminals are 1..=9. Scripts go off when
  scripted instances come from two registrations (files), as
  `script_sources` decided, not "> 1 instance" (§3.7): a duplicate of
  a scripted page keeps them on, as before; `ScriptState::Off {
  instances }` is set by `scripts_apart` and kept by the rebase, and
  undo turns them back on (the sticky field didn't). AnnotDelete
  refuses a widget ("a field's widget goes only with its page");
  a widget's `name()` is empty and the `/NM` index skips it.
  `copy_fields` copies terminals and the ancestors read from a file,
  never a parent sheaf made. `next_copy` reads `FormDoc.names`.
  `slot` was added to `file::InFile` and `open::FileAnnot` so
  widgets merge into `/Annots` order at open and on insert (marks
  after them, as before). DeletePages keeps the terminals and takes
  the page's widgets out of `Terminal.widgets`, today's behaviour
  (`form_fields_follow_their_pages`: 9 terminals stay, widget lists
  from 10 to 6); proposal 25's widget.field cascade is slice 11's.
- **Accounting:** the whole-value over-count is `Order`,
  `OutlineRoots` and `Form`, not "Order, OutlineRoots and Labels":
  `Labels` is an Arc'd slice im doesn't share, so counting it whole
  is exact. A value records share (a duplicate's stack, a copy's
  props) counts once a version but again in a later version that
  shares it. The snapshot half moved into `DocState::snapshot_bytes`,
  since the fields are private to `state.rs`; the rebase recounts
  through it (its rebinds return the keys they re-expressed), which
  over-counts the name index slightly (a rebind doesn't reindex).
- **`check_invariants` checks what slice 8's records guarantee**
  (above) and leaves for slice 11 what today's `compute` can still
  break or real files do: label anchors, the legacy header's page
  scope after DeletePages, reply parents and connector partners (bugs
  11 and 12), and `/NM` and full-name uniqueness. It doesn't check
  blob liveness (a `BlobRef`'s presence in the Store needs the
  Store), "no ContentStack names an AnnotId or FieldId" (`Static`
  holds only OCR), `ScriptState::Off`'s instances (the rebase keeps
  them) or `Foreign.copy` against its page's (RebasePages keeps the
  records). `save::subset`'s throwaway state is never checked: it
  keeps every field node while a left-out page's widgets stay listed
  in their terminals. The indexes are checked by containment and
  counts rather than by building new ones (the 2,600-action release
  run, 3.98 s → 3.2 s).
- **Smaller choices:** `get`/`set` moved to `state.rs` (the fields
  are private); accessors past the plan's list (`pages()`,
  `render_delta`, `bookmark_place`, `bookmark_count`, the crate-only
  comparisons); `Document::index_of` stays (O(1) now); `geom` and
  `render_delta` give Letter and R0 for a page the state hasn't got,
  as the old fallback did; `Bookmark.title` is `Arc<str>`; the open's
  baseline leaves out renamed marks' pages; `Document::new` reads its
  source's annotation fact, or the projection would count every
  other tool's annotation as removed; the app hides others' comments
  in PDFium by the names of each page's visible `Foreign` records
  against its base source's fact (derived sources skipped, as
  before); `subset` keeps the kept pages' OCR with their records.
- **Fixes found on the way:** the outline panel's `new_bookmark`
  renamed the selected bookmark's first child when it had kids (it
  took the next item in pre-order, not the sibling after); and
  ReplacePages with a page listed twice in `ids` replaced it, deleted
  it as an extra, then put the replacement's marks and widgets on the
  deleted page. Each page is now replaced once (found by the
  invariants in the replay property; its shrunk seed is in
  `proptest-regressions/model/history_tests.txt`, and
  `a_page_listed_twice_is_replaced_once` tests it). At the final
  check, `check_invariants` named either page of a copy clash, in the
  pages map's order, which `im`'s random hasher changes run to run, so
  `the_invariants_find_each_rule_broken` failed in four runs of six; it
  walks the order now and names the later page.
- **The rebase removes another tool's annotation the save left out**
  (the plan's rebase only re-points records): a Rewrite drops a link
  to a page it doesn't keep (`kept::clean_links`), as does an insert
  of part of a file, and pairing a page's `Foreign` records by place
  left all of them on such a page bound to the old primary, pinning
  its bytes, with the dropped link still in the document. They pair
  by name and subtype now, so the document says what the file holds.
  Field nodes pair by full name in tree order and widgets by their
  field's saved node, so two fields of one name (real forms have
  them) stay two objects; and bookmarks deeper than the reader reads
  no longer shift every later one onto another item's object, whose
  `/A` and `/Dest` a `Kept` one would have copied at the next save.
- **One full-name rule:** `forms::full_name_of` (PDFium's: an empty
  partial passed over), which the reader and the full-name index
  each wrote out; `open::MAX_DEPTH` and `open::shown_title` serve the
  rebase's pairing of bookmarks.
- **The bench's second assertion is against blank pages,** not the
  first 100 adds: the last 100 of 1,000 adds took 4.5× the first 100
  on the heavy 7k, and 5.5–6.5× on 7,000 blank pages, since a push
  walks the versions to trim them and grows towards `MAX_VERSIONS`
  (Found); and a scan over 21,000 records barely grows across 1,000
  more, so last-against-first can't see one. The heavy's median and
  last-100 median are held to 2× the blank's, which a scan per add in
  `fresh_annot_id` fails (1.15 ms; checked and put back).
- **Ledger rows name DocState's fields at `model::state`,** where
  its struct is now, so a field defined again is caught; its retired
  methods have rows at `model::state`, `model` and `prim`, the three
  files with an `impl DocState` (the walk names an impl's items by
  its file's module).
- **Test changes:** `rebase_after_save_keeps_ids_and_history` saves
  for real (the adapted rebase reads the saved page table);
  `a_batch_keeps_each_slot_s_change_on_its_last_step` turns one page
  twice (two pages are two `Geom` keys now); `damage_pages_before`
  became `damage_turn_before`; the app's open-equality test seeds
  fastrand before its duplicate (adding `fastrand = "2"` to
  sheaf-app's dev-dependencies); gate_7k's 20,000-copy file is
  written with `annot::to_pdf`, since a state can't hold two marks
  of one id, and the history and Rewrite rows share `marked()`;
  combine's `marks_bring_what_they_hold` uses real attachment stems;
  `a_file_whose_pages_come_in_cant_replace_a_stamp` now inserts the
  incoming marks and checks each draws its own page; the fixture's
  `MARK_STAMP` and `MARK_FILE` are real stems (`mark_stamp_pdf`,
  `mark_blobs`, `holding` replace `mark_assets`); the history `Run`'s
  `other` is every_kind's real bytes, so an insert from the primary's
  registration brings no form; the `Run` and the no-op table open
  with other tools' comments, replies, grouped parts and links on
  every page (`with_theirs`, `their_annots`; `other` has them on both
  of its pages), AnnotAdd adopts one by its record's id and
  AnnotDelete takes theirs too, so adoption, CommentsDelete,
  DuplicatePages, ReplacePages and inserts move `Foreign` records
  under the replay and invariant asserts, and the no-op table deletes
  a comment of theirs by name again; and
  `fields_dirty_compares_with_the_saved_form` builds a consistent
  state (the rebase's assert refused its old one), and saves
  every_kind's own file for real, where a rebase onto blank pages
  bound no node to anything. `mark_adds_on_the_heavy_7k` asserts the
  heavy file holds at least 20,000 annotations, and
  `the_comments_panel_lists_what_the_file_holds` pins the frozen
  corpus's totals below.
- **Interop past the plan:** the ignored
  `bookmarks_links_and_comments_come_through_both_strategies` (a URI
  bookmark with `/SE`, a link and two comments, a reorder and a
  rename appended, then one comment deleted and rewritten), since the
  suite had nothing slice 8's writers changed.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The 7k harness,** synthetic and heavy, two runs at the end
  (slice 7's in brackets; slice 6's where slice 7 gave none):

  | Row | Synthetic | Heavy |
  |---|---|---|
  | geometry walk | 48.1 ms [48.9–49.5], asserted | 74.5–77.7 ms [75.8–75.9], asserted |
  | classify every page (blocks), cold | 142–152 ms [144–146] | 203–206 ms [202] |
  | ready | 54.0–65.0 ms [49.9–50.1] | 349–356 ms [377–386], asserted |
  | ready, peak / added RSS | 35.3–35.4 / 28.5–28.6 MB [29.5 / 22.9] | 218 / 199 MB [278 / 258] |
  | incremental save, one mark | 57.2–63.9 ms [62.9–83.5] | 289–325 ms [288–292] |
  | incremental save, RSS added | 36.6–36.7 MB [36.9] | 211–213 MB [211–213] |
  | Rewrite | 132–135 ms [123–126] | 602–604 ms [568–585] |
  | Rewrite, peak / added RSS | 103–105 / 52.5–53.1 MB [99–102 / 52.9–53.2] | 488–490 / 247 MB [547–548 / 246–250] |
  | crop all pages / the edit | 158–162 / 12.0–12.2 ms [270 / 1.33] | 706–710 / 11.8–12.2 ms [828 / 1.20] |
  | Bates apply / its Rewrite | 148–157 / 372–380 ms [141 / 361–363] | 207–211 / 802–803 ms [202 / 797–816] |
  | flatten | 284 ms [189–284] | 1,369–1,390 ms [1,400–1,510] |
  | incremental save, one mark, AES-256 | 285 ms [285–317] | 574–658 ms [582–665] |
  | Rewrite, AES-256 | 191–219 ms [213–214] | 1,031–1,095 ms [1,075–1,144] |

  Renders and previews are as before (heavy cold 747–787 ms, warm
  4.94–5.00 ms, 169–171 pages/s). The Rewrites are 3–10% slower:
  they build the projection once a snapshot (each page's removed
  comments from its source's fact, the legacy fields) and walk the
  outline records; not measured apart (Bates' Rewrite moves 0–5%, the
  Rewrite of 20,000 marks 1%). **The crop's edit is ten times
  slower,** 12 ms against 1.2–1.3: RebasePages of 7,000 pages now
  puts a `Page` and a `Geom` per page, 14,000 prims, where it put one
  page list; §3.9 costs a whole-document turn at 7,000 `Geom`s, and
  slice 15's crop commit gate (< 200 ms) is on the whole crop,
  158–710 ms today. Ready is 7–10% faster on the heavy fixture, and
  its RSS a fifth under slice 6's; 4–15 ms slower on the synthetic
  (one run 54 ms).
- **History** (`history_at_7k_pages_and_20k_marks`, asserted): AnnotAdd
  0.014–0.015 ms [0.699], a full reorder 0.713–0.739 ms [0.950].
  **1,000 mark adds** (`mark_adds_on_the_heavy_7k`, asserted, new):

  | | median | first 100 | last 100 |
  |---|---|---|---|
  | heavy 7k, 21,000 annotations held | 0.144 ms | 0.041 ms | 0.183–0.185 ms |
  | 7,000 blank pages, none held | 0.134–0.147 ms | 0.028–0.033 ms | 0.180–0.181 ms |

  against slice 1's 0.686 ms on blank pages with 20,000 marks. With
  the old scan put back in `fresh_annot_id` the heavy's median was
  1.15 ms. 200 OCR versions: 4,094 MB of steps [4,074] and 4,768 MB of
  RSS growth [4,738]. Ready on 20,000 sheaf marks 531–534 ms [544],
  under 1 s, asserted; their Rewrite 1,141–1,142 ms [1,126] and
  238–239 MB added [228].
- **The replay property:** 64 cases 0.24 s (debug) and 0.17 s
  (release); 4,096 cases 8.8–8.9 s in release (6.4 s without the
  invariant asserts); past `MAX_VERSIONS` 5.2 s (debug), 3.2 s
  (release), 1.3 s without the asserts. With other tools'
  annotations in the run (review): 64 cases 0.33 s (debug) and 0.24
  s (release), 4,096 cases 13.2 s, past `MAX_VERSIONS` 5.3 s and
  3.26 s. Each of these mutations failed
  the 64 cases and was put back: `set` not updating the `/NM` index,
  not resetting positions on a new order, not reindexing names, and
  `coalesce` keeping each step's own before-value.
- **The corpus** (with ../pdf-corpus): the Comments listing of every
  manifest file, dumped before step 3's edits and after them,
  byte-identical: 4,392 files, 4,389 open (3 don't, as before), 233
  with comments, 898 threads and 943 comments, all other tools'
  (none sheaf's), none hidden. Every file that opens opens as a
  Document whose state keeps the invariants (4,389).
- **Accounting:** renaming one bookmark of 2,000 costs 942 step bytes
  and 1,864 snapshot bytes (375,627 step bytes before).
- **Interop** (qpdf 12.3.2, node 24 and pdfjs-dist 6.4.299): the
  ignored suite passes, 3 tests: `qpdf --check` clean on 30 outputs
  (the 28 of slice 7's bases and the 2 of the new test), the prefixed
  append identical in qpdf and pdf.js, and the new test's outputs read
  in pdf.js as written: "Site" opening https://example.com/, "3" going
  to page 1, the link on page 2 and the comment kept, the deleted one
  there in the Append and gone from the Rewrite. Dropping the `Kept`
  bookmark's `/A` from the writer fails it ("Site" <undefined>).
- **Suites:** `cargo test --workspace` 808 passed, 25 ignored (the
  new bench and interop test), in 2 m 22 s (786 and 23 at slice 7);
  sheaf-doc's unit tests 458 in 29.0 s (439 in 30.1 s). After the
  review's fixes, 814 passed and 25 ignored, sheaf-doc's unit tests
  463 in 29.0 s; the ignored interop suite passes as above, `qpdf
  --check` clean on its 30 outputs, and the mark adds bench holds
  21,000 annotations (0.145 ms median, last 100 0.184 ms). fmt and
  clippy (`-D warnings`, all targets) are clean; `cargo check
  --target x86_64-pc-windows-msvc -p sheaf-doc -p sheaf-ops` passes,
  with `--tests` too.

Found:
- **A version push costs more as history grows:** `trim` builds the
  line, walks every version for `droppable`, and `stats_for` asks
  `droppable.contains` for each kept snapshot, O(snapshots²), at
  every push. From the 1st to the 1,000th version an add goes from
  0.03 to 0.18 ms, on any document; the mark adds bench shows it.
  Not fixed: slice 12 replaces trimming by count with a byte bound.
- **Real files break name uniqueness:** pages repeating an `/NM`, and
  forms repeating full names, are in the corpus, so the indexes hold
  every record of a name.
- **Annotations written straight into `/Annots`** as dictionaries: 16
  in 5 corpus files.

Checked by tests:
- **The model tests are ported:** model.rs's, prim.rs's,
  open/tests.rs' and history_tests' over every table, with new ones
  for each: `a_page_listed_twice_opens_as_copy_0_and_copy_1`,
  `copies_take_the_next_copy_of_their_base`,
  `a_reorder_puts_only_the_order`, `ocr_text_is_the_page_s_content`,
  `positions_follow_the_order`; `a_bookmark_edit_puts_one_bookmark`,
  `a_uri_bookmark_is_kept` (`/A /URI` and `/SE` through a reorder by
  Rewrite and an Append, and `src` rebound after a save),
  `the_header_footer_is_a_generator`,
  `renaming_a_bookmark_costs_it_not_the_outline`;
  `reopening_gives_their_comments_the_same_ids`;
  `fields_are_found_by_their_full_names` (and after a replay),
  `a_template_s_widgets_are_outside`,
  `an_empty_partial_name_is_passed_over`, stronger
  `any_field_tree_reads`; `the_invariants_find_each_rule_broken` (25
  breaks, each found by its own message: every rule of the pages,
  annotations, form, outline and generators, and the outline edits
  refusing a missing page), `the_invariants_find_each_index_broken`
  (state.rs: six breaks of the positions, the `/NM` index and the
  full-name index) and `a_page_listed_twice_is_replaced_once`. The history and
  model suites run on the eleven keys with `check_invariants` after
  every edit and action.
- **The comments panel lists the same comments over the corpus:** the
  before-and-after dump (Measured), and the corpus test
  `the_comments_panel_lists_what_the_file_holds` (sheaf-ops
  tests/comments.rs) against `file::annots_in`'s marks and comments,
  with `links_are_foreign_records`; it asserts the totals (4,392
  files, 4,389 open, 233 with comments, 898 threads, 943 comments), so
  a change in what the reader finds fails it too.
- **Deleting another tool's comment and saving by both strategies
  leaves its string absent:**
  `deleted_comments_and_their_threads_stay_out_of_the_file` (the
  comment and its thread's `/Contents` absent from the decoded bytes
  of the Rewrite and of the save in place, which a removal makes a
  Rewrite), with `a_save_rebinds_marks_and_their_comments` and the
  adoption checks in
  `a_comment_made_ours_takes_its_place_and_keeps_its_replies`,
  `a_comment_made_ours_is_saved_in_place` (an Incremental save, then
  the mark deleted is a removal) and
  `of_two_comments_named_alike_the_one_made_ours_goes`; a link to a
  deleted page gone after the save that left it out
  (`a_link_a_save_left_out_is_gone_after_it`); and the interop test's
  Rewrite in pdf.js.
- **Duplicating a page whose comment was deleted doesn't bring it
  back:** `a_duplicate_of_a_page_without_their_comment_stays_without_it`
  (the model, and both saves' bytes).
- **M10 slice 4's merge and rename tests and M10 bugs 2–4 pass by
  both strategies, with `forms::check` clean:** sheaf-ops'
  forms_merge.rs, forms_pages.rs, forms_saves.rs and forms_open.rs,
  unchanged in substance, on `FieldNode`/`FormInst` records through
  the projection; with `form_fields_follow_their_pages`,
  `fields_dirty_compares_with_the_saved_form`,
  `inserting_the_same_file_twice_gives_two_registrations_and_two_forms`
  and `scripts_from_two_files_stay_off_after_a_save`.
- **`page_templates_stay` passes** (forms_pages.rs).
- **`a_files_pages_in_runs_join_one_form` and
  `corpus_a_form_combined_with_itself_is_two_forms` pass on records,**
  one instance per registration (forms_merge.rs).
- **A save, an edit and a second save keep everything:**
  `a_save_an_edit_and_a_second_save_keep_everything` (forms_saves.rs:
  each old strategy, marks, comments, fields and bookmarks compared
  after a reopen, `forms::check` clean), with
  `rebase_after_save_keeps_ids_and_history`,
  `fields_named_alike_stay_two_across_saves` (two fields of one name,
  each its own object after a save, both copied with their page) and
  `a_bookmark_after_a_deep_branch_keeps_its_own_item` (35 levels: the
  deep ones from no item, a URI bookmark after them its own `/A`
  through a second save).
- **The bench:** `mark_adds_on_the_heavy_7k` (ignored, run in release,
  Measured).
- **The replay property on the new keys:**
  `prims_rebuild_every_version` (64 cases),
  `prims_rebuild_every_version_4096` (ignored) and
  `prims_rebuild_every_version_past_max_versions`, the registration
  check covering page definitions, field and widget instances and
  form names; the mutations above.
- **Blob liveness:** `blob_liveness` (an attached file and a placed
  stamp, every caller's blob dropped, then undo, redo and a reopen
  after each old strategy: the bytes and the stamp's page are
  intact), with `a_stamp_resolves_from_the_store_a_source_or_the_cache`,
  `an_attachment_is_a_blob_never_a_cache_file`,
  `store::a_stem_names_its_blob`, save.rs's
  `stamps_and_attachments_survive_an_empty_cache`,
  `a_page_stamp_is_written_once_under_a_name_of_its_own`,
  `two_attachments_of_one_file_embed_it_once` and
  `a_file_whose_pages_come_in_cant_replace_a_stamp`, exchange's
  `our_marks_bring_their_stamps_and_files`, combine's
  `marks_bring_what_they_hold`, the app's
  `copied_marks_take_what_they_hold` and `a_picture_is_asked_for_once`.
- **Also:** `the_app_opens_as_the_command_line_does` (whole states
  equal), the corpus open test asserting the invariants, and the
  ledger's `nothing_retired_is_back` with the 92 rows.

Left for later slices:
- **Slice 9:** the emitter identifies a page by (src, index, copy);
  writes `Kept` bookmarks from other sources (inserted files, an old
  primary after undo across a save) with their actions; decides the
  records of Popup, Screen and the other unmodeled subtypes; writes
  the owned AcroForm keys and XFA state from `FormDoc` (which gains
  them); keeps other tools' replies to marks given fresh ids on a
  duplicate or insert (the old writers still drop them); and filters
  `save::subset`'s state, or checks it with care. It deletes the
  projection, `Snapshot.legacy`, the baseline with the dirty flags,
  the adapted rebase, `set_source_renamed` (5→9) and what's left of
  the app's read after a save; `WriteCx`'s trust that a stem names the
  same bytes goes with the old writers.
- **Slice 11:** `SourceId = BlobId`, so `PageBase.source` and
  `FormInst.src` go; `reply_to`/`partner` by id and §6.5's thread
  remap inside a duplicated set (bug 12); `NewMark` and mark style;
  the cascades (proposal 25's widget.field, page.labels, the header's
  page scope); the invariants left above (label anchors, the header's
  scope, reply parents and connector partners; name uniqueness stays
  unasserted); `bring_annots`, `take_in_fields` and `copy_fields` as
  lowering; decoding attachments in the resolve phase, off the UI
  thread; `Tx::pin` for `Imported.held` and `import::Interned`.
- **Slice 12:** a push that doesn't walk history (Found), with the
  byte bound.
- **Slice 13:** `PageDef.digest` and render keys; hiding others'
  comments as data (the app computes it from facts by name); the
  overlay off `Generator`.
- **Slice 14:** search over `ContentStack`'s text, and
  `docview::index_of_source_page`, which still scans by (source,
  index) for search hits.
- **Slices 16–17:** `Generator`'s `Scope`, `Frozen`, `z` and
  `replaces`, other `GenSpec`s, `/SheafGenerators` and `/SheafBlock`
  at open, and `ContentStack`'s filters and `under`.
- **Slice 18:** `Layer` ids and kinds (`BlockId`), OCR as blocks.
- **Slice 19:** the stamps' generated-cache fallback
  (`model::find_stamp`'s cache arm) and the stamps panel's cache
  write.
