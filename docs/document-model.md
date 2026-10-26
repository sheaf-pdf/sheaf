# Document model and edit history

Status: accepted 2026-10-11. Decides the shape of `sheaf-doc` for M2+.
M10.5 (the edit API, `plan/complete/m10.5-edit-api/README.md`) rebuilt it in twenty
slices, each of which wrote its section below; slice 20 made the last
pass, with the invariants and the tests that hold them.

## The problem

Three things want to look at "the document" and they disagree about what it is:

- **PDFium** (rendering, text, search) sees a *file*: bytes it parsed once.
  Reparsing is cheap for small files and ~100 ms for a 7,000-page one, so we
  don't want every edit to reserialize.
- **lopdf** sees an *object graph*: dictionaries and streams with
  numbers. It's what a file is read as (lazily, a source at a time: M10.5
  slice 3) and what sheaf's own serializer writes (slice 7; lopdf's
  writer writes nothing outside tests from slice 20), but it is a
  terrible structure to edit interactively: nothing has stable identity,
  and inverse operations are hard to get right.
- **The user** sees a *sequence of pages* with bookmarks and markup, and
  expects reorder/rotate/delete/insert to be instant and undoable.

The user's view is the one to model. The other two become *inputs* (sources)
and *outputs* (serialization) of it.

## Core idea: a virtual document over immutable sources

```
Store                the document's bytes, named by their content: one Blob for
                     the same bytes however often they're interned, and one Src
                     for a Blob while anything holds it. Weak references only.

Blob                 { id: BlobId { hash, len },   // compared byte for byte too
                       bytes: Arc<[u8]>,           // shared with PDFium
                       kind: Pdf { password, origin: File(path) | Memory(Library | Recovered) } }

Src = Arc<Source>    a Blob parsed lazily (lopdf's LazyPdf), under one Limits,
                     with the facts read from it once: the page table (each
                     page's Geom, inherited /Resources, /Contents, /Annots),
                     XrefFacts, conformance. Immutable.

SourceId = BlobId    a source is its blob (M10.5 slice 11): the same file
                     inserted twice is one Src. Which copy of its pages and
                     of its form each insert brought is the page's (copy) and
                     the form instance's (FormInst), never the source's.

PageDef              { base: PageBase { src, index }, copy: u32,
                       content: Arc<ContentStack> }
                     what one page of the virtual document shows: page index
                     of a source (src, the bytes it pins, by whose id render
                     handles are keyed), which copy of it (0 for the page
                     itself, fixed when the page is made: a duplicate is one
                     past the highest copy its base has), and what sheaf draws
                     over it. Its PageId is a fresh, stable identity (never
                     reused); its boxes and absolute /Rotate are its Geom.

ContentStack         { over: im::Vector<Layer { body: Static }> }
                     drawn over the page's own content, bottom first; the one
                     layer so far is Static::OcrText(Arc<TextLayer>). Every
                     page with nothing drawn over it shares one empty stack.

AnnotRec             { page: PageId, body: Mark | Foreign | Widget }
                     Mark    { annot: Annot, src: Option<ObjRef>,
                               file: Option<BlobRef>, stamp: Option<FormRef> }
                     Foreign { src: Option<ObjRef>, copy, summary }
                     Widget  { field: FieldId, src, inst: FormInst, merged,
                               rect, flags, mk, da, state, states, props }
                     one annotation in one id space: sheaf's mark, holding its
                     attachment's file and its stamp's page (Page content);
                     another tool's comment or link, kept as the object it was
                     read as (src: None for a dictionary written straight into
                     /Annots, found by its slot); a field's widget.

FieldNode            { parent, partial: Arc<str>, src: Option<ObjRef>,
                       inst: FormInst { src: SourceId, copy }, kids,
                       term: Option<Arc<Terminal>> }
                     a node of the field tree. A terminal holds its kind,
                     flags, value, default and props, its widgets by AnnotId,
                     and outside, its widgets no page lists (page
                     templates, which a save keeps). A FormInst is one
                     copy of a file's form: one per insert (a file's page
                     selection is one insert) and one per duplicate, its copy
                     the next its source has, fixed as it's made; copy 0 of
                     the primary's is its own objects.

FormDoc              { roots, names: im::OrdMap<FormInst, Arc<str>>,
                       scripts: On | Off { instances } }
                     the form as a whole: its top-level nodes, the name each
                     instance is renamed apart under, and whether its scripts
                     still run (off once two scripted instances meet).

Bookmark             { title: Arc<str>, target: Option<Page { page, view } | Kept>,
                       color, style, open, kids: im::Vector<OutlineId>,
                       src: Option<ObjRef> }
                     Kept: an action or destination sheaf doesn't model (a
                     URI, another file), written from the item it was read as.

LabelSpec            { start: PageId, style, prefix, first }
                     a page-label range, anchored to the page it starts at.

Generator            { id: GenId, spec: GenSpec, z: Z, scope: Scope,
                       frozen: Frozen, replaces: BTreeSet<PaginationKind> }
                     what sheaf draws on pages from one definition: a
                     watermark or a background (GenSpec::Watermark, M10.5
                     slice 16) and the header and footer
                     (GenSpec::HeaderFooter, slice 17: its live page tokens
                     read where each page is, its date, file and Bates
                     numbers fixed when it was set, in Frozen), laid out on
                     each page it covers when a save or a view draws the
                     page, found again at open from /SHEF_Generators
                     (Watermarks and backgrounds as generators; Headers,
                     footers and Bates as generators).

DocState             { order: im::Vector<PageId>,
                       pages: im::HashMap<PageId, Arc<PageDef>>,
                       geoms: im::HashMap<PageId, Arc<Geom>>,
                       bookmarks: im::HashMap<OutlineId, Arc<Bookmark>>,  // kids by id
                       outline_roots: im::Vector<OutlineId>,
                       page_annots: im::HashMap<PageId, im::Vector<AnnotId>>,  // /Annots order
                       annots: im::HashMap<AnnotId, AnnotRec>,  // sheaf's marks, others'
                                                                // comments and links, and
                                                                // the form's widgets
                       fields: im::OrdMap<FieldId, Arc<FieldNode>>,  // the field tree, each
                                                                // node in an instance of a
                                                                // file's form (FormInst)
                       form: Arc<FormDoc>,          // its roots, instances, scripts
                       labels: Option<Arc<[LabelSpec]>>,
                       generators: im::OrdMap<GenId, Arc<Generator>> }  // the header/footer,
                                                                // watermarks, backgrounds
                     a complete, immutable snapshot of what the user sees,
                     made by DocState::builder() and read through accessors.
                     Records hold Srcs and blobs, so a version keeps the bytes
                     it shows alive. It keeps three indexes, never compared:
                     each page's position (built the first time one is asked,
                     shared by the clones of one order), every named
                     annotation by page and /NM, and every field node by its
                     full name; neither name need be unique in a file, so each
                     holds every record it names. check_invariants says the
                     tables and indexes agree (a debug build asserts it after
                     every edit, at open and after a save).

Document             { store: Arc<Store>,
                       primary: Src, name: Arc<str>,  // the file it opened on
                       versions: HashMap<VersionId, Version>,  // a tree (see Undo/redo)
                       cursor: VersionId,            // which version is current
                       saved: Option<VersionId> }    // which version is on disk, its
                                                     // record the file's (a restored
                                                     // document's with the sidecar's
                                                     // sections pending)

Snapshot             { version: VersionId, state: DocState, primary: Src, name,
                       file: Option<Arc<Path>> }  // the document's file
                     what exports, print and plugins read, taken from the
                     current version; an export never replaces `file`.
                     Nothing is computed when it's taken: no dirty flags
                     compare it with a baseline (slice 9 deleted them, the
                     old writers' legacy projection and the renames table
                     with the old writers). What a save does is its plan's
                     (`materialize::plan`), from the records and the saved
                     file's record.
```

**Editing never touches a source or a lopdf object.** An edit is an op
lowered in a transaction (M10.5 slice 11; see Ops and transactions): a
pure function of the state, the sources' facts and the host's attributes
that builds a new snapshot sharing everything it didn't change. Reordering 7,000 pages puts one new order, an
`im::Vector` that shares all but the paths the move changed, and no page's
record.

**Rendering never waits on an edit.** The viewer displays the pages in
order; a render request for position *i* becomes the page's base (its
source's engine and its page index) and its `render_delta`, the turn
PDFium adds to the source page's own `/Rotate`. PDFium is never told about
reorders, deletions, or insertions. Inserting a 400-page PDF means
interning it as a second source, splicing 400 ids into the order and
adding their records — milliseconds.

**Saving is a materialization, and an output.** Given a version's
`DocState` and its sources, produce bytes and a record of what they hold
(see Saving): nothing when the state is what the file holds, an update
after the file of what changed since, or the whole file when something
was removed. The version is sealed while it's written and the state is
immutable and `Send`, so saving runs on a background thread while the user
keeps editing; a save changes no version, state or source, so undo across
it is exact and what the screen shows stays.

**Sources are read lazily** (M10.5 slice 3). A file is read once, into a
blob, and a source parses the blob in place:

- **The open (`Store::source`) reads the cross-reference sections, the
  trailer and the page tree.** Every other object is read when it's first
  asked for, decrypted on its own (a source keeps `/Encrypt`), and kept,
  as is a failure to read it that the bytes alone decide; an object
  stream is decoded once. `XrefFacts` records what a later
  append needs: the newest section's kind and start, `/Size`, the largest
  cross-reference stream number, `/ID` and where the header starts.
- **The page table** numbers pages as PDFium does (a kid with `/Kids` is
  a node, any other a page) and reads the inheritable keys up `/Parent`
  as it does. Each page's `Geom` is its boxes and rotation as PDFium
  sizes them: an empty media box is Letter, the crop box is clipped to the
  media box (one outside it shows nothing), `/Rotate` is whole quarter
  turns, `(degrees / 90) mod 4`, and `/UserUnit` doesn't scale the size.
  The layout's page sizes are `Geom::shown()` from the first paint, and
  equal PDFium's on every corpus page; PDFium isn't asked for sizes. A
  page the tree lists twice is two entries, which share what the page
  holds, as pages naming one `/Annots` array share it. The table ends at
  the root's `/Count` when PDFium counts by it, since PDFium loads no
  page past it.
- **One `Limits`** caps every reader of a source: lopdf's (256 MiB a
  stream decompressed, 1,024 sections, 8,388,607 objects and twice as
  many cross-reference stream entries, references 64 deep), the page
  tree's (1,024 levels, fewer than 0xFFFFF pages), the form reader's and,
  from slice 6, the content lexer's (`Limits.content`: 256 MiB of a
  page's streams decompressed together, 65,536 operands before an
  operator, arrays, dictionaries and marked content 512 deep, and 64 MiB
  of an inline image's data).
  Breaking one, a `/Kids` cycle, a `/Parent` loop or an object that
  doesn't decrypt is a typed error, never a panic, a hang or memory
  without bound; nothing is repaired. So the app doesn't open such a
  file, though PDFium shows some of them (none in the corpus: every
  corpus file PDFium opens opens as a source).
- **A table that can't be read** is rebuilt by scanning, as lopdf's eager
  reader rebuilds it, and every object is read then: `base_sound()` is
  false, and such a file is never appended to. With no `trailer` keyword,
  the newest cross-reference stream that decodes is the trailer, as
  PDFium takes it.
- **Bytes stay in memory.** PDFium opens the blob's own `Arc<[u8]>`
  (the engine pool's base engine, `Views::base`). No edit makes a copy
  of a page's file: not a crop (M10.5 slice 15), a watermark (slice 16)
  or a header and footer (slice 17, which deleted derived copies); blank
  and image pages, a scan's and a library stamp's are made in memory, as
  `Memory` blobs (slice 19, which deleted the generated cache; see
  Exports, memory sources, Session and `sheaf edit`).
  `LazyPdf` can also read a file by position (`FileBytes`).
- **The open reads every fact from the source** (M10.5 slice 4; see
  Opening a file): its marks, other tools' comments, labels, header and
  footer, form, attachments, layers and outline, as File > Properties
  and the signature check read theirs. Nothing it reads loads the file
  eagerly; a Rewrite loads its primary eagerly for `keep_only`
  (`materialize::graph`).

At 7,000 pages the open (the geometry walk, asserted by the 7k gate
harness, `crates/sheaf-ops/tests/gate_7k.rs`) takes 48 ms on the
synthetic fixture and 72–80 ms on the heavy one, against gates of 100 and
250 ms.

## Invariants, and the tests that hold them (M10.5)

The design's twelve rules (§2 of `plan/complete/m10.5-edit-api/README.md`), its
registry rule and "nothing ships unproduced", as M10.5 left them, each
with what holds it. "Corpus" tiers read `../pdf-corpus` (or
`$SHEAF_CORPUS`) and skip without it; "ignored" ones run before a merge.

1. **DocState is the document's definition**, and one emitter derives
   every object from it; it holds no object number that can go stale,
   and provenance (`ObjRef`) points only into immutable sources. Held by
   Append ≡ Rewrite (`materialize::append_rewrite_tests`'
   `append_rewrite_reopen_alike`, 32 cases by default and 1,024 in the
   release property `append_rewrite_reopen_alike_1024`, and its named
   cases), `DocState::check_invariants` (asserted after every edit, at
   open and after a save in a debug build, and after every action of the
   replay property), and sheaf-doc's `tests/object_ids.rs` (rule 7).
2. **Sources are immutable, lazily read and content-addressed**, facts a
   memoized function of a source's bytes, and records hold liveness
   handles. Held by `source::lazy_tests`' `lazy_objects_equal_the_eager_load`
   and the corpus's `every_corpus_page_s_geom_is_pdfium_s_size`
   (sheaf-ops' `source_corpus`), `store`'s
   `interning_the_same_bytes_twice_gives_one_blob`, and history's
   `pruning_an_undone_400_page_scan_frees_its_bytes` (a blob goes with
   the last record holding it).
3. **Two layers and one primitive:** ops lower to `Put { key, before,
   after }` over eleven key kinds. Held by the type (`Prim` has one
   variant) and by the replay property, which applies only prims.
4. **History stores resolved prims with before-values; a `VersionId`
   names one immutable state; a commit onto a moved cursor validates by
   value what it read or overwrites.** Held by `model::history_tests`'
   replay property, `prims_rebuild_every_version` (64 cases;
   `prims_rebuild_every_version_4096` in release; every version rebuilt
   from its prims equals the state it was made as, `Mismatch` on a wrong
   before), `each_kind_s_no_op_pushes_nothing`, and `tx::stale_tests`
   (`Validate` and `Relower`, and from slice 20 handed-out ids:
   `a_handed_out_relowering_that_makes_more_conflicts`).
5. **Effects are a pure function of `(key, before, after)`.** Held by
   `effects/tests.rs`, which classifies puts by their values alone
   (`changed_is_by_value`, `what_differs_means_something`,
   `a_misfit_is_everything`), and `model::change_tests`
   (`a_save_changes_nothing`).
6. **One frame below the page: PageSpace;** box and rotate edits never
   rewrite a stored coordinate, and rotation is absolute. Held by
   `open::tests`' `every_mark_kind_round_trips_on_every_rotate`,
   `model`'s `a_mark_on_a_rotated_page_stays_put_through_a_save` (both
   ways), `tx::boxes_tests`' `a_crop_writes_the_boxes_it_changes_alone`,
   and, for plugins, host.rs's PageSpace tests on `turned_doc`
   (`a_plugin_places_through_the_page_s_view_matrices`).
7. **No id is an ObjectId, a slot or an index;** ids written to files
   are random below 2^53, session ids are counters never reused. Held by
   `tests/object_ids.rs` (slice 20: no lopdf `ObjectId` in model-level
   code, outside a reasoned allowlist), `model`'s
   `mark_ids_are_random_and_distinct`, `open::tests`'
   `a_copied_page_gets_fresh_ids`, `tx::tests`'
   `ids_mean_nothing_in_ops_read_from_a_file`, and the replay property
   (a replay draws no id).
8. **A save is an output:** the model never re-expresses a version, a
   `SaveRecord` drives the next save, and the version being saved is
   sealed. Held by `change_tests`' `a_save_changes_nothing`, `model`'s
   `an_amend_on_a_sealed_version_makes_a_child` and
   `record_save_and_abandon_save_unseal`, and `saving_tests`'
   `an_unchanged_plan_writes_nothing`.
9. **One emitter** feeds the Rewrite, the Append, the recovery section
   and the render overlay. Held by Append ≡ Rewrite, the determinism
   tests (`materialize::tests`' `a_rewrite_is_the_same_bytes_every_time`,
   `append_tests`' `the_same_section_twice_is_the_same_bytes`,
   `serialize::tests`' `full_writes_are_deterministic`, and the corpus's
   `the_same_document_writes_the_same_bytes_twice_and_from_two_threads`),
   `recovery`'s `sidecar_round_trips_and_detects_staleness`, and
   `materialize::overlay_tests`.
10. **The screen equals the file** (GPUI draws sheaf's marks until M10
    slice 12). Held by sheaf-ops' `tests/realize.rs` (`…_render_as_saved`,
    `print_preview_draws_what_prints`) and the ignored corpus tiers
    `realizations_render_as_saved`, `scans_render_as_saved` and the
    untouched predicate's
    (`the_predicate_agrees_with_the_emission_on_every_page`).
11. **Nothing touches disk** but the user's files, their temp files, the
    sidecar and the journal. Held by sheaf-ops' `tests/exports.rs`
    (`every_way_out`, in a child process whose temp, cache and home
    directories are empty and must stay so), sheaf-cli's
    `the_command_line_writes_nothing_but_its_outputs`, and the
    `std::env::temp_dir` clippy ban (Lint ledgers).
12. **Fail, never repair, and fail closed on removal.** Held by
    `source::tests` (`cyclic_kids_are_a_typed_error` and the limits),
    the fuzz targets (nightly), `removal_tests`
    (`a_forced_misclassification_is_vetoed_and_counted`), and the corpus's
    `deleted_means_deleted_both_ways` (ignored) and
    `the_removal_check_finds_what_a_page_delete_rewrite_took_out`.

- **The registry rule** (every cross-cutting function destructures
  `DocState` with no `..`, or matches `Key` exhaustively) is held by the
  compiler: a new table or key kind fails to build until each of them
  handles it (the plan's names were sketches; the code's are a put's
  effects, `effects::judged`; the cascades' triggers; `ChangeSet::of`;
  an Append's closure, `materialize::deps::closure`; `step_bytes` and
  `DocState::snapshot_bytes`; `DocState::{get, set}`;
  `check_invariants`; and `render_tables`, which the render key reads).
- **Nothing ships unproduced** (no key, variant, field or public type
  without a producer and a consumer in M10.5): held by review and by
  clippy's `-D warnings` for what's private (a helper whose last caller
  goes fails it), and §19 of the plan lists each public type with its
  producer. What a slice deletes stays deleted: `tests/retired.rs`'
  `nothing_retired_is_back`, a `syn` walk of every crate against the
  retired names.
- **sheaf's private data is a table** (§15, slice 19): an output
  written without it holds none (`private_tests`'
  `no_sheaf_name_but_the_private_keys_is_written`, and `private::found`
  run over every Strip output), and an output that would is refused.

## Opening a file (M10.5 slice 4)

A file opens once, the same way in the app, on the command line and in
every other headless caller (`sheaf_doc::open`; `open::file` takes the
three steps in a row; from M10.5 slice 19 `open::read(path, password,
p)` takes them with progress, which `open::file` calls without, and the
command line's `Session::open` runs, see Exports, memory sources,
Session and `sheaf edit`):

- **A save a crash cut short is undone first** (M10.5 slice 10): when a
  journal beside the file says an append in place didn't finish,
  `serialize::append::undo_torn` cuts the file back to what it was
  before it, its time too, while its bytes before it are the ones the
  journal stamped, and leaves it, with a notice, when they aren't.
  `open::file` reports it (`Opened.torn`); the app runs it in
  `DocView::open` before it looks for a recovery sidecar, and shows the
  notice.
- **`start` makes the document from its pages.** Its one version holds
  the pages from the page table the source built as it opened, and
  nothing else, so the first page paints at once. It reads nothing of
  its own. (`Document::new` is for a file sheaf has just made, which says
  nothing more about itself: it's complete at once, with its outline and
  the annotations its file holds.)
- **`finish` reads the rest, on any thread, from the source alone:** sheaf's
  marks and other tools' comments and links in one pass over the page
  table's annotations, read once a source (`Source::annots`, which an insert
  reads too), each with its object and an id (a page stamp's page and an
  attachment's file are found in the file, not in this machine's cache, and
  a mark whose file doesn't hold them is another tool's comment; a mark
  another tool has edited since sheaf wrote it, whose `/SHEF_Annot` digest
  no longer matches, is that tool's comment; copies of marks another program
  made, which say ids earlier marks have, get new ones; other tools' get ids
  no mark has, drawn from an rng the file's hash seeds, so the same bytes
  open with the same ids), the form (`Source::form`, also read once a
  source: every node of the field tree with the object it was read as, each
  terminal's widgets by page, and those no page lists, a page template's),
  the page labels, the generators sheaf drew (`/SHEF_Generators`, read
  as written, the pages each is on from its blocks' stream markers; from
  slice 17 the header and footer too), attachments, the portfolio
  schema, layers, and the outline with its colors, styles and closed
  state (`/C`, `/F`, a negative `/Count`), each
  item with its object and whether it holds an action or destination sheaf
  doesn't model (`kept`). Of a page it reads the dictionary and the
  annotations, never its content (`Source::blocks` reads that when an op
  first asks; see Page content). Each reader passes over what it can't read,
  so `finish` doesn't fail once the source has opened, and reads a tree a
  node at a time however often the file lists a node (a name or number tree,
  the layers' order, a portfolio's folders, the outline), so it ends. It
  counts the pages of its first pass to the progress it's given. The
  outline's destinations (`/Dest`, named destinations, a GoTo action's `/D`)
  are found through the page table, and their views are read as PDFium reads
  them, their positions kept in PageSpace as the file has them; the renderer
  is never asked for the outline.
- **`Document::ready(facts)` builds the complete opened version once**
  and puts it in the loading version's place under a new `VersionId`,
  the way a group's amend does. The pages keep their ids, and the current
  and saved versions follow it, with the opened file's `SaveRecord`
  (synthesized at open, see Saving), so the document isn't dirty and
  there's nothing to undo. It is records (M10.5 slice 8; see the Core
  idea): each page a `PageDef` over its source page, copy 0 (a page the
  tree lists twice is copies 0 and 1 of one object), its `Geom` the page
  table's. Every annotation is a record: sheaf's marks, holding the stamp
  and file their objects hold, other tools' comments and links, and the
  form's widgets, each the object it was read as, in `/Annots` order;
  other annotations (popups, screens and the rest sheaf doesn't model)
  stay in the page's file, and a save passes them through. So is
  every node of the form's field tree, numbered in tree order, a terminal
  holding its value and its widgets by id, all in one instance of the
  file's form; every bookmark, with the item it was read as, a `Kept`
  target for one whose action sheaf doesn't model; the page labels; and
  the header and footer, a `Generator`. Its `GenId`, then the widgets'
  ids (none an id the file's annotations have), are drawn from the rng
  the file's hash seeds, so the app's open and the command line's make
  the same state. It gives back what the view shows beside the model:
  attachments,
  the portfolio, layers, what sheaf says about the form, and the marks
  given new ids, whose pages the next save writes, and which the app
  counts in a notice. Nothing is hidden in an engine at ready (from M10.5
  slice 13): under the screen's inputs a page's realization leaves
  sheaf's marks to GPUI, whatever ids they had. A second call is
  refused.
- **Until then the document is loading.** A transaction's ops and
  `commit` return `NotReady`, making no version and drawing no ids.
  The app greys out what edits the document or writes it out (saves,
  exports, print, split, extract, comment import, page and bookmark
  edits, the markup tools, undo and redo) and every plugin command, says
  "Opening…" in the status bar, with the page the first pass is on, and
  refuses any of them run by its keys with a notice (an insert or a
  replace before it reads the file it was given). A plugin call given
  meanwhile (a command, a panel action, an event) waits: once the file is
  ready, plugins get `opened`, then the calls that waited run, in order,
  each seeing the document as it is then. Only a plugin's newest region
  event waits, and a file closed before it's ready drops what waited,
  saying `closed` only after an `opened` (a recovery opens a file again).
- **There are no late facts.** Everything the file says arrives in the
  one `ready`, so nothing is read after an edit and dropped; a file
  that's slow to read keeps edits waiting longer, with its progress
  shown.

On the heavy 7k fixture, ready (the source's open, `finish` and `ready`,
`open::file` in the 7k gate harness) takes 349–356 ms against a gate of
1 s, asserted, its 21,000 annotations and widgets and its field nodes
records (M10.5 slice 8); the scanned one's took 539–545 ms and 514 MB at
its peak (slice 4), the 474 MB file's bytes and little more.

## Coordinates: PageSpace (M10.5 slice 5)

Everything sheaf keeps about a place on a page is in **PageSpace**: the
page's default user space, points, y up, before `/Rotate`, whatever its
boxes; the space `/MediaBox`, `/CropBox`, `/Rect`, `/QuadPoints` and
`/XYZ` are written in. Marks, other tools' comments, OCR words, bookmark
positions, connectors and regions are stored in it, so nothing moves them
but an edit of them: turning or cropping a page changes how they show,
not what's stored.

- **One conversion.** `Geom::to_view()` (`geom.rs`) takes PageSpace to
  the page as shown: points, y down, from the top-left corner of the
  visible box (the crop box within the media box) after `/Rotate`, so the
  visible box covers `(0, 0)` to `shown()`. It is PDFium's page-to-device
  mapping at one pixel a point, and `from_view()` undoes it. Both are
  `Affine`s, 2×3 maps in PDF's order: `Affine::upright(r)` is the linear
  part for a page turned `r`, `then` composes, `apply_box` maps a box,
  and `apply_axes` maps an `/XYZ` point whose missing coordinate stays
  missing. A page's boxes as a file has them are read as `Geom::read`
  reads them, through the page table (`Source::geom`), or for a page of
  an eager document (a stamp collection's) from its dictionary and what
  it inherits; no other conversion is left.
- **Rotation is absolute.** A page's `Geom.rotate` (`DocState::geom`) is
  the `/Rotate` a save writes: a page starts with its source page's, and a
  save keeps it. `Document::geom(page)` is the page's `Geom`: its source
  page's boxes with that rotation.
  PDFium applies a source page's own `/Rotate` when it renders, so a
  render job takes `render_delta(page)`, the turn from the source page's
  rotation to the page's. A full rewrite writes the rotation in quarter
  turns (a `/Rotate 135` comes out as 90, which PDFium shows the same).
- **The app's frames.** A pane draws page *i* through
  `Doc::pane_geom(i)`, the page's `Geom` turned further by the pane's
  rotate view: its `to_view`, times the zoom, puts PageSpace on the
  screen. Gestures work in the **edit frame**, `Doc::edit_geom(i)`, the
  page as it shows without rotate view: the pointer goes through the
  pane's `from_view` and the edit frame's `to_view`, a tool builds its
  mark there, and `Annot::to_page` maps it into PageSpace, the one way a
  mark made on a page as it shows reaches the model (a plugin maps its
  own through `page-info`'s `from-view` from API 0.4, slice 20; 0.3's
  adapter did it with this); `Annot::to_frame` is its inverse, what a drag moves, a handle
  reshapes and the clipboard holds. Find and pattern hits, the
  text selection, handles, other tools' comment outlines, regions and the
  print preview draw through the pane frame, and `Doc::reveal` scrolls a
  PageSpace box into view.
- **A mark's `turn`** is the rotation it reads upright in, absolute: its
  page's when it was made (`to_page` adds the frame's rotation; a text
  markup takes its text's). Its layout (bounds, hit tests, where its
  text, carets, labels and tape lines go) is written y down, for its shape
  in its **upright frame**, `Annot::upright()`: PageSpace turned by
  `turn`. `page_bounds()` maps those bounds back. A tape's own
  `look.rotation`, the turn PDFlyer's options give its text within its
  box, is a style laid out within that frame, not a frame of its own.
  The writer lays a mark out upright and maps that into user space, so it
  needs no page geometry, and a page turned later turns its marks with
  what's printed there. GPUI can't turn text, so a free text or stamp
  whose turn isn't the pane's draws its text unturned in its turned box.
- **A crop moves nothing.** A crop is a box edit (`SetPageBoxes`, from
  M10.5 slice 15; see Crop as geometry): it sets the page's crop box, the
  kept area as shown mapped through `from_view`, in the page's own user
  space, so no mark, OCR word or bookmark moves.
- **`/SHEF_Annot` version 2** (`annot::Payload`) is the mark's JSON in
  PageSpace with its `turn` (none when upright), `"v":2` and `"std"`, a
  staleness digest of the dictionary it was written into
  (`annot::staleness`). The digest covers what the writer sets from the
  mark: `/Subtype`, `/Rect` with its corners normalized, `/Contents`,
  `/T`, `/M`, `/NM`, `/C`, `/IC`, `/CA` (1 when absent), `/QuadPoints`,
  `/InkList`, `/L`, `/Vertices`, `/CL` and `/RD`. Numbers count as the
  writer computes them, as f32s, in steps of 1/64 pt for geometry and
  1/1000 for colors and `/CA`, and the writer puts them on those steps
  (`annot::on_grid`; `/RD` as the inset to an inner box on them), so a
  value is never near the middle of two; text strings decode as PDFium
  decodes them (a BOM, else PDFDocEncoding), then to NFC. So another
  program's re-serialization (`100` as `100.0`, reals written to three
  decimals or more, two for geometry alone, `/Contents` in UTF-16BE,
  `/Rect`'s corners swapped) keeps a mark sheaf's, and its edit doesn't.
  A text string is written as is only where PDFDocEncoding reads it back
  so: one with a DEL or a byte from 0x18 to 0x1F is UTF-16BE. Left out:
  `/IRT` and `/RT`, which the save sets after the mark is written, `/IRT`
  an object reference a rewrite renumbers; a reply's or a status's
  `/Rect`, its parent's, set later too; and `/AP`, which other tools
  regenerate. A tick is a stamp named `/Tick`; it is known by the mark's
  `id`, as every mark is.
- **Reading a mark** (`file::sheaf_mark`): a payload is a mark when it
  says this build's version (`v`, which every payload holds) and while its
  digest matches. sheaf is pre-release: a payload of another version, one
  without a version among them (version 1, before slice 5), isn't read. A
  mark whose digest doesn't match was edited by another tool, and opens as
  that tool's comment, a record as other tools' comments are: shown,
  deletable and adoptable, never reverted. `file::is_ours` asks the same
  reader, so a save keeps its dictionary as the other tool wrote it.
- **Copied marks get new ids.** `/NM` need only be unique on its page,
  and Acrobat's Insert Pages copies `sheaf:<id>` with the page. At the
  open the first mark, in page and then `/Annots` order, keeps an id, and
  each later one with it gets a new one from an rng seeded by the file's
  hash, so the same bytes open with the same ids; a reply copied with its
  parent follows it, and so does another tool's reply to the copy, which
  the open lists under the new name. `ready` says which have a new `/NM`
  coming (`ViewFacts.renamed`; one that kept another tool's `/NM` only
  changes id), the app shows one notice and hides each by its old name
  until the file is opened again, and the next save writes the marks
  anew (`FileAnnots::fresh`), another tool's reply to a copy following it
  by its `/IRT` to the copy's key. A page the tree lists twice shares its
  `/Annots`, so its second listing's marks are renamed too.
- **OCR words** are PageSpace, and `upright` is the page's absolute
  rotation when it was recognized: the frame its words group into lines
  in and its invisible text runs in. The engine's pixels go through the
  `Geom` the page was rendered with, captured with the job.
- **Bookmark positions** (`DestView::Xyz`) are PageSpace, as the file has
  them, read and written unchanged. The outline panel records the
  window's top and the page's left edge as shown, through the pane frame,
  and a jump goes back the same way, a missing coordinate staying
  missing.
- **Regions** (Shift+drag) are held in PageSpace, and the plugin host
  gets them so.
- **Plugins under API 0.3 kept their frame** until slice 20: a page's
  coordinates were its source page as the file displays it, after the
  file's own `/Rotate` and before any turn sheaf adds, through an
  adapter (`sheaf-plugin`'s `api/shared.rs`) in both directions. API 0.4
  (slice 20), the only API served since, is PageSpace throughout, each
  `page-info` carrying its `to-view` and `from-view` maps, and the
  adapter is gone.
- **PDFium's text is PageSpace** (slice 14): `page_chars` gives each
  character's loose box as PDFium has it, in the page's user space, with
  the angle it's drawn at, and `PageChars` finds lines in the frame most
  of the page's characters are drawn upright in, not by its `/Rotate`
  (Text and printing). The device 16 times the page's size that mapped
  text into the displayed space is gone; plugins' text was mapped into
  0.3's frame at the API's edge, as marks were, until API 0.4 gave it in
  PageSpace (slice 20).
- **The clipboard** (version 2) holds marks in the edit frame of the page
  they were copied from, each turned relative to it. A paste maps them
  through the target page's frame, so they land where they were seen and
  read the same way on a page with another `/Rotate`, crop box or media
  box origin. A version 1 clipboard, in the old displayed space, isn't
  pasted.

`to_view ∘ from_view` is the identity within 1/16 pt over `/Rotate`, the
pane's rotation, crop boxes and media boxes off the origin (a property
test, `geom.rs`), and `to_view` is PDFium's page-to-device mapping on
every corpus page and widget checked (`sheaf-render`'s
`pdfium::frames`).

## Page content: the toolkit (M10.5 slice 6)

sheaf reads and writes page content through `sheaf-doc`'s `content`
module, brought forward from M11, never through lopdf's
`Content::decode` and `encode`, which re-encode what they read, drop
comments and spacing, and end an inline image at the first `EI` they
meet. The flatten (until 19) was their last user, the watermark's install
and read having gone in slice 16, and slice 19's clippy ban (the plan's
lint ledger) keeps them out of everything but tests.

- **Lexing** (`content::lex`) gives each token with its bytes as
  written and where they are: an operator, an operand (a number, name,
  string, array or dictionary, each one token, nested strings, escapes
  and all), or an inline image, `BI` to `EI`. Comments are skipped, and a
  stray closing delimiter is an operand of its own, which PDFium skips
  too. An error (`LexError`: a string, array, dictionary or image that
  doesn't end, or a cap) says where; lexing stops there, never panics or
  loops, and is linear in what it reads.
- **A page's content is one concatenation.** PDFium lexes a page's
  `/Contents` streams joined with a space between each, so a string or a
  marked range may open in one stream and close in the next.
  `PageContent::read` decodes them within `Limits.content` (see the
  Core idea) into one buffer (`Concat`), and maps any place in it back
  to a stream and an offset (`Pos`, `SpanAcross`). A missing entry reads
  as nothing, as PDFium draws nothing for it; a stream that won't decode
  is a fault, never its raw bytes. `forms::check` reads a `/DA`'s font
  (the last `Tf`'s name) by lexing it, and the stamp library's page form
  takes its page's content through the concatenation, so a page of
  several streams, state carried across them, draws as it shows.
- **Inline images end where PDFium ends them** (`ReadInlineStream`'s
  rules, in order): unfiltered data by its exact count (rows padded; no
  `/CS`, or a name the resources don't have, counts as one 1-bit
  component, as in PDFium); `/AHx`, `/A85` and `/RL` data at its end of
  data; DCT at the JPEG's end of image, the search remembered so many
  such images stay linear; then `/L` or `/Length`; and only then a scan
  for `EI` with white-space either side. After the first four the image
  ends at the next `EI` operator. An image whose end marker or exact
  count runs past the content is an error; PDFium loses the rest of the
  page then. An ignored test compares the lexer's painting operators by
  kind (text, paths, images inline or not, forms, shadings) with
  PDFium's page objects on the 402-file pool: they agree on all 23,628
  pages.
- **Marked content, balance and names.** `marked` gives every marked
  range with its tag, its property list (inline, or named in the
  resources' `/Properties`) and how deep it is; marked content that
  doesn't balance is an error, which no page in the corpus or the pool
  has. `balance` is the `q`/`Q` count: what's left open at the end
  (`net`) and the lowest it went (`min_prefix`). `names_used` gives the
  resource names the content uses (fonts, XObjects, graphics states,
  shadings, color spaces and patterns, named property lists), for slice
  16's pruning.
- **Splicing is safe or refused.** `splice` takes ranges out of a page's
  streams and keeps every other byte as it was; a stream nothing is taken
  from is borrowed, and a space goes where what's kept either side of a
  cut (or of cuts that touch) would join two tokens. A range is taken out
  only when that can't change what's drawn after it, lexically. It sets
  no graphics state outside a `q`…`Q` of its own (no `cm`, color, line
  style, `gs`, clip or text state, which would last past it). Outside a
  text object its `q`s and `Q`s and its `BT`s and `ET`s balance on their
  own. Inside one it stays inside, its `q`s balance, it moves no line
  (`Td TD Tm T* ' "`, which `q` doesn't save), and if it shows text no
  text is shown after it before the line moves or the object ends, since
  shown text moves the next on; setting text state doesn't move a line.
  It starts and ends outside a path being made. Marked content draws
  nothing, but for optional content (`/OC`), which hides what it marks:
  so a range may open marked content, none of it optional, that a later
  range closes, as long as no marked content is kept between them, and
  closes only what it or an earlier range opened. A range must also
  start and end between operators, never inside a token or a comment or
  between an operator and its operands. Anything else is refused:
  removing a running head drawn in the middle of a text object can't
  shift the body text, and a color an artifact sets can't leak onto what
  follows. An op reports a refusal as a `ContentError` that names the
  page ("page 12's content can't be changed without changing what else
  it draws; nothing was changed"; one that can't be read, "page 12's
  content can't be read; nothing was changed").
- **What a page draws, classified: `Source::blocks(i)`**, a
  `PageBlocks`, read the first time an op asks and kept by the source,
  one per page, for as long as the source lives: a fact of the bytes,
  never history, and never read at open. Its streams are read without
  being kept (`LazyPdf::object_unkept`, a vendor patch), so classifying
  7,000 pages keeps none of their content. It holds the artifact ranges
  and sheaf's marked blocks, the **pagination artifacts** (an
  `/Artifact` range whose `/Subtype` is `/Header`, `/Footer`,
  `/Watermark` or `/Background`, the outermost one) with who drew each,
  sheaf's blocks by stream marker (`/SHEF_Block`) and by marked-content
  marker (`/SHEF_Gen`, `/SHEF_OcrBlock`), the isolating streams
  (`/SHEF_Block << /K /Isolate >>`), the balance, and
  whether a form the page draws outside its pagination artifacts has one
  inside, which removing the page's would leave in place. A source reads
  each XObject its pages draw once, whatever names and pages draw it
  (`DrawnForms`); a form with no resources of its own draws with its
  page's, so it's read for each page. Forms are read within what the
  page's streams leave of `Limits.content.decompressed`: one past the
  cap, or met once that's spent, counts as having an artifact inside.
  Who drew an artifact (`PagOrigin`):
  - **`Sheaf`:** it's in a `/SHEF_Block` stream or its list has
    `/SHEF_Gen` (watermarks and backgrounds from slice 16, the header and
    footer from slice 17; from slice 18 the recognized text's blocks are
    `/SHEF_Block` streams too, but no artifacts);
  - **`Acrobat`:** it paints nothing but forms, at least one, and each
    has `/PieceInfo /ADBE_CompoundType /Private` a pagination kind;
  - **`Authored`:** any other, the author's own running heads, as Word,
    LibreOffice and InDesign mark them, which sheaf never removes
    unasked.

  On the corpus's 24,256 pages Acrobat drew 117 headers, 45 watermarks
  and 30 backgrounds, and authors 3,122 headers, 3,638 footers, 70
  watermarks and 7 backgrounds; 246 Word and LibreOffice files mark
  2,323 artifacts, every one `Authored`. Spliced out alone, as a strip
  cuts them, 191 of Acrobat's 192 go (the one left has the body's `q`
  and `BT` inside its range), and 2,443 of the authors' 6,837: theirs
  set color and fonts at the top level of a text object, or nest `q`
  and marked content across each other.
- **Removing others' artifacts** splices the page's streams, writes
  each it changed as a new object and rewrites `/Contents`, so a stream
  another page shares is never changed under it. Acrobat opens the first
  artifact on a page with the `Q` that ends the `q` it put before the
  page's content: that run of `Q`s stays (`Pagination::cuts`), the
  artifact's start and the rest of it cut either side. Until slice 17 a
  header and footer was stripped into a derived copy of the page
  (`stamps::strip`); from slice 16 a watermark or background, and from
  17 the header and footer, takes others' of its kinds out as a save
  writes the page, by filters on the page's content stack, and Remove
  Watermark and Remove Header/Footer take them by origin,
  `include_authored` taking the author's too (Watermarks and backgrounds
  as generators; Headers, footers and Bates as generators).
- **`ContentWriter`** writes content the same bytes for the same calls,
  on any thread: `q`/`Q` and `cm`, gray, RGB and CMYK colors, graphics
  states (`/ExtGState` with opacity and blend mode), line styles, paths
  and their painting and clipping, text through a `Font`, images, and
  `place_form`, which maps a form's `/Matrix`·`/BBox` onto a box.
  Numbers have at most four decimals, never an exponent or `-0`; strings
  are literal, or hex when a byte is a control code. Marked content names
  its property list in the resources' `/Properties`: lists are never
  written inline. Resources are held by what they are (`Res`, named by a
  `ResId`, a digest of their objects) and named only when the content is
  finished: `/Sh` and six base32 characters of the digest, the next name
  up where the resources already bind it to something else, and the
  same name again where they bind it to the same thing, so a header
  written again keeps its list's name. No name is given: the names the
  header and the OCR layer were drawn in went with them in slices 17 and
  18, and what a sheaf before the edit API wrote isn't read (sheaf is
  pre-release). The emitter writes a
  content's resources by digest,
  one object a resource a file; `ResObjects` (`serialize::built` from
  slice 20, `content::to_lopdf` before) writes them into a PDF sheaf
  builds in memory (exchange's comments, the comment summary's sheets, a
  tape's picture), one object a resource a document.
- **Fonts** (`content::Font`: `encode`, `advance`, `wrap`, `covers`,
  `resource`). `Std14` is the 14 standard fonts with Adobe's Core 14 AFM
  widths, glyph boxes and heights (`crates/sheaf-doc/assets/afm`, with
  Adobe's notice) and exact encoders: WinAnsi as cp1252, and Symbol's
  and ZapfDingbats' own, glyph names read by the Adobe Glyph List; text
  is composed (NFC) first. `Std14::named` reads a file's `/BaseFont` as
  PDFium does: the 14's names, and the alternates PDFium takes for them
  (`Arial,Bold`, `TimesNewRomanPSMT`).
  `Glyphless` is the OCR layer's: Tesseract's glyphless font, each UTF-16
  code unit a CID, read back through its ToUnicode, so it draws any
  text. A font sheaf embeds keeps its glyph ids (CID = GID); Glyphless,
  whose one glyph serves every CID, is the exception, and the rule is
  for M10 slice 7's Noto subsets. These replace four encoders and three
  width tables (the header and footer's, the marks', the watermark's
  UTF-8 written as WinAnsi, and the summary's); marks, tapes, the header
  and footer, the watermark's text, OCR layers and the comment summary
  all draw through them. A tick draws ZapfDingbats' heavy check and
  cross (✔ ✘), as it always has.
- **Text a font can't draw shows as `?`** in the drawing only:
  `/Contents`, the mark's JSON and the header's spec keep the exact text
  (ZapfDingbats has no `?`, so there the character is left out). An op
  whose drawing would lose characters says so (`Notice::Unencodable`,
  from M10.5 slice 11: adding or changing a mark or a connector, the
  marks pages bring with them when inserted or replacing others, a header
  and footer, and from slice 16 a watermark's text), and a commit tells
  it once a
  document: "Some characters can't be drawn in the built-in fonts and show
  as “?” in the file; the text itself is kept". A tape's minus and subtotal sign, which
  Helvetica lacks, draw in Symbol. M10 slice 7's Noto fallback ends it.
- **The words a page draws** (`text_bag`) are decoded through its fonts
  (a `/ToUnicode` CMap, else a simple font's encoding and
  `/Differences`, else Symbol's or ZapfDingbats' own, else the code as it
  is; in a Type0 font without a `/ToUnicode`, its predefined CMap's
  codes or CIDs as PDFium reads them) and counted as NFC words, so the
  same text in two subsets with different codes gives equal bags. Slice
  10's removal check compares them. From M11's slice 3 `text_bag` reads
  content as the content interpreter does (The content interpreter,
  below), each form judged on its own (`Forms::Report`), its fonts a
  source's `Fonts` made for text (`Fonts::for_text`: no program read),
  each read once within the page's budget. The bags are the old walk's
  on every stream the removal check reads, but for the hash of an empty
  string shown in no font, which stands for no text, and CJK text where
  the old walk hashed: a Type0 font with no `/ToUnicode` over a
  predefined CMap, or Identity with an Adobe ordering. A string with a
  CID 0 in it (`.notdef`, which an unmapped code takes) is still hashed,
  though PDFium reads U+FFFD for it, so unmapped codes match only the
  same bytes.
- **Marks hold their stamps and attachments** (M10.5 slice 8). A mark
  names its stamp's page and its attachment's file by a stem, a blob's
  name (`BlobId::stem`), and holds them: `Mark.stamp` is a `FormRef`,
  the form a source's saved appearance draws (`Base`) or a one-page PDF
  blob (`Blob`), and `Mark.file` a `BlobRef` to the file's bytes, an
  `Attachment` blob. So a stamp or file lives as long as a version holds
  its mark, and the store counts it. At ready, and for the marks of
  files whose pages come in, each holds what its object holds there
  (`FileAnnots`, from `file::annots_in`): the form, or the embedded file
  decoded, no further than the mark says it's long. From M10.5 slice 11
  a new mark names what it holds as an asset (`NewMark.asset`, a
  `BlobSel`), resolved before the op is lowered: a blob the caller
  interned and pinned in the transaction (a stamp placed, a file
  attached, a mark pasted) or granted it by a stem the host looked up
  (`find_stamp`, `find_file`: the store's blob of that name, else what a
  source's marks hold under it, the primary's first, else, stamps only,
  the generated cache by path, until slice 19). An inserted page's marks
  hold what `ready_to_insert` decoded off the UI thread. A stem that
  names nothing is `NotFound`; lowering never decodes bytes. A save
  writes each once a file, by digest (`ObjKey::Res`), and a mark it
  writes as the file holds it keeps the objects the file has (see
  Saving). Nothing is made again in the cache at open, so a file opens
  with its stamps and attachments on a machine that never saw them. A
  page stamp whose appearance draws no page, or an attachment with no
  embedded file, opens as another tool's comment, as a stale mark does,
  and a save keeps it so. The stamp library's page form bakes its page's
  rotation and visible box into the form's `/Matrix`, so a stamp from a
  cropped or turned page lands as PDFium shows that page. From slice 19
  a library stamp, an imported one and a pasted one are one-page PDF
  blobs made in memory, an adopted one holds its file's form, and
  there's no cache (see Pages made in memory, under Exports, memory
  sources, Session and `sheaf edit`).
- **Stamp and tape pictures are made in memory**: the app renders a
  stamp's page and a turned tape with PDFium, transparent where nothing
  is drawn, and keeps the pictures for the session, a stamp's by its
  stem and a tape's by what it is, a stamp's from the page its mark
  holds. One that can't be made isn't asked for again. Nothing is
  written to disk for them.

Every corpus page's content (24,256 pages, 575 MB, and the 5,273 forms
they draw) reads, lexes into tokens that tile it, splices back to each
stream's own bytes read apart, balances as lopdf's decoder counts it, is
classified, and has each pagination artifact spliced out, in about 8 s
in the test profile; classifying every page of the 7k fixtures, cold,
takes 141 ms (synthetic), 203 ms (heavy) and 77 ms (scanned), which was
most of what a first Bates apply cost there until slice 17 made it one
op that reads no page's content (under 1 ms).

### The content interpreter (M11 slice 3, between milestones)

M11's slice 3 landed early, on its own between M10.5 and M10's return
(2026-10-23), since M11's redaction, M13's content editing, M14's
optimize and M17's tagged documents all build on it. `content::interp`
reads content as PDFium interprets it (`CPDF_StreamContentParser`), so
where it says a glyph is, PDFium draws it. It lexes through the toolkit,
never lopdf's `Content`.

- **What it reads, and hands on.** `interpret(objects, &Concat,
  resources, &Limits, &Fonts, Opts, visit)` hands each event to a
  visitor in order, with the state before it and the marked ranges it's
  in (`Event { at, state, marks, kind }`):
  - every operator, inline images among them (`Kind::Op`; `At`: where,
    and the operands as written);
  - each string shown (`Text(TextShow)`), its glyphs laid out when asked
    (`Glyph`: bytes, code, CID, origin, advance, quad, pen, what's
    guessed, and `tm()`, the absolute `Tm` that starts a string there);
  - each form use (`FormBegin` and `FormEnd`, a `FormUse` with its
    matrix, `/BBox` and `/OC`, named by a `UseId`, the chain of `Do`
    offsets that reach it) or refusal (`FormRefused`);
  - each path painted or made the clip, in PageSpace (`Path`, `Seg`),
    its bounds as far as a stroke paints (half the line width, √2 times
    that at square caps, the miter limit's at a join or curve, 0.5 pt
    for a zero width, as PDFium draws it);
  - each image, an XObject or inline (`ImageDrawn`: the unit square
    through the CTM, and its stencil, soft mask, stencil mask and `/OC`,
    read without decoding its data); each shading `sh` paints
    (`ShadingDrawn`, bounded by the clip within its `/BBox`); an XObject
    PDFium doesn't draw (`Ignored`);
  - marked content (`MarkedBegin`, `MarkedEnd`, `MarkedPoint`: a `Mark`
    with its tag, properties, `/MCID`, `/ActualText` decoded, its `/OC`
    list, the form use it's in and where its operator is).

  `Summary` counts operators, glyphs, bytes of content lexed, bytes
  taken from the page's budget (`decompressed`: its content, each form's
  data once, what the fonts it picks decompress to, and what drawing
  their programs' glyphs costs), path segments,
  form uses and refusals, and the deepest forms, `q` and marked content
  went. With `Forms::Report` no form is drawn and no XObject read, a `Do`
  being an operator like another, as where each is judged on its own
  (the removal check's `text_bag`). `Opts.wants` (`Wants { ops, text,
  paths }`, everything by default) says what the visitor reads: only the
  operators `ops` holds (`Ops`, a set over those the interpreter knows,
  one bit for the rest) are handed as `Op`, and strings and paths only
  where it wants them, so what nobody reads isn't built. What
  interpreting works out is the same whatever the interest: the layout,
  each cap and its count, geometry an `f32` can't hold, the clip and the
  `Summary`; forms, images, shadings and marked content are always
  handed. Each operator's name is decoded once (`Op`) and dispatched by
  one match.
- **State as PDFium keeps it.** `State` holds the CTM, the line's width,
  cap, join and miter limit, the clip, and `TextState` (font and size,
  `Tc`, `Tw`, `Tz`, `TL`, `Ts`, `Tr`, the text matrix, where the line
  starts and the pen, and whether a width guessed earlier on the line
  moved it). What PDFium does its own way, so does this: an operand that
  isn't a number is 0, and a non-finite one leaves the state as it was;
  `Tz` is taken with one operand only; a `Q` with nothing saved is
  passed over, and `q` saves the text position too; `Tw` widens a code
  32 that's one byte by its CMap's measure (`CPDF_CMap::GetCharSize`,
  so never in Identity-H); a `TJ` is laid out as one text object
  (`TextShow.offset`), the pen moving after it, and one that passes over
  an empty string loses its last adjustment, as PDFium looks for it
  past the strings it kept; a `Tf` naming a font the resources haven't got draws
  in PDFium's stock Helvetica (`TextShow.stock`, `Fonts::stock`, built
  once); an ExtGState's `/Font` is looked up by name; paths are built in
  user space and placed by the CTM at the painting operator, with
  PDFium's reading of path operators after an `m`; a resource category a
  form's resources lack is looked up in the page's (`FindResourceHolder`);
  and a form starts in PDFium's `AddForm` state, `Tz`, `TL`, `Ts` and
  the text matrix reset.
- **Fonts give glyph boxes.** `Foreign::read` reads a file's font as
  PDFium does, failing only past the budget it decompresses within
  (`TooLarge`): its kind (Type1, MMType1, TrueType, Type3, or Type0 over
  CIDFontType0 or 2); its codes (`codes`, split as PDFium's
  `CPDF_CMap::GetNextChar` splits them: by an embedded CMap's codespace,
  a byte each by one range but two where it's of two bytes, by more each
  code by PDFium's mixed rule, 0 for bytes no range begins; Identity-H
  and -V by name; the other predefined CMaps (PDF 32000-1 Table 118's,
  and Adobe's UCS2 maps) from hayro-cmap's bundle of Adobe's, each parsed
  once, split as PDFium's table of them says (`kPredefinedCMaps`, since
  hayro-cmap keeps their codespaces to itself); a code of two bytes cut
  off at a string's end padded with a 0; a `/UseCMap` chain followed
  through streams, a predefined base read for the codes it maps none of,
  guessed where it can't be read whole) and CIDs; its widths (`/Widths` from
  `/FirstChar`, `/MissingWidth`, CID `/W`, `/DW`, `/W2` and `/DW2`, kept
  as the ranges they're given in, the first for a CID winning, so no
  array expands past its own length; cut to whole numbers as PDFium cuts
  them); its heights and `/FontBBox` (`metrics`); a Type3 font's
  `/FontMatrix` and each CharProc's `d1` box (`glyph_box`), rounded as
  PDFium rounds them; and each code's text (`code_text`: its
  `/ToUnicode`'s, else its encoding's; in a Type0 font without a
  `/ToUnicode` as PDFium's `GetUnicodeFromCharCode` reads it, a UCS2 or
  UTF16 map's code its own text, another predefined map's CID, or
  Identity's where `/CIDSystemInfo` orders GB1, CNS1, Japan1 or Korea1,
  through Adobe-<collection>-UCS2). Its embedded program (`/FontFile`,
  `/FontFile2` or `/FontFile3`: TrueType, bare CFF, OpenType over CFF,
  Type 1) is read through skrifa and read-fonts as PDFium's FreeType
  reads it (`Foreign::program`, `FontProgram::{None, Read, Malformed}`),
  each code mapped to its glyph PDFium's way, for each glyph's outline
  box (`CPDF_Font::GetCharBBox`) and the heights a descriptor lacks
  (`CheckFontMetrics`); programs are untrusted, so a TrueType glyph's
  composites are counted first (glyphs and points), charstrings run
  within fuel, each glyph its share of its font's, a Type 1 program is
  scanned ahead of read-fonts (its nesting, its integers in a row and
  its work bounded) and read once, and one that can't be read is
  `Malformed`, its boxes and heights guessed as without one; what
  drawing costs (charstring evaluated, glyph data read) is charged to
  the page's budget, a simple font's as it's read, a CIDFont's glyph
  the first time an interpretation shows it, whichever page drew it
  (`Foreign::charge_outlines`). The
  standard 14 have Adobe's AFM widths, glyph boxes and
  heights, and the names PDFium takes for them (`Std14::named`); a glyph
  PDFium's own faces draw that Adobe's metrics lack, or draw at another
  width (`Std14::drawn_otherwise`), has its width guessed
  (`width_guessed`). What's guessed is said (`FontApprox`: codes,
  widths, heights, boxes), and per glyph (`Glyph.approx`, whose widths
  also means moved by a guess before it on its line). PDFium takes
  heights from the face it draws in wherever a descriptor has no
  `/FontBBox` that isn't all 0, and that face varies by OS (its own,
  Arial, Helvetica), so only a descriptor with a box and its `Ascent`
  and `Descent`, or a program that gives them, is exact; and a
  predefined CMap neither hayro-cmap bundles nor PDFium knows has its
  codes guessed. A standard 14 font without
  `/Widths` or a program has PDFium's own faces' widths, and so does
  PDFium only where it draws in them: on a system that has Arial or
  Helvetica, it takes widths from those, as it does heights (the default
  PDFium tests give their standard 14 fonts `/Widths`). A glyph's quad is
  PDFium's loose character box (`GetLooseBounds`): the metric box (the
  advance from descent to ascent through the character's matrix), and in
  vertical writing the em-wide column its advance spans, united with its
  outline's box where its program says it, a side shorter than 0.01
  taken out as PDFium takes it out; `FontApprox::boxes` says where the
  outline isn't known (no program read, as for the standard 14 that
  aren't embedded, a malformed or tricky one, a code whose glyph sheaf
  can't say, a Type3 glyph with no box). Simple fonts whose codes are
  bytes are laid out by a fast path that makes the same sums in the same
  order (`Foreign::byte_widths`). `Fonts` keeps a source's fonts by
  object, each read once (`Fonts::get`), and `FontScope` one
  interpretation's picks by owner and name, a `FontId` each, so a `q`
  copies no reference count; `Fonts::for_text` reads no program, for
  text alone (the removal check's). What a font's streams decompress to
  (its `/ToUnicode`, its CMap's chain, its CharProcs and its program,
  each stream once), and drawing a simple font's glyphs, is charged to
  the page's budget, with its content and forms: once an interpretation
  for a font by object, cached or not, so a page costs the same
  whichever page read it first, and each time for one written in the
  resources as a dictionary.
- **Forms per use, refused as PDFium skips them.** Each `Do` of a form is
  interpreted where it's drawn, in a state of its own: its matrix, its
  `/BBox` clipping, its resources its own, else those of what draws it;
  each form is read once an interpretation. A use deeper than
  `ContentLimits.form_depth` (40, PDFium's `kMaxFormLevel`) is refused
  (`Refused::Depth`) and the page goes on, as PDFium skips it; so is a
  use within itself (`Refused::Cycle`), at once. PDFium's own cycle check
  compares the buffers it reads each use into, which it makes afresh for
  every use of a stream read from a file, filtered or not, so it draws a
  form that draws itself 40 deep: a cycle refused means the interpreter
  reports less than PDFium draws, and redaction fails closed on any
  refusal.
- **Clipping and marked content.** `W` and `W*` wait for the path to
  end; text shown in modes 4–7 (never in a Type3 font) clips at `ET`,
  within PDFium's 1,024 text objects; each form's `/BBox` clips its
  content. The clip (`Clip`, `ClipPart::{Path, Text}`) is a shared list
  of parts with PageSpace bounds (`State::clip_bounds`), a part within
  the last rectangle replacing it as PDFium merges them, dropped without
  recursion. A text clip's bounds are its glyphs' quads, which an
  outline may pass. Marked content is read as PDFium reads it, not as
  `marked` does (strictly): a `BDC` whose name `/Properties` lacks opens
  nothing, a stray `EMC` is passed over, a range left open ends where
  its content does, and a form's `EMC`s can't end what its drawer
  opened.
- **Bounded.** `ContentLimits` gains `form_depth` (40); `states`, the
  `q`s saved at once (65,536: PDFium has no cap, so this one bounds
  memory only); `operators` and `glyphs` (2²⁴ each), `interpreted`, the
  bytes of content lexed (2³⁰), and `segments`, path segments (2²³: of
  a path being built, and of all painted or made the clip, which bounds
  what paths and the clip hold), each counted across every form use, so
  a form drawn many times costs each time, one of whitespace too. The
  lexer's `operands` cap is the operand stack, `depth` caps marked
  nesting (`Cap::Marked`), and forms and fonts decompress within what
  the page's streams leave of `decompressed`. Text in a clipping mode
  past the 1,024 objects PDFium's clip takes keeps no glyphs. Past a cap
  is `InterpError::Limit(Cap)`, never a partial answer; a form that
  can't be read is `InterpError::XObject`; and geometry an `f32` can't
  hold (a CTM, a form's matrix, the line, a glyph's box by a bound
  worked from its font's extent, its outlines' reach included, a path,
  an image's corners, a shading) is `InterpError::NotFinite`. It never
  panics or recurses past `form_depth`. The most any page of the corpus
  comes to: 1,217,210 operators (13.8× under the cap), 41,050 glyphs,
  17,864,912 bytes lexed, 17,945,356 bytes decompressed with its fonts'
  programs (6.7% of `decompressed`), 995,739 path segments (`segments`
  stays 2²³, 8.4× that), 745 form uses, forms 10 deep, 67 states saved,
  marked content 9 deep.
- **Written again** (`interp::encode`, giving `Encoded { content, forms
  }`): the page's operators as one stream, and once each the content of
  every form it draws, as written (`ContentWriter::operation`), but
  `Td`, `TD` and `T*` as a `Tm` of the text matrix and a `Td` from its
  origin to the line they move to, `'` as those and a `Tj`, and `"` as
  `Tw`, `Tc`, those and a `Tj`. A `Td` that moves nothing stays as
  written, since any operator ends PDFium's reading of path operators
  after an `m`. Numbers so worked out are written as the state holds
  them (`object::real`), so positions come out to the bit, and no line
  depends on the leading or an earlier line: a line can be taken out
  without moving the lines after it. Strings on one line still follow
  each other, so where glyphs are taken out of one, the next on its line
  needs a `Tm` and an absolute `Td` of its own (M11 slice 5). What can't read back the
  same is refused (`EncodeError::Unwritable`): a line that isn't finite,
  and an inline image whose `/L` isn't its data's length
  (`lex::image_length`), whose end would move with what follows it.
- **Tests and the corpus.** `content::fixture` (in other crates' tests
  through the `fixtures` feature) builds a page for each operator, font
  kind, nesting of forms, kind of image, clip and marked range, each
  with its boxes worked by hand (`every()`); PDFium places every glyph
  and image of them where the interpreter does, and renders each page
  written again pixel for pixel as before; their standard 14 fonts have
  `/Widths`, so PDFium lays them out the same on every OS; fonts over
  predefined CMaps (`fixture::predefined`) and over hand-built TrueType,
  CFF, Type 1 and CIDFont programs (`fixture::programs`) among them. The
  `interp` fuzz target draws two streams on `fixture::fuzz_page`, with
  the fonts' CMap, a CharProc, `/W`, `/W2`, `/FontMatrix` and a font
  program from the input where it gives them, and fonts over predefined
  CMaps beside them, and checks that what it writes draws the same, every
  glyph, path, image and form where it was to the bit. Three ignored
  tiers in sheaf-render's `content_interp.rs` (`just tier-interp`;
  `SHEAF_INTERP_ONLY` narrows them) read every form, the pool sample,
  every scan and conformance's 540 files that claim PDF/UA
  (`Corpus::conformance`), 54,009 pages: every page interpreted within
  the caps, with no panic, failure or box not finite (10 s); 117,492,213
  glyphs matched to PDFium's characters, none past a thousandth of a
  point: at the origin, and equal to the loose box on every side where
  the outline is known and nothing guessed (90,677,142), else within it
  and equal where the outline lies within; 3,637 with codes or widths
  guessed counted; one file's 38 glyphs, whose corrupt program PDFium
  can't load and sheaf reads, allowed by count; and the glyphs and
  characters it can't say why it matched none for (73,296 and 279) held
  to each file's count in a checked-in list
  (`content_interp_unexplained.tsv`), so text the interpreter misses
  fails (4 minutes); and every page written again rendering as before,
  but 2 within 16 px and 24 drawn, pixel for pixel, as PDFium draws the
  file loaded again (its caches warm for both), none left unwritten (50
  minutes).
- **Its callers.** `text_bag` reads content through it (`Forms::Report`;
  an interest of the strings shown and the operators that end a word;
  fonts made for text). Its bags were proven equal to the old walk's on
  every stream the removal check reads (pages, forms, tiling patterns,
  Type3 glyphs) across the corpus, the pool sample, the scans, the PDF/UA
  files and the 7k fixtures, but for the hash of an empty string shown
  in no font, which stands for no text; the predefined CMaps then turned
  69 files' hashes into CJK text, no asserted corpus count moving (a
  string with a CID 0 in it stays hashed). With the
  wrap-up's speed step it's 5% faster than the old walk was on the heavy
  7k's streams, 2% slower on the synthetic's, and 31% on the scanned's
  (about a millisecond over its 7,000 pages). M11's slice 5 is the first caller
  to lay out glyphs. The 7k gate interprets every page of each fixture,
  with budgets asserted with `SHEAF_GATE_INTERP`, and measures every
  page's text bag beside it.

## Undo/redo: snapshots, not inverse commands

Three candidates were considered:

| Model | Undo | Cost per edit | Risk |
|---|---|---|---|
| **Inverse commands** (`apply`/`revert` per op) | run the inverse | O(edit) | every op needs a correct inverse; a wrong one silently corrupts state. Deleted pages must be stashed for restore. |
| **Event replay** (base + edit log, state = fold) | drop the last event, re-fold | O(edits) per undo | re-fold cost grows; inserts need their sources kept alive anyway |
| **Snapshots** (persistent state, structural sharing) | move the cursor | O(changed part) | memory per version — bounded by history cap |

Snapshots win. Undo/redo cannot be wrong because there is no inverse logic;
"what is the document" is always one pointer. Grouping is free: a drag that
moves 50 pages pushes one version at the end of the gesture. Dirty tracking
is `cursor != saved` (and a restored document is dirty while its recovery
sidecar's sections are pending, M10.5 slice 10). "Revert to saved" is
`cursor = saved`. A document made in memory opens saved at its first
version, with no record; `Document::never_saved` (M5 slice 2, for an
untitled document's recovery) leaves it with no saved version, so every
version is dirty until its first save, a Rewrite.

History also keeps what each edit changed (M10.5 slice 1), so a snapshot
dropped to save memory is rebuilt exactly, never by running the edit again:

```
Version   { parent, children, redo_to,   // undo goes to the parent, redo to redo_to
            label, actor, author, at,    // who made it (M10.5 slice 11)
            steps: Vec<Step>,            // the ops from the parent
            keys: Arc<BTreeSet<Key>>,    // the slots they changed (M10.5 slice 12)
            blobs: HashSet<BlobId>,      // the sources its steps bring, noted once
            state: Option<DocState>,     // the snapshot, unless trimmed
            snapshot_bytes, step_bytes,  // what it holds (see Decisions)
            group, damaged,              // the group it's of: an OCR run, a plugin call
            on_line,                     // on the line from the first version kept to the current one
            sealed,                      // saves of it in flight
            record,                      // the saved version's SaveRecord
            saves: Vec<SaveNote> }       // { path, at, strategy } of each save
Step      { op: Arc<ResolvedOp>,         // the op, its references resolved
            prims: Arc<[Prim]>,          // what it changed
            effects: StepEffects,        // its effects, pages and keys
            line: String,                // what it did, in words
            cascades: Arc<[Cascade]> }   // what the cascades did with it
Prim      { key: Key, before: Option<Val>, after: Option<Val> }
```

- **An edit records what it changed.** It reads the state and writes only
  through a recording context (`prim::Rec`), which makes a `Prim` for
  each slot it wrote: the slot (`key`), the value it held and the value it
  holds now, `None` for absent. The keys are the records': `Order`,
  `Page(id)`, `Geom(id)`, `PageAnnots(page)`, `Annot(id)`, `Field(id)`,
  `Form`, `Bookmark(id)`, `OutlineRoots`, `Labels` and `Generator(id)`,
  eleven kinds, one per table of the state (M10.5 slice 8; slice 1's nine
  keys were the old state's fields). Before-values cost nothing: they are the
  parent's im and `Arc` values. Setting a key keeps the indexes current:
  a new order starts a new position index, an `Annot` moves its `/NM`
  entry, and a `Field` whose partial name, parent or kids changed finds
  the full names under it again.
- **A no-op makes no version.** A slot written back to what it held makes
  no prim (the same im nodes or `Arc`, else equal), and a batch's or a
  group's prims are coalesced per key into one, from the first before to the last
  after, on the last step that changed the slot. So a batch that rotates a
  page and rotates it back has none, and nothing is pushed. No whole state
  is ever compared.
- **Replay is exact and checked.** Undo, redo and jump rebuild a version
  whose snapshot was trimmed from the nearest ancestor that kept one, by
  applying each version's prims on the way. `DocState::apply` refuses a
  prim whose before isn't what its slot holds (`Mismatch`), and replay
  never runs an edit again, so what an edit reads without recording it
  (source sizes, the forms, random mark ids) can't make it differ. A
  mismatch is a bug: the version is marked damaged and can't be reached,
  the History panel shows "History is damaged at version N", undo and redo
  return `EditError::Mismatch { version }` instead of doing nothing, and
  a debug build panics.
- **A VersionId names one state, which never changes.** A group's amend
  (see One funnel and the history budget) and the end of a file's
  opening (`Document::ready`) each put a new version, with a new id, in
  the old one's place under the same parent, and the old id stops
  resolving. A save puts none.
- **One `IdAlloc`.** Page, bookmark and field ids come from counters that
  only go up, so an id is never handed out twice, whatever undo does (the
  opened file's field nodes keep the ids its tree numbers them with). The
  prims hold the ids an edit drew, so a replay draws none. Mark and
  generator ids are random below 2^53; other tools' annotations' and the
  form's widgets' at open come from an rng the file's hash seeds.
- **Lines are made once.** describe runs as an op is lowered, over the op
  as resolved, against the state just before it, so the second op of
  "move pages 3–4 before 1, then delete page 4" says "Delete page 4". The
  line is stored on the step, and the History panel reads it whether or
  not the parent's snapshot is kept; a run of ops of one kind shows as one
  line (`describe::lines`, M10.5 slice 11).
- **A save changes no version** (M10.5 slice 9). It used to re-express
  its version over the saved file, which pinned that version's snapshot
  and its children's; now each version replays across a save as any
  does, and only the saved version keeps its snapshot for the save after
  it (`keeps_snapshot`: the first kept, current, saved and sealed ones).
- **A save seals its version** (M10.5 slice 9; see Saving). A sealed
  version keeps its snapshot, a group's next commit makes a version after
  it instead of amending it, and pruning never removes it: a branch
  holding one stays, and the first version kept doesn't move past it.
  The saved version may go once it's old (M10.5 slice 12): it detaches,
  its state and record kept as an anchor.
- **A save is judged by its path** (M10.5 slice 10). The keys the steps
  changed on the tree's path from the saved version to the one being
  saved, through where the two branches meet, are what a save compares,
  by value, the saved state's against the target's (`effects::judge`).
  So undo back to the saved state, by whatever route (undo, a jump, or
  edits that set every slot back), is an Unchanged save: nothing is
  written, the saved record is attached to the version
  (`record_unchanged`, no save note) and the document is clean. Once
  pruning has taken the saved version, its anchor keeps the keys from it
  to the first version kept, so the path is never lost (M10.5 slice 12).

`model/history_tests.rs` is the proof: over random runs of every op (from
M10.5 slice 11 generated as `Op`s, 22 kinds with the internal page base,
naming what they act on by id, position, local, `/NM` and title path),
with batches (one transaction, each op against what the ones before it
made), groups (amends, fragments, reverts and rollbacks), transactions
made on earlier versions landed by value or lowered again, undo, redo,
jumps, trims down to one snapshot and pruning to a byte budget (M10.5
slice 12), every version rebuilt from its prims equals the state it was
made as, ids included, and each kind's no-op pushes nothing; and what
history counts it holds, after every action, is what it holds, counted
again.

Costs and how they're bounded:

- Per-version memory is proportional to what changed. The order is an
  `im::Vector`, so a reorder of 7,000 pages copies tree nodes, by one split
  and two joins, not the list. Every other table is an `im` map of
  records, each record its own key, so per-page edits don't scale with
  document size, and nothing is found by a scan: a page's position, an
  annotation by id or `/NM`, and a field by its full name are index
  lookups. At 7,000 pages and 20,000 marks, adding a mark takes 0.019 ms
  and moving half the pages before the first 0.94 ms, each an op in a
  transaction committed (release; `history_at_7k_pages_and_20k_marks` in
  the 7k gate harness, `crates/sheaf-ops/tests/gate_7k.rs`; 0.015 and 0.74
  ms from slice 8 until ops; 0.7 and 0.95 ms before slice 8, when an add
  looked through every mark). 1,000 adds on the heavy 7k,
  21,000 annotations held, cost what they cost on blank pages, about
  0.03 ms each, and a push doesn't walk history (M10.5 slice 12): with
  200 snapshots held, which the default policy weighs at every push, the
  last 100 adds on blank pages take 0.021 ms against the first 100's
  0.019 (`mark_adds_on_the_heavy_7k`, asserted within 1.5×; before slice
  12 a push walked the versions to trim them, and the last 100 took
  0.18 ms).
- Sources referenced only by undone versions are counted and freed. Each
  version notes once, as it's made, the blobs its steps bring (a page
  shown from another source), so counting them walks no step, and a blob
  goes when the last record holding it does.
  `HistoryStats` counts every live blob once (`blob_bytes`) and those only
  versions off the current line bring (`undone_source_bytes`), and
  `MemoryBudget` weighs the second (see Decisions), and prunes the
  versions that hold them when it must (M10.5 slice 12). Inserting a
  400-page scan, undoing it and editing on frees its bytes
  (`pruning_an_undone_400_page_scan_frees_its_bytes`).

## Ops and transactions (M10.5 slice 11)

Every change to a document is an **op** (`sheaf_edit::Op`), said by its
caller, lowered in a **transaction** (`Tx`) into the prims history keeps,
and committed as one version (§6 of `plan/complete/m10.5-edit-api/README.md`). The old
`Edit` enum and `compute`, which said and did a change at once, are gone,
and so is every place a caller guessed what its edit made.

```rust
pub enum Op {
    MovePages { pages: PageSel, before: Option<PageRef> },
    RotatePages { pages: PageSel, by: Turn },
    DeletePages { pages: PageSel },
    DuplicatePages { pages: PageSel, locals: Vec<Local> },
    InsertPages { from: PageSource, pages: Option<RangeSet>, before: Option<PageRef>,
                  bring: Bring, locals: Vec<Local> },
    ReplacePages { pages: Vec<PageRef>, from: PageSource, with: Option<RangeSet>, bring: Bring },
    SetPageBoxes { pages: PageSel, crop: Patch<BoxSpec>, trim: Patch<BoxSpec>,  // slice 15
                   bleed: Patch<BoxSpec>, art: Patch<BoxSpec> },
    SetPageLabels { ranges: Option<Vec<PageLabelRange>> },
    // bookmarks
    AddBookmark { parent: Option<BookmarkRef>, at: Sibling, title: String,
                  target: Option<PageRef>, view: Option<DestView>, /* color, style, */ local: Option<Local> },
    PatchBookmarks { bookmarks: Vec<BookmarkRef>, patch: BookmarkPatch },
    MoveBookmarks { bookmarks: Vec<BookmarkRef>, parent: Option<BookmarkRef>, at: Sibling, with_pages: bool },
    DeleteBookmarks { bookmarks: Vec<BookmarkRef> },
    // marks, and other tools' comments
    AddMark { page: PageRef, mark: NewMark, reply_to: Option<AnnotRef>, local: Option<Local>,
              imported: Option<Imported> },
    AddConnector { a: Anchor, b: Anchor, label: String, style: Style, locals: Vec<Local> },
    PatchMarks { marks: Vec<AnnotRef>, patch: MarkPatch, imported: Option<Imported> },
    MoveMarks { marks: Vec<AnnotRef>, by: Delta, to_page: Option<PageRef> },
    DeleteAnnotations { annots: Vec<AnnotRef> },
    AdoptAnnotations { annots: Vec<AnnotRef> },
    // OCR text, watermarks and backgrounds, header and footer
    SetOcrText { page: PageRef, layer: Arc<TextLayer> },
    ClearOcrText { pages: PageSel },
    SetWatermark { spec: WatermarkSpec, z: Z, scope: ScopeSel },       // slice 16
    RemoveWatermark { z: Option<Z>, pages: PageSel, include_authored: bool },
    SetHeaderFooter { spec: HeaderFooterSpec, scope: ScopeSel },        // slice 17
    RemoveHeaderFooter { pages: PageSel, include_authored: bool },
}

impl Document {
    // ops lowered as they come, by `actor`; one commit is one version
    pub fn tx(&self, actor: Actor) -> Tx;
    pub fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError>;
    // what it changed, or None with nothing to undo (from slice 12)
    pub fn undo(&mut self) -> Result<Option<ChangeSet>, EditError>;
    pub fn redo(&mut self) -> Result<Option<ChangeSet>, EditError>;
    pub fn state(&self) -> &DocState;
}

impl Tx {
    pub fn apply(&mut self, op: Op, p: &dyn Progress) -> Result<OpResult, EditError>;
    pub fn apply_batch(&mut self, ops: Vec<Op>, p: &dyn Progress) -> Result<Vec<OpResult>, EditError>;
    pub fn check(&self, op: &Op) -> Result<(), EditError>;  // resolve and caps only
    pub fn reader(&mut self) -> Reader<'_>;                   // decision reads
    pub fn pin(&mut self, blob: Blob);                        // what the caller interned
    pub fn grant(&mut self, grant: Grant) -> BlobSel;         // BlobSel::Asset(n)
}
```

Ops address pages, bookmarks and annotations by **id**, or as something
an earlier op in the transaction made (a local), never by an index the
caller kept: indices are a view-layer concern and go stale the moment
anything moves. This is also what makes PDFlyer's "move bookmark with its
pages" a plain op: the bookmark targets a `PageId`; `MoveBookmarks {
with_pages: true }` moves the node and then moves the pages its subtree
targets to sit after the preceding bookmark's pages.

- **Two small crates.** `sheaf-edit` holds the vocabulary and nothing
  else: `Op` and what ops carry (the ids, `BlobId` written
  `"b<32 hex>-<len>"`, `Local`, the refs and selectors, `Patch`,
  `NewMark`, `MarkKind`, `MarkPatch`, `Imported`, `BlobSel`, the header
  and footer's, labels' and Bates specs, the tape, measure and OCR text
  types, `Rotation`, `PageSize`, `DestView`), with serde and a schemars
  schema, no I/O and no other sheaf crate, so it builds for
  `wasm32-wasip2`; a CI step checks both. sheaf-doc re-exports each type
  at its old path, and what needs the model became sheaf-doc's own
  functions (`store::blob_id`, `text_layer::from_engine`). `sheaf-util`
  is what's left of sheaf-core: the checksum and file reading, twox-hash
  its only dependency (a CI step checks that too); `PageIndex` went to
  sheaf-render. Every crate but sheaf-edit and sheaf-plugin-api is
  `publish = false`.
- **Op JSON.** `{"op": "delete-pages", "pages": {"positions": "3-5"}}`:
  each op kebab-case under `"op"`, every op-layer type
  `deny_unknown_fields`, so a misspelled field is an error rather than a
  default. The exceptions are the record types `/SHEF_Annot` stores
  inside its lenient `Annot` (`Pt`, `Box2`, `Scale`, `Tape` with its
  `Line`, `Row` and `TapeLook`): strict, a mark another sheaf wrote with
  a field more would read as another tool's comment. Their fields but
  the tape's note and look are required, so a misspelling there is still
  an error; an extra field is ignored. `Patch<T>` keeps a field when it's
  absent, clears it on `null` and sets it to a value. `Envelope { v:
  "1.0", ops }` refuses another major version or a later minor. The
  committed `crates/sheaf-edit/schema/ops-1.0.json` is compared with the
  schema by a test that fails on anything lost or changed, on a
  constraint added where there was none (a pattern, a bound, an enum, a
  closed object, ...) and on a property newly required;
  `SHEAF_SCHEMA_UPDATE=1` rewrites it only when the schema has purely
  grown.
- **References.** A page, bookmark or annotation is named by its id (a
  number), as what an earlier op in the transaction made (a local,
  `"$1"`), or by an object: a page's 1-based position (`{"pos": 2}`), a
  bookmark's titles from the top (`{"path": [...]}`), a comment's `/NM`
  (`{"nm": ..., "page": ...}`). Pages are chosen by ids, all, a span,
  positions (`"1-3,5"`) or a bookmark's span. Each reference resolves
  against the state the op finds, so `[delete-pages 1, rotate-pages
  {"pos": 2}]` turns the page that's second after the delete, and the
  step stores the id it resolved to. A reference that names nothing is
  `NotFound`, in every op; a local named twice or of the wrong kind is
  `Invalid`, and an `/NM` that names more than one is `Ambiguous`.
- **A transaction's three stages** (`Tx::apply`, all or nothing for the
  op; `apply_batch` all or nothing for the batch):
  1. **resolve:** references become ids, locals what made them, blobs
     what the store or the transaction's grants hold, and what the op
     can't do as said is refused here (pages moved before one of
     themselves, a bookmark into its own subtree, a widget deleted as an
     annotation, a reply's thread rules, which adopting another tool's
     reply or comment keeps too, a part grouped under a comment adopted
     beside it). Bytes are decoded before this,
     never in lowering: what an inserted file's marks hold by
     `ready_to_insert`, off the UI thread, pinned in the transaction
     (`Tx::pin`); a stem a plugin or an import names, by the host, into a
     grant (`Tx::grant`, named `BlobSel::Asset(n)` or `With(name)`). A
     plugin naming a blob by id is refused. One exception, until slice
     19: adopting another tool's page stamp finds its page in the
     generated cache, reading and interning it while it lowers; from
     slice 19 it holds the file's own form (`ForeignAnnot.stamp`), and
     nothing is read.
  2. **caps:** the op's capability, from the op as resolved and the
     kinds of what it acts on (`cap_of`), against the actor's
     (`actor_allows`), both total matches.
  3. **lower:** the op written through the recording context, one module
     a family (`tx/{pages, bookmarks, marks, content, forms}.rs`), then
     the cascades to a fixed point.

  `Tx::check(op)` runs the first two only, draws no id and puts nothing:
  what a menu asks before it offers an op. Lowering reads only the
  transaction's state, sources' facts, the store, the host's attributes
  and the id allocator; what it reads without writing is kept by value,
  and so is what a caller decided its ops on (`Tx::read`, `Tx::reader`,
  which the app's macros read through), which a commit on another
  version checks (slice 12's stale bases, below). A
  canceled op leaves the transaction as it was; inserts, replaces,
  duplicates and deletes report progress by page.
- **Actors and caps** (§6.7). `Actor::{User, Plugin { id, version },
  Cli, Job(Ocr | Trim | Import | Insert | Scan)}`, bound by
  `Document::lower_cx`, the only way to make a lowering context.
  `Cap::Plugin` is what any plugin may do (pages moved, turned, deleted
  or copied, labels, bookmarks, sheaf's own marks); `Approval` changes or
  removes what the user didn't make in sheaf (another tool's comment,
  link or widget deleted or made a mark, OCR text cleared, the header set
  or removed, from slice 15 a page's boxes set, from slice 16 a watermark
  or background set or removed); `User` brings content in
  (pages inserted or replaced, OCR text set, an import's names and
  dates). A plugin gets `Plugin` only, until
  plugin API 0.4's permissions; an OCR job sets and clears OCR text and
  nothing else, and Trim White (`Job(Trim)`, slice 15) sets page boxes
  and nothing else. A scan (`Job(Scan)`, M5 slice 1) inserts its pages
  and sets their recognized text, and nothing else.
- **What the host owns** (§6.8). Ids and `/NM`s, a new mark's dates (one
  `HostAttrs.now` a transaction), its author (the user's name, or a
  plugin's id for its marks), the identity tokens in its text
  (`<<name>>` and `<<initials>>` filled for the user and left as written
  for a plugin; `<<date>>` and `<<time>>` filled), a connector's partner,
  a reply's parent `/NM` (from `reply_to`), a stamp's and an attachment's
  stem and an attachment's size (from its asset), and a new header's date
  and file (the name of the file that holds the document, after a Save
  As the new one). No caller sets them; `Imported`, the user's comment import's
  alone (`Cap::User`), keeps a comment's own name, dates and author.
  `Document::set_identity(Identity { person, initials, date_format,
  time_format })` says who the user is.
- **Cascades, declared once** (`tx/cascade.rs`, §6.5): `page.annots`,
  `annot.thread`, `annot.partner`, `widget.field`, `page.bookmarks`,
  `page.labels` and `page.generators`. What a round removed is read once
  (`Gone::of`, one exhaustive match on `Key`); each rule writes ordinary
  prims into the same step, and they run in rounds until one writes
  nothing (more than eight is `Internal`). So deleting a page takes its
  marks, other tools' comments and links, and widgets; the replies to
  them; a connector's partnership; the fields left with no widget, as a
  save would have dropped them; the bookmarks to it, their kids moving
  up; a label range starting there moves to its range's next page, or
  goes; and the page leaves the header's pages, the header going when
  none is left, and its Bates number goes with it (from slice 17, under
  any scope; a number is never given again). `Commit.cascades` counts
  each rule's work.
- **Fail, never repair** (§6.4). `EditError::{NotFound, Invalid,
  Ambiguous, Refused(Cap), NotReady, Cancelled, Mismatch { version },
  Internal}`, each naming the op as Op JSON does. `check_invariants`
  (debug builds, after every op and every commit, at open and after a
  save) also says that label ranges and the header's pages name pages
  that are there, that connectors pair both ways, and that a sheaf reply
  is to a comment on its own page. The open clears a connector's partner
  that doesn't name it back, and an export (`save::subset` until slice
  19's `KeepPages`) keeps what it writes so: partners left out cleared,
  labels re-anchored, the header's pages pruned.
- **Steps, lines and effects.** A step is `{ op: Arc<ResolvedOp>, prims,
  effects, line, cascades }`. describe reads the resolved op as it's
  lowered, its page numbers by `pos_of`, so a line costs what the op
  names; `describe::lines` shows a run of one kind as one line
  ("Recognize text on 7000 pages"), and the internal page base's line
  names its flow (header stripped; crop's until slice 15, the
  watermark's until 16). A
  line says whether its op removed something (`Line.removes`), which the
  History panel marks with a "Removes" badge. `StepEffects` is a step's
  `Effects` (§7.1's flags and signature class) with the pages and keys
  it touched, judged on the step alone (`ClassifyCx::of_step`), so it
  too costs what the op touched; a save still judges its whole path
  (`judge`, slice 10).
- **Commits** (`Document::commit`): refused while the file opens
  (`NotReady`); a transaction made on another version lands as its
  commit says (from slice 12, below); prims coalesced per key; no
  version when nothing changed. `Commit { version, amended, created,
  effects, lines, cascades, notices }`: `created` is what each op made
  and what each local names, which the app selects or opens (a new mark,
  bookmark, reply or paste, a text editor) instead of guessing; the
  notices are typed (`Notice::{FormRenamed, CopiesRenamed,
  SignatureNotValidHere, ScriptsOff, Unencodable}`, the last once a
  document), and `notice_text` gives the texts M10 pinned. A group's
  commits join its version (slice 12, below). Undo, redo and jumps
  return `EditError` (`Mismatch` for a damaged version, `NotFound` for
  one no longer kept).
- **A source is its blob; a form instance is its op's.** `SourceId =
  BlobId`: nothing registers a file, and records hold their `Src`.
  `InsertPages { from: PageSource::File { file: BlobSel, name }, .. }`
  names its form instance by the op's `name`, never the blob's origin, so
  the same bytes inserted as `copy.pdf` go under "copy". Each insert,
  replace and duplicate is one `FormInst { src: BlobId, copy }`, its copy
  the next its blob has in the state (copy 0 of the primary's kept for
  its own), fixed when it's made: two inserts of one file are two forms,
  renamed apart, and a saved file's objects for an instance never move to
  another, so slice 10's form-shift refusal is gone. Scripts count per
  instance (§3.7): two scripted instances turn them off, a scripted
  page's duplicate among them. An insert's marks, links and outline come
  from its source's facts as `Bring` says (`marks`, `form: New`,
  `outline: None | MergeByTitle`), partners and reply names remapped.
  Each version notes once the blobs its steps bring, so
  `undone_source_bytes` walks no step. (Until slice 17 a derived copy's
  origin was in a table by blob, `Document::derived`, outside undo; no
  edit makes a copy now.)
- **Callers.** The app commits through `DocView::commit(tx, opts) ->
  Result<Commit, EditError>`, from slice 12 the only way it changes the
  document but for OCR's flushes (below): a drag is `MoveMarks`, a
  handle, style or text change a `PatchMarks` naming only what changed
  (`mark_changes`). From slice 15 a crop is `SetPageBoxes` and makes no
  copy (Crop as geometry), from slice 16 a watermark or background is
  `SetWatermark`, and from slice 17 a header and footer
  `SetHeaderFooter` and its removal `RemoveHeaderFooter`, each drawn by
  a generator (Watermarks and backgrounds as generators; Headers,
  footers and Bates as generators): the internal page rebase the derived
  flows used, `Tx::set_page_base`, is gone. Combine is one transaction
  (`Actor::Job(Insert)`) on the first file's document: the first file's
  pages left out deleted, one `InsertPages` a further file, its
  bookmarks added; it was written and reopened until slice 19, which
  hands the combined session to a window (see Exports, memory sources,
  Session and `sheaf edit`). Plugins spoke WIT 0.2 and 0.3 until slice
  20: `api::shared::Adapter` said each edit as ops (an update a
  `PatchMarks` naming only the record's fields, so what the API didn't
  show stayed), the host resolved a mark's stamp or file stem into a
  grant, and the batch was one transaction of `Actor::Plugin` (from slice
  12 a call's batches were one group, the first landed on what the call
  read). API 0.4 submits Op JSON, lowered into the call's one
  transaction (Plugin API 0.4). A
  comment import is `PatchMarks`, `AdoptAnnotations` and `AddMark` with
  `Imported` (`Actor::Job(Import)`; `Actor::Cli` on the command line).

At 7,000 pages, `Tx::check` of deleting every page by id takes 0.39 ms
and draws no id (asserted under 1 ms on the synthetic and heavy
fixtures); one transaction of 7,000 `SetOcrText` lowers in 38 ms and
commits in 6 ms as one History line; with 20,000 marks, an `AddMark`
committed takes 0.022 ms and a full reorder 1.0 ms (gates 1 and 2 ms).

## One funnel and the history budget (M10.5 slice 12)

A transaction can be made on one version and committed on another, so
the work that plans an edit runs off the UI thread and a dialog commits
on the version it opened on. Every transition between versions says
what it changed, as a `ChangeSet`, which is how the app (one door, one
refresh), its panels and plugins follow the document. An OCR run, a
scan and a plugin call are each a group of versions that undo as one
step. History is kept to a byte budget, pruning the oldest versions, the
saved one too (§8 and §14 of `plan/complete/m10.5-edit-api/README.md`).

```rust
pub enum Base { Current, At(VersionId) }          // the version a Tx was made on
pub enum Stale { Validate, Relower }              // how it lands on another
pub struct CommitOpts { label, base: Base, stale: Stale, group: Option<GroupId> }
pub enum GroupKind { Ocr, Scan, PluginCall { plugin: Arc<str> } }

impl Document {
    pub fn tx_at(snap: &Snapshot, cx: Arc<LowerCx>) -> Tx;   // on snap's version, off the UI thread
    pub fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError>; // Commit.change
    pub fn undo(&mut self) -> Result<Option<ChangeSet>, EditError>; // and redo, jump
    pub fn record_save(&mut self, rec: SaveRecord) -> Result<ChangeSet>;
    pub fn begin_group(&mut self, kind: GroupKind) -> GroupId;
    pub fn end_group(&mut self, g: GroupId);
    pub fn revert_group(&mut self, g: GroupId, label: &str) -> Result<Commit, EditError>;
    pub fn rollback_group(&mut self, g: GroupId) -> Result<Option<ChangeSet>, EditError>;
}
impl Tx { pub fn preview(&self) -> Preview; }       // lines, effects, created
pub struct ChangeSet { from, to, cause: Cause, actor, label, keys,
                       order, pages_added, pages_removed, geometry, content,
                       annots, fields, outline, labels, form, generators }
impl DocView {
    fn lower(&mut self, ops, label, cx) -> Option<Tx>;   // and lower_in(tx, ops, label, cx)
    fn commit(&mut self, tx, opts, cx) -> Result<Commit, EditError>;
    fn on_change(&mut self, ch: &ChangeSet, cx);         // the one refresh
}
```

- **ChangeSets** (`change.rs`). Every transition gives one: a commit
  (`Commit.change`), an amend, an undo, a redo, a jump, a save
  (`record_save` and `record_unchanged`, from and to the current
  version, nothing changed), a rollback, a revert, and the file's facts
  coming in (`ready`, `Cause::Opened`: every page and field in its sets,
  no keys). `Cause` says which, `actor` who (a commit's or an amend's
  transaction's, a rollback's group's, the user's otherwise) and `label`
  what, in words. It's made from the slots that changed, a lookup or two
  each, never by comparing the states: a `Page` slot that came is a page
  added, one that went a page removed, one in both content (looked up
  only when the order changed, since every page is in the order);
  `Geom` is geometry; `PageAnnots` and each `Annot`'s page in either
  state are annotations; the other keys set their flag or list. Each
  version keeps its keys (`Version.keys`, the union of its coalesced
  steps' prims' keys, shared with its step when it has only one), and an
  undo, a redo or a jump names those on the tree path, walked up from
  both ends until they meet (`edges_between`), which is a save's path
  too (see Saving).
- **Stale bases** (`model/stale.rs`, §8.5). `Document::tx_at(snapshot,
  cx)` makes a transaction on a snapshot's version, and its commit says
  so (`Base::At(v)`; a mismatch is a bug, `Internal`). A snapshot names
  its document too (`Snapshot.instance`, a number no other document in
  the process has, which the transaction keeps): version ids are counted
  per document, so one made on another document, such as the one a
  recovery replaced, is a bug too, whatever its version's number. On the
  version it
  was made on it commits as any does. On another, what its caller
  decided by (`Tx::read`) must hold the same values there (by value, so
  a change undone since is none), and then:
  - **`Validate`** (a dialog's, an OCR flush's): every slot its ops read
    while lowering (`seen`), and the first before-value of every slot
    they wrote, must hold there too. Its steps' prims are put on the
    current state in order (`merge`), each line said again against what
    it lands on and its effects worked out again, and the post-merge
    check looks for anything the cascades, the uniqueness rules or the
    references would still change or break: one round of every cascade
    rule over what the merge wrote (`cascade::unsettled`); one generator
    per kind and place (`Generator::same_place`: a watermark's z, and
    the header and footer, which is always over, one a document), a
    source page's copy shown once,
    `/NM`s per page and full names over the written fields
    (`DocState::unique_after`); and every page a written slot names
    still there (`resolves_after`: a mark's, a bookmark's, a label
    range's start, the header's pages). Debug builds check the
    invariants too.
  - **`Relower`** (a planner's, a plugin call's): the ops are lowered
    again on the current version, in a new transaction with the same
    context, grants, names and pins, each op drawing first the ids it
    drew before (`Entry::Op { op, drawn, positional }`; every id is
    drawn through `Lower`: pages, bookmarks, fields, annotations and
    generators), so what it made keeps its ids. An op whose reference
    no longer resolves fails as it would have (`NotFound` for an insert
    before a page deleted since). An op that resolved a position or a
    span (it read the order) needs the order unchanged.

  Anything else is `EditError::Conflict { key, by }` and changes
  nothing: the slot, and the label of the newest edit on the current
  line, since the two lines met, that changed it ("“Crop page 1”
  changed the same data"); none when an undo did. `Tx::preview()` says
  what a commit on its own version would do (`Preview { lines, effects,
  created }`).
- **Groups** (§8.4). `begin_group(GroupKind::Ocr | Scan | PluginCall {
  plugin })` starts a group of versions that undo as one step: an OCR
  run's flushes, an intake's batches (M5 slice 1), a plugin call's
  edits. It takes commits (`CommitOpts.group`) from the actor its kind
  names, the OCR job, the scan job or the plugin called (another's is a
  bug), until `end_group`. A commit joins
  the group's version, an amend (steps added and coalesced, a new id in
  its place, `Cause::Amend` with the commit's own keys), when that's
  the current version, has nothing after it, was never saved, isn't
  being saved and has a parent; otherwise it pushes another version of
  the group's, "… (continued)" once the group has one. A commit after
  the group's newest version was undone or jumped away from (it's
  neither the current version nor before it), or after the group
  ended, is `EditError::GroupEnded` and changes nothing. History's
  "Undo all of …" (`revert_group`) takes the group's versions on the
  current line back as one version after the current one, the user's
  (`Cause::Revert`, `ResolvedOp::Revert`): their prims inverted on the
  current state, newest first, every before-value checked, coalesced,
  then put through `Validate`'s merge, so an edit since to what they
  wrote, or a rule their going would break, is a Conflict naming that
  edit, and edits between the fragments stay. `rollback_group` (a
  plugin call that failed) takes the group's only version out when it
  could still amend it, leaving nothing to redo; otherwise it reverts
  by the group's actor (`Cause::Rollback`), a saved version staying.
  Either ends the group. Once pruning has taken a version of the group,
  or the prims of the first version kept, which is of it, either is
  `Invalid` ("older than the history kept") and History offers no "Undo
  all of" for it (`Document.cut_groups`): what's left would take back
  only part of it. An amend whose keys don't meet its version's
  counts only its new steps' bytes (`join`), so 700 flushes of 10 pages
  take 79 ms together and the last ones 0.12 ms each.
- **The history budget** (§8.7, `history.rs`). After each push or amend,
  `trim` weighs history only when the policy could act on what it holds
  (`HistoryPolicy::could_act`, from running totals). It prunes versions
  one at a time while there are more than `MAX_VERSIONS` or the policy
  says (`prunes`; `MemoryBudget`'s is `untrimmable_bytes() > budget`),
  weighing again after each, then drops snapshots. `prune_one` takes the
  oldest leaf off the current line, never the saved or a sealed version,
  and otherwise moves the first version kept on along the line
  (`advance_root`): never past the current version or a sealed one, nor
  while a branch off it holds a sealed one; the other branches off it go
  with it. The new first version keeps its steps' lines, effects and
  keys but not their prims, since nothing is rebuilt into it, and its
  bytes are counted again; a group it takes a version or the prims of
  can no longer be undone all at once (above). The saved version may go, as the first
  version or on a pruned branch: it detaches
  (`SavedVersion::Detached(Anchor { state, record, keys_to_root })`),
  keeping what the next save needs, and each later first version adds
  its keys; a save replaces it. One made in memory has no record, so
  nothing is kept and the next save is `NoSaveRecord`'s Rewrite. A push
  doesn't walk history: what history holds is counted as versions come
  and go and snapshots are dropped and rebuilt (`Held`: the versions
  keeping snapshots, their bytes, the steps' bytes, and the undone
  sources' bytes until the line or a version bringing a source
  changes); each version says whether it's on the current line
  (`on_line`, set along the path a move walks); and the snapshots the
  policy may drop are found in one pass over those held (`weigh`). 200
  OCR versions of a 1,000-page scan keep within 256 MB (4,254 MB of
  steps before): history gets shorter instead.
- **The app: one door, one refresh** (§14.1). `DocView::lower(ops,
  label)` and `lower_in(tx, ops, label)` lower and say why an op
  failed. `DocView::commit(tx, opts)` commits, says what it did or why
  not (a Conflict among them), brings the bookmarks it made into view
  (walking up from each), and calls `on_change`, as undo, redo, a
  History jump, "Undo all of …", a save's end, a plugin call's rollback,
  OCR's flushes and the file's opening do with their changes. OCR's
  flushes commit on the model in their run's group and call `on_change`
  themselves: the door's notice would replace the run's own, and a
  page-by-page retry's conflicts are pages left out, not errors to
  show; they make no bookmarks. No view
  method both lowers and commits. `on_change` catches up what follows
  the document (`Doc::follow`). `Doc.changes` counts changes, and keys
  the Comments view, the History rows and the labels cache. What's
  selected of removed pages and annotations goes, looked at only when
  the change touched them. Images are found by what their page draws
  (its render key, slice 13), so none is dropped: a page shows its last
  one until its new one lands. The panes lay the pages out again
  (`Doc.layout`) only on an order or a geometry, so a mark's edit, or a
  page shown from another source page of its size, lays nothing out.
  Then the plugin cell takes the new snapshot (from slice 20 what
  `Document::published()` publishes), autosave is scheduled (not for a
  save or the opening), and
  listeners hear once: `DocEvent::Opened` when the file opened, else
  `DocEvent::Changed { change }`. Plugins heard it as `edited` ("Undo:
  …", "Redo: …", "Go to: …" or the label) until slice 20, and from it as
  `changed(change-info)`, its scopes, merged while the plugin is busy
  (Plugin API 0.4); never of their own edits (by the change's actor) or
  of saves. A recovery sidecar is keyed by its
  `VersionId` alone. The `--script` harness's `dump` gains `"version"`
  (the version, the change and layout counts, whether it's dirty) and
  `"changes"` (the count, and the last change).
- **Planners and dialogs.** `DocView::run_planner(name, work, status,
  land)` plans an edit off the UI thread on a snapshot and lands it on
  the UI thread. Any number run at once, beside a file job (`run_job`,
  now only Export and Split) and OCR; Escape stops the newest, and
  opening or recovering another document forgets them, and blank
  review's pages (`forget_planners`), and closes the dialogs opened on
  the document it replaced (`Workspace::close_dialogs_of_another`, by
  `Snapshot.instance`). Inserting a PDF's pages, blank pages or images,
  and replacing pages, read the file into the store and lower
  `InsertPages` or `ReplacePages` on the snapshot off the UI thread
  (`plan_insert`, `plan_replace`), then land with `Relower`
  (`CommitOpts::based(v, stale, label)`); nothing opens an engine on the
  file, which the render worker opens when a page of it is first drawn
  (from slice 13). From slice 15 Crop Pages makes no copy: margins are
  one `SetPageBoxes` committed with `Validate` on the version its dialog
  opened on, and Trim White a planner that re-plans on a Conflict (Crop
  as geometry). From slice 16 Watermark / Background is one
  `SetWatermark` committed so too, and its Remove a planner (Watermarks
  and backgrounds as generators); from slice 17 Header & Footer is one
  `SetHeaderFooter` committed so too, its pages ids of the version its
  dialog opened on, and its Remove a planner of the same kind
  (`DocView::plan_removal`), the notice each one's line ("No change to
  the header and footer", "No header or footer to remove"). Both
  dialogs start from the model's generator (`Start::at`) and show, once
  counted off the UI thread as they open, what others drew of their
  kinds on the selected pages, or all of them, which applying replaces
  and Remove takes off (`generator_dialog::Count`, from
  `tx::others_on`; proposal 31). The labels dialog previews its op
  lowered on the current version (`Tx::preview`), again at each change,
  and commits on the version it opened on with `Validate`; once the
  labels changed since it opened, which that commit would find, it says
  it can't apply. The Header & Footer dialog previews its page as
  applying would draw it (slice 13): its op lowered on the current
  version off the UI thread (from slice 17 that op alone, the header set
  again replacing the one drawn there), again at each change of its
  settings or the document, and the page realized from that state under
  the screen's inputs; a lowering's picture shows only while no later
  lowering has begun. The Crop Pages (slice 15) and Watermark /
  Background (slice 16) dialogs preview the same way. Blank review's
  delete reads each confirmed page as its search measured it
  (`Tx::read_render(p, &ViewInputs::content_only())`: what the page
  shows, from M10.5 slice 13) and lands with `Relower`, so a page whose
  content changed since is a Conflict, and one given a bookmark, a
  connector or a header since has the cascades applied. A comment
  import is planned off the UI thread (`Actor::Job(Import)`) and lands
  with `Relower`, reading what its matching decided by
  (`Imported.reads`, read by `ops_in`): the order, each page it brings
  comments onto (its `Geom` and `PageAnnots`) and each comment it
  updates, so two imports of one file planned together don't both add
  its comments: the second is a Conflict.
- **OCR** is a group (see Text layers): flushes made on the version
  their round began on (the run's first; from slice 18, a later round's
  for pages read again), deciding by each page as it was rendered,
  landed with `Validate`.
- **Plugins** (§14.3), as slice 12 left WIT 0.2 and 0.3; slice 20
  replaced the batches with one transaction a call (Plugin API 0.4).
  Each document has a cell (`Doc.plugin_cell`, an `ArcSwap<Snapshot>`,
  from slice 20 an `ArcSwap<Published>`) that `on_change` stores the new
  snapshot in. A call loads its base from it at its first read or
  submit, so a queued call, or one that showed a form first, starts from
  the document as it is then, and its reads record the slots it decided
  by (until slice 20 `HostState::read` into `CallOutput.reads`, from it
  the call's transaction): `pages()` the order and each page's `Page` and
  `Geom`; `annotations(p)` `PageAnnots(p)` and each mark shown; the
  outline its roots and each bookmark; text, search, words and
  recognition `Page(p)` (from slice 13 `render_reads` under
  `content_only`, which is that), and a render what it drew under its
  inputs (`render_reads`: the page, its drawn annotations and fields, the
  form, the generators on it, and the order or the labels when their
  live tokens read them); an update the mark it kept parts of. When the
  call returned, `plugins::land` began a `GroupKind::PluginCall` group
  and committed the first batch on the base (`tx_at`, each read,
  `Relower`) and later ones at the current version in the group
  (`EditBatch::lowered(tx, doc)`); from slice 20 `sheaf_plugin::Landing`
  commits the call's one transaction. The first failure rolls the group
  back; if the user changed meanwhile what the call wrote (a Conflict),
  its edits stay, and the notice says "its partial edits were kept". A
  call whose cell is no longer its view's (the view opened another file)
  changes nothing.

At 7,000 pages a snapshot takes about 0.2 µs (asserted under 50 µs on
every fixture). A crop of every page but one, landed by value after the
user deleted that page, takes 27–31 ms, against 14–15 ms on the current
version (from slice 15 one op with no copy: 15 ms and 10 ms, Crop as
geometry). An insert of 10 pages lowered again after a move commits in
1.5 ms. One transaction of 7,000 `SetOcrText` lowers in 36 ms and
commits as an OCR group's version in 9.5 ms, about 4.5 ms of it pruning
the previous run's version, which doesn't fit the budget beside it (the
bench gives every page one shared layer, which each step counts).

## What the viewer changes

- Images key by what their page draws, not its display index or id:
  tiles by `(RenderKey, Job)` and previews by `(RenderKey, Rotation)`
  (M10.5 slice 13), so a reorder, a save or an edit to another page
  re-renders nothing, and a duplicate shares its original's. A page whose
  key changed shows the images of the key it was last drawn by until its
  new one lands; nothing is invalidated by hand.
- `Job` gains `rotation`: the page's `render_delta`, the turn on top of
  the page its engine has (its base page's `/Rotate`, which PDFium
  applies itself, kept in an overlay), plus the pane's rotate view.
  PDFium renders rotated bitmaps, so tiles at a different rotation are
  just a cache miss.
- `DocView` holds a `Document` and its views (the document's realizer
  over the process's one engine pool), not a render handle a source;
  sidebar thumbnails become the primary editing surface (drag to
  reorder, multi-select, keyboard rotate/delete).
- The layout holds each page's id and geometry, not where it renders
  from, and is made again only on an order or a geometry.
- Sizes come from the sources' page tables (`Geom`) from the first paint,
  and the outline from `DocState`, which the open reads from the source;
  the worker is never asked for either.

## Saving

Materialization of a version's `(DocState, sources)` → bytes, and a
record of what was written (M10.5 slices 9 and 10, below): `save_input`
seals the version, `materialize::plan` judges what changed since the
saved file and picks how to write it and what to ask, `materialize`
emits the state's objects by key (all of them, or for an Append only
those the change reaches) and writes them through sheaf-doc's own
serializer (`serialize/`, below), the file is replaced durably or
appended to in place, and `record_save` keeps its `SaveRecord`, which the
next save plans from. lopdf only holds the object graph a Rewrite prunes.
Two strategies, and a third that writes nothing (Unchanged):

**Rewrite** (M2 slice 3, first; from M10.5 slice 9 the new writer's,
`materialize`, below): the state's objects emitted by key over the
primary's (every page in a new balanced tree, sheaf's marks each with its
appearance and, in `/SHEF_Annot`, its PageSpace JSON and staleness
digest: version 2, see Coordinates), then whatever still points at a page
that isn't kept cut (`kept::keep_only`: links, named destinations, the
open action, form widgets, the structure tree) so pruning takes it out,
and the whole file written (`serialize::full`), the primary's objects at
their numbers. A copy never follows a reference to a page it isn't
copying (`materialize::copy`). Always correct, O(file). A save that
removed something, onto a file that can't take an update, or to another
file (but a signed one's Save As), and every export.

**Append** (M10.5 slice 10, below): a section after the file's bytes
holding only what changed, a new cross-reference section and a trailer
with `/Prev`, the original bytes kept verbatim (audit-friendly, and what
Acrobat does by default), encrypted as the file is (`serialize::append`),
emitted from the records that changed, closed over what depends on them,
and numbered through the saved file's record. It's written in place with
a journal beside the file, or for Save As of a signed file after a
verified copy of it; the removal check runs before a byte of it is
written. The default whenever nothing was removed: one mark on the
scanned 7k is about 21 ms. The old incremental writer, which diffed the
state against a baseline through the Snapshot's dirty flags and a legacy
projection of the records, went in slice 9 with the old rewrite path; in
slice 9 every save was a Rewrite.

What a save draws goes through the content toolkit's `ContentWriter`
(see Page content): marks' appearances, tapes, the header and footer,
OCR layers and the watermark's text, in the built-in fonts' exact
encodings, where a character a font can't draw is a `?` in the drawing
and the text itself is kept. The resources they draw with are named
`/Sh` and six characters of their digest, and are one object a file
whichever marks and pages use them (bug 19): one mark of every kind
draws with three fonts and two graphics states between them. A page
stamp's page is written once a file, as a form drawn under such a name,
and an attachment's file once. Both come
from what the marks hold (a form a source's mark draws, a library
stamp's page and an attached file from their blobs), not this machine's
cache. A generator's marked content names its property list in the
page's `/Properties` (the header and footer's from slice 17, as a
`/SHEF_Block`).

A save is an output (M10.5 slice 9, below): the document doesn't change,
nothing is reopened or read back, and what the screen shows stays.

### One writer (M10.5 slice 7)

Saves, Save As, extracted pages, Clean Copy, Export Flattened, print and
recovery sidecars reach their bytes through `serialize/` (§11.7 of
`plan/complete/m10.5-edit-api/README.md`). lopdf's writer still writes:

- the derived copies, in memory, until slice 17 deleted them (crop's
  went in slice 15, the watermark's in 16, the header's last);
- the comment summary and FDF files, until slice 19's exports;
- PDFs made from images and blank pages (`images::to_pdf`,
  `generated::blank_pages`): Images to PDF and a scan without OCR, at
  the path the reader picked, and inserted image and blank pages and
  Combine's image items, through the generated cache, until slice 19's
  memory sources;
- the stamp library's files (`stamp_lib`: a collection, a stamp's
  one-page PDF, an image stamp), the reader's own and outside the edit
  API (§15), until slice 19;
- pictures PDFium draws from memory (a tape), the only one from slice
  19, whose memory sources and exports write the rest through
  `serialize::full::write_built` and `materialize`, until slice 20 built
  it with `serialize::built` too. From slice 20 lopdf's writer writes
  nothing outside tests.

- **Objects** (`object`): a direct object as PDF syntax, and an indirect
  object's `n g obj` frame. Reals are f64 in their shortest round-trip
  form, always with a point and never an exponent: a NaN or an infinity
  is refused and nothing is written, magnitudes are clamped to ±3.4e38,
  and nonzero ones under 1.175e-38 are 0 (Annex C). Strings keep their
  format, names are escaped, and a stream's `/Length` is its data's.
  Integers, references, names and strings are written byte by byte, a
  real alone through Rust's formatting, for its shortest form (object
  streams' slice 2: the same bytes, in less time on a large Rewrite).
- **Cross-references** (`xref`), as a table or a stream in the base's
  kind: a whole file's one subsection `[0, Size)`, every number it
  doesn't list free and the free entries linked from entry 0; an
  update's runs of the numbers it lists, and nothing freed (lopdf's
  readers keep an earlier section's entry for a number a later one
  frees, so an object an update unlinks stays in use and its number is
  never given again; slice 10). A stream lists itself, with `/Index` and
  a `/W` as narrow as its fields allow. A whole file's is deflated under
  the PNG Up predictor (`/Predictor 12`, `/Columns` the sum of `/W`), as
  qpdf writes its own; an update's stays uncompressed (an Append is
  unchanged: [B62](../plan/backlog/B62.md)). An object stream's member
  is listed as its container's number and its place there (type 2),
  which only a stream can say: a table refuses one. Positions are
  written from the header, so a file with bytes before `%PDF-` appends
  as PDFium, pdf.js and qpdf read it; an offset a table's ten digits
  can't hold is refused.
- **A whole file** (`full`): the header at the document's version, every
  object in number order, and one section. Object streams,
  cross-reference streams and linearization dictionaries are left out
  (an object stream's members are written anew), and the trailer keeps
  its keys but its old sections' own, `/Encrypt` and `/ID`. A stream the
  save made was compressed where it was made; a source's is written as
  stored, never re-encoded, so a Rewrite of an uncompressed file stays
  uncompressed. Asked to pack (`Layout.pack`, with a stream's section
  only), what it writes anew that may go in an object stream
  (`objstm::packable`, ISO 32000-1 7.5.7: not a stream, an object whose
  generation isn't 0, nor an encryption, signature or linearization
  dictionary) goes in a container of 100 at most, after the `/Encrypt`
  dictionary: grouped by the pages that use it (`group`: page 1's alone,
  then each later page's, the shared, the rest, as Annex F lays a
  linearized file out; `Pack.uses` finds them on a thread of its own
  while the rest is written), built on a thread per core (8 at most),
  deflated and encrypted once, as a stream under its own number. What's
  copied as it was stays where it is. A container whose data would be
  more than the reader takes is written as its members, and so is
  everything when the cross-reference stream would be left no number, or
  under an encryption that leaves streams in the clear but not strings.
- **An update** (`append`), after the base the source's `XrefFacts`
  describe: `/Prev` at its last section's real start, `/Root` and
  `/Info` as stored, `/Size` past every number the base uses, its
  cross-reference streams' included. Nothing to write is no section.
- **`/ID`:** the first string is kept and the second is a hash of what
  was written, so the same objects write the same bytes. The first is
  read as lopdf makes an RC4 or AES-128 key from it, the array's first
  string however many it has, so a file whose `/ID` is one string keeps
  its key, and gets a second. An encrypted file without one (only
  AES-256 can open without it) gets the hash for both; a plain one
  without one gets none.
- **Encryption** (`crypt`, bug 4 and proposal 23): an encrypted source
  keeps how it's encrypted (`Source::crypt()`: the decryption its
  password opened, the `/Encrypt` dictionary as stored, its number, and
  `/ID[0]`). A save encrypts each object it writes on its own, under the
  number it's written as, or an object stream's members with it (the
  container encrypted once, as a stream, under its own number, so no
  member's string is encrypted on its own), through vendored lopdf's
  `encrypt_object_with` (`vendor/lopdf/SHEAF.md`): strings in stream
  dictionaries too, an embedded file by `/EFF`'s filter, and the
  `/Encrypt` dictionary, cross-reference streams, metadata when
  `/EncryptMetadata` is false and a signature's `/Contents` left as they
  are. An AES IV is the first 16 bytes of HMAC-SHA256, under the file
  key, of the object's `ObjKey` (`ObjKey::bytes`, see Saves are outputs)
  and what it encrypts (`Crypt::encrypt_as`, slice 9), so a save is
  deterministic, an object keeps its IV whatever number it's written at,
  and no IV serves two plaintexts, an update's section's too
  (`indirect_as`, slice 10; the number-keyed `serialize::indirect` and
  `Crypt::encrypt` went). An encrypted string keeps its format, a
  literal escaped as any is, so it takes about its own length, not twice
  it. The `/Encrypt` dictionary, its `/P` with it, is written as stored
  and `/ID[0]` kept, so the file opens with the same passwords and
  permissions: a Rewrite with stable numbers keeps the dictionary at its
  number, an export numbered from 1 writes it as a new object, and an
  update repeats the base's reference. lopdf opens an encrypted file
  only when its trailer refers to the dictionary, never when it's
  direct. lopdf's eager reader still takes `/Encrypt` out of the
  document it loads, so the writers take it from the source; sheaf's
  loads no longer strip it.
- **Which outputs stay encrypted** (proposal 10): saves, Save As,
  extract and split, Clean Copy, Export Flattened and Combine keep the
  primary's encryption; print, page images and the comment summary are
  unencrypted: a `Profile`'s `encryption`, `Keep` or `None` (slice 9).
  Encryption follows the primary: pages from an encrypted
  file inserted into a plain one come out plain, and a plain file's
  pages combined into an encrypted one come out encrypted.
- **Durable writes** (`durable`, bug 2): a file is replaced whole
  through a temp file beside it, `.<name>.<16 hex digits>.sheaf-tmp`
  (the name cut short so the whole fits in 255 bytes), made with
  create-new, so two saves never share one. It gets the target's
  permissions (Unix), is fsynced, is renamed over the target, and the
  directory is fsynced (Unix): a directory that can't be read or synced
  is logged, not a failed save, since the file is replaced by then. A
  symbolic link is followed to its file and stays a link. Any failure
  removes the temp file. Saves, Save As, extracted pages, Clean Copy,
  Export Flattened, recovery sidecars, image PDFs, scans and the stamp
  library's collections are written so; print's PDF, page images and
  comment exports aren't yet (slice 19). From slice 19 they are: Save as
  PDF through `Out::File`, page images, text and comment exports through
  `durable::write`, a TIFF streamed to `.tif.part` and renamed; print's
  bytes go to the printer from memory.

**A file another program changed** (proposal 33). Reading a file stamps
it from the open handle, `FileStamp { len, mtime, tail, window }` (the
tail an xxh3-128 of its last 64 KiB; from slice 10 the window one of up
to 4 KiB at the position its last `startxref` names, read on its own
when that's further back than the tail, as a 7k-page file's
cross-reference table is): `Store::read_file` gives it beside the
source, for the opened file's record, and a save stamps the file
it wrote, its own bytes with the file's length and time, for the save's
record. A save to the document's file checks that file's stamp (the
saved record's) when it's planned, before writing anything and again
just before the rename, or for an Append just before the journal and
once the file is open to append. If the file was appended to, rewritten
at its length (at its end or its last cross-reference section) or only
touched, nothing is written and the save stops with
`materialize::SaveStop::Ask(Ask::FileChanged)`; the app asks Save As…,
Overwrite or Cancel, and Overwrite answers the ask
(`AskAnswers::overwrite`), which makes an Append a Rewrite
(`Why::FileChanged`) that replaces the file and the other program's
change with it. A record also keeps the whole file's checksum when sheaf
knows it (`content_hash`, slice 10: a Rewrite's output, an Append's
carried over its section, a copy's hashed as it's copied; not an opened
file's). A source keeps no stamp: the record holds the file's.

### Saves are outputs (M10.5 slice 9)

A save is an output, never an edit (§9 of `plan/complete/m10.5-edit-api/README.md`): it
writes a version's state and leaves a record of what it wrote, and no
version, state or source changes, so undo across a save is exact and the
screen keeps what it shows. Slice 9 built it under `materialize/`: a save
seals its version (`save_input`), plans (`plan`), writes
(`materialize`, then `write_file`'s durable write) and records the file
(`record_save`). Every save, Save As, export and recovery sidecar is this
writer's; in slice 9 every save was a Rewrite, and slice 10 brought Append
back (the next section). The old writers, their entry point, the dirty
tracking, the legacy projection and the rebase onto the saved file are
gone; the tests that save both ways run both halves (`both_ways!`,
below).

- **A save, start to end** (`materialize::{save, write_file}`).
  `Document::save_target(path)` says whether a path is the document's
  file and, when it is, the stamp the file must still have. `write_file`
  writes a plan's file once the answers answer every ask: `materialize`,
  then `durable::write` with the stamp checked again before the rename
  (unless the reader chose to overwrite), then the record, stamped as
  the written file; an Append and an Unchanged plan as the next section
  says. `save(doc, path, answers, p)` runs it all on one thread,
  `save_input` to `record_save`, and `abandon_save` on anything that
  stops it: the command line's and the tests' (the command line answers
  `break_signatures` and drop XFA, as its old `Signed::Rewrite` did, so
  a signed input saved elsewhere copy-appends when its signatures allow
  the change) until slice 19's `Session::save`, over the save core
  `save_with(doc, path, SaveHow, answers, p)` (see Sessions and saves,
  under Exports, memory sources, Session and `sheaf edit`).
  `save_appended` and `save_rewritten` are the both-ways tests' halves.
- **Sealing.** `Document::save_input(v)` gives what a save writes from (a
  `SaveInput`: the version's state, the primary, and the saved
  version's state and record when there is one) and seals `v`: an OCR
  flush or a typing commit during the save makes a version after it, which
  stays dirty, and pruning keeps it. `record_save(rec)` makes the version
  saved and holds its record (the version saved before keeps only its
  `SaveNote`, the History panel's `{ path, at, strategy }`, and a saved
  version pruning detached goes with its anchor, M10.5 slice 12), and
  `abandon_save(v)` ends a failed or canceled one; either unseals.
  `recovery_input()` seals nothing, and `SaveInput::detached(&Snapshot)`
  is an export's, with no saved side.
- **Plans** (`materialize::plan`): a `SavePlan` is its `Strategy`
  (`Unchanged`, `Append { onto }` or `Rewrite { why }`), its `Ask`s
  (`FileChanged`, `Signatures`, `XfaHoldsValues`), the form transforms
  save policy applies (`SaveTransform`) and the notes for the reader. A
  save that stops says why (`SaveStop::Ask`, `Failed` or `Cancelled`).
  A `SaveTarget` says where it writes and how (`Profile`). In slice 9
  `plan(input, target)` made every save a Rewrite (`Why::AlwaysRewrite`)
  and asked a signed file Rewrite or Cancel by a count of its signatures;
  slice 10's planner (next section) decides the strategy and asks by
  each signature's impact, offering Append anyway. It asks `FileChanged`
  when the target's stamp no longer holds (`SaveTarget.stamp`) and
  `XfaHoldsValues` when it drops XFA whose datasets hold values.
  `AskAnswers` are the reader's answers; `unanswered` gives the first
  ask they don't answer.
- **Records** (`materialize::record`). A `SaveRecord` says what sheaf
  knows about the file it last wrote: the version it holds, its path and
  time, how it was written (`StrategyKind`), its `FileStamp`, its
  cross-reference facts and encryption, where each object sheaf wrote
  sits (`ObjMap`), which of the primary's object numbers are still in it
  (`present`), the root objects' keys the state doesn't own (`roots`:
  the catalog's, the form's, the outline root's and the name
  dictionary's, as written), and its page tree's nodes with their size
  at open (`tree`). The keys a save always emits from the state are
  fixed per root object (`CATALOG_OWNED`, `ACROFORM_OWNED`,
  `OUTLINES_OWNED`), so set, save, undo, save gives each one back.
- **The opened file's record** is synthesized at open
  (`SaveRecord::opened`) when the primary was read from a file: the map
  is the identity (the primary's objects at their own numbers, which it
  doesn't list; only the catalog, info, form, outline root, labels and
  page-tree nodes are keyed), every number in use is present, and the
  roots and tree are the file's. The opened version holds it; `ready`'s
  new version keeps it. A primary made in memory (blank pages) has none,
  so its first save is a Rewrite (`NoSaveRecord`); a document restored
  from its recovery sidecar has its file's, with the sidecar pending
  (slice 10).
- **Objects by what they are** (`materialize::key`). The emitter names
  an object by an `ObjKey` (a page, an annotation, a field, a bookmark, a
  source object as a copy, a resource by digest…), never by number;
  `ObjKey::bytes` is the fixed encoding an AES IV is keyed by. A source
  object a page's annotations reach is that page's copy (`PageDef.copy`,
  counted across every page of its bytes), so a copy's annotations are
  its own; what its content and resources reach is copy 0, which every
  copy of the page shares. One a form instance reaches is the instance's
  copy (`FormInst.copy`, fixed when the op that brought it made it, M10.5
  slice 11), so two inserts of one file never share an instance's
  objects.
- **The emitter** (`materialize::emit`) gives a state's objects by key,
  each built object referring to others by placeholder (`Syms`: a
  reference at generation 65,535 names a key), with the PDF version they
  need and notes for the reader. A page's dictionary is its base page's,
  read lazily, every key sheaf doesn't model kept: a Rewrite places every
  page in a new balanced tree (fanout 32, every page at one depth, its
  nodes carrying nothing), so the page writes all four inheritable keys
  itself, `/Rotate` 0 included, its boxes as the file had them while the
  state keeps them and the state's otherwise. `/StructParents` stays only
  on copy 0 of a primary page, with a note when a copied page's file was
  tagged, and `/Thumb` goes when sheaf draws over the page. The catalog is
  written by its ownership table: owned keys from the state (`/Pages`,
  `/PageLabels` from the `LabelSpec`s, `/SHEF_Generators` from the
  generators; no `/Version`, which the header gives), every other key
  the primary's,
  which `keep_only` prunes. Copies go by key (`copy::Closure`): each
  object a copy reaches is copied once, and a reference to a page goes
  to that page's copy, else its lowest copy written, else null (bug 14).
  A copy never follows into its file's document-level objects: its
  catalog is the written catalog and its form the written form (a
  signature's `/Reference /Data`, a stray key naming the form), and its
  page tree, outline, structure tree, name dictionary, destinations,
  threads, optional content, page labels and information are null; a
  widget the closure reaches is null too (the widgets a save writes are
  its pages' annotations, so a copied one would be on no page), and so is
  an object the file can't read (it doesn't parse, or decrypt), which the
  reader is told about.
  The keys written at the number of the primary object they are (copy 0
  of a primary page, an annotation, field or bookmark read from the
  primary, the outline's root, the form) are claimed before anything is
  emitted (`emit::claim`), and one nothing changed is the file's object as
  it is (`SymObject::Raw`, judged by `Cx::unchanged`: the same values,
  each placeholder naming the number the file's object refers to).
- **Pages, content and the outline** (`emit::{pages, content,
  outline}`). A page is written as its base page is (`emit::Shown`: its
  source, index and copy), that page's object (copy 0 of a primary page
  keeps its number), keys and annotations, with the state's boxes and
  what sheaf draws around it; until slice 17 a derived copy's page
  (a header stripped; a crop until slice 15, a watermark until 16) was
  written as the page it was made from with the copy's `/Contents` and
  `/Resources`. The generators on a page (watermarks and backgrounds from
  slice 16, the header and footer from 17) and, from slice 18, its
  recognized text are drawn as blocks around its isolated content
  (Watermarks and backgrounds as generators; OCR as content). Until slice
  18 the OCR layer went through the old installer, with the emitter as
  its `content::sink::Sink`: `text_layer::install` (`[q, base…, layer]`),
  which first took out a layer the page held already; until slice 17 the header and footer went through
  `stamps::install` too, its Bates numbers counted at each Rewrite. The
  page's own
  streams are referenced, never re-encoded; a resource is one object a
  file (`ObjKey::Res`, its other objects `ObjKey::ResPart`), so every OCR
  page draws in one font, and identical blocks are one (`ObjKey::Block`,
  by digest). The outline is written in place by provenance: a bookmark
  read from the primary keeps its item's number (`Emission::owned`, as
  copy 0 of a primary page does), its `/SE` and the keys sheaf doesn't
  model, its title, color and style as written while they read the same,
  and its `/Dest` or `/A` while it goes to the page written as the object
  they name (a named destination or a GoTo stays one); an item nothing
  changed, its place included, is copied byte for byte. A `Target::Kept`
  bookmark keeps its item's `/A` and `/Dest` from whichever file it came
  from, another file's copied with what they reach (bug 15). A closed
  bookmark's `/Count` is negative.
- **Annotations** (`emit::annots`): a page's `/Annots` is its records',
  in their order. A mark is written through `annot::emission`, pure: its
  dictionary with `/SHEF_Annot`, its appearance (an `AnnotPart(id, Ap)`
  form, its resources by digest through the emitter's sink) and a
  replacement's StrikeOut (`AnnotPart(id, Companion)`), which carries
  `/IRT` to its Caret and `/RT /Group` and nothing else: `file::is_ours`
  knows a companion by that group under a sheaf Caret. A page stamp draws the form its
  `FormRef` names (the source's object, or a `Blob`'s page made a form,
  once a file), an attachment embeds its `BlobRef`'s bytes, once a file.
  A mark read from the primary keeps its number; one the state holds as
  read is the file's object as it is, with its companion, but for a copy
  another program made that the open gave a fresh id
  (`FileAnnots::fresh`), whose object says the id it was copied with. Another tool's
  comment or link is its source dictionary, references copied with the
  page, `/P` the page as written; a reply's `/IRT` names the key of what
  it replies to (by name on the page, else by object), so a reply follows
  a mark given a fresh id, and a mark's reply takes its parent's `/Rect`.
  A connector's `/Dest` is the next of its group, by the page's key. What
  the records don't hold stays with its page: a pop-up goes with the
  comment that opens it (`AnnotPart(id, Popup)`), and is dropped with
  anything else; screens, printer's marks, trap networks, watermarks, 3D,
  media, widgets outside the form and entries with no subtype are kept as
  the page has them, copied with it. `/StructParent` stays only where the
  page keeps its `/StructParents`.
- **Forms** (`emit::form`, generalizing what forms/merge.rs did). Field
  dictionaries are their source's, copied as their instance
  (`FormInst.copy`): an instance past copy 0 of its file's bytes gets
  objects of its own. `/Parent` and `/Kids` are the tree's; a parent
  sheaf made is written from its record. A field merged with its one
  widget is one object under `ObjKey::Field`, which the page's `/Annots`
  names, its widget key an alias in the record's map (`Emission::aliases`);
  a copy stays merged. One whose widget the state shows twice is written
  apart. A primary field's kids the records don't account for and no page
  shows stay in its `/Kids` when they're widgets (widgets on page
  templates); any other such kid, a page or the catalog (0092321's
  `Page2[0]` lists its catalog), isn't the field's and isn't written as
  one (Greg, 2026-10-22: copied, it carried what the save wrote anew, a
  removed generator's text among it). A field the state
  shows nowhere that showed as its file was read (a widget of it, or of a
  node under it, on a page or a page template) isn't written, nor the
  parents it leaves with none, as `keep_only` takes such a field out of a
  file (an edit that takes a node's last widget takes the node too, by
  the `widget.field` cascade of M10.5 slice 11). Nor is a node whose
  `/Kids` another node's read took, left with nothing under it: written,
  it was a typeless terminal field of its own, and a duplicate where its
  name was another's (0982696; Greg, 2026-10-22, reversing slice 8's
  keeping it as the file had it). A form left with no
  fields isn't written either, unless its file had none as read (XFA
  alone). The form's
  dictionary writes the keys the state owns from `FormDoc`, which reads
  them at open (`/CO`, `/DA`, `/Q`, `/NeedAppearances`, `/SigFlags`,
  and the XFA part's facts, `XfaState`); an insert adds its form's fields
  that calculate and whether it needs appearances. `/DR` is the
  primary's with each other file's merged in, a clashing font renamed in
  that file's `/DA`s; that file's form's `/DA` and `/Q` are pushed down
  onto its roots, and its document scripts join the catalog's
  `/Names /JavaScript`.
- **Save policy** (`forms::rules::plan`, pure, at plan time): `DropXfa`
  when the fields differ from the primary's as read (its tooltips onto
  `/TU`), `DropUr3` on every Rewrite of a file with usage rights (on an
  Append, from slice 10, only when they don't allow what it changes),
  and `FixSigFlags` beside it when nothing signed needs `/SigFlags`. The
  emitter applies them to the form's dictionary and the catalog's
  `/Perms`; the record keeps them.
- **Numbering** (`materialize::number`). Stable, for saves: the primary's
  objects keep their numbers, and so does each key the emitter owns at a
  primary object's (copy 0 of a primary page's dictionary keeps the page's
  object wherever it moved, bug 3; a bookmark its item's); a key the previous
  save numbered keeps its number; new keys are numbered past the
  primary's `/Size`, its cross-reference streams, every number in use
  (the eager load's too), the previous map and every primary number the
  emission names, so a reference to a number nothing is stays one: the
  pages in order, then the rest by key. Dense,
  for exports: a finished file's objects from 1, a stream byte-identical
  to one before it written once. lopdf's `renumber_objects` isn't used.
  No number past what a PDF may use (`serialize::NUMBERS`, Annex C's
  8,388,607, as many as sheaf's reader takes) is kept or given: a
  primary object numbered past it (an object stream's member the table
  doesn't place, which only the eager load reads) is a key like a new
  one, moved in the Rewrite's graph with every reference to it followed
  (`Graph::relocate`, a reference to such a number that nothing is made
  null), with a note, and the next save keeps its number. The primary's
  `/Size` and cross-reference stream are reserved only while they leave
  the numbers the save needs, its own cross-reference stream's included;
  a save that would need more stops, and `full::write_bodies` refuses any
  such number, since its section lists every number from 0.
- **The Rewrite** (`materialize::materialize` of a Rewrite plan; an
  Append's is the next section's, and an Unchanged plan has no file to
  make). The primary is loaded
  eagerly without its `/Encrypt` (`graph::rewrite_graph`), the emitted
  objects go in at their numbers, the pages it doesn't keep let go of the
  annotations it writes as the objects they are (`graph::release_written`:
  a widget a deleted page shared with a kept one isn't a dropped page's),
  `keep_only` runs against the pages and tree nodes, what nothing reaches
  is pruned, and `serialize::full::
  write_bodies` writes each object either as the primary's bytes
  (`Source::raw_span`, which keeps nothing it read) or anew. An object is
  copied as it was only when the numbering is stable, it's the
  primary's, the encryption is the same `Crypt` (`Profile.encryption`:
  `Keep` keeps it, `None` writes none), neither the emitter nor
  `keep_only` touched it, it isn't an object stream's member and its
  generation is its header's, and the base is sound. A source that used
  object streams (its newest section a stream, or a hybrid, whose table
  goes) is packed again, its section a stream: an object stream's
  members, and whatever else is written anew, go in containers grouped
  by the pages that use them (`graph::page_uses`), unless the encryption
  leaves streams in the clear but not strings. On a stable save the
  containers take the numbers the source's own had first, but one
  something still names, then numbers past every one the primary and the
  previous save used, so saving again doesn't grow `/Size`; on a dense
  one, numbers after the objects. A table's objects, and those not
  packed, are written as objects of their own, encrypted each on its
  own. On a stable save the `/Encrypt` dictionary stays at its number,
  and a cross-reference stream takes a number past every one the primary
  and the previous save used.
  An AES IV is HMAC-SHA256 under the file key of the object's `ObjKey`
  and the plaintext (`Crypt::encrypt_as`), an update's section's too
  from slice 10, and a container's under `ObjKey::Container`, its
  number. The header gives the version needed: the
  primary's header or catalog `/Version`, 1.5 for cross-reference
  streams, 1.6 for AES. The `Output` holds the bytes, the notes and an
  `OutputRecord`: the `SaveRecord` but for the stamp, which the write to
  disk adds (`stamped`). Progress counts the pages emitted; a cancel
  stops it as `SaveStop::Cancelled`.
- **Post-conditions** (`materialize::verify`), failing closed: before
  writing, copy-on-write (an emitted object sits at a primary number only
  as the primary object it is, or as a root object; and every other
  object at a primary number is the primary's object there as its source
  reads it, but what `keep_only` reported changing, the `/Annots` that
  `release_written` let go of, the objects naming one `relocate` moved,
  and those copied as their bytes were: `verify::copied`), every reference
  resolved (a primary object's to a number the primary never used, or
  past what a PDF may use, is the input's damage), and the pages reached
  the tree's and the page templates'; an export's post-pass runs after
  these, and the references and pages are checked again on what it
  leaves. After writing, the form check finds no widget, field or font no
  input form's finding of its kind names (by name, or as a field under a
  parent sheaf gave a form whose names clashed; a damaged input's
  findings by kind, its names not being its fields'), and no damaged tree
  unless an input's was (save policy's findings aside: broken signatures,
  `/SigFlags` and XFA kept as read, earlier revisions; and a value with
  no appearance, which a page delete leaves where the chosen button's
  widget went), and in debug builds every object reads back as the
  checked graph has it, through the file's keys when it's encrypted, one
  copied as its bytes were too. A stream whose `/Length` is a reference,
  or whose data its `/Length` doesn't fit, or an object without its
  `endobj`, is never copied as it was (`Source::raw_span_whole`): the
  copy would name the length's object, which the Rewrite's graph doesn't,
  say a length the copy's place doesn't bear out, or carry the damage on.
  The spans are found on other threads while the graph is built and
  pruned, told to stop and waited for when the save stops first. An
  annotation, widget or field nothing changed is the file's object as it
  is without being built, only while it's what the built one would be (a
  mark's `/P` its page's and no pop-up; another
  tool's annotation no `/RT` without a parent; an outline root typed; a
  field no empty `/Kids`), and the form check runs only when the save
  wrote any of the form anew. The graph is lopdf's eager load, which
  reads what the lazy source does (an object at another entry's offset no
  longer shadows the one the table lists: `vendor/lopdf/SHEAF.md`), so
  the bytes copied say what the graph checked says. At 7,000 pages a
  Rewrite takes 152 ms on the synthetic and 781 ms on the heavy fixture
  (slice 10, the medians of seven processes), against 134 and 707 ms
  for slice 8's writer (1.13× and 1.10×), and adds 50 and 253 MB against
  its 62 and 251 MB; the 7k harness asserts at most 1.3× the time
  (flagging past 1.2×) and 1.5× the memory added
  (`rewrites_on_the_synthetic_and_heavy_7k`). Nearly a fifth of it is
  freeing: the primary's page dictionaries the emitted ones replace, and
  the graph at the end. From object streams' slice 2 the heavy's Rewrite
  packs: 808 ms (1.14×; 774 ms just before), adding 256 MB, and its file
  is 13.8 MB where it was 17.9.
- **Pruning's changes.** `kept::keep_only` is unchanged but for what it
  gives back: its notes, and every object it changed or added
  (`Kept.modified`), which a Rewrite re-serializes while copying the
  rest as the file had them.
- **Exports** (save.rs's API until slice 19's `export::run`, an
  `ExportTx` of transforms, see Exports, memory sources, Session and
  `sheaf edit`). Each is a Rewrite of `SaveInput::detached`, planned as
  one, so save policy's form transforms apply on every way out:
  `save::rewrite` and `rewrite_to_path` keep the encryption and the
  numbers (`Profile::default()`: `Keep`, `Stable`); extracted and split
  pages, Clean Copy and Export Flattened keep the encryption and number
  from 1 (`Keep`, `Dense`); print writes none (`None`, `Dense`). The old
  rewrite path (`build_rewrite`, `rewrite_with`, `finish` with its prune
  and renumber, `kept_encryption`) and `full::write` are gone. An export
  writes what `subset` keeps of the state, held to its invariants as the
  cascades of deleting the other pages would leave it: a field node that
  showed only on pages left out goes, with the parents it leaves with no
  fields, and the form's roots and calculation order follow; a connector
  whose other end is left out has no partner; a label range starts at
  the first page of its range kept; and a generator is on the pages kept
  of its own, the header's Bates numbers the binder's while its live
  tokens count the pages written (slice 17). Clean Copy, and print
  without markup, filter sheaf's marks and other tools' comments out of
  the target, keeping links and widgets, and strip what the records
  don't hold in a post-pass on the checked graph (`materialize_then`'s
  `PostPass`); Export Flattened, and print with markup, draw marks into
  their pages with `flatten_annots` as that post-pass. Nothing of a
  graph a post-pass ran on is copied as its source had it. So an export
  that leaves out a hybrid form's widget page drops its XFA copy, as a
  page delete does.
- **The document's file** (`Document::file`) is the saved record's path
  and stamp (a restored document's record is its file's, from slice 10;
  slice 9 kept a `disk_base` for it), never the primary's name: a Save As
  is a save, its record the new
  file's, so the new file is the document's from then on, and the
  primary stays the file the document opened. `SaveTarget.same_file`,
  the export guards (`Snapshot.file`: Clean Copy and Export Flattened
  never replace it) and the recovery sidecar's place follow it.
- **The app's saves** (`DocView::save_to`). Save and Save As seal the
  current version (`save_input`), plan in the background, ask what the
  plan asks (FileChanged: Save As…, Overwrite or Cancel; Signatures:
  from slice 10 Append Anyway, Rewrite or Cancel; XfaHoldsValues, agreed
  to once a window), write in
  the background (`write_file`), then `record_save` (`Doc::take_save`),
  or `abandon_save` when the reader cancels or the write fails; a
  FileChanged found at the rename asks again, the version still sealed.
  From M5 slice 2 a view's place is a file or an untitled document
  (`Place`): Save of an untitled one is Save As (`DocView::save`),
  suggesting "<name>.pdf" in its images' folder, and once it's written
  the place is the file, whose name the window's title and dialogs take.
  A second save while one is sealed is refused ("Already saving…"), since
  its base would be the first's file. Nothing is reopened or re-read
  after a save, and no rendered image is dropped: images are cached by
  render key (slice 13), which a save doesn't change. A generator is
  drawn on each page whose base doesn't hold its block as drawn now
  (`content::intact`, shared by the emitter, `untouched` and the render
  key), before a save and after (from slice 17 the header and footer
  too). Autosave writes a sidecar 3 s after the last edit (at least
  every 30 s), never while a save or another sidecar is in flight, and not
  again while the current version and edits are the ones the last
  sidecar holds; as a save ends, a sidecar is written on the UI thread,
  before the save is done (D39), when edits followed the version saved
  (the one before is stale against the new file), and removed otherwise;
  one already being written then is stale, and writes it again in the
  background. Recover restores from memory, with no temp file. The
  notice after a save says how it wrote and why it rewrote ("Saved
  a.pdf (Rewrite: 1 comment removed since the last save) in …"), or
  "Nothing to save" for an Unchanged plan.
- **Recovery, format 2** (`recovery`, §11.10). The sidecar beside the
  document's file (`<name>.pdf.sheaf-recovery`) is `sheaf-recovery 2`,
  `append` or `full`, the base file's length and its stamp in hex (from
  slice 10 the tail's hash, the window's, then its time in nanoseconds),
  its flags and the bytes, written from `recovery_input()`, nothing
  sealed, as a save writes them (`Keep`, `Stable`: encrypted when the
  file is, save policy's form transforms applied, so a page deleted from
  a hybrid form leaves no XFA in it). `find` takes one only while its
  base is still the file (`FileStamp::holds`), and removes a stale,
  malformed or format-1 one. `restore` checks the base again and opens
  from memory (`MemoryKind::Recovered`) under the file's name. From M5
  slice 2 a document no file holds yet (an untitled one) has a third
  kind, `sheaf-recovery 2\nuntitled\n`, the folder its Save As starts in
  (UTF-8 hex, empty for none) and the whole document, as Full's is
  written (`recovery::write_untitled`, its parts written in turn by
  `durable::write_parts`, no copy made whole), in a folder of the
  caller's (the app's `state_dir()/recovery/`) as `<name>.sheaf-recovery`.
  Its name is claimed (`recovery::Claim`, `claim_untitled`): a lock on
  `.<name>.sheaf-lock` there, taken and let go under the folder's `.lock`
  so a lock file is removed only while no one has it open, held by the
  one view of the document for its life and by each write in flight for
  the write's, in any sheaf process; only the holder writes or removes
  the sidecar. `find_untitled` takes the sidecars no one holds, claimed,
  sorted by name, reading their heads alone, removing what isn't one and
  an unheld name's temp files and lock file, and `find` beside a file
  takes none. `restore_untitled` reads the body once into the shared
  buffer and opens it from memory with no record, and
  `Document::never_saved` leaves it with no saved version, so it's dirty
  and its first save is a Rewrite. Format 1,
  an update section over the original and its temp-file primary, is
  gone. In slice 9 every sidecar was the whole file (about 0.5 s and the
  file's size written per autosave on the scanned 7k), and
  password-protected documents got none; slice 10's are Append sections
  (next section).
- **Moved.** The cross-file copy (`copy_object`, `remap`,
  `copy_page_dict`, `is_page_node`, the inherited keys, the page
  reservation `copy::reserve`) is `materialize::copy`, and a source
  loaded whole is `materialize::graph` (`graph::load`, `graph::Loaded`).
- **The old writers are gone:** incremental.rs, with its
  replaced-layer refusals; the old entry point (`save_to_path_noting`,
  `Signed`, `WouldBreakSignatures`, `IfChanged`); save.rs's outline,
  annotation and page plumbing; forms/merge.rs and `rules::take_out`;
  the legacy projection (and `forms::Field`'s `source`, `copy` and
  `under`, which only it set); the baseline and every dirty flag, so a
  snapshot computes nothing; the renames table, since a save writes a
  renamed mark anew (`FileAnnots::fresh`) and the app hides it by its
  old name from the view's facts; `SourceInfo.stamp`; `WriteCx`'s trust
  of the file's own stems; and `serialize::append::Base::of`. What
  lived on moved: derived copies (until slice 17) left sheaf's marks out
  with the emitter's filter and read their pages' boxes as `Geom::read`
  does, stamp collections read theirs
  from what each page inherits, `file::annots_in` takes each page's turn
  from the page table (`Source::geom`), and tests write marks onto a
  fixture page through `annot::emission` (`forms::fixture::write_marks`)
  and read a file's back through its source.
- **Both ways.** A test that saves both ways is a case taking a
  `materialize::How` (`Rewritten`, `Appended`), run twice by
  `both_ways!(name, name_appended: case)`: the Rewrite as `name`, the
  Append as `name_appended`. In slice 9 the Append halves were ignored
  twins, since no save could append; slice 10 runs them by default (the
  next section says how each half saves).

### Appends, the planner and the removal check (M10.5 slice 10)

A save writes the least that carries what changed since the saved file:
nothing (Unchanged), an update after the file (an Append, in place, or
after a verified copy of it for Save As of a signed file), or the whole
file (a Rewrite) for the reasons its plan gives. Strict removal is the
rule: a save never leaves text the document removed readable in an
earlier revision of its file, unless the reader chose that for a signed
file (Append anyway). One mark saved onto the 500 MB scanned 7k takes
about 21 ms, its fsyncs included.

- **Effects** (`effects`, §7.1). `classify(key, before, after, cx)`, the
  one place that knows what a change means for saving, judges one slot's
  change as `Effects { flags, sig }`: `Flags` (pages added, removed or
  reordered, geometry, content, `REMOVES`, `REPLACES_TEXT`, annotations,
  field values, the field tree, the form, the outline, labels) and the
  classes of change a signature may allow (`SigClass`: `FILL`, `ANNOT`,
  `OTHER`). It is total over the keys: a value a slot can't hold counts
  as every flag. A page's source page is removed only when no target page
  shows it, an OCR layer only when no target page shows that layer (from
  slice 18 also when a filter takes out the recognized text the page's
  file holds, a re-OCR's or a Clear's, counted once a page with the layer
  it replaces); a field rename, and a tooltip, items, scripts or a default value
  replaced, are replaced text; another tool's comment is compared with
  the mark its summary reads it as. `judge(saved, target, keys)` judges a
  save's path once per key: its flags and classes, what it removed by
  kind and count (`Removed { kind, count }`: pages, comments, widgets,
  fields, bookmarks, OCR layers, comments' text, field values and names,
  bookmark titles, label prefixes, the header and footer, a form's XFA
  part, and a recovered tail's removals), the full names of the fields it
  touches (every node under one renamed or moved, for FieldMDP), what it
  does to annotations (`AnnotOps`: create, modify, delete, for usage
  rights) and `changed`, the keys whose values differ.
- **The path** (`SaveInput.path_keys`). `Document::input(v)` gives the
  keys any step changed on the history tree's path from the saved
  version to `v`, through the version where their branches meet, both
  sides: each version's own keys (`Version.keys`, from M10.5 slice 12),
  walked up from both ends until they meet (`edges_between`). Until
  slice 12 the root stopped at the saved version or the branch point
  holding it, so the path was always in history. Now pruning may take
  the saved version, which detaches into an anchor (`SavedVersion::
  Detached(Anchor { state, record, keys_to_root })`): its state and
  record, and the keys from it to the first version kept, which grow as
  that first version moves on; the path is then the anchor's keys and
  those from the first version kept to `v` (see One funnel and the
  history budget). Each key is judged by value, the saved state's
  against the target's, so undoing back to the saved state by any route
  changes nothing.
- **The plan** (`plan`), in order: an Append onto a copy (`Onto::CopyOf`)
  for a save to another path when the primary has signatures or usage
  rights, they hold (`SaveRecord.signatures_hold`: true when opened,
  false after a Rewrite, inherited by everything else) and the saved file
  still has its stamp, else `OtherTarget`; no record that knows its file,
  `NoSaveRecord`; a file another program changed, `FileChanged` with its
  ask; **Unchanged** when nothing else was found, the target is the saved
  file, the path changed nothing by value and no recovered tail is
  pending, before `BaseUnsound`; then `BaseUnsound`
  (`SaveRecord::takes_updates`: a rebuilt cross-reference table, an
  unsound base or a Full restore), `RecoveredRemovals` (until slice 17
  a header and footer forced a Rewrite here, `LegacyHeaderFooter`),
  strict removal's `Removes` and `ReplacesText` (by
  `RemovedKind::is_text`: OCR layers and the header are text), and
  `TransformNeedsRewrite` (dropping an XFA part whose datasets hold values
  the path replaced); otherwise `Append { onto }`. `Why` has a reader's
  `Display` (§7.3), which `SaveReport.why` carries to the app's notice.
  The save transforms are planned for the strategy: `rules::plan` takes
  the Append's class of change and annotation operations, and drops
  usage rights on an Append only when `UsageRights::allow` refuses it. An
  Append's class counts `OTHER` for any transform and for a `/Version` the
  section must raise (`emit::content::needs_after`), which the emitter
  uses too.
- **Signatures** (`forms::sigs`, §9.3). `Source::signatures` reads once
  what each signature allows after it: a certification's DocMDP `/P` (2
  when it gives none, 1 when it can't be read), a field's PDF 2.0
  `/Lock /P`, the fields FieldMDP and `/Lock` lock (`All`, `Include`,
  `Exclude`; one that can't be read, or past `FormLimits`, locks every
  field), and Reader's usage rights (`/Perms /UR3` or `/UR`: `/Form
  /FillIn` and `/Annots`' create, modify and delete). A signature whose
  dictionary can't be read (1263925's) is an approval signature judged
  as `/P` 1. `sig_impact` gives each signature's `Verdict` for a save:
  every one `Invalid` on a Rewrite; on an Append `Allowed` within what it
  allows and locking none of the fields touched, `Noted` for an approval
  signature with no MDP (taken to allow filling in and comments), and
  outside them `Invalid` for one with a `/P`, `Flagged` for an approval
  signature; `/P` 1 allows no change, so only an Append of none (Save As
  of a file nothing changed, a plain copy) passes it. `Flagged` and
  `Invalid` ask, while
  the record's signatures hold: `Ask::Signatures { impact, append_anyway,
  reason }`. A signature they'd ask about is checked first (backlog B5,
  `forms::sigs::integrity`): one whose signed bytes no longer match its
  digest is `AlreadyBroken`, which doesn't ask, so a file a Rewrite (or
  `--signatures break`) wrote, or one whose signatures were broken when
  it came, isn't asked about again. `Signature::integrity` is `Intact`,
  `Broken`, or `Unknown` with why, which asks as an intact one does
  (never broken on a guess): the `/ByteRange` hashed, from the source's
  bytes, by the digest algorithm the SignerInfo names, against its
  messageDigest signed attribute, a document timestamp's messageImprint,
  or for no signed attributes the DigestInfo its RSA PKCS#1 v1.5 value
  opens to with the key of the signer's certificate in the CMS. The CMS
  is read as BER (cms 0.3.0-rc.0, pinned until M12's kickoff), as far as
  the digest needs; the ranges as pyHanko hashes them (in order, cut at
  the file's end, overlapping, starting anywhere), but counted from the
  file's header as sheaf reads every offset (from its first byte too,
  pyHanko's count, where bytes come before the header: either that
  matches is intact), and those covering more bytes than the file are
  `Unknown`. It's computed once per
  signature dictionary of a source, when a save first needs it, within
  `FormLimits` (`signatures` checked, `cms` the longest `/Contents`); a
  CMS that nests its indefinite lengths past 32, or isn't SignedData of
  one SignerInfo, an algorithm sheaf doesn't hash (GOST, SHA-3), and a
  signature value that isn't RSA PKCS#1 v1.5, are `Unknown`. Over the
  corpus's `signatures/` it agrees with pyHanko's `intact` on every
  signature it can check. The form check reports a signature that fits
  its revision but no longer matches as `BrokenSignature`. Append anyway (`AppendAnyway { stays_readable }`, what the
  save leaves readable, by kind) is offered on an Append the signatures
  don't allow, and on a Rewrite whose every reason is what an Append would
  leave readable (`Removes`, `ReplacesText`, `TransformNeedsRewrite`,
  `RemovalCheck`, `RecoveredRemovals`), never on one for a file it can't
  follow. The mapping stays conservative until Greg's Acrobat cross-check
  (`/P` 2 allows only filling in, so a comment on a P2 file asks even with
  usage rights that allow comments).
- **Answers** (`SavePlan::answered(input, target, answers)`, which
  `write_file` applies first): an Append or Unchanged plan answered
  Overwrite is `Rewrite { why: [FileChanged] }`, which asks about
  signatures that hold, with no Append anyway; a Rewrite whose ask offers
  Append anyway, taken (`AskAnswers::take`), is that Append onto the
  saved file or a copy at the target, its transforms the Append's; an
  Append the signatures don't allow, answered `break_signatures`, is
  `Rewrite { why: [Chosen] }`. Taking an offer that says something stays
  readable sets `stays_readable` too, and only then does the removal
  check report (`Mode::Report`) rather than stop: an Append anyway to an
  offer that said nothing stays readable (the signatures alone) runs it
  as `Mode::Anyway`, which stops on what it finds, the planner having
  missed a removal, and asks again with the reason and `RemovedKind::Found`
  in what stays readable.
- **What an Append writes** (`deps::closure`, §7.2's `emit_deps`): the
  keys of the path whose values differ, closed over what each one's
  object depends on (the module's table): a page's dictionary and, when
  its base or copy changed, its annotations, the bookmarks to it and the
  links to the page written for a source page; when it turns or its
  boxes change, every connector of each group with one on it (a group
  goes in the order its connectors show on their pages, so a turn may
  change each one's next; found by the Append ≡ Rewrite property in
  M10.5 slice 12, `append_rewrite_a_connector_s_next_follows_a_turned_page`;
  each group is taken once, however many of its pages turned or of its
  connectors are written, so neither scans the annotations again nor
  recurses as deep as the group is long,
  `append_rewrite_a_long_connector_group_is_taken_once`);
  an annotation's parts,
  replies and connector group; a field's widgets, every node over it, and
  the form's dictionary when the node came, went, moved or shows other
  widgets; the pages of a widget that stops or starts being its field's
  object; the outline by place; labels; the catalog for
  `/SHEF_Generators` and labels that came or went, a transform the file
  hasn't had
  (`DropUr3`'s `/Perms`; the form's dictionary and every field for
  `DropXfa`'s tooltips) or an XFA copy policy keeps again; and the
  spliced tree's changed nodes and moved pages. A page the file has at
  the number of a primary page that another page is copy 0 of now (a page
  replaced, appended over that page's object, and the page brought back)
  leaves the number to it and is written at one of its own, with what
  names it by number, as for a page whose base changed
  (`Closure::renumbered`); a copy's reference to its source page counts
  as moved for `stale`. Until slice 11 it also reported a form instance's
  index that moved (`FormCopies`, `form_shift`); an instance's copy is
  fixed now.
- **The emitter's Append** (`EmitScope::Keys`, `EmitBase::Saved`). Only
  in-scope pages, pages holding in-scope annotations, and the form,
  outline, scripts, labels and catalog when any of theirs is in scope are
  visited; a key the state derives is written only when in scope, and one
  out of scope is a placeholder that resolves to the saved file's object:
  its map's number, a primary object at its own number while the file has
  it, or a primary page, annotation, field or bookmark at the number it
  owns. A key the map has a resource or block at is referred to, not
  built again, and what only left-out objects reached is dropped. The
  catalog is written when the section needs a later version than the file
  (its `/Version` the later of the two whenever the catalog is written),
  or the form or outline came or went. A page an Append writes always
  carries `/CropBox`, since a node of the saved tree may crop, and keeps
  its own `/Rotate` as written when its turn didn't change; a link to a
  page not written is left out, as `keep_only` drops it from a Rewrite;
  an attachment refers to the file's embedded stream when that holds its
  bytes; and transforms are the record's plus the plan's. The objects of
  a source one of whose pages is written as another page, or none, than
  in the saved state (`deps::Closure::relinked`) are copied again where
  the section reaches them, so a link's indirect action names the page as
  written now, and so is an annotation the records don't hold that a page
  written lists, whose `/P` is the number of the page it was written
  with.
- **The page tree, spliced** (`emit::tree::spliced`, §11.6). A page stays
  in its node and a page inserted or moved joins its predecessor's, so a
  reorder in a flat tree writes one node. A node grown past max(128, its
  size at open) splits into siblings holding its kids in turn (the root,
  which keeps its number, into nodes under it), and an emptied node
  leaves its parent. Only nodes whose `/Kids`, `/Count` or `/Parent`
  changed are written, each with the keys its file's node held
  (`TreeNode.from`), and each page under another node is written with
  all four inheritable keys. A node is its key wherever it moves, found
  by its number through the map, never by its place; a node split off
  takes a key past every node key the map has, so an emptied node's key
  is never another's. With the order unchanged, no page showing another
  page than it did (two primary pages may trade their objects, so their
  numbers), and every page written in a place, `tree::parents` answers
  without splicing.
- **Numbering** (`number::appended`): a key the file's map has keeps its
  number there, unless that's the number of a primary object another key
  of the emission is, a primary object the emission owns its own, and the
  rest are numbered past every number the file uses and every primary
  number the section names (what its raw primary objects refer to
  included), so a dangling reference stays dangling, pages first, the
  section's cross-reference stream after them. An update frees nothing.
- **The section and the present check** (`append::append`). Before a byte
  is written the section is checked (`verify::section`: copy-on-write, a
  primary object written at its own number only over itself, the widget
  merged with it now, keys moved elsewhere or records the state no longer
  has, looked up once a section; and every reference resolved in the file
  or the section), and refused (`append::Refused`) when it refers to a
  number the file hasn't got (`Missing(NotInFile)`: base objects a
  Rewrite pruned; until slice 11 also when a form instance's objects in
  the file were another instance's, `FormShift`), when a primary object's
  number in the file is another record's of that object still (one the
  section writes elsewhere only when it's a page the closure renumbered),
  or the file has it elsewhere (`Held`), when what the file has and the
  section doesn't write again names a page that a copy's reference names
  another page by now, or the catalog names a primary page that's no
  page's copy 0 now (`Stale`, `deps::Closure::stale`), when the file
  can't take an update, its tree can't be spliced or too few numbers are
  left past the primary numbers the section names (`Unsound`), when the
  removal check vetoes (`Removes`), and, in Veto mode while signed
  fields' signatures hold, when it writes the catalog or the form's
  dictionary (`Unplanned`), which no signature allows. `write_file` turns
  a refusal into the Rewrite the plan would have given (`plan::fallback`,
  its reason `BaseObjectsMissing`, `BaseUnsound` or `RemovalCheck`, with
  the Rewrite's asks: a signed document asks first, Append anyway offered
  only for the removal check, saying what the path removed and the text
  the check found stay readable), or for `Unplanned` into the ask an
  Append they don't allow makes; `Written.strategy` says which was
  written. The section's form is checked only when it writes any of it,
  or writes a page at another number than the file has it at (its
  widgets' `/P` name that number), against what the newest revision
  reaches (`forms::check_latest`), since what an Append unlinks stays in
  the file; debug builds read every object back. The output is the
  record's pending tail, then the section, after the file's length on
  disk.
- **The removal check** (`materialize::removal`, §11.9) runs on every
  Append. The old side is the saved file on disk, read lazily through the
  primary's password. It examines each object the section writes over,
  then what that object referred to that nothing the section writes
  refers to, followed through (never by `/Parent`, `/P` or `/IRT`, a page
  or node only as a node's kid, the catalog never). Derived objects are
  judged on their record inputs: a sheaf mark's objects on what the mark
  says (contents, author, name, its kind's text, tape rows, its file or
  stamp by hash), a page's blocks on its OCR words and header text (an
  OCR block the opened file holds, from slice 18, is one of its own
  objects, judged on its words decoded), a resource as its user's. From slice 17 a generator's old block that
  the section leaves, drawn again from the same recipe (a live redraw),
  is an allowed drop (`Emission::allowed_drops`; Headers, footers and
  Bates as generators): per page, as a block is content-addressed and
  pages laid out alike share one. A page that draws a generator again
  from its block's recipe isn't judged on what that block drew there;
  the block itself goes unjudged only when every page it was on draws
  it again or is gone, and is otherwise judged on each page that held
  it. A block is kept as it is only when it's laid out alike (`/L`) from
  the same recipe (`/R`, which says its Bates number too), so the record
  and an open read what's drawn. Other objects are judged as a `TextBag`:
  strings, what content draws through its fonts (pages with inherited
  resources, forms, tiling patterns, Type 3 glyphs), scripts, rich text,
  XFA and XMP as words, images and embedded files by hash; their bag must
  be within the bag of everything the section writes. A string in codes
  its font doesn't map is its raw bytes' hash, so only the same bytes
  match it (`text_bag` says when it falls back), and what can't be read
  counts as removed: content past a cap of the content interpreter,
  which `text_bag` reads through, among it. A veto on a derived object is a
  misclassification, logged as `RemovalMisclassified { key }`; vetoes are
  counted by kind (`removal::vetoes`). Modes: `Veto` stops the Append,
  `Anyway` too (an Append anyway the signatures alone asked), `Report`
  (Append anyway to what stays readable) adds a note
  (`append::STAYS_READABLE`) but stops on a derived object whose removal
  the save's path didn't say (`PathEffects::removing`, `Section::said`),
  a misclassification, to ask again with `RemovedKind::Found`,
  `ReportAll` (that agreed to, `AskAnswers::found`) reports whatever it
  finds, `Recovery` flags a sidecar. As a test
  oracle on a Rewrite (`removal::of_rewrite`), it runs over what the
  Rewrite wrote as though it were one section: in the removed-text suite
  it finds what each Rewrite took out, and in the Append ≡ Rewrite
  property no derived object's text gone unless the plan said something
  was removed or replaced. Its cost on the scanned 7k is about 7 ms, nearly
  all of it opening the file's reader.
- **In place, with a journal** (`serialize::append::append_in_place`,
  §11.7). The stamp is checked again, keeping the tail it read; a
  journal beside the file a link names (`<name>.pdf.sheaf-append`: the
  length before, the stamp, and the update's `startxref`) is written
  whole, locked and synced under a temporary name, renamed into place and
  its directory synced; the file is opened to append, checked to be that
  long still, written and synced; then the journal goes, its lock with
  it, and the directory is synced. The new stamp is computed from the
  bytes written (the old tail and the section), not read back, its time
  read once the file is closed (Windows sets it as the handle closes). An
  append that fails is cut back at once, its time restored. `undo_torn`
  leaves a journal its writer still holds (an open in another window or
  process while it appends), and runs before every open (`open::file`, which
  reports it in `Opened.torn`, and the app's `DocView::open` before
  `recovery::find`, which shows the notice): a file longer than the
  journal's length that doesn't end in that update or a later one is cut
  back, while its bytes before it are the ones the journal stamped, its
  time restored so a sidecar written against it still holds
  (`Torn::Undone`); a complete append only loses its journal, and a shorter or
  changed file is left (`Torn::Left`). A test-only hook tears the next
  append (`tear_next`).
- **CopyOf** (`durable::copy_appended`, proposal 32): a streamed copy of
  the saved file, hashed as it's copied and compared with the record's
  `content_hash` when there's one, the section after it, synced and
  renamed into place as `durable::write` does, its time read once it's
  there; the saved file's stamp is checked before the copy and again just
  before the rename. The copy's record keeps its checksum. It never
  reflinks: the copy reads the file anyway to check or learn its
  checksum. So Save As of a signed file keeps its
  signatures, the original untouched.
- **Unchanged.** `write_file` writes nothing for an Unchanged plan: its
  record is the saved one's for the version saved, which
  `Document::record_unchanged` attaches with no save note, so the
  document is clean. `SaveReport.strategy` is None, and the app says
  "Nothing to save".
- **Records** gain `content_hash` (a `sheaf_util::Checksum`, streamed
  xxh3-128 that a record extends over each Append's section), `sound`,
  `signatures_hold`, the file's `pdf_version`, and `pending_tail`; the
  roots keep the trailer's `/Info` (`RootValues.info`), and each tree node
  which primary node's keys it holds (`TreeNode.from`).
- **Recovery sections** (§11.10). A sidecar is `append`: the sections an
  Append of the current version would write after the file, whatever the
  plan would decide and with nothing stopping them
  (`removal::Mode::Recovery`; save policy's transforms from
  `plan::sidecar`), encrypted as the file is, a password-protected
  document's too. Its flags say what
  the sections do, for the save that writes them: `removes` (the path's
  removals or replaced text, a transform that needs a Rewrite, a pending
  tail that removes, or what the removal check finds), their classes
  (`fill`, `annot`, `other`, `other` too when they write the catalog or
  the form's dictionary) and `field:` with each field's name. It is
  `full`, the whole document, when there's no record, the record can't
  take an update (a Full restore's too), or the section is refused or
  fails to be made (logged), so a fault in the Append never leaves a
  document without one; until slice 17 a header and footer made it
  `full` too. A restored document opens from the file's bytes on
  disk with the sections after them, read as one, its record the file's
  with them pending (`SaveRecord::restored`,
  `PendingTail { bytes, flags: TailFlags }`; `Came::Restored`); its sidecar is its pending tail plus
  its own section, against the same base (chained), and none when they
  change nothing. It's dirty while the tail is pending
  (`Document::on_disk`). Its first save appends the tail, then its own section, with
  the tail's classes and fields judged with the path's, or rewrites for
  `RecoveredRemovals`, offering Append anyway on a signed file. A Full
  restore's tail has no bytes, so its record doesn't know its file
  (`knows_file`) and its first save is a Rewrite (`NoSaveRecord`). The
  Append sidecar of a mark on the scanned 7k is 1.2 KB, written in about
  27 ms.
- **The command line** saves through `materialize::save`, answering
  `break_signatures` and dropping XFA values, so a signed input saved to
  another path copy-appends when its signatures allow the change, and
  rewrites (`Chosen`) when they don't, until slice 19's flags (`SaveHow`
  and `SaveTo`, see Sessions and saves).
- **Tests.** The both-ways harness runs both halves by default:
  `save_rewritten` (`plan::forced_rewrite`) and `save_appended`
  (`plan::forced_append`, Append anyway, which stops rather than
  rewrite); a document made in memory appends as though opened from its
  primary's bytes (`Document::append_input`, `opened_from`). The Append ≡
  Rewrite property (`materialize/append_rewrite_tests.rs`, 32 cases by
  default and 1,024 ignored) runs history's edit kinds, batches, groups,
  undo, redo, jumps, trims and, from M10.5 slice 12, pruning to a byte
  budget over a real form file with planned saves,
  Save As and forced Appends between, and after each save compares the
  reopened file's facts (pages' boxes, content and resources by value,
  annotations, outline, labels, the field tree, the form's dictionary,
  the header) with a fresh Rewrite's. Every feature's removed text, both
  ways, is checked gone from raw, hex, UTF-16 and decoded bytes
  (`kept/tests.rs`), and over the corpus (sheaf-ops
  `tests/append_corpus.rs`): every file takes a mark as planned (3,770 of
  4,389 append, 359 ask about signatures, 260 rewrite), a page, a mark, a
  reply, a mark's text, another tool's comment or a bookmark deleted
  after a first save each way is gone from every file (an ignored tier,
  run before each slice's commit), the removal check
  finds what a page-delete Rewrite took out of 2,899 of 2,901 files, and
  nine signed and certified files append a comment, or ask and append
  anyway, their signed bytes intact. The 7k harness asserts one mark's
  incremental save on the scanned 7k under 50 ms
  (`INCREMENTAL_GATE_MS`).
- **What went:** the always-Rewrite rule (`Why::AlwaysRewrite`,
  `plan::signatures_broken`, `Ask::Signatures`' count and the app's
  Rewrite-or-Cancel prompt), number-keyed IVs (`serialize::indirect`,
  `Crypt::encrypt`), Full-only recovery and `Document.disk_base`
  (`set_disk_base`, `mark_unsaved`), the password-protected autosave
  skip, and the ignored twins.

## Rendering edited pages: the spikes (M10.5 slice 2)

From M10.5 slice 13 an edited page renders from an **overlay**: an append
section built in memory over the source's own bytes, in which each edited
page is replaced in place under its own object number, so it keeps its
inherited attributes and shared resources. The section is its group's,
and scoped to it (GROUP, below): its root `/Pages` lists only the
group's pages and its AcroForm only their fields, so in the group's
engine a page's index is its place in the group. PDFium loads the base
and the section as two segments through `FPDF_LoadCustomDocument`,
without copying the base, and an edit reopens only its group's engine
(`plan/complete/m10.5-edit-api/README.md`, §10.1 and §10.3).
Before anything depended on that, slice 2 measured it with three
throwaway spikes against written criteria and set the realizer's and the
engine pool's numbers from the results. The spike code is gone; this
section keeps the criteria, the results and the verdicts.

**How they ran.** Sections came from vendored lopdf's
`IncrementalDocument`: the base's xref kind, `/Prev`, `/ID` kept,
`/Encrypt` repeated and each new object encrypted by `encrypt_object`; a
stale `/XRefStm` and `/DecodeParms` dropped; offsets counted from the
header on a file with leading junk; new objects numbered above `/Size`.
The reference was lopdf's Full write of the same edit, decrypted for an
encrypted base. A two-segment `Read + Seek` reader went to pdfium-render's
`load_pdf_from_reader`. Pages were compared at 1,024 px, pixel for pixel,
with no tolerance, two ways: **appearances only** (each shown annotation
flattened, then the page rendered, as `render_appearances` does), which
the criteria use, and **live** (widgets by `FPDF_FFLDraw`, as the page
view draws them). Machine: AMD EPYC 7302P, 32 threads, release builds,
libpdfium chromium/7881, pdfium-render 0.9.4.

### Spike A: encrypted overlay sections

*Criterion:* overlays on RC4-128, AES-128 and AES-256 bases render
pixel-equal to a decrypted Full write. *Fallback:* a decrypted in-memory
image per source.

The synthetic 7k was encrypted by lopdf (user and owner passwords), pages
0, 3,499 and 6,999 were replaced in place, and the base and section were
loaded through two segments with the password. Three edited and two
untouched pages were compared:

| Algorithm | Pixels differing, appearances / live | Overlay open | Base open | Section |
|---|---|---|---|---|
| RC4-128 (V2) | 0 / 0 on all five | 1.52 ms | 1.49 ms | 2,840 B |
| AES-128 (V4, AESV2) | 0 / 0 | 1.52 ms | 1.49 ms | 3,181 B |
| AES-256 (V5 R6, AESV3) | 0 / 0 | 8.14 ms | 8.09 ms | 3,180 B |

Each edited page differs from the base and each untouched one doesn't,
the section holds none of the edit's text in plain text, and poppler
renders the overlay as it renders the Full write. 50 encrypted corpus
files (the first 75 encrypted rows of the inventory, at most 15 an
algorithm; 30 of them xref-stream bases) pass too: AES-128 15, AES-256
10, RC4-128 11, RC4-128 under V4 6, RC4-40 8. **A passes on all three; no
fallback.** An AES-256 (R6) source pays its key derivation on every
open: 8.1 ms at 7k, against 1.5 ms.

### Spike B: overlays on real files

*Criterion:* per base class, at least 99% of sampled pages pixel-equal
(appearances only) to a Full write, and RSS per engine within the cap the
spike sets. *Fallback:* a failing class realizes as a standalone PDF per
group.

883 bases: the 402-file pool sample (`bench/pool-sample.txt`), all 76
government forms with fields, a seeded 400 of the corpus's other 3,976
SafeDocs forms with fields, and five built bases (the synthetic, heavy and
scanned 7k, the synthetic with leading junk, and the synthetic with a
broken `startxref`). The first, middle and last pages are edited; two
others are left alone. A base is in each class it has: classic table or
xref stream (its newest section's kind, which the overlay writes), hybrid
(`/XRefStm`), linearized (`/Linearized` within 1,024 bytes of the
header, the check before B3, which since counts only a linearization
that holds), damaged (lopdf rebuilt the xref, or lopdf and
PDFium count different pages), leading junk, encrypted. Times are
medians, base → overlay, of the middle edited page drawn live at
1,024 px; cold is the first render in a fresh engine, warm the next.

| Class | Bases | Pages | Pixel-equal | Open, ms | Cold, ms | Warm, ms | MB per engine, median / p99 |
|---|---|---|---|---|---|---|---|
| classic table | 355 | 1,057 | 100% | 0.23 → 0.24 | 24.9 → 32.9 | 17.3 → 17.7 | 1.20 / 9.63 |
| xref stream | 527 | 1,517 | 99.93% | 0.58 → 0.61 | 24.3 → 29.1 | 14.5 → 14.5 | 1.80 / 13.36 |
| hybrid | 36 | 107 | 100% | 0.44 → 0.42 | 22.1 → 29.4 | 15.8 → 16.4 | 2.74 / 9.09 |
| linearized | 502 | 1,448 | 99.93% | 0.54 → 0.56 | 24.5 → 30.2 | 14.8 → 14.8 | 1.80 / 10.47 |
| damaged | 3 | 6 | 100% | 55.0 → 55.7 | 22.9 → 24.3 | 12.7 → 14.3 | 5.35 / 5.35 |
| leading junk | 2 | 6 | 100% | 1.55 → 1.56 | 24.3 → 24.0 | 15.1 → 15.5 | 5.34 / 5.34 |
| encrypted | 69 | 166 | 99.40% | 0.58 → 0.64 | 22.9 → 28.1 | 15.2 → 14.9 | 1.51 / 10.11 |
| all | 883 | 2,574 | 99.96% | 0.43 → 0.45 | 24.5 → 30.3 | 15.4 → 15.4 | 1.55 / 10.87 |

- The one miss in the xref-stream, linearized and encrypted rows is one
  page: lopdf's write of `pool/safedocs/2932/2932995.pdf` recolours page
  0's logo with no edit at all, and the overlay matches the base. In
  this run no page differed because of the overlay, and live renders
  matched at the same rates. Two pool files are untestable: lopdf finds
  no pages in 0676477 (PDFium 47) and rejects 2289457's xref.
- **RSS per engine** is measured over 24 overlay engines on one base,
  each with its own section and one page rendered, from
  `/proc/self/statm` after `malloc_trim`; the growth is glibc heap,
  linear in the engine count. The pool sample's: median 1.82 MB, p95
  7.74, p99 10.87, max 22.11. **The per-engine cap is 16 MB:** the pool
  sample's p99, rounded up to a multiple of 8 MB. Every class's p99 is
  within it.
- The 7k bases, one at a time:

  | Base | MB per engine | Open, ms | Cold, ms | Warm, ms |
  |---|---|---|---|---|
  | synthetic | 5.34 | 1.52 → 1.50 | 23.0 → 24.1 | 5.63 → 5.97 |
  | heavy | 35.42, over the cap | 8.66 → 9.36 | 738 → 751 | 5.50 → 6.31 |
  | scanned | 7.20 | 3.66 → 3.67 | 37.3 → 63.8 | 16.4 → 17.1 |
  | leading junk | 5.34 | 1.55 → 1.56 | 24.3 → 24.0 | 5.32 → 5.91 |
  | damaged | 5.35 | 55.0 → 55.7 | 22.9 → 24.3 | 5.75 → 5.93 |

  25 MB of the heavy's engine, and nearly all of its cold render, is
  PDFium's form environment loading all 7,000 fields at the first page
  load. Scoped group sections (GROUP, below) remove both.

**The thin classes, again.** That run's damaged and leading-junk rows
are 3 and 2 bases, mostly built: too few for a 99% criterion. A second
run took 90 pool files outside the sample, in inventory order: the first
50 with `%PDF-` past offset 0 (1 to 307 bytes in, 37 of them behind a
UTF-8 byte-order mark) and the first 50 whose `startxref` leads to no
xref (10 are both). Both runs together:

| Class | Bases | Pages | Pixel-equal | MB per engine, median / p99 |
|---|---|---|---|---|
| damaged | 56 | 133 | 80.45% | 0.96 / 5.35 |
| leading junk | 52 | 130 | 100% | 0.11 / 5.34 |

- Leading junk passes, with the section's offsets counted from the
  header (§11.7). 32 of the 50 picks load without a rebuilt xref; 18 are
  damaged too.
- Damaged fails: 26 pages of 24 bases differ, all of them damaged and
  none because of lopdf's writer. On 22 signed files the overlay loses
  the visible signature stamp on page 0 that the Full write shows; on
  one a page left alone loses a seal; on one (encrypted, linearized) the
  edit doesn't show. Five picks were untestable (lopdf finds no
  pages in two and rejects three xrefs).

**B passes on every class but damaged,** and damaged bases realize as a
standalone PDF per group, which §10.1 has for them by design.

### Spike C: render-time box edits

*Criterion:* a crop rendered from the base engine, through
`FPDF_RenderPageBitmapWithMatrix` and a clip, is pixel-equal to the crop
written as `/CropBox` on 100% of the sample. *Fallback:* crops realize
as overlays.

1,267 pages: the first page of 400 pool files; spike B's widget pages of
the forms (165 government, 683 SafeDocs); 16 synthetic pages, four boxes
(plain, off the origin, an offset fractional CropBox, an upside-down
MediaBox with a negative origin) at each `/Rotate`; and three heavy
pages. 1,231 are at `/Rotate` 0, 19 at 90, 4 at 180 and 13 at 270, and
1,046 have widgets. The crop is inset 7.3% left, 11.1% bottom, 5.7% right
and 13.9% top. The matrix is solved in PDFium's own f32 arithmetic so
that it composes to the written page's transform, bit for bit on 3,364
of 3,801 transforms. Two other geometry edits were compared the same way:
the crop with a 90° turn, and the turn alone through today's rotate
parameter.

| Edit | Pixel-equal, live | Appearances only |
|---|---|---|
| crop | 590 (46.60%) | 1,266 (99.92%) |
| crop and a 90° turn | 549 (43.36%) | 1,167 (92.11%) |
| the turn alone | 1,228 (97.00%) | 1,232 (97.31%) |

- `FPDF_FFLDraw` takes an integer rect and a rotate, never a matrix, so
  a matrix render draws no live widgets: 676 of the 1,046 widget pages
  lose them. All 221 pages without widgets are equal both ways.
- Some translations can't be landed exactly in f32: one crop (3 px, one
  level), and 17 (live) to 100 (appearances) with a turn, 99 of those by
  at most one level. A sub-ulp difference resamples a whole image.
- The turn alone isn't the written `/Rotate` on 38 pages live: 32
  transforms f32 can't make exact, 22 of them resampling an image across
  its area (over 1,000 px) and 10 off by one level, and 6 forms whose
  widgets `FPDF_FFLDraw` draws differently under a rotate.
- Edge rounding and annotations outside the box caused no failure.

**C fails: crops, and every box edit, realize as overlays.** No
geometry-only page renders from the base engine.

### The verdicts

**GROUP = 128 pages, scoped.** *Rule:* the largest G whose overlay opens
within 0.5 ms of the bare base and whose engine, with every page of its
group previewed, is within the 16 MB per-engine cap, on all three 7k
fixtures (8 engines a fixture, a header and a watermark on every page of
the group). In place, the heavy's engine is 35–46 MB at every G, so no G
fits. **Scoped**, the section replaces the root `/Pages` with one whose
`/Kids` are the group's pages, in order, and the AcroForm with one whose
`/Fields` are the top-level fields with a widget on those pages, so an
engine's page j is its group's page j and its form loads no other field.
Engine MB, in place → scoped, and the scoped open and cold render:

| G | Synthetic, MB | Heavy, MB | Scanned, MB | Scoped open, ms | Scoped cold, ms |
|---|---|---|---|---|---|
| 8 | 5.01 → 0.94 | 35.16 → 4.02 | 7.19 → 1.82 | 1.39 / 7.72 / 2.02 | 9.0 / 8.8 / 28.5 |
| 16 | 5.11 → 1.05 | 35.48 → 4.41 | 7.75 → 2.39 | 1.39 / 7.78 / 2.05 | 9.0 / 8.9 / 28.3 |
| 32 | 5.32 → 1.28 | 36.20 → 5.21 | 8.88 → 3.54 | 1.36 / 7.70 / 2.06 | 9.2 / 8.9 / 28.3 |
| 64 | 5.75 → 1.73 | 37.63 → 6.77 | 11.18 → 5.87 | 1.42 / 7.76 / 2.12 | 9.1 / 9.2 / 28.7 |
| **128** | 6.62 → 2.68 | 40.44 → 9.89 | 15.74 → 10.51 | 1.49 / 7.86 / 2.16 | 9.1 / 10.1 / 28.3 |
| 256 | 8.44 → 4.57 | 46.27 → 16.32 | 24.91 → 19.80 | 1.60 / 7.90 / 2.28 | 9.1 / 12.8 / 28.2 |

The bare bases open in 1.35–1.42, 8.58–8.73 and 3.59–3.69 ms (synthetic,
heavy, scanned), and in place the cold renders are 24, 740 and 48 ms. A
scoped section of 128 is 55–58 KB. Scoped renders matched in place on
every page previewed, and on the corpus (spike B again) matched the Full
write on 1,982 of 1,982 edited pages over 881 bases, 854 of them form
pages with widgets, live and appearances only. Scoping also spares
PDFium's first `FPDF_LoadPage(k)` in a document, which loads every page
dictionary up to k (20 ms at the synthetic's middle page).

**What scoping gives up.** §10.1 has an edited page keep "its page
index, AcroForm, OCProperties, inherited attributes and shared
resources". A scoped section keeps the last three and not the first two,
which slice 13 builds on:
- **The page tree is flattened, and inconsistent:** each page's
  `/Parent` still names its own node, whose `/Kids` and `/Count` still
  list pages outside the group. Inherited attributes resolve because
  PDFium walks `/Parent`. Slice 13 can instead write each page's
  inheritable keys out and point its `/Parent` at the new root, as §11.6
  does for duplicates.
- **An edited page's index is its place in its group's engine**
  (`PageView.index`), not its base index. Whatever reads an edited page
  from its realization, a render, a size or §10.5's text, goes through
  `PageView`.
- **Destinations into other groups** (a link's `/Dest` or `/GoTo`, a
  named destination) name page objects the group's engine doesn't list,
  so they resolve to no page there. Sheaf reads links and the outline
  from the file through lopdf, not from engines.
- **A field whose widgets span groups** is listed in each group that has
  one of them, and its other widgets are on pages that engine doesn't
  list; a calculation order (`/CO`) or script naming a field outside the
  group finds nothing. Widgets still draw from their appearances and the
  field's value.

**LRU count: 6 group engines, pinned ones included.** *Rule:* in each
pane of the page view (two when the window is split, each scrolled on its
own) the visible pages with `PREFETCH` span at most two groups, and so do
the thumbnails in view, while each shows fewer than GROUP + 1 pages. They
do: two-up at `ZOOM_MIN` in a 2,160 px view shows 48 pages and prefetches
2; the strip shows 10 thumbnails at `THUMB_W` and 27 at its 40 px
minimum. Two groups a pane, two panes and two for the strip: 6. The count
holds the pinned engines (those serving visible pages, §10.3) with the
rest, so it never closes a visible group's engine, and what the pinned
ones leave keeps the groups last scrolled away from: 2 with one pane.
Each visible source's base engine is pinned and outside the count.

**Engine byte cap: 96 MB,** 6 × the 16 MB per-engine cap, pinned engines
included. In the fill below the screen's engines peaked at 48.5 MB
(heavy) and 12.9 MB (synthetic), no group was opened twice, and both
panes' pinned engines stayed open.

**Background budget: 2 engines, 32 MB** (2 × the per-engine cap).
*Rule:* an engine for each of two background consumers walking at once,
such as OCR beside Trim White, a job or a plugin's page scan (a document
runs each of those on its own). One walk in page order never goes back
to a group, so it needs one engine; two taking turns need one each.
Measured on the heavy 7k at 300 dpi, every page behind a scoped group of
128, while the screen holds its 6 engines pinned: two walks taking turns,
one from the first page and one from the middle, 1,024 pages each, opened
14 engines with 2, none twice, at most 16.8 MB of them, in 106.9 s; with
1, they opened 1,538, 1,524 of them a group again, in 136.3 s. Neither
closed any of the screen's. One walk over all 7,000 pages with 2: 364.5 s
(52.1 ms a page), 49 engines opened, none twice, at most 20.6 MB, and 768
renders served by the screen's engines. A third consumer at once shares
the 2 and reopens at every turn, as the 1-engine case shows: 2 is chosen
for two at once, not measured to be enough for more.

**Thumbnail fill rate: 310 pages/s.** *Definition:* 320 px previews
(`PREVIEW_W`) of all 7,000 pages in order, every page behind a scoped
group overlay of 128 with a header, a watermark and the crop in the
section, through the screen's LRU (6 engines, 96 MB) with two panes'
groups pinned; pages over the time spent in engine opens and renders.
*Rule:* the heavy 7k's rate, 318 pages/s, rounded down to a multiple of
10. The synthetic fills at 389. The bare base, through one engine, fills
at 389 (heavy) and 512 (synthetic) pages/s while that engine grows to 338
and 91 MB. The scanned 7k isn't held to the rate: decoding a 1275 × 1650
image a page caps its previews at about 150 pages/s on the bare base.

The rate leaves out building the sections: lopdf took 121 ms a group on
the heavy and 21 ms on the synthetic, and that isn't slice 13's assembly.
Slice 13's strip gate counts its own assembly in the time: at 310
pages/s, the heavy's 22.0 s of opens and renders leave 0.57 s for its 55
groups, about 10 ms a group of 128.

### What the spikes found

- **Widgets draw only through `FPDF_FFLDraw` or a flatten.** A render
  with `FPDF_ANNOT` alone draws none, form environment or not, and
  `FPDF_FFLDraw` takes no matrix.
- **A section numbers its objects above `/Size`** and above every xref
  stream's own number. Numbered from lopdf's `max_id`, a section on
  `pool/safedocs/5801/5801985.pdf` (`max_id` 76, `/Size` 78, the newest
  xref stream 77) put a content stream at 77, and PDFium read it as the
  xref stream.
- **The two-segment reader fills each read across the boundary.**
  pdfium-render hands each of PDFium's block requests to one
  `Read::read`, and PDFium takes any nonzero count as the whole block.
- **What a fresh engine costs:** an open of 1.5 ms (synthetic 7k),
  8.6 ms (heavy), 3.6 ms (scanned) or 55 ms (damaged, the xref rebuilt),
  and about 6.6 ms more on an AES-256 R6 source; then, unscoped, the
  first page load walks the page tree up to it, and the first form page
  loads every field (0.7 s and 25 MB on the heavy).
- **The base engine isn't bounded by the LRU:** after previewing every
  page it holds 91 MB (synthetic) and 338 MB (heavy).
- **A rotation delta carried by the render isn't a written `/Rotate`**
  on 38 of 1,267 pages (`RotatePolicy::KeepBase`, §10.1).
- **Slice 13's realize gates, read early, without the realize:** with
  scoped sections the synthetic's open and cold render take 1.49 + 9.1 =
  10.6 ms, so realizing the page, the section's assembly included, must
  fit in the 4.4 ms left of the gate's 15 ms. The heavy's open and cold
  render take 18.0 ms, 2.9× its warm render in the same run (6.12 ms)
  against ≤ 1.7×, and its bare open alone, 8.6 ms, is about 1.7× its
  warm render.
- **A damaged base's overlay isn't its Full write:** on 22 damaged,
  signed files the overlay loses the signature stamp the Full write
  shows. Damaged bases realize standalone (§10.1).
- **Appearances-only renders need care.** PDFium's `FPDFPage_Flatten`
  writes `/MediaBox` and `/CropBox` from the page's own keys, so a page
  that inherits a box other than letter comes out 612 × 792
  (`forms/safedocs/0163162.pdf` page 2, 419.5 × 595.3), and
  `render_appearances` sizes its render from the flattened page.
  pdfium-render 0.9.4's `PdfPage::flatten` reloads the page under a new
  handle but leaves `boundaries()` on the closed one.

The 7k fixtures and the planning scripts are in `bench/`
(`bench/README.md`). The gate harness, `crates/sheaf-ops/tests/gate_7k.rs`,
prints today's numbers on the three fixtures beside slice 2's baselines.

## Rendering edited pages (M10.5 slice 13)

PDFium draws the model. Each consumer says what a page should draw
(`ViewInputs`); the realizer says where PDFium draws it from (a
`PageView`): the source's own bytes when the page draws as its base page
does, else its group's overlay, an append section built in memory over
the source's bytes by the same emitter and serializer as saves; and one
engine pool, on one thread, opens and keeps the engines. What a page
draws is a key (`RenderKey`), which the app's images are found by, so an
edit re-renders only what it changed and nothing is invalidated by hand.
Nothing reaches disk (§10 of `plan/complete/m10.5-edit-api/README.md`).

```rust
pub struct ViewInputs { marks: MarkDraw, generators, ocr_over, widgets, foreign: bool,
                        hidden, dragging: Arc<im::HashSet<AnnotId>>, ocg_off: Option<Arc<OcgOverride>> }
pub enum MarkDraw { ByHost, ByEngine, Off }
pub fn render_key(st: &DocState, page: PageId, inputs: &ViewInputs) -> Option<RenderKey>; // RenderKey(u128)
pub fn render_reads(st: &DocState, page: PageId, inputs: &ViewInputs) -> Vec<Key>;
impl Tx { pub fn read_render(&mut self, page: PageId, inputs: &ViewInputs); }
pub enum EngineKey { Base(BlobId), Group(GroupKey, u128), SourcePatch(BlobId, u128) }
pub struct PageView { engine: EngineKey, index: u32, key: RenderKey, job: JobXform, annots, forms: bool }
impl Realizer {                                    // sheaf_doc::realize, one a document
    pub fn view(&self, st, page, inputs) -> Result<PageView>;            // cheap: the UI thread's
    pub fn open(&self, st, engine, inputs) -> Result<Realization>;       // the pool's worker's
}
pub struct EnginePool<K> { .. }                    // sheaf_render::pool, one a process
pub struct Views { realizer: Arc<Realizer>, pool: EnginePool<EngineKey> } // sheaf_ops::views
```

### What each consumer draws

`ViewInputs` says what's drawn beside the page's own content, and each
consumer passes its own, so hiding a comment on screen changes neither
print preview nor what OCR reads:

| Consumer | Inputs | Draws |
|---|---|---|
| The page view, the thumbnails, blank review's previews, the Header & Footer, Crop Pages (slice 15) and Watermark / Background (slice 16) previews; Find, Patterns, selection, calculator picks and text markup (slice 14) | `screen()`, with `hidden`, `dragging` and `ocg_off` the view's | everything; sheaf's marks GPUI draws over the page (`ByHost`) |
| Print preview | `print(markup)` | with "Comments and markups", as the screen; without, no marks and no other tools' comments (as print prints them), the form's widgets either way. From slice 19, with markup, the page's printed annotations flattened as print draws them (`export::print_preview`) |
| OCR (`ViewPages`), blank detection and blank review's reads, plugins' recognition; Trim White (slice 15), on its measuring state | `content_only()`; blank detection's text with `ocr_over` (slice 14), and OCR's "skip pages with text" check's (slice 18) | the page's own content: no header, watermark, background (slice 16: nor the blocks of the document's own that the file holds), recognized text on its stack (slice 18: the blocks of recognized text the file holds are the page's content, and stay unless a filter takes them out), widgets or annotations, at its shown box (Trim White's: its file's own crop box) |
| Plugins' renders | `plugin()`: `screen()` with `marks: ByEngine`, for the render and its reads alike | as the saved file will draw it, sheaf's marks and the header, watermarks and backgrounds, saved or not, included, in the file's own orientation |
| Plugins' text (slice 14) | `plugin_text()`: `plugin()` with no marks, widgets or other tools' annotations, for the text and its reads alike | what of the page PDFium reads as text: its content, and the header and recognized text, saved or not; no annotation's text, which PDFium's text page never reads |

`foreign` (other tools' comments and links) isn't in §10.1's list: print
preview without markup and `content_only` draw no annotations at all,
which no other field can say. There's no `form_draw`: every consumer
draws widgets live, and an appearances-only render flattens a page,
which a shared engine can't do (the corpus check renders appearances from
an engine of its own). Whole classes are left out by the render's flags,
not by what the overlay writes: `PageView.annots` is PDFium's
`FPDF_ANNOT` (annotations but widgets), `forms` its `FPDF_FFLDraw` (the
form's widgets, which PDFium draws only that way, slice 2), and a render
with neither goes by a matrix whose placement matches the `FFLDraw`
path's pixel for pixel, at every rotation and for tiles
(`PdfiumEngine::render_with(page, job, format, Draw)`). Per-id hiding and
dragging go through overlays: a hidden or dragged annotation is written
under its own key with its hidden flag (`/F` 2), so it hides on its page
and not on a duplicate. Under `ByHost` the overlay leaves sheaf's marks
out, renamed ones too, so a page whose own file lists sheaf's marks in
`/Annots` realizes as an overlay even unedited: a document sheaf saved
with marks on many pages uses more group engines (§10.4).

### Render keys

`render_key` hashes what the page draws under the inputs, each record
destructured whole (a new field is a compile error until it's fed), a
length before what it counts and a tag before each variant:

- the inputs' flags, always, whatever the page holds (they also decide
  what's drawn of annotations no record holds), so `print(true)`'s key is
  the screen's;
- the page's `PageDef` digest (its base and content stack, not its copy
  number) and its boxes without the rotation: a turn is the job's
  (`JobXform.rotation`), so a turned page keeps its key. A page with a
  header its file doesn't draw folds its rotation in, since the header is
  laid out in the turned page;
- each annotation of a kind the inputs draw, by what it's drawn from,
  never its id or copy, so a duplicate shares its original's key: sheaf's
  marks only under `ByEngine`; other tools' comments and links with
  `foreign`, each link with whether it leads nowhere now (a save drops
  it); widgets with `widgets`, with their terminal's value, and then the
  form's `FormDoc` digest; each with its hidden and dragged flags;
- each generator on the page: its id, its recipe digest
  (`recipe_digest(g, page)`: its spec, z, `replaces` and, for the header
  and footer, its date, file and this page's Bates number) and whether
  the page's file holds its block as drawn now; and the page's position,
  the page count and its label, as far as its tokens read them
  (`Generator::needs()`, `CtxNeeds`; from slice 17 `render_reads` adds
  `Order` or `Labels` for them, whether or not the page is intact);
- the layers `ocg_off` turns off of the page's source.

Digests are kept with what they digest (`PageDef::new`, `FormDoc`'s cell),
so a key costs a lookup or two once a value has been seen, and keys are
computed only for the pages being drawn. Position, labels and which
page shows a source page come from tables each state builds once, when
first asked (`Indexes.labels`, `Indexes.shows`; the page map is a
B-tree), and Bates numbers are the generator's own, fixed at apply
(slice 17; until then `Indexes.header` counted them). So a reorder of
7,000 pages changes no key without a page-number header (none with Bates
numbers alone) and with "Page n of N" only the keys of the covered pages
whose place it changed, a mark dragged under `ByHost` changes none, and no
`ChangeSet` drives a cache.

`render_reads` is what the page draws from, as slots: under
`content_only` exactly `Page(p)`; otherwise `Page`, `PageAnnots` when
annotations or widgets are drawn, `Annot` for each drawn annotation not
hidden or dragged, `Field` for each widget's terminal, `Form`,
`Generator(g)` for every generator of the state when generators are drawn
(one whose pages widen to take the page in draws on it), `Page(q)` for
the page a drawn link leads to (a replace of it leaves the link leading
nowhere), and `Order` when a generator on the page that the file doesn't
draw numbers pages or a drawn link is on it; never `Geom`.
`Tx::read_render(p, inputs)` notes them as the transaction's decision
reads: an OCR flush and blank review's delete under `content_only`, a
plugin's recognition likewise (as free functions, `render_reads`, into
`HostState`), and a plugin's render and, from slice 14, its text under
its own inputs. Two changes still don't conflict with such a read: a
generator added after it, whose id no read could name, and a page that a
link leading nowhere comes to lead to again by a replace (the link's
target showing again without an order change); both change the key.

### The realizer

`Realizer::view` takes the key and asks whether the page is
**untouched**: copy 0 of its base page at that page's own object, with
its boxes (its turn is the job's), nothing drawn over it that the inputs
draw (recognized text with `ocr_over`; a generator with `generators`,
where its file doesn't hold its block as drawn now), and, as far as the
render's flags draw
them, its base page's annotations as they are: each drawn record as read
from its own entry, none hidden or dragged, no entry left out (a record
deleted, sheaf's marks and their companions under `ByHost`, another
tool's without `foreign`, a link that leads nowhere), no merged field
split, each drawn widget's `/Parent`s (merged,
its field's) as the model's field tree has them, which a file's `/Kids`
made where its `/Parent`s say otherwise, and the form's `DA`, `Q` and
`NeedAppearances` as the source's form has them. A field's value isn't
counted: no save writes `/V` before M10 slice 9's fills. The predicate is
O(the annotations on the page); `emission_is_base` (test and fixture
builds) checks it against the page's overlay compared with the base
modulo placement (`/Parent`, `/Type`, `/Thumb`; boxes and `/Rotate` as
effective values; resources against the page facts; annotations' `/P`,
`/IRT`, `/RT`, `/NM`, `/Popup`; fields' `/Kids`; the AcroForm's
`/Fields`, `/CO` and `/SigFlags`; the catalog's `/Pages`, `/AcroForm` and
patched `/OCProperties`), and `realize_corpus` checks the two agree on
every page of every corpus form and the pool's sample, under each
consumer's inputs: a disagreement fails, never an exclusion.

An untouched page renders from its source's base engine
(`EngineKey::Base`), or, when the view turns layers of a layered source
off, the source's view patch (`SourcePatch`): its catalog alone in a
section, `/OCProperties` showing every layer but those (`layers::
ocg_patch`, the layers panel's choice, never saved). Any other page renders
from its **group**: the pages over 128 consecutive base pages of one
source (`GROUP`, slice 2's) that aren't untouched, in base order then
copy, as one overlay (`EngineKey::Group(GroupKey { source, chunk },
digest)`, the digest of the pages, their keys, the inputs' flags and the
layers they turn off of the source, so a group whose pages changed is
another engine, and a comment hidden or dragged on one group's page
leaves every other group's engine as it was: what's hidden on a group's
pages is in their keys). A page's `PageView.index` is its base index on a
base engine and its place in its group on a group's. A state's groups are
found once a state and inputs (`DocState::draws_alike` compares states in
O(1)), the inputs known by a digest the realizer keeps for the last eight
that hide or drag something, matched by their sets' pointers, so Hide
Comments' set of every other tool's comment is sorted once and not at
every view; a view is a key and the predicate, cheap enough for the UI
thread at every frame.

**A base PDFium refuses** has no base engine (Greg, 2026-10-20). The
corpus's 4422502 is one: a later revision cut out of the file but for its
trailer, whose `/Root` names a catalog the file hasn't got, and its
`startxref`, which leads into the middle of the first revision's table.
PDFium rebuilds the cross-references, takes that last trailer's `/Root`
and fails with `FPDF_ERR_FORMAT`; sheaf's reader rebuilds them too and
finds the catalog the file has. A file sheaf reads as sound can be one
too: sheaf finds `%PDF-` anywhere, PDFium only in the first 1,024 bytes.
Files PDFium opens pay nothing for this: the pool tells the source when
an engine over its bytes (its base engine, its view patch, or a group's
section over a sound base) fails to open with a load error
(`OpenError::Refused`: any error of PDFium's load but a password; never a
cancelled request or a closed engine), at that open and at each request
told of the failure after (`Opener::on_refused`, so another document on
the same bytes learns it too), and the source keeps it for its life
(`Source::base_refused`). From then on no page of the source is
on its base: every page renders from its group, untouched or not, and
each group is a file of its own, as a damaged base's is; the group's
digest says which, so an engine the pool remembers as failed isn't asked
for again under its old key. A render already asked of an engine over
the source's bytes, by a view made before anything had found them
refused, renders from the page's view found again (`Req::fallback`, which
`Views::draw` gives every such view), by the same job, a view's turn
being the page's from its base page either way; such an overlay asked
for after the refusal by a view made before it fails as refused without
opening, so its fallback runs too. The screen, print preview, OCR,
plugins' renders and the dialogs' previews never see the failure, and as
a render key is what a page draws, not where from, the image that lands
is under the key the next frame's view has, so no blank image is kept and
nothing is asked again. A section PDFium refuses over a base it reads
would be drawn standalone too; `realize_corpus` fails on one. From slice
14 a page's text is read the same way (a query's fallback), so text,
Find, Patterns, selection, plugin text, OCR's "skip pages with text"
check and Remove Blank Pages work on such a source, and printing it
prints sheaf's own print file from memory, as every print does (Text and
printing), and from slice 15 Trim White measures its pages through the
views too (Crop as geometry). What still fails on it is the form type's
query, which has no page to fall back to and is how the document learns
of the refusal. The document's notice says its pages are drawn from
sheaf's copy.

**The overlay** is an emission (`EmitScope::Pages`, `EmitBase::Overlay`)
over the source's bytes, with that source the emission's own, its
setup O(its pages) (the state's tables, not walks of
the document; a visit counter in the tests holds it to that):

- **Scoped, with its keys written out.** The group's pages hang under a
  new root (`PagesNode(0)`) listing only them, each page's `/Parent` that
  root and its four inheritable keys written on it, as §11.6 does for
  pages placed under another node. The catalog is the source's, at its
  `/Root`'s number, its `/Pages` the root, its `/AcroForm` the group's
  form or none, its `/OCProperties` patched when the view turns layers off.
- **The group's form** lists the fields with a widget on its pages and
  the nodes above them: `/Fields` their roots (in field id order), each
  node's `/Kids` only those nodes and the widgets on the group's pages,
  so a one-root form (every XFA-made form's `form1[0]`) loads only the
  group's fields; when the scoped `/Fields` is the file's, the form
  dictionary is the file's and the root is rebuilt. A field whose
  widgets span groups is in each, and `/CO` or a script sees only its
  group's fields. An inserted file's form has its `DA` and `Q` pushed down
  onto its roots, as a save pushes them.
- **`RotatePolicy::KeepBase`:** each page keeps its base page's `/Rotate`
  as its file writes it, and the render's job adds the turn
  (`st.render_delta`), so a turned page with recognized text turns once.
  Rewrite and Append write `/Rotate` from the model (`RotatePolicy::Model`).
- **What it draws** is what the inputs draw, filtered as above: sheaf's
  marks only under `ByEngine`; other tools' comments, links and the
  annotations sheaf doesn't model only with `foreign`; widgets always
  (the flags decide); a reply isn't threaded to what isn't drawn,
  connectors carry no `/Dest`, recognized text only with `ocr_over`, the
  header, watermarks and backgrounds only with `generators`, links
  whose target page the document no
  longer has are left out as a save leaves them, and a reference to a page
  outside the group is null.
- **Copy 0 of the source's objects** keeps its own number; one unchanged
  isn't written (the base has it). Other blobs, and copies, are copied.
- **Derived copies' pages** (a stripped header, until slice 17; a crop
  until 15, a watermark until 16) were drawn as the copy had them, over
  the copy's bytes; no edit makes a copy now.

**Assembly.** Each page's delta is its overlay alone without the
group's frame (`EmitScope::Delta(p)`, which builds no root, catalog or
form for `Part::page` to drop; from slice 17's follow-up), kept by page and
key, `(PageId, RenderKey)`: a delta names its page's own keys, which a
duplicate with the same key doesn't share, while duplicates still share
images by key. The deltas kept weigh at most 32 MB by an estimate that
counts their objects' allocations (`DELTAS_BYTES`), the oldest let go
first; a header page's delta is about 11 KB (11.6 KB by the estimate on
the synthetic 7k, 14.7 KB on the heavy). The frame (`EmitScope::Frame`:
the root, the catalog, the form) is emitted over all the group's pages
each time, and the parts are deduplicated by key, the first claim of a
source number winning (a key that loses its own number is numbered anew
and its object read again from the source). Each source has one
**allocator** for the session (`realize::alloc::Numbers`): numbers start
past `/Size`, every cross-reference stream's own number and every number
in use (slice 2's 5801985), the first of them the sections' own xref
stream's; a key keeps its number in every section over that source, and
none is reused, so the allocator grows by each key its sections ever
wrote (a few dozen bytes each; past the numbers a file may use, opening a
group is an error). The section is written by `serialize::append` in the
base's cross-reference kind, **encrypted with the source's key** (spike
A: no decrypted copy is made). A **damaged base** (`!Source::base_sound`,
its table rebuilt; spike B) takes no section: its group is a file of its
own, unencrypted in memory, its catalog kept to `/Type`, `/Pages`,
`/AcroForm` and `/OCProperties`, everything its pages reach copied at its
own number but other pages and page-tree nodes, and an untouched page of
it under a layers patch goes to its group. Crops are realized (spike C:
no box edit on the base engine): from slice 15 a cropped page is its
source's page in its group's overlay, which writes the page's new boxes,
and no copy. Realizing runs in the pool's opener, on
its worker, so a group's assembly delays other renders by its time.

### The engine pool

`EnginePool<K>` (sheaf-render) is one worker thread (`sheaf-render`) for
every engine of every document: the app makes one for the process
(`docview::engines()`), and each document's `Views` (sheaf-ops) pairs it
with that document's realizer. A request names its engine and how to
open it (`Target { engine, open: Opener, budget }`): the opener gives the
engine's segments and password (a source's bytes; the bytes and a section
after them; or a file of its own), read through one `Read + Seek` that
fills each read across the segments' boundary (`Segments`, slice 2) by
`PdfiumEngine::open_segments`, with no copy; `Opener::base` or
`overlay` says whether it's a base engine; and its probe says whether the
source is still a document's (a `Weak<Source>`). sheaf-render doesn't
depend on sheaf-doc, so the pool is generic over its key and knows
nothing of the realizer. Renders (`Req`), queries (`query`, a closure
run on an engine's page, falling back as a render does, from slice 14;
`form_type`) and walks over a document's pages (`walk`, slice 14: four
pages a turn between renders, each on its own engine, several at once,
ended by dropping the receiver) keep the old worker's ways: priorities,
and a dropped receiver's render skipped. Requests in a batch for one
render (an engine's page by one job, drawing and format) render once and
each is answered with the image, since in a pool of the whole process
two documents, print preview or a plugin may ask for the same one. An
opener, a fallback, a render, a query or a walk's page that panics (a
malformed file) fails its own request, or page, with why, as an engine
that doesn't open does, and the worker serves every other.

- **Pinned** engines are each pane's, the thumbnail strip's and print
  preview's visible ones (`docview::Pins`, which tells the pool only
  when its set changes and unpins when dropped; the strip unpins while
  the Thumbnails panel is hidden). No budget closes one.
- **The screen's** group and patch engines are at most 6, pinned ones
  included (two groups a pane, two panes, two for the strip), and all its
  engines, base engines too, weigh at most 96 MB; past either the least
  recently used unpinned engine closes. A base engine is outside the
  count and grows with every page it loads, so once the screen's engines
  weigh more than 96 MB with nothing unpinned left to close, the heaviest
  base engine over 16 MB closes, pinned or not, and opens again at its
  next request (about every 1,256 distinct pages a lone base engine draws,
  by the estimate; on the heavy 7k each reopen takes about 0.7 s, its
  form environment loading every field).
- **The background's** requests (OCR, blank detection, plugins' text and
  scans, Trim White from slice 15) use any open engine without moving
  the screen's order, and otherwise open their own, at most 2, so a walk
  never closes a screen engine; a screen request or a pin makes one of
  theirs the screen's. Their group and patch engines weigh at most 32
  MB. From slice 14 their base engines grow into what the screen's
  engines leave of 96 MB, and past it the heaviest over 16 MB closes, as
  the screen's does: held to 32 MB, a base engine opened again every
  ~380 pages (19 times in Find Blank Pages over the synthetic 7k; 6
  now).
- **Walks** (from slice 14) hold the engine they read their last page
  on: no count or byte rule but the base engines' closes it until the
  walk reads on another or ends. With pinned engines filling the
  screen's 6, Find's group engine closed after every page it read and
  opened again for the next (6,232 opens over the headed synthetic 7k,
  37.5 s; 49 and 12.1 s held).
- **Weights** are an estimate, since PDFium reports none: 4 MiB an engine
  (`OPEN_BYTES`, slice 2's scoped engines measured 0.8–3.6 MB open), 75 KiB
  a page loaded (`PAGE_BYTES`, the scanned's 74 KB a page) and the bytes of
  the segments only that engine holds (a section, a file of its own). The
  heavy's base engine holds about 35 MB once its form has loaded, against
  the 4 MB counted at its open; the 7k gates measure RSS.
- **Liveness and failures.** An engine whose probe fails closes at the
  worker's next turn, and an engine lives as long as its newest
  requester's source; a process-wide pool shares a `Base(blob)` engine
  between documents opened on the same bytes, and counts every document's
  group engines against the 6. An engine that fails to open isn't tried
  again while its source lives (`Unopened`, wrapping the first failure,
  so a password error still reads as one); the sweep forgets it once the
  source is gone. When PDFium refuses an engine's bytes, its opener is
  told (`Opener::on_refused`), and a render that names where else its
  page is (`Req::fallback`) renders from there (the realizer's section,
  above).
- **Idle,** the worker blocks on its channel; with no walk pending it
  wakes only for a request.

### The app on views

- **The screen.** `Doc.views` is the document's realizer over the
  process's pool and `Doc.inputs` the screen's inputs, made again by
  `Doc::set_inputs` when the comment view is rebuilt, on Hide Comments
  and Show All, when a comment drag starts or ends, on a layer toggle,
  and in `Doc::follow`: `hidden` the comments the Comments filter hides,
  or every other tool's comment under Hide Comments (comments only, by
  id, never a link or one its file hides itself); `dragging` the comment
  being adopted; `ocg_off` the primary's layers the Layers panel turns off,
  once it has changed them (until then a layered file draws under its own
  default configuration). Each frame finds each nearby page's `PageView`
  once (`plan_pane`) and renders from that view (`Want`, `Doc::request`,
  `Views::draw`). Tiles are cached by `(RenderKey, Job)` and previews by
  `(RenderKey, Rotation)`, the job's turn the pane's and the view's
  together, since a key leaves the turn out; a page shows the images of
  the key it was last drawn by at that turn (`Doc.drawn`, by page and
  turn, so two panes at two turns each keep theirs) until an image of its
  new key lands, and eviction keeps those. The layout (`pane::Shown { id,
  geom }`) holds no source page: it's made again only on the order or a
  geometry, and a page shown from another source page at the same size
  changes keys only.
- **Hiding is data.** Nothing is hidden in an engine: the Comments filter,
  Hide Comments, a drag and the Layers panel set inputs, and the screen's
  pages whose keys changed re-render. The layers panel makes no decrypted
  copy and opens no engine of its own: a hidden layer is the source's
  patch engine.
- **Print preview** draws each page under `print(markup)` turned as it
  prints, its images keyed by `(RenderKey, Job)`, the shown page's engine
  pinned while the dialog is open.
- **OCR** reads pages through `ViewPages`: each page of the run's snapshot
  realized under `content_only`, in the background's budget at the lowest
  priority, turned as it shows; from slice 14 its text length is the
  text service's (`Text`, the background's budget) under `content_only`,
  each page read from its own view. From slice 18 a later round of the
  run reads the pages that changed during the one before from the
  current version's snapshot (Text layers (OCR)).
  sheaf-ocr's `PageSource` takes the `OcrPage` (`text_len(&OcrPage)`,
  `render(&OcrPage, width)`), one source a run. OCR no longer reads
  widgets or comments as a page's content.
- **Plugins.** A call's text is the text service's (from slice 14;
  `DocContext.text`, until then `TextSource` by source page), and
  `OcrSource::recognize` and `RenderSource::render` take the call's
  snapshot and the page: a recognition through `ViewPages`, a render
  realized under `ByEngine` inputs in the background's budget, in the
  file's own orientation (docs/plugins.md).
- **Text, search and opening.** Find, Patterns and text selection read
  each page from its view through the text service (from slice 14, Text
  and printing; slice 13 had Find read the primary's base engine, and
  selection each page's source's). Opening, recovering and
  inserting open no engine: `DocView::ask_form_type` asks the primary's
  base engine for its form type at install, which is what first opens it
  (the XFA-only bar waits for it), and a PDFium failure there becomes the
  document's notice ("The pages can't be drawn: …") rather than a failed
  open, or, when PDFium refused the file, that its pages are drawn from
  sheaf's copy; a password is the model's reader's to ask for.
  `--timing` prints the engine's open on a line of its own.
- **The Header & Footer dialog's preview** lowers its op on the current
  version off the UI thread (a newer lowering supersedes an older one's
  picture, by a generation token; an op equal to the last isn't lowered
  again), and renders the active pane's page under the screen's inputs
  through a realizer of its own on the document's pool, so its states
  don't evict the screen's deltas; it pins nothing, and doesn't draw
  sheaf's marks, which GPUI draws on the page. From slice 15 the Crop
  Pages dialog previews the same way, through the same
  `dialog_preview::Preview` (Crop as geometry).

**Deleted:** `RenderHandle` (its thread per source, `open_bytes`, `info`,
its searches), the old worker's `Msg`, `worker_loop`, `SearchWalk` and
`DocInfo`; `Renderer::hide_annots` and `set_annots_hidden` and every
replay of them (at ready, `Doc::take_in` after an insert, the layers
panel's re-hide, the Comments panel's `sync_foreign_visibility`);
docview's `drawn_marks`, `hide_renamed`, `marks_by_page` and
`foreign_hidden`; the layers panel's decrypted in-memory re-save and new
engine per toggle, and `layers::set_hidden`; the GPUI header and footer
overlay (`header_footer_elements`); `drop_stale_images`,
`invalidate_tiles`, `tiles_invalidated`, `Doc.shown` and the relayout on a
rebase; `Doc.handles`, `Doc::primary`, `PlannedInsert.handle`,
`with_engines`, `PrintContext.handles`; sheaf-ocr's `PageSource` for the
handle and by source index. `tests/retired.rs` lists them.

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness):

| Gate | Synthetic | Heavy | Scanned |
|---|---|---|---|
| realize an edited page (emit + assemble), a fresh realizer | 2.00 ms | 2.38 ms | 2.01 ms |
| realize + open + render 1,024 px, an edited page (asserted < 15 ms on the synthetic, < 25 ms on the heavy) | 12.1–12.3 ms (2.3× the warm render) | 21.7–23.4 ms (4.4–4.7×) | 33.5–34.0 ms (1.5×) |
| engines after 220 groups: weighed, RSS added (asserted ≤ 96 MB) | 28.8 MB, 37.5 MB | 28.8 MB, 62.1 MB | 28.8 MB, 51.1 MB |
| thumbnail strip after a crop and the header on all 7,000 (asserted ≥ 310 pages/s, ≤ 96 MB weighed and RSS added) | 406–410 pages/s, 80.8 MB, 32.5 MB | 344–346 pages/s, 81.4 MB, 80 MB | 164 pages/s, 80.8 MB, 81.5 MB |

The plan had the heavy's realize + open + render at most 1.7× its warm
base render, which no design that opens an engine meets (its scoped open
and cold render alone are 2.9×, slice 2); Greg accepted < 25 ms in its
place (2026-10-20), asserted beside the synthetic's, the ratio printed
as information. Making it faster waits for a later perf pass, once the
algorithm is settled.

`realize_corpus` (ignored; `cargo test -p sheaf-ops --test
realize_corpus -- --ignored`) edits every corpus form and the pool's
sample (the 402-file pool sample, 50 encrypted, 50 with leading junk and
50 whose `startxref` leads nowhere): a header on three pages, a deleted
comment and a field value where there are some. Its predicate tier checks
the predicate against the emission on every page under each consumer's
inputs; its pixel tier compares each edited page and an untouched one,
appearances only at 1,024 px, with the saved Rewrite. A page passes when
at most 16 pixels differ (`ANTI_ALIASING_PX`, Greg, 2026-10-20): slice 13
found 1 to 9 pixels of PDFium's anti-aliasing at a picture's or a shape's
edge on 6 or 7 of 13,289 pages, on pages whose dictionary and content
streams are the same in both files, and which pages differ changes with
what the process drew before (PDFium's process-wide caches). Every page
that differs at all is printed with its count, and the class table counts
those within the allowance apart. A base PDFium refuses is told so when
an engine over its bytes fails to open, as the pool's openers tell it,
and its pages are viewed again, each from its group; a section refused
over a base PDFium reads fails instead. `SHEAF_REALIZE_ONLY`, corpus
paths comma-separated, narrows a run to those files by hand (set but
naming none, it fails).

## Text and printing (M10.5 slice 14)

A page's text is read from what it draws, as its pixels are: the text
service reads each page's characters from the engine and page its view
renders from, under the inputs its caller passes, so the screen, the
file and the text agree (§10.5 of `plan/complete/m10.5-edit-api/README.md`). An
inserted file's page, a duplicate, a crop, a page with recognized text
or a header not yet saved, and every page of a source PDFium refuses have
the text they draw. Printing, print preview and page images come from
bytes in memory, and nothing reaches disk (§10.6).

```rust
pub trait TextService: Send + Sync {                  // sheaf_doc, blocking
    fn chars(&self, snap: &Snapshot, page: PageId, o: &TextOpts) -> Result<Arc<PageChars>>;
}
pub struct TextOpts { pub inputs: ViewInputs }
pub enum Query { Literal { query: String, match_case: bool }, Pattern(Arc<Regex>) }
impl PageChars { pub fn find(&self, q: &Query) -> Vec<TextHit>; pub fn text_len(&self) -> usize; }
pub struct Text { views: Views, budget: Budget, .. } // sheaf_ops::text, impl TextService
impl Text {
    pub fn chars(&self, st, page, inputs) -> oneshot::Receiver<Result<Arc<PageChars>>>;
    pub fn cached(&self, st, page, inputs) -> Option<Arc<PageChars>>; // a paint's
    pub fn search(&self, st, q: Query, inputs) -> mpsc::UnboundedReceiver<Found>;
}
impl<K> EnginePool<K> {                               // sheaf_render::pool
    pub fn query<T>(&self, target, page, fallback: Option<Fallback<K>>,
                    f: impl FnOnce(&PdfiumEngine, PageIndex) -> Result<T>) -> oneshot::Receiver<Result<T>>;
    pub fn walk<T: From<Walked>>(&self, count, item, work, tx: UnboundedSender<T>);
}
```

### Characters in PageSpace

`Renderer::page_chars` gives each character PDFium extracts with its
loose box as PDFium has it, in the page's user space (PageSpace, §5), and
the angle it's drawn at, counter-clockwise (PDFium's
`FPDFText_GetCharAngle` is clockwise, and is turned around); a character
PDFium generates (a space between words, a line break) has no box. The
device 16 times the page's size that mapped text into the displayed
frame is gone. `PageChars::from_engine` reads lines in the frame most
of the boxed characters are drawn upright in (`upright`, the nearest
quarter turn, the lesser on a tie, R0 with none), not by the page's
`/Rotate`, so text that runs down the page selects a line at a time, and
a page's characters don't depend on its `/Rotate` or crop box: the same
for a page and its turned or cropped copy (its render key still has its
boxes). A page whose text reads upright as it shows reads as before.
`pdfium::frames` checks every turned or offset corpus page's character
boxes, mapped through the page's `to_view`, against PDFium's own
page-to-device mapping within 1/16 pt (on a plain page the frame is the
y flip `to_view_is_pdfium_s_page_to_device_on_every_page` checks), and
sheaf-ops' text tests check each turned or offset corpus page
duplicated, read from its group's realization, against its source page
(780,238 characters, none apart, asserted as a floor over the full
corpus).

`PageChars::find` matches the text as `text()` reads it: words apart by
a space, lines by a line break. A literal's words match where the
page's are apart by any whitespace, a line break, two spaces or a
no-break space, as `FPDFText_Find`'s did, so a phrase wrapped onto the
next line is found, a rect on each line. It's one regex, compiled once
per search (`Query::literal`): the query's words escaped and joined by
`\s+`, case-insensitive unless it matches case, and its matches don't
overlap. A
pattern runs over the characters as they read and keeps its capture
groups; a match of separators alone isn't one. Each hit has a rect a
line, in PageSpace. This replaces PDFium's `FPDFText_Find` and the OCR
layers' search beside it, so every consumer matches the same way.

### The text service

`Text` (sheaf-ops) is one document's text in one budget: the app's
`Doc.text` in the screen's, plugins' and blank detection's in the
background's. A page's characters are read on the pool's worker by a
query (`EnginePool::query`): its view (`Views::view`), then where that
view reads it (`Views::locate`: the engine, the page's index there, and
the fallback a render of it would have), then `page_chars` and
`PageChars::from_engine` on the engine. What's read is kept by the page's
render key, the last 64 pages (`CHARS_KEPT`, the oldest read let go
first): a page that draws the same under the same inputs, in this state
or another, or as a duplicate, is read once, and anything it draws
changing changes its key, so an edit reads it again. §10.5's cache of
untouched pages by source and index is this one, since an untouched
page's key is its base page's. `Text::cached` is the paint's, which can't
wait; `TextService::chars` blocks, for a thread that may wait on the pool
(a plugin call's, blank detection's), never the worker's.

A query is the pool's generic request: a closure run on the page of the
engine, given the page it reads. It falls back as a render does: when
PDFium refuses the engine's bytes, the fallback runs once on the worker
and the closure reads the page of the engine it names (the page's group,
a file of its own), so a source PDFium refuses has text, through its
groups, even for a page viewed on its base before the refusal was known.
A password error doesn't fall back, and a fallback that panics fails its
query alone. The form type's query has no page and no fallback: it's how
a document learns PDFium refused its file.

What each consumer reads under (`TextOpts.inputs`, §10.4):

| Consumer | Inputs | Budget |
|---|---|---|
| Find and Patterns, selection, calculator picks, text markup (the app's `Doc.text`) | the screen's (`Doc.inputs`) | the screen's |
| A plugin's `page-text`, `search`, `pattern-search`, `text-boxes` and `words` | `plugin_text()`, the page's text as the saved file will draw it (§14.3) | the background's, a `Text` a call |
| OCR's "skip pages with text" (`ViewPages::text_len`) | `content_only()` with `ocr_over` (slice 18) | the background's |
| Blank detection's text | `content_only()` with `ocr_over` | the background's |

PDFium extracts a page's text from its content, never from a widget's
appearance, so no consumer reads a form field's value as text; §10.5's
`fields` (fields' display text, M10 slice 14) has no use yet, and
`TextOpts` names only the inputs.

### Find and Patterns

A search walks the document's own order on the pool's worker
(`Text::search` over `EnginePool::walk`), four pages a turn between
renders: at each position the walk views the page and locates it, then
reads its characters on that page's engine (falling back as a query
does), runs `find`, and sends the page's hits with its position and id
(`Found::Hits`), then how far it has got (`Found::Scanned`), and `Done`
last, on one channel so a page's hits come before its `Scanned`. Each page
is read where its view is: an untouched page on its source's base engine,
an edited, inserted, duplicated or (from slice 15) cropped one on its
group's realization (a crop's was its derived copy's until then). A walk
keeps nothing of what it reads (a 7k walk never evicts the screen's
selection characters), holds its state until it's done or its receiver
is dropped (the app drops it on a new search and at close), and passes
over, logged, a page whose engine doesn't open or whose read fails or
panics. It holds the engine it reads on, so the screen's count never
closes a group's engine between two of its pages. The worker blocks on its channel when no walk is pending. The app
reads under the screen's inputs, so a header not yet saved and recognized
text are found, and hits are in PageSpace, by `PageId` and position
(`Hit::on`).

### The app's text

Text selection, calculator picks and markup dragged over text go through
`DocView::with_chars`, which answers from `Doc.text`'s cache or asks for
the page and calls back when it's read; an answer whose page's render
key changed while it was read (an edit in between) is read again as the
page is now. A selection keeps the characters it was made in
(`TextSel.chars`), and paints, copies and marks up from them, so an edit
that changes the page's render key leaves it working: a comment on it
hidden, or a page deleted under a "Page <<1>> of <<N>>" header (a turn
changes the key only where a generator is laid out in the turned page;
characters are in PageSpace). On a page with recognized text and a
header, the text reads in draw order: the page's own, then the
recognized words, then the header (§11.4). In slice 17, whose header was
a generator block while the OCR layer was still the old installer's,
the header read before the recognized words; slice 18 made the layer a
block of its own, drawn before the generators over the page.

When PDFium refuses the primary, the notice says "PDFium can't read this
file as saved, so its pages are drawn from sheaf's copy": Find, Patterns,
selection and plugin text read its pages from their groups.

### Plugins, blank detection and OCR

- **Plugins.** A call's `DocContext.text` is a `TextService`, a `Text` of
  its own in the background's budget. Its text calls read under
  `plugin_text()`, what of `plugin()` PDFium reads as text, and record
  that page's `render_reads` under it (the page, every generator, and the
  order where a page-numbering header calls for it), so an unsaved
  header's edit conflicts with a call that read the page's text and a
  comment's doesn't; `recognize` still reads `content_only()`. Matches
  went out through the page's 0.3 frame (`to_frame`) until API 0.4 made
  them PageSpace (slice 20). The OCR layer's own special cases
  (searching an unsaved layer beside PDFium, `ocr_text` branches) are
  gone: the realization draws the layer. From slice 18 API 0.3 had no
  `ocr.page-text-layer` either: recognized text, saved or not, is the
  page's content, which `text` reads as the file will draw it now; 0.2's
  answered none until slice 20 deleted 0.2.
- **Blank detection** (`blank::find`, Find Blank Pages and `sheaf
  remove-blanks`) walks the wanted pages in the document's order through
  the document's views: annotations first as before, then the page's text
  under `content_only` with `ocr_over` (recognized text counts, saved or
  not, as it did; a header never does), then its pixels under
  `content_only`, rendered at about 60 dpi by the pool in the
  background's budget, with `progress.check` between pages. A failed text
  read counts as no text, so the pixels decide. A page at a time, each a
  query: over the synthetic 7k 12.3 s and 6 base engine opens, beside
  10.4 s for its old engine's `page_text` (the heavy's pages all have
  annotations, so none is read). It no longer opens an
  engine of its own (nor, from slice 15, does Trim White: `engine_for` is
  gone); the command line, which has no process pool, makes views of its own
  (`Views::start`).
- **OCR's "skip pages with text"** reads each page's text length through
  `ViewPages`' `Text` under `content_only`, so a page of a refused source
  is checked through its group rather than failing. From slice 18 it reads
  with `ocr_over`, as blank detection does: the blocks of recognized text
  a file holds are the page's content, so text recognized in this session
  counts too, and a page recognized before is skipped whether its text is
  saved or not.

### Printing and page images

The print file (`save::print_pdf`; from slice 19 the Print preset,
`export::printed`) is made in memory and handed to the spooler from
there (`sheaf_print::submit(job, Arc<[u8]>)`): CUPS runs `lp <options>
-`, reading the job on its stdin from a thread of its own while `lp`'s
output is read (a failed `lp` says why first; a write that fails after
`lp` succeeded fails the job), and GDI opens the bytes with
`PdfiumEngine::open_bytes`. That was `PdfiumEngine::open(path)`'s last
use, and it's gone: PDFium opens only bytes. Print preview draws through
the views (slice 13), and image export (PNG, TIFF) renders the print
file's bytes on an engine of its own, already in memory before this
slice; the only file it writes is its output's, the TIFF through a
`.part` beside it. A child-process test runs print, print preview and
both exports with `TMPDIR` an empty, unwritable directory and a fake
`lp` reading its stdin. A source PDFium refuses prints: its print file
is sheaf's own.

**Deleted:** the app's own character cache (`CharsKey`, `chars_key`,
`cache_chars`, `CACHED_PAGES`, `Doc.page_chars`) and
`PageChars::from_layer`; Find on the primary's base engine
(`Doc::search`, `index_of_source_page`, `layer_hits`,
`merge_source_hits`, `Matcher`, `SearchEvent`, the pool's one-engine
search); the OCR layers' own search (`text_layer::search`,
`pattern_search` and their helpers, `TextLayer::text` and
`boxes_in_rect`); the 16× device and the queries through it
(`Renderer::search_page`, `pattern_search_page`, `text_boxes_in_rect`,
`SearchHit`, `sheaf_render::Rect`) and the pool's text queries
(`page_chars`, `page_text`, `page_text_len` and the rest, now `query`);
plugins' `TextSource`, `TextMatch` and `EngineText`; blank detection's
own engine; the print temp file and `PdfiumEngine::open(path)`.
`tests/retired.rs` lists them, but for `sheaf_render::Rect`, whose name
`geom::Rect` still has.

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness, one run each,
no baseline or gate):

| Find "Draft" over every page, the screen's budget and inputs | Synthetic | Heavy |
|---|---|---|
| as opened (no hit; every page on the base engine) | 11.4 s, 612 pages/s, 6 engines opened | 8.0 s, 874 pages/s, 6 engines opened |
| a header on every page (7,000 hits, each page from its group) | 12.2 s, 574 pages/s, 55 engines opened | 9.1 s, 773 pages/s, 55 engines opened |

As opened, the base engine opens six times: once, then again each time
the pages it has loaded take the screen's engines past 96 MB (about every
1,200 pages, as the pool's bound says); headed, each of the 55 groups'
engines opens once and the base engine never. Reading a page's
characters with their boxes is about 1.2× PDFium's own `FPDFText_Find`
on the same page (one engine, no pool: 11.1 s against 9.0–9.3 s for the
synthetic's 7,000 pages, 7.5 s against 6.2 s for the heavy's), since it
asks PDFium four things a character.

## Crop as geometry (M10.5 slice 15)

A crop is a box edit: it changes the page's `Geom` and nothing else, so
nothing stored moves (Coordinates), no copy of the page is made, and an
incremental save writes only the cropped pages' dictionaries (§5,
proposal 27 and bugs 16 and 28 of `plan/complete/m10.5-edit-api/README.md`). Crop Pages
by margins is one op; Trim White measures each page through the views
and commits its crops as ops; the dialog previews either through the
realizer.

```rust
pub enum BoxSpec { Margins(Margins), Rect(Box2) }          // sheaf_edit, kebab-case
pub struct Margins { pub top: f32, pub bottom: f32, pub left: f32, pub right: f32 }
Op::SetPageBoxes { pages: PageSel, crop: Patch<BoxSpec>, trim: Patch<BoxSpec>,
                   bleed: Patch<BoxSpec>, art: Patch<BoxSpec> }
pub const MIN_SIDE: f32 = 18.0;                             // sheaf_doc::geom
pub const PAD: f32 = 9.0;                                    // sheaf_ops::crop, the app's pad
pub enum Urgency { Background, Preview }                    // sheaf_ops::crop
pub fn trim_white(views: &Views, snap: &Snapshot, lower: Arc<LowerCx>, pages: &[PageId],
                  pad: f32, urgency: Urgency, progress: &dyn Progress)
    -> Result<Trimmed>;                                     // sheaf_ops::crop
pub struct Trimmed { pub crops: Vec<(PageId, Box2)>, pub measured: Vec<PageId>,
                     pub left: Vec<PageId> }
impl Trimmed { pub fn lower(&self, tx: &mut Tx) -> Result<(), EditError>; }
```

### SetPageBoxes

- **The boxes.** Each of `crop`, `trim`, `bleed` and `art` is a `Patch`:
  `Keep` leaves the box as it is, `Clear` makes the crop the media box
  and takes any of the others away, and `Set` gives it. Margins
  (`BoxSpec::Margins`, the `Margins` the header and footer use too) are
  points off each side of the box the page shows, after its `/Rotate`
  and sheaf's turn: the kept box goes through `from_view` into PageSpace
  and is intersected with the visible box, for rounding only. Every
  box's margins come off the page as it showed before the op, so the
  four fields don't depend on one another. A `BoxSpec::Rect` is a
  PageSpace box within the media box, as the op's JSON gives it.
- **It fails, and is never repaired** (§6.4). A negative or non-finite
  margin, a box that isn't finite, a box off the media box by more than
  0.01 pt (only float noise is clipped), or one under `geom::MIN_SIDE`
  (18 pt, moved from sheaf-ops' crop.rs) on either side as its caller
  states it fails the op as `EditError::Invalid`, naming the page's
  position: "those margins leave nothing of page 3", "the box leaves
  nothing of page 3", "the box isn't on page 3". A file's own crop
  outside its media box is still read as PDFium reads it (§3.3); an op
  doesn't get that leniency.
- **`Geom(p)` alone.** Lowering reads each page's `Geom` (a lowering
  read, so a validated commit conflicts when a page's boxes changed
  since, even one the op leaves as it was) and writes it only where it
  changes; no change, no version. A crop writes no `Page(p)`, so an OCR
  flush, which reads what its page shows (`read_render` under
  `content_only`, which is `Page(p)`), lands after a crop of its page
  made during the run, and the other way round (the transitional
  ledger's 12 → 15 row). Two crops of one page made on one version
  conflict on `Geom(p)`.
- **Its line** is "Crop page 3", "Crop pages 3–5", "Remove the crop from
  page 2", or "Change the page boxes of …" for anything else, and a run
  of crops is one ("Crop 7000 pages"). It needs `Cap::Approval`;
  `Actor::Job(JobKind::Trim)` may submit it and nothing else.
- **Saving.** The emitter writes a page's boxes where they differ from
  what it inherits (§11), so an Append of a crop writes the cropped
  pages' dictionaries and nothing else: the same `/Contents`, `/Annots`
  and `/Parent` references, no content stream, no copy, and forms stay
  clean. Bug 13's forced Rewrite for a crop went in slice 10, and bug
  28's linear id lookup with `crop::plan`.
- **Drawing.** Spike C failed (a box edit at render time on the base
  engine), so a page whose `Geom` differs from its base page's isn't
  untouched: it realizes in its source's group overlay, which writes its
  new boxes over the source's own bytes. The page view, the thumbnails,
  print preview and the text service all read it there, so a cropped
  page's characters are its source page's, read from the group, and a
  cropped form page keeps its widgets and shows its values with no copy
  and no cache file.

### Trim White

Trim White is a resolve stage (§6.3) in sheaf-ops: `trim_white` measures
pages of a snapshot through the views, and its `Trimmed` lowers the
crops into a transaction made at that snapshot.

- **The measuring state.** Each page is measured as its file crops it,
  whatever crop sheaf gave it: a transaction at the snapshot
  (`Document::tx_at`) crops each page sheaf cropped back to its base
  page's own crop box (`Source::geom(index).crop`, a fact of the source,
  not a read). A page whose only change is its crop is untouched there
  and is drawn from its source's own bytes, so trimming every page of a
  cropped 7,000-page file opens no group engine. A page whose own box is
  under `MIN_SIDE` isn't measured.
- **Within the file's own crop box, not the whole media box,** so what a
  file's `/CropBox` hides (printer's marks, bleed) is never taken for
  content. Trim White can widen a margins crop back out to the page's
  content, but never past the file's box. §10.1 said "over the whole
  MediaBox"; the file's box is a decision for Greg, and the whole media
  box is one line (`geom.media` for the own box in `measuring`).
- **The render.** Each page of that state is drawn under
  `ViewInputs::content_only()` (its content alone: no header, recognized
  text, widgets or annotations) at 72 dpi, turned as it shows, through
  the views, one page at a time with `progress.check()` between: a
  planner's at priority `u8::MAX` in the background's budget
  (`Urgency::Background`), a dialog's preview at priority 0 in the
  screen's (`Urgency::Preview`, §10.3). A page of a source PDFium refuses is drawn
  from its group, so Trim White works on such a source
  (forms/safedocs/4422502.pdf: 5 of 5 pages trimmed). Anything darker
  than gray 235 is content; its bounds, padded (`crop::PAD`, 9 pt in the
  app, an eighth of an inch), mapped back through the measuring state's
  `from_view` and intersected with the own box, are the page's crop. A
  blank page, or one with too little content to keep `MIN_SIDE`, is left
  as it was (`Trimmed::left`), and one whose crop wouldn't change gets
  no op.
- **The commit.** `Trimmed::lower` records `read_render(p,
  content_only)` for every page it measured (`Page(p)`), and `Geom(p)`
  for each it found a crop for, since whether the page already had that
  crop decided whether it takes an op (a page it leaves because it's
  already trimmed conflicts with a change of its crop since, as one it
  crops does); then one `SetPageBoxes` with a `Rect` crop a page through
  `apply_batch`, all of them or none. The app runs Trim White as a planner
  (`DocView::trim_white`, `run_planner("Trim White", …)`) on views of
  its own over the process's pool, as `Actor::Job(JobKind::Trim)`, and
  lands it in a transaction at the snapshot it measured, `Stale::Validate`
  on that version, labelled "Crop N pages" for the pages it crops. A
  Conflict (another crop of one of its pages since, `Geom(p)`, or a
  change to what one shows, `Page(p)`, such as an OCR flush) measures
  again on the document as it is, with the pages still in it, at most
  three tries (`TRIM_ATTEMPTS`); the new planner's notice takes the
  conflict's place. Of two overlapping trims, the second re-plans and
  both crops hold. The notice reads "Trimmed 7 pages, 1 left as it was".
- **A float limit.** A `Rect` carries a corner and a size, so an own box
  whose far corner isn't its near one plus its size in `f32` comes back
  about an ulp off: such a page is drawn from its group, with the same
  pixels, at the cost of a group engine. Integer boxes and media boxes
  from the origin round-trip exactly.

### The app

- **Margins** commit one op (`page_insert::crop_op`) on the version the
  Crop Pages dialog opened on, with `Validate`, on the UI thread: no copy
  is loaded, so the first crop no longer starts rayon's pool.
- **A crop survives the generators.** A page's boxes are its own, not
  its base's. Until slice 17 the derived flows' `Tx::set_page_base`
  (Remove Header/Footer; Watermark / Background until 16) moved a page
  onto its copy keeping its `Geom` whole. From slice 16 a watermark, and
  from 17 the header and footer, is a generator, laid out at emit on each
  page's crop as the document shows it then, so one set before a later
  crop lands and is centred, or placed, on the new crop.
- **The dialog's preview** (§8.6) is the Header & Footer dialog's,
  shared (`dialog_preview::Preview<K>`, keyed by the request,
  `crop_dialog::Request::{Margins(m), Trim}`): the request lowered off
  the UI thread in a transaction at the current version, margins as
  `crop_op` on the previewed page and Trim as `trim_white` and
  `Trimmed::lower` through the previewer's views, then the page drawn
  from that state under `screen()` by the previewer's realizer, both
  renders at priority 0 in the screen's budget, and the
  hint saying the size the page takes ("The page below becomes 6.50 ×
  8.00 in") or why it can't be cropped so. It follows each change of the
  document (`Workspace::watch`), and a lowering's picture shows only
  while no later one has begun. The textures are released when the
  dialog closes.

**Deleted:** `save::cropped_copy`, crop's derived file and the pool
engine each one took; sheaf-ops' `crop::{plan, CropSpec, Margins,
CroppedSource, content_margins}` and its `MIN_SIDE` (moved to
`geom::MIN_SIDE`); `sheaf_ops::engine_for`, Trim White's own engine;
`Derive::Crop` and crop's use of `Tx::set_page_base`; `derived_copy`'s
crop parameter; the dialog's `CropEvent::Crop`. `tests/retired.rs`
lists them.

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness):

| Row | Synthetic | Heavy | Scanned |
|---|---|---|---|
| crop all pages, one op made on the opened version and committed by value (asserted < 200 ms) | 9.81 ms (slice 2: 445 ms, copy and all) | 9.90 ms (2,326 ms) | 9.61 ms (2,256 ms) |
| crop all pages but the last, validated after it was deleted (asserted < 200 ms) | 15.7 ms | 15.3 ms | 14.6 ms |
| thumbnail strip after a crop and the header (asserted ≥ 310 pages/s, ≤ 96 MB weighed and RSS added) | 446 pages/s, 80.6 MB, 30.8 MB | 372 pages/s, 80.7 MB, 71.0 MB | 167 pages/s, 80.7 MB, 81.4 MB |
| Trim White over every page, cropped first (no gate; `trim_white_on_the_7k`): measured, then committed | 38.5 s (182 pages/s), 71.3 ms | 40.6 s (172 pages/s), 79.5 ms | 104.8 s (67 pages/s), 74.0 ms |
| Trim White: pages cropped, engines opened, group engines opened | 7,000, 6, 0 | 7,000, 6, 0 | 7,000, 6, 0 |

A crop of every page is the transaction made on the version the dialog
opened on, the op lowered over 7,000 pages and the commit; slice 12
measured the dialog's edit at 14–15 ms on a precomputed copy, its copy
apart. Trim White measures every page of a cropped document from its
source's base engine, which opens six times as the background's base
engine grows past what the screen's engines leave (as Find Blank Pages'
does), a page at a time on the pool's worker; the walk is timed after
an untimed one that counts its group engines and reads each page's
facts cold, as the app has by the time it trims. Making it faster is a
later perf pass's.

## Watermarks and backgrounds as generators (M10.5 slice 16)

A watermark (over the page's content) or a background (under it) is a
generator: one record in the state (`GenSpec::Watermark`), laid out on
each page it covers when a save writes the page or a view draws it, on
the page as it shows then. Setting one is an op and so is taking one
off; a file sheaf saved with one finds it again when it opens; and what
other programs drew of its kind goes from the pages it draws on (§3.5,
§6.5, §11.4, proposals 30, 31 and 39 and bugs 16–18 and 28 of
`plan/complete/m10.5-edit-api/README.md`). The derived copy that drew a watermark into
a copy of each page is gone, and with it `watermark::install` and
`read`, the per-page ExtGState, font and image, and the names and
settings it wrote into the page, which nothing writes or reads now.

```rust
pub enum Z { Under, Over }                                    // sheaf_edit, kebab-case
pub enum GridPos { TopLeft, Top, TopRight, Left, Center, Right, BottomLeft, Bottom, BottomRight }
pub enum FontChoice { Helvetica, HelveticaBold, /* …the 12 Std14 text faces */ }
pub struct WatermarkSpec { pub content: WatermarkContent, pub position: GridPos }
pub enum WatermarkContent {
    Text { text: String, font: FontChoice, size: f32, color: Color, opacity: f32, angle: f32 },
    Image { image: Option<BlobSel>, scale: f32, opacity: f32 },
    Page { pdf: Option<BlobSel>, page: u32 /* 1-based */, scale: f32, opacity: f32 },
}
Op::SetWatermark { spec: WatermarkSpec, z: Z, scope: ScopeSel }
Op::RemoveWatermark { z: Option<Z>, pages: PageSel, include_authored: bool }

pub struct WatermarkGen { pub content: WatermarkContent, pub position: GridPos,  // sheaf_doc::model
                          pub z: Z, pub scope: Scope, pub replaces: BTreeSet<PaginationKind> }
pub enum Scope { All, Pages(im::OrdSet<PageId>) }
pub enum ImageRef { Blob(BlobRef), Placed(Placed) }
pub enum PageForm { Blob { pdf: BlobRef, index: u32 }, Placed(Placed) }
pub struct Placed { pub at: ObjRef, pub res: ResId, pub turns: u8 }
pub enum Filter { DropMarked(MarkedSel), DropStreams(StreamSel), StripPrivate(PrivateSel) }
pub enum MarkedSel { SheafGen(GenId),
                     Pagination { kinds: BTreeSet<PaginationKind>, origins: BTreeSet<PagOrigin> } }
pub enum StreamSel { SheafGen(GenId) }
pub enum PrivateSel { Marked(MarkedSel) }
impl Generator { pub fn covers(&self, page: PageId) -> bool; pub fn z(&self) -> Z;
                 pub fn scope(&self) -> Cow<'_, Scope>; pub fn replaces(&self) -> &BTreeSet<PaginationKind>; }
```

In slice 16 `Generator` was still `{ id, spec }`: a watermark carried
its own z, scope and `replaces` in its record, and `covers`, `z`,
`scope` and `replaces` read them as §3.5's fields would be (the
header's made from `HeaderFooter.pages`, `Over` and none). Slice 17
moved them onto `Generator` as fields, with `frozen`, for both kinds;
`WatermarkGen` keeps only its content and position. `GenId` is one type,
`content::blocks::GenId`
(`g<hex>`), which `model` re-exports.

### The ops

- **`SetWatermark`** puts the watermark at its z in place of the one
  there, keeping that one's `GenId`, so the blocks a file holds of it
  stay its own and one laid out alike is used again; else it draws a new
  id. It checks its values (text not empty once trimmed, size finite and
  above 0, opacity 0–1, scale above 0 and at most 2, the angle finite,
  the scope not empty), each failure `EditError::Invalid` naming the
  field ("set-watermark: the size is 0, not above 0"). An image is an
  image blob (`BlobKind::Image`, its type sniffed by
  `sheaf_image::io::sniff`) and a page is a page of a PDF blob that
  parses (page 0 is Invalid, one past the end NotFound); a stamp grant is
  Invalid. With no `image` or `pdf`, it keeps what the watermark at its
  z draws now, so a reopened image moves or scales without being
  embedded again (Invalid when nothing's drawn there). `replaces` is
  `{Watermark}` over the content and `{Background}` under it. Text the
  face can't draw all of is kept exactly and told
  (`Notice::Unencodable`). It reads no content and no `Geom`: the layout
  is the emitter's, so a watermark set before a later crop lands and is
  centred on the crop. Lines: "Add a watermark to all pages", "Add a
  background to pages 2, 3", "Change the watermark on …".
- **`RemoveWatermark`** narrows each generator of its z (both, with
  none) to the pages it's left on, and removes one left on none; an
  `All` scope becomes `Pages(order − pages)`, which reads `Order`. Then
  it reads each page's blocks (`Source::blocks`, a read of `Page(p)`)
  for what others drew of its kind and adds filters: `DropStreams` for
  another file's blocks in streams of their own, `DropMarked(SheafGen)`
  for blocks another tool merged into a stream, and a `Pagination`
  filter for Acrobat's ranges, the author's own too with
  `include_authored`, each origin only where a splice takes at
  least one of its ranges. What it found, by who drew it, and what it
  left in place is counted at lowering, in its resolved op's `others`
  (`tx::OthersFound`), and said in its line: "Remove the
  watermark from pages 1, 2 (2 made in Acrobat; 12 the author's left in
  place)". Ranges a splice refuses, forms that draw one, and pages that
  can't be read are counted, never a failure (D61). A step's line is
  described after it's lowered, so the counts reach it.
- **Invariants and cascades.** One watermark a z; a scope's pages
  exist, and a `Pages` scope is never empty (one on no page is
  removed). A deleted page leaves the scope (`on_pages_left`), a
  duplicate joins a `Pages` scope, and a commit made before a page
  went that names it conflicts. Both ops need `Cap::Approval`.
- **Effects** (§7.1). Added, or changed only in its layout (position,
  size, angle, colour, opacity, font, scale, z, or a wider scope):
  `CONTENT`, so it can append. Other text, another image or page, a page
  left that's still in the document, or the watermark removed: `REMOVES`
  (`RemovedKind::Watermark` or `Background`), and `REPLACES_TEXT` when
  it was text. A filter added is `CONTENT | REMOVES`
  (`RemovedKind::Filtered`, "page's watermark or background from another
  program or file"), and so is a watermark newly drawn on a saved page
  that holds others' of its kind, which a save's `judge` finds from the
  page's blocks (a step's own judging doesn't read them).

### Drawn on save

- **The layout** (`generate::layout`): upright on the page's visible box
  as it shows, turned as the page is turned, in a cell of a 3×3 grid, 36
  pt in from a side it keeps to, its rotated bounding box inside; an
  image or page `scale` of the shown width wide, a JPEG's quarter turns
  honoured. Text is in the `FontChoice`'s standard 14 face, a character
  it can't draw `?`. The content is `/Artifact /Sh… BDC q /Sh… gs cm …
  Q EMC`, its property list `<< /Type /Pagination /Subtype /Watermark
  (or /Background) /SHEF_Gen (g…) >>` named in `/Properties` like every
  resource it adds, its graphics state `/ca` and `/CA`. The layout's
  digest (`/L`) is of its operators and its resources' ids, never their
  names; the recipe's (`/R`) is the render key's digest of the
  watermark (`generate::recipe_digest`).
- **Blocks and isolation** (`emit::content`). A covered page's
  `/Contents` is `[under…, QOpen, base′…, QClose, over…]`. Each block is
  a stream of its own marked `/SHEF_Block << /K /Gen /G (g…) /L <…> /R
  <…> >>`, keyed by its bytes (`ObjKey::Block`), so a watermark laid out
  alike on 7,000 pages is one object; a page whose image or page can't
  be had gets an empty block, so its scope can still be read back. The
  page's own content is isolated by shared marked streams
  (`Shared::QOpen(n)`, `Shared::QClose(n)`, `/SHEF_Block << /K /Isolate
  >>`) counted by its balance (§3.4): `open = 1 + d`, `close = 1 + d +
  net`, `d` the deepest its `Q`s go below where it starts, so no `Q` of
  its undoes theirs and nothing it leaves set reaches the watermark. Its
  streams are referenced, never re-encoded. A page whose content can't be
  read gets no watermark, one Note naming every such page ("No watermark
  or background is drawn on page 3, whose content can't be read."), and
  the save goes on.
- **`PageRes`.** The page's resources are a new dictionary
  (`ObjKey::PageRes { base, dropped, added }`, by the base Resources'
  identity and the digests of the names dropped and added): its base
  page's, the categories it adds to copied one level down, the names it
  draws with (`/Sh…`) clear of every name the base binds. Pages that
  share base Resources share one; the font, graphics state and image are
  one object a file. `/Thumb` goes whenever `/Contents` changes. Images
  are decoded once a blob and pages made forms once a page (`Cx.arts`).
- **`/SHEF_Generators`** in the catalog (`ObjKey::Generators`, a root)
  is an indirect literal string of versioned JSON (`v: 1`), ASCII with
  every other character `\uXXXX`: each watermark's id, kind, z, scope
  (`all` or `pages`, the pages read back from the blocks), `replaces` and
  spec, an image or page as `{"placed": "<the ResId it was drawn as>",
  "turns": n}`, never a path or an object number. No watermark, no key.
- **The PDF version** (§11.7). Marked artifacts and transparency need
  1.4 (`PdfVersion::ARTIFACTS`): a 1.3 file's Rewrite is `%PDF-1.4`, and
  its Append writes the catalog's `/Version /1.4` (`needs_after`; the
  planner classes it `OTHER`). ISO 32000-1 marks `/Subtype` in an
  artifact's list as 1.7; sheaf follows the slice's 1.4, for Greg.
- **Append.** A change of `Generator(g)` writes `/SHEF_Generators`, the
  catalog when the first watermark comes or the last goes, and the pages
  covered before or after; a change of scope alone writes only the pages
  that came or went. Adding a watermark to a saved file appends its
  pages, its block, the shared resources and the JSON, and a turned
  page's Append writes the page and one block.
- **The removal check** (§11.9) judges the new keys on record inputs:
  `PageRes`, like `Res` and `Shared`, says nothing of its own; a block
  is judged by what the page draws (`Drawn::on`, which adds each
  covering watermark's text, or a hash of its image or page); and the
  old `/SHEF_Generators` by every watermark's text, image or page, saved
  against now. So a layout change appends with no veto, and a text
  change plans a Rewrite.
- **A PDF/A or PDF/UA claim** (`Source::conformance`, slice 3's fact)
  gets a Note when a save draws a generator: a Rewrite for each it
  draws, an Append for those its path changes (the header and footer
  counted too): "This file claims PDF/A-2b; the watermark sheaf draws on
  it isn't made to conform, so the saved file no longer does". A claim
  that can't be read is logged, with no Note.

### Drawn on screen

A view draws the generators when its inputs do (`ViewInputs.generators`:
the screen's, a plugin's and print preview's), through the same
emission as a save, so a page renders as the saved file will. Under
`content_only()` (OCR, blank detection, Trim White) none is drawn, and
the blocks of the document's own its file holds are set aside there too.
A covered page is untouched only where its file already holds the blocks
drawn, in place (`intact`, through `untouched`'s `generated`); the render
key holds what each covering watermark draws, not its scope, so a
reorder re-renders no watermarked page. Images and pages are kept by the
realizer, at most four (`content::Arts`), so a group's deltas decode an
image once; the realizer never decodes an image in the store to tell
whether a page is intact, and counts such a page as changed.

### Found again after reopen

- **Markers.** A source whose catalog has `/SHEF_Generators` (and,
  until slice 17, a derived copy) is `marked()`, and only then are each
  page's stream markers read (`Source::markers(i)`): each `/Contents`
  stream's dictionary, never decompressed, once a stream. The corpus has
  none, so its files take the paths they took before. From slice 18 the
  markers include the recognized text's blocks (`Marker::Ocr`); a file
  saved with recognized text alone isn't `marked()`, and a page of it
  that something is drawn on is told by its blocks (OCR as content).
- **Recognition** (`Source::generators()`, `generate::read`): each
  `/SHEF_Generators` entry alone, one that can't be read whole passed
  over with a log line (a version it doesn't know, a bad id, values the
  op would refuse, `replaces` other than its own z's kind, a `pages`
  scope with no block, an image or page it can't bind, a second of one
  id or z). A page holds a generator's block by its streams' markers, or,
  on a page with no block in a stream of its own whose resources name a
  list with `/SHEF_Gen` in `/Properties`, by the ranges its content marks
  (a block another tool merged into a stream, `qpdf --coalesce-contents`,
  say).
  `pages` is the pages holding one of its blocks, empty ones too; `all`
  stays `All` unless a page whose content can be read holds none (a page
  another tool added), when it's the pages that do, with a log line. An
  image or page is bound by decompressing a block in a stream of its
  own, finding its `Do`, and looking that name up in the page's
  `/XObject`, its subtype checked (`ImageRef::Placed`,
  `PageForm::Placed`). At ready they're in the state, their ids and the
  file's in `DocState.opened_generators`.
- **Backed blocks** (`DocState::backs(g, page)`: a generator of the
  state that covers the page; or, on a page of the opened file (and,
  until slice 17, of a derived copy), a generator of the state or one
  the opened file had) are set aside from the page's own content (base′)
  whenever it's written, so a watermark changed, removed by either op, a
  cascade, an extract, a Clean Copy or a print leaves no block, and
  nesting never grows (five cycles of save, reopen and watermark keep the
  deepest `q` at 2). Another file's blocks (inserted pages, an earlier
  save of the same file's among them, whose generator ids are the
  document's) are kept, with the streams that isolate them, until a
  filter takes them out or a covering generator of their id draws the
  page anew. A stream is marked by its dictionary, read again in the
  rebuild whatever the catalog says, and one marked `/Isolate` that
  holds more than `q`s or `Q`s is the page's own content, kept, which
  the removal check judges on its text. A block laid
  out alike (`/L`) is the file's stream, an isolating stream of as many
  as base′ needs is used again, and a page whose blocks are all alike, in
  place around its isolated content, is written as it is (`intact`, the
  one rule the emitter, `untouched` and the render key share). Names
  only what's set aside or cut used are dropped from the page's
  resources, and one a new block binds to the same object keeps the
  file's binding. A PDFium `FPDF_SaveAsCopy` re-save keeps the blocks
  found (an ignored test).

### Others' watermarks (proposal 31)

What a generator draws on a page replaces what others drew there of its
`replaces` kinds: Acrobat's and another file's blocks
(`PagOrigin::REPLACED`), never the author's own, which only Remove with
`include_authored` takes. The cut is made at emit (`replacing`), only on
a page whose blocks hold such a range, through `content::splice_each`
(each range spliced out alone where the whole set can't be) into
`ObjKey::Filtered` copies, so a stream other pages share stays whole for
them; Acrobat's leading `Q` stays (`Pagination::cuts`), and another
file's block of a replaced kind, a stream of its own, is left out whole
with its isolation. Names only the cut ranges used leave the page's
resources, so Acrobat's form, its graphics state and its fonts are
unlinked and a Rewrite holds none of them. A range a splice refuses
stays, said once a save: "1 watermark or background other programs or
files drew stays on page 1: taking it out would change what else the
page draws." `PageBlocks::selected` (and `Pagination::selected_by`) is
the one rule the emitter, Remove's lowering and the effects use, and
`Pagination::generator` tells a block's generator, so a backed one isn't
counted. The cut goes with drawing the generators: under `content_only`
a replaced artifact stays as the file has it, so OCR may read a
replaced Acrobat watermark's text.

### What marks sheaf's merged blocks, and Clean Copy

`Filter::StripPrivate(PrivateSel::Marked(sel))` writes the property lists
of the ranges selected again without `private::LIST_KEYS` (`/SHEF_Gen`
and `/SHEF_OcrBlock`, which mark a generator's block or an OCR block
another tool merged into a stream), keeping what they draw:
`lex::without_entries` removes the entries and the spaces before them and
keeps every other byte, and the result must read back (`lex::object`) as
the list less those keys, or the rewrite is refused (`SpliceError::List`:
a named list, one across streams, a key that isn't a name or a value
that isn't an object, a range that doesn't open with `BDC`). Rewrites go
in the same `Filtered` copies as cuts (`Spliced::streams(content,
rewrites)`; one inside a taken cut goes with it); a refused one keeps
what marks it, with a Note. Clean Copy, and print and page images
without markup, read each page's blocks and add the filter where such a
block is merged into a stream (`private_filters`, the
`RemovePrivateData` transform's); adding it is `CONTENT | REMOVES`
(`RemovedKind::PrivateData`), so an Append can't leave the path in an
earlier revision. A list named in the page's resources is
RemovePrivateData's too (`PrivateSel::NamedLists`; see Private data,
under Exports, memory sources, Session and `sheaf edit`). sheaf is
pre-release: what a sheaf before the edit API kept in what it drew isn't
read, recognized or taken out.

### The app

The Watermark… and Background… dialog is the op's: text, font (a family
row and a style row for the 12 faces), size, colour, opacity, angle
(text only), the 3×3 position, the pages; or an image or a PDF, whose
page 1 is drawn, read and interned off the UI thread (`read_art`); or
what the watermark at that z draws now ("the image in the file" after a
reopen), sent as `image: None` or `pdf: None`. It starts from the
generator at its z (`Start::at`), or DRAFT over the content and
CONFIDENTIAL under it. Its preview is Crop's and the header's
(`Preview<Op>`, `Previewer::page_in`): the op lowered in `tx_at(now)` on
the previewed page, drawn under `screen()`. Apply commits the op
synchronously with `Validate` on the version the dialog opened on,
holding the file chosen until it lands ("Watermark" or "Background");
Remove is a planner, since its lowering reads every page's content, with
an "the author's own too" choice for `include_authored`, removing from
every page. The notice is the commit's line and notices (the
Unencodable sentence, or what Remove found by who drew it), or "No
watermark to remove". Choosing the menu item again closes the dialog.

**Deleted:** `derive::watermark`, `current_watermark` and `has_kinds`;
all of `watermark.rs` (`Watermark`, `Mark`, `Anchor`, `FONT`,
`install`, `read`, the per-page ExtGState, font and image and the
names it drew with); `Derive::Watermark` and `RemoveWatermark` with
`set_base`'s Unencodable arm; `save::HeaderKept` and `derived_copy`'s
`header` and geometry parameters; `stamps::WATERMARK` and `BACKGROUND`
(`stamps::strip` stayed the header and footer's alone until slice 17); the
app's `apply_watermark`, `open_watermark` and the dialog's `behind` and
`current`. `tests/retired.rs` lists them.

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness):

| Row | Synthetic | Heavy | Scanned |
|---|---|---|---|
| thumbnail strip after a crop, a watermark and the header (asserted ≥ 310 pages/s, ≤ 96 MB weighed and RSS added) | 413 pages/s, 80.7 MB, 26.2 MB (slice 15, no watermark: 446) | 344 pages/s, 80.7 MB, 63.0 MB (372) | 162 pages/s, 80.7 MB, 77.6 MB (167) |
| watermark every page, the commit | 0.014 ms | 0.014 ms | 0.015 ms |
| a text watermark and an image background on every page: Rewrite, file, Append | 343 ms, 3.97 MB, 483 ms | 890 ms, 18.5 MB, 681 ms | 1,028 ms, 475 MB, 691 ms |
| that file opened again: ready, both found | 92.7 ms | 496 ms | 536 ms |
| its census (asserted): images, fonts, graphics states, forms added; page resources | 1, 1, 1, 0; 1 of 1 | 1, 1, 1, 0; 70 of 70 | 1, 1, 1, 0; 7,000 of 7,000 |

The strip draws a watermark on every page now, which costs it about
0.2 ms a page on the heavy; the census counts what the written pages'
resources bind that the file's didn't, page by page.

## Headers, footers and Bates as generators (M10.5 slice 17)

The header and footer is a generator too (`GenSpec::HeaderFooter`): one
record, drawn as `/SHEF_Block` blocks on each page it covers when a save
writes the page or a view draws it, found again from `/SHEF_Generators`
when the file opens. Its page tokens are live, read from the document as
it's written, and its Bates numbers, date and file are fixed when it's
set (§3.5, §6.5, §7.1–7.2, §9.1, §11.3–11.4, §11.9, proposals 8 and 31
and bugs 16, 24, 28 and 29 of `plan/complete/m10.5-edit-api/README.md`). The old
header went with it: `stamps::install`, its catalog record, Bates
numbers counted at each Rewrite, the `LegacyHeaderFooter` Rewrite and
the Full sidecar; and so did derived
sources, whose last maker was Remove Header/Footer's strip.

```rust
pub struct HeaderFooterSpec {                                 // sheaf_edit
    pub texts: [String; 6],          // header left, center, right; footer left, center, right
    #[serde(default = "legacy_face")] pub font: FontChoice,   // Helvetica when it says none
    pub size: f32, pub color: Color, pub margins: Margins,
    pub first_number: u32, pub bates: Option<BatesSpec>,
}
pub struct BatesSpec { pub prefix: String, pub suffix: String, pub digits: u8, pub start: u64 }
Op::SetHeaderFooter { spec: HeaderFooterSpec, scope: ScopeSel }
Op::RemoveHeaderFooter { pages: PageSel, include_authored: bool }

pub struct Generator { pub id: GenId, pub spec: GenSpec, pub z: Z,   // sheaf_doc::model
                       pub scope: Scope, pub frozen: Frozen,
                       pub replaces: BTreeSet<PaginationKind> }
pub enum GenSpec { HeaderFooter(Arc<HeaderFooterSpec>), Watermark(Arc<WatermarkGen>) }
pub struct WatermarkGen { pub content: WatermarkContent, pub position: GridPos }
pub struct Frozen { pub date: Arc<str>, pub file: Arc<str>,
                    pub bates: Option<Arc<im::OrdMap<PageId, u64>>> }
pub struct CtxNeeds { pub index: bool, pub count: bool, pub label: bool, pub bates: bool }
pub struct PageCtx { pub geom: Geom, pub position: u32, pub count: u32,
                     pub label: Arc<str>, pub bates: Option<u64> }
impl Generator { pub fn needs(&self) -> CtxNeeds; pub fn kinds(&self) -> BTreeSet<PaginationKind>;
                 pub fn covers(&self, page: PageId) -> bool;
                 pub(crate) fn same_place(&self, other: &Generator) -> bool; }
pub struct DrawnBlock { pub layout: u128, pub recipe: u128, pub key: ObjKey }   // materialize::record
pub type DrawnBlocks = HashMap<(PageId, GenId), DrawnBlock>;                   // SaveRecord.drawn
pub fn others_on(st: &DocState, kinds: &BTreeSet<PaginationKind>,             // sheaf_doc::tx
                 pages: &[PageId], include_authored: bool) -> OthersFound;
```

Slice 16's z, scope and `replaces` moved from `WatermarkGen` onto
`Generator` as fields, with `frozen`, so both kinds read them alike;
`WatermarkGen` keeps its content and position (§3.5's `WatermarkSpec` is
`sheaf_edit::WatermarkSpec`'s name).

### Tokens, live and frozen

`generate::header` holds the six tokens (`TOKENS`) and the layout.
`<<1>>` (the page's place, from `first_number`), `<<N>>` (the count) and
`<<label>>` (its page label, or its number with no labels) are live:
they read where the page is as the document is written, so a reorder or
an extract says what each page is there. A print reads where the
document has each page (Greg, 2026-10-22): pages 5 and 6 of 10 print
"Page 5 of 10" and "Page 6 of 10", as print preview shows them, with the
document's labels, though the print file keeps none. Its `KeepPages`
numbers `AsDocument`, so the export's state holds the version it kept the
pages from (`DocState::numbered_in`, set by the `ExportTx`, never by an
edit), and `generate::page_ctx` and the render key read a page's place,
the count and its label there; a version's own state holds none, one
`Option` checked a page. `<<date>>`, `<<file>>` and
`<<bates>>` are fixed when the header is set (`Frozen`): a new one's
date and file are the host's now and the document's name, kept by every
later set; its Bates numbers count the pages it covers in order from the
spec's `start`, keyed by `PageId`, so a reorder never renumbers them, a
page added later has none, and setting it again is how to renumber.
`needs()` says what the tokens read (`CtxNeeds`), and
`generate::page_ctx` builds a page's `PageCtx` reading only that (a
label only for `<<label>>`, through `DocState::label_of`, cached by
state). A deleted page takes its number with it under any scope
(`on_pages_left`, which clones the map and removes the entries, so the
rest is shared), and the cascade puts the generator back when only
numbers went; a duplicate's copy gets none (`join_scope`); and an
extract or a print (`save::subset` until slice 19's `KeepPages`) keeps
the kept pages' numbers, so an extract says the binder's numbers while
its page tokens count the pages written.

### The ops

- **`SetHeaderFooter`** puts the header in place of the one there,
  keeping its `GenId`, date and file; z is `Over` and `replaces`
  `{Header, Footer}` (`header::kinds()`). It reads no page's content and
  no `Geom`, and the order only when it numbers pages. Empty texts take
  it off; a spec `header::unfit` refuses (every slot blank, a size not
  above 0, a margin that isn't a number) or an empty `Pages` scope is
  `EditError::Invalid`. Text its face can't draw, and a page label under
  `<<label>>` it can't (`label_unencodable`, which reads the pages only
  when a range's prefix can't be drawn), is drawn `?` and told
  (`Notice::Unencodable`); `SetPageLabels` says it too when it makes a
  label unencodable there. The same settings set again make no version.
- **`RemoveHeaderFooter`** is Remove Watermark's lowering for the kinds
  `{Header, Footer}`: one `remove_generated` serves both. It narrows the
  generators drawing those kinds to the pages left (`narrowed`, reading
  `Order` when it narrows an `All` scope), then reads each page's blocks
  a page at a time, with progress and a way to cancel (`pages_done`,
  §6.3), and adds the filters `found_on` gives: `DropStreams` or
  `DropMarked(SheafGen)` for another file's blocks, and a `Pagination`
  filter for Acrobat's ranges, the author's too with `include_authored`, each only where a splice takes it out.
  `OthersFound` counts them by who drew them, and what's left (the
  author's, refused, inside forms, unreadable); what the page's filters
  take out already isn't counted again. Its line: "Remove the header and
  footer from pages 1–3 (2 made in Acrobat; 1 the author's left in
  place)", a run's counts summed.
- **`tx::others_on`** is the same count, nothing changed: it narrows a
  clone of the state as the op would, then counts `found_on` over the
  pages. It reads blocks, so it's for off the UI thread; the dialogs'
  counts are it (below).
- **Invariants.** At most one header; a `Pages` scope is non-empty and
  names pages that are there; every Bates key is a page it covers;
  `resolves_after` names the pages it numbers.
- **Effects** (§7.1). Other text, `first_number`, the Bates spec, date
  or file, another Bates number on a page still in the document, or a
  page left that's still there: `REMOVES | REPLACES_TEXT`
  (`RemovedKind::HeaderFooter`). Font, size, colour, margins or more
  pages: `CONTENT`, so it can append. A filter, or a header newly drawn
  on a saved page holding others' headers, is `CONTENT | REMOVES`, as a
  watermark's is.

### Drawn on save and on screen

- **Blocks.** Each slot that isn't blank is its own `/Artifact /Sh… BDC
  q BT … ET Q EMC`, its list `<< /Type /Pagination /Subtype /Header (or
  /Footer) /SHEF_Gen (g…) >>`, set in `Std14::from(spec.font)` from the
  page's shown frame, so it's upright at every turn and on the page's
  crop. A page with nothing to draw gets an empty block, so its scope
  reads back. The block's `/SHEF_Block` says `/N (n)`, decimal, when the
  page has a Bates number. `/L` is the layout's digest; `/R` the recipe's
  (`recipe_digest(g, page)`: the spec, z, `replaces`, and the page's own
  fixed values, never the whole map).
- **Order.** The generators on a page go background, the page's
  content, watermark, header (`generators_on`), one order the emitter,
  `intact`, recognition and the realizer share, so a reopened page
  tests intact; from slice 18 the recognized text's block goes between
  the page's content and the watermark (OCR as content).
- **Replacing others' headers** is the watermark's rule (Others'
  watermarks): Acrobat's and another file's headers and footers go from
  the pages the header covers, the author's stay.
  A page whose content can't be read gets no header, said once in a
  Note, as watermarks do.
- **`/SHEF_Generators`** has a `header-footer` kind: the spec with its
  font, and `frozen` `{date, file}`; the Bates numbers are the blocks'
  `/N`. A watermark's entry is what slice 16 wrote, byte for byte. What
  a sheaf before the edit API wrote of its header (a catalog record, a
  font and a list of its own names) isn't read: sheaf is pre-release. A
  header needs PDF 1.4, as a watermark does (`needs_after`).
- **On screen** a view draws the header through the same emission
  (`ViewInputs.generators`). A page's render key holds each generator's
  id, recipe and whether its file holds its block as drawn now, and only
  the `PageCtx` fields its tokens read (`layout_reads`); `render_reads`
  adds `Order` for `<<1>>`, `<<N>>` or `<<label>>` and `Labels` for
  `<<label>>`, whether or not the page is intact. So a reorder changes
  the keys of pages whose number changed, and no key under Bates numbers
  alone.

### Found again after reopen

Recognition reads the `header-footer` entry as it reads a watermark's,
and passes over one sheaf wouldn't write (z not `Over`, `replaces` not
`{Header, Footer}`, no `frozen`, a spec `unfit` refuses, a second header).
`generate::Saved` holds each block's `/N`, and `Saved::generator` rebuilds
`Frozen.bates` keyed by the new session's `PageId`s, so the numbers
survive a save, a reopen and a reorder after. A page whose block another
tool merged into its content has no `/N` to read (`Saved::unnumbered`):
its number is lost, recognition logs it, and a save says so ("The Bates
numbers on pages 2, 3 can't be read: another program merged the header
into the page. …").

### Live redraws (proposal 8)

- **`SaveRecord.drawn`** (§9.1) says, for each generator's block on each
  page of the saved file, its `/L`, its `/R` and its key as the removal
  check finds it (`DrawnBlock`): the record map's `ObjKey::Block`, or
  `ObjKey::Base { blob, obj, copy: 0 }` for a block in the opened file.
  An open fills it from what recognition read of the markers
  (`record::drawn_of`, through `Document::opened`), so an unmarked file is
  never walked; a Rewrite's record is what its emission drew
  (`Emission.drawn`); an Append's is the record before, less the pages it
  wrote and the pages gone, plus what it drew. Kept blocks are noted
  with their own markers.
- **`LIVE_REDRAW`** (§7.1): an `Order` change sets it when a generator's
  tokens read the place, the count or the label, and a `Labels` change
  when one reads the label. It's never a Rewrite reason.
- **The closure** (§7.2, `deps::live`): on an `Order` or `Labels` change,
  the pages a live generator lays out otherwise now than `drawn` says its
  block there is (by layout digest), or that have none, are written;
  pages alike in what the layout reads (`realize::layout_reads`) are laid
  out once. A reorder of two pages under "Page n of N" writes those two
  pages and the tree node.
- **A header is laid out once a page** for every version of the
  document that lays it out alike (slice 17's follow-up): its layout
  digest is kept by the states, one memo for all versions derived from
  an open (`DocState::laid`), keyed by the generator, its recipe on the
  page (`recipe_digest`, its spec and what it fixed for the page) and
  what its tokens read of the page (`layout_reads`), which is all its
  layout reads. `untouched`, `render_key`, the closure, `intact` and
  `rebuilt` ask it, and a block drawn adds its digest. A watermark's
  isn't kept: its art is read from the file it's written to. Past 32,768
  digests it starts again; debug builds lay each one kept out again and
  assert they agree.
- **Allowed drops** (§11.9). When the emitter draws a block where the
  saved file's block of that generator is another object drawn from the
  recipe it draws from now (a renumbered page, or one laid out again for
  its boxes), the old block's key goes into `Emission::allowed_drops`, and
  the removal check lets it go; on a page whose other blocks are judged,
  that generator's old words are left out of the comparison. A text
  change is another recipe, so it vetoes and plans a Rewrite
  (`ReplacesText`) with the old words gone; a margins change appends. A
  Rewrite's own drops reach its oracle (`removal::of_rewrite`).
- **Recovery.** A header no longer makes a sidecar Full: an unsaved
  header's sidecar is an Append section, and a live redraw's (save,
  reorder, sidecar, restore, save) appends with nothing removed.

### The app

The Header & Footer… and Bates Numbering… dialog builds
`Op::SetHeaderFooter` itself and previews that op alone, lowered on the
current version off the UI thread: the header set again replaces the one
drawn there, so no strip is needed and the preview is what applying
draws. It starts from the model's header (`Start::at`), which after a
reopen is the recognized one: texts, font (family and style chips,
shared with the Watermark dialog in `generator_dialog`), size, colour,
margins, the pages as text, and Bates. Apply commits on the version it
opened on with `Validate` ("Header & footer"; "No change to the header
and footer" when nothing changed); Remove is a planner
(`DocView::plan_removal`, shared with Remove Watermark) with "the
author's own too" for `include_authored` ("Remove header & footer";
"No header or footer to remove"). A planner stopped part way says it
stopped (`sheaf_ops::was_cancelled` knows `EditError::Cancelled`).
Choosing either menu item again closes the dialog; the other one swaps
it.

**Counts** (proposal 31, D61). Both dialogs show what others drew of
their kinds on the selection (more than one page selected) or on every
page: `generator_dialog::Count`, counted once as the dialog opens, off
the UI thread, by `others_on` without and with the author's own, and
dropped if it isn't of the version the dialog opened on. "On all pages:
1 made in Acrobat will be replaced; 2 the author's stay (Remove can take
1 of them)", "Counting…" until it lands, or "On all pages: no header or
footer others drew".

### Derived sources, gone

No flow makes a copy of a page now, so the side record and everything
that served it went: `model::Derived`, `DerivedCopies`,
`Document::derived` and `add_derived`, `Snapshot` and `SaveInput`'s
`derived`; `MemoryKind::Derived` (`Source::marked` reads the catalog
alone, `DocState::backs` names the opened file's generators only);
`save::derived_copy` and `copy_form`, with `graph::Loaded`,
`copy::inherited_attrs`, `INHERITED` and `reserve` and `file::not_ours`;
`ResolvedOp::SetPageBase`, `tx::Derive`, `Tx::set_page_base` and
`tx::Entry`'s base variant (`Entry` is the struct `{ op, drawn,
positional }`); the emitter's and the realizer's derived pages
(`Shown` is `{ src, index, copy }`); sheaf-ops' `derive.rs`; and the
app's derived flows (`show_copies`, `keep_copies`). Tests that used a
page rebase as a content change use `ReplacePages`, which writes
`Page(p)` as it did. A form keeps its fields, widgets, values and
appearances through Set and Remove on Acrobat's header, both ways
(`forms_pages`).

**Deleted:** all of `stamps.rs` (`HeaderFooter`, `install`,
`to_content`, `to_catalog`, `from_catalog`, `stored`, `CATALOG_KEY`,
`drawn_over`, `base_draws`, `strip`, `stripped`, `HEADER_FOOTER`, `FONT`,
`FONT_NAME`, `PageFacts`, `Placed`, `Shared`, `Bates`; `TOKENS` and
`Placed` moved to `generate::header` as `TOKENS` and `Line`);
`Source::header_footer`, `StoredHeaderFooter` and
`open::Facts::header_footer`; `DocState::{header_covers, bates_of,
header_pages, number_header_pages}` and `Indexes::header`; `Cx::bates`
and `emit::content::header_footer`; `Why::LegacyHeaderFooter` and
`plan::sidecar`'s Full rule; `REPLACES_NONE` and `CtxNeeds::any`; the
derived sources above; and the app's `header_footer_op`,
`open_header_footer`, `watermark_notice` and the dialog's `today`,
`current`, `file` and `scope`. `tests/retired.rs` lists them.

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness, after slice
17's follow-up):

| Row | Synthetic | Heavy | Scanned |
|---|---|---|---|
| Bates numbers on every page, one op on the file opened afresh (asserted < 50 ms) | 0.62 ms (slice 2: 114 ms, a copy of every page) | 0.64 ms (270 ms) | 0.63 ms (428 ms) |
| the Rewrite that draws them, and the fonts it adds (asserted 1) | 419 ms, 1 | 949 ms, 1 | 921 ms, 1 |
| "Page n of N" on every page, saved, every page moved: the Append, planned and written (asserted ≤ 1 s), its section (asserted ≤ 8 MB) | 469 ms, 1.36 MB | 655 ms, 1.47 MB | 469 ms, 1.37 MB |
| the same move on the file opened again: the Append (≤ 1 s, asserted on the scanned), its section (asserted ≤ 8 MB) | 0.89–1.07 s, 3.31 MB | 1.57 s, 3.33 MB | 0.94 s, 3.32 MB |
| a page drawn at 1024 px, its group not drawn before: the header just set, every page moved, one deleted, one deleted on the file opened again (the realize gate 15 and 25 ms) | 22.3, 18.8, 19.3, 27.3 ms | 36.7, 33.9, 31.8, 41.8 ms | 33.6, 32.4, 31.0, 41.0 ms |
| thumbnail strip after a crop, a watermark and a "Page n of N" header (asserted ≥ 310 pages/s) | 420 pages/s | 346 pages/s | 163 pages/s |

Proposal 8's verdict is live page numbers (Greg, 2026-10-22). The
Append holds its gate in the session that saved the header, and on the
scanned opened again. The first page drawn of a group every page of
which is laid out anew (a header just set, a reorder, a delete under
"Page n of N") realizes all 128 of them before PDFium opens the group,
so it misses the realize gate on the synthetic and heavy; and on a file
opened again the reorder's Append classifies and rebuilds every page's
content cold, 1.57 s on the heavy. Greg accepted those misses: the 7k
harness lists them (`ACCEPTED_MISSES`) and asserts them only with
`SHEAF_GATE_PROPOSAL_8`. A lead window that realizes a group's visible
pages first (option A) is to revisit.

## OCR as content (M10.5 slice 18)

Recognized text is page content drawn as a block of its own: a layer on
the page's stack, with a block id, written as a `/SHEF_Block` stream
after the page's isolated content and before the generators drawn over
it, and found again in a reopened file by that stream's marker. A layer
a file holds is like any other, saved or not: a re-OCR or Clear takes it
out, and the save says so (§3.4, §6.1, §6.4, §7.1, §11.4, §11.9, §13,
proposal 26 and bug 14 of `plan/complete/m10.5-edit-api/README.md`). The
old installer went with it: `text_layer::install`, `strip` and
`has_layer`, `[q, base…, layer]` with names of its own, and the
emitter's special cases for it; and later the reading of the layer it
wrote (sheaf is pre-release: what a sheaf before the edit API wrote
isn't read).

```rust
pub struct Layer { pub id: BlockId, pub body: Static }            // sheaf_doc::model
pub enum StreamSel { SheafGen(GenId), SheafOcr }
pub enum MarkedSel { SheafGen, Pagination { kinds, origins }, SheafOcr }
impl ContentStack { pub fn ocr_layer(&self) -> Option<&Layer>;
                    pub fn with_ocr(&self, layer: Option<Layer>) -> Arc<ContentStack>;
                    pub fn with_filters(&self, f: impl IntoIterator<Item = Filter>) -> Arc<ContentStack>; }
impl Filter { pub fn drops_ocr(&self) -> bool }
pub enum Marker { Gen { id, layout, recipe, number }, Ocr(BlockId), Isolate }   // sheaf_doc::content
impl BlockId { pub(crate) fn draw(rng: &mut fastrand::Rng,     // random in [1, 2^53), `b…` as hex
                                 taken: impl Fn(BlockId) -> bool) -> BlockId }
EditError::Content { op: &'static str, page: u32, why: String }      // sheaf_doc::tx
pub(crate) fn block(layer: &TextLayer, id: BlockId, reserved: &Reserved)  // text_layer
    -> (Vec<u8>, ResourceNeeds);
```

There's no `BlockKind` stored: the body's variant says what a layer is,
and from slice 19, which brings the flattened kind with
`FlattenAnnotations`, `Layer::kind()` says it (`BlockKind::{Ocr,
Flattened}`).

### The ops

- **`SetOcrText`** puts the layer on the page's stack in place of the
  one there, keeping its id (the replaced layer's, saved or not), else
  drawing one (`Lower::block_id`: a pinned id first, so a relowered
  transaction draws the ids it drew, else random in [1, 2^53)). A
  duplicate shares the stack, and with it the id. A layer with no words
  means none, and nothing is put when the page is as it was.
- **`ClearOcrText`** takes the stack's layer off, back to the shared
  empty stack when nothing else is on it, and reports progress a page at
  a time.
- **What the file holds.** Both read the page's blocks (`Source::blocks`,
  a fact of its source read once; `saved_ocr`) and add the filters that
  take the recognized text sheaf wrote in its file out: `DropStreams(
  StreamSel::SheafOcr)` for an OCR block that's a stream of its own, and
  `DropMarked(MarkedSel::SheafOcr)` for blocks another tool merged into a
  stream, once a splice is shown to take them together. Both record a
  read of the page's `Page(p)`, so a flush that changes nothing still
  conflicts with a change to the page under `Validate`.
- **In the app** (since the backlog-fixes PR, F8): Document > Remove
  Recognized Text… asks whether the selected pages or every page, then
  runs `ClearOcrText` as a planner (`ocr_clear.rs`), one undo step. On
  the command line it's `sheaf edit`'s `clear-ocr-text`.
- **`EditError::Content { op, page, why }`** (§6.4): a page whose content
  can't be read, or whose merged block a splice refuses, fails the op,
  naming it, the page's position and why (`ContentError::on_page`). Where
  the old installer drew without lexing, Set on such a page now
  fails; the app's OCR run counts the page as failed and goes on.
- **Effects** (§7.1). A layer set where the saved state had none is
  `CONTENT`, so it appends; one the saved state had, replaced or gone
  where no page shows it, is `REMOVES | REPLACES_TEXT`
  (`RemovedKind::OcrLayers`), as before. From this slice so is a
  `SheafOcr` filter added, counted once a page with that layer, so a
  re-OCR or a Clear over a layer the file holds plans a Rewrite
  (`ReplacesText`) with the old words gone, where it went through the
  removal check's veto before. History marks such a step as removing.

### Drawn on save and on screen

- **The block.** `text_layer::block` writes `/ShOCR /Sh… BDC q BT 3 Tr …
  ET Q EMC`: invisible text in the glyphless font, each word fitted to
  its box in the layer's upright frame, in a `q` of its own so its text
  mode reaches nothing drawn after it, marked with a property list `<<
  /SHEF_OcrBlock (b…) >>` that names its block. The font and the list
  take `/Sh…` names clear of the page's (`Reserved`). The emitter
  (`recognized`) writes it as a stream marked `/SHEF_Block << /K /Ocr /B
  (b…) >>`, keyed by its digest (`ObjKey::Block`), its resources in the
  page's own (`ObjKey::PageRes`); the glyphless font is one object a
  file (`ObjKey::Res`). Each block names its own list, so pages don't
  share their resources dictionaries as they do under a watermark.
- **Order** (§11.4): `[under… QOpen base′… QClose text over…]`. A page
  with OCR and a header reads its own text, the recognized words, then
  the header, as before slice 17. A page with recognized text is
  isolated as a generator's is, and needs PDF 1.2 (`CID_FONTS`). One whose
  content can't be read is drawn as it is, without the text or the
  generators, said once in a Note ("No recognized text, watermark,
  background, header or footer is drawn on page 3, whose content can't be
  read.").
- **Taken out.** `rebuilt` acts on the filters: OCR blocks that are
  streams of their own are left out of `/Contents`; merged OCR ranges
  are cut out of their streams (`splice_each`, a copy of each stream,
  `ObjKey::Filtered`), and the names only they used are dropped from the
  page's resources. A cut a splice refuses stays, said in a Note
  ("Recognized text sheaf wrote earlier stays on page 3: taking it out
  would change what else the page draws."). What's left is counted again, so the isolation
  matches.
- **On screen** a view draws the stack's layer when its inputs ask for
  it (`ocr_over`: every consumer's but `content_only`'s, which blank
  detection's text read turns on), through the same emission;
  `content_only` draws the page as its file has it, the blocks the file
  holds included unless a filter takes them out.

### Found again after reopen

- **`Marker::Ocr(BlockId)`.** `Source::markers` reads a page's OCR blocks
  by their stream's dictionary, as it reads a generator's. A flattened
  block is none: the page's content.
- **The static-over slot.** A saved OCR block no filter drops stays where
  sheaf wrote it: `[gen-under… QOpen inner… QClose ocr… gen-over…]`.
  `intact` peels the OCR blocks off before it checks the isolation, so a
  reopened page with recognized text and a header is written as it is;
  with no generator on the page it's intact when no block a generator
  backs is there and the page has no isolation, an unbacked block or
  saved text. `rebuilt` keeps the blocks by reference, in file order,
  after the new `QClose` and before the model's new block and the
  generators over the page, leaves them out of base′'s balance while
  keeping the names they draw with (one `content::names_used` pass over
  those streams alone), and isolates the page even with nothing else
  drawn, reusing the file's isolating streams when they isolate as many.
  So nesting never grows: five cycles of a header changed over a saved
  block on an unbalanced base keep `q`, the base, `Q Q`, the block, then
  the header.
- **Files saved with OCR alone.** `Source::markers` stays gated on the
  catalog's `/SHEF_Generators`, so a file sheaf saved with recognized
  text and no generator shows no markers to `intact`, `untouched` and
  the render key: the base engine draws such a page, which is right, as
  nothing is drawn over it. A page something is drawn on reads its
  blocks anyway (`stack`'s unmarked case, the read `wrapped` made), and
  when they list sheaf's isolation or a stream-level block the page goes
  to `rebuilt`, which reads the markers from what it decodes, rather than
  wrapping the old isolation again.
- **The removal check** (§11.9) judges a saved OCR block of the opened
  file as one of its own objects, on its words decoded; a block drawn in
  the session is derived, judged on its layer's words.

### The OCR run

A flush lands by value on the version its round began on (`land`): the
batch first, then page by page on a Conflict or `EditError::Content`. A
page that changed since its round began (a `ReplacePages`; a crop or a
turn doesn't, as neither writes `Page(p)`) keeps no text from it and is
queued (`Requeue`): when the round finishes, not stopped or undone, the
pages queued that are still in the document start a new job over the
current version's snapshot, in the run's group, with a new task (the
engine is loaded again). A page is read again twice at most
(`REQUEUES`); past that, gone before its round, or left when the run
stops, it's counted as changed. A page whose content can't be read is
counted as failed; the rest land. A dropped result isn't counted as
recognized (`Tally.dropped`), so a page read again counts once, as its
last read went, and a page never read again only as changed. The
message says ", 2 read again after a change". Undo still stops the run,
and starts no round. The run's bookkeeping (`Tally`: its counts, its
flushes, and whether a finished round is followed by another or the
run ends, with what message) is apart from the view and its jobs, so
tests drive it with a job's events and a document; the view only
starts each round's job and follows the commits.

### The plugin API

API 0.3 had no `ocr.page-text-layer`: recognized text, saved or not, is
the page's content, which `text` reads as the file will draw it now.
0.2's, which the frozen 0.2 fixture imported, answered none until slice
20 deleted 0.2 with its fixture (and `api::v02`, `api::v03` and
`api::shared` with it); API 0.4 has only `ocr.recognize`. 0.3.0 lost the
function without a version bump: it was unreleased, and no plugin in the
repository imported it.

**Deleted:** `text_layer::{install, strip, has_layer, layer_entries,
to_content, MARKER, FONT_NAME}`; `ContentStack::with_ocr_text` (now
`with_ocr`); `emit::content::fresh_balance`, the emitter's `replaced`
layer and `page_res`' inline resources; `ContentWriter::finish` and
`named`'s given names; `content::sink::Sink::{add, stream_dict}` (the
sink stays for the marks' appearance forms and the emitter's resources);
the plugin host's 0.3 `page_text_layer`. `tests/retired.rs` lists them.

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness):

| Row | Synthetic | Heavy | Scanned |
|---|---|---|---|
| recognized text set on every page (300 words each), one transaction on the file opened afresh, each page's blocks read cold | 197 ms | 268 ms | 141 ms |
| the Append that writes it, planned and written, and its section | 9.96 s, 17.5 MB | 9.97 s, 17.4 MB | 10.06 s, 17.6 MB |
| its census: fonts added (asserted 1), blocks (asserted one a page), property lists added | 1, 7,000, 7,000 | 1, 7,000, 7,000 | 1, 7,000, 7,000 |
| realize + open + render an edited page, a header and recognized text on it (asserted < 15 and < 25 ms) | 12.0 ms (slice 17: 12.0) | 22.7 ms (21.2–21.8) | 32.2 ms (31.8) |

The Append takes what slice 17's did (9.4–9.8 s in the same probe):
writing each page's operators (0.8 ms for 300 words) and deflating them
(0.5 ms). Its section is larger because the gate gives every page the
same words, which slice 17 wrote as one object, where each layer has its
own block, list and resources now; recognized text differs page by page,
so a real run writes a stream a page either way. Every other gate holds
where slice 17 left it, proposal 8's accepted misses included.

## Exports, memory sources, Session and `sheaf edit` (M10.5 slice 19)

Every way out of a document but a save goes through one writer,
`sheaf_doc::export::run`: an export is a version of the document and
transforms lowered on a transaction of its own that's never committed,
written by `materialize` with every check a save has. Flattening is an op
and a layer of forms placed by reference; sheaf's private data is a table
an output can be checked against; the pages sheaf makes (blank pages,
images, scans, library stamps, the comment summary) are made in memory;
and a document with no window is a `sheaf_ops::session::Session`, which
every subcommand runs on, Combine hands to a window, and `sheaf edit`, the
first producer of persisted Op JSON, edits with ops naming things as a
person sees them (§3.4, §3.9, §6.1, §6.3, §12, §14, §15, proposals 10, 35
and 40 and bugs 16, 21, 22 and 29 of `plan/complete/m10.5-edit-api/README.md`).

```rust
pub struct ExportFrom { pub snap: Snapshot, pub cx: Arc<LowerCx> }    // sheaf_doc::export
pub struct Export { pub from: ExportFrom, pub transforms: Vec<Transform>,
                    pub profile: Profile, pub out: Out }
pub enum Out { File { path: PathBuf, overwrite: bool }, Bytes }
pub enum Gate { OwnFile { path }, Exists { path }, Refused { transform, cap } }
pub fn preflight(e: &Export) -> Vec<Gate>;
pub fn run(e: Export, answers: &AskAnswers, p: &dyn Progress) -> Result<Exported, ExportError>;
pub fn check(e: &Export, answers: &AskAnswers, p: &dyn Progress) -> Result<(), ExportError>;
pub fn not_its_file(snap: &Snapshot, path: &Path) -> Result<(), Gate>;
pub struct Profile { pub encryption: Encryption, pub numbering: Numbering,   // sheaf_doc::materialize
                     pub private: Private, pub unheld: Unheld }
pub enum Private { Keep, Strip, Scrub }
pub enum Unheld { Keep, Only(MarkupKeep) }
pub enum SaveHow { Planned, Rewrite, Append }
pub fn save_with(doc: &mut Document, path: &Path, how: SaveHow,
                 answers: &AskAnswers, p: &dyn Progress) -> Result<SaveReport, SaveStop>;
pub enum Transform { Op(Op), RemoveMarkup { keep: MarkupKeep }, RemovePrivateData,  // sheaf_edit
                     KeepPages { pages, outline: OutlinePolicy, labels: bool, numbering: PageNumbering } }
pub enum PageNumbering { AsKept, AsDocument }
Op::FlattenAnnotations { pages: PageSel, which: FlattenSet }   // FlattenSet { All, Printable }
PageSource::Blank { size: PageSize, count: u32 } | Images { images: Vec<BlobSel>, fit: Fit }
pub enum Static { OcrText(Arc<TextLayer>), Placements(Arc<[Placement]>) }    // sheaf_doc::model
pub struct Placement { pub form: FormRef, pub matrix: Affine, pub tag: PlaceTag }
pub enum FormRef { Base(ObjRef), Blob(BlobRef), Pinned(Arc<PinnedAp>) }
pub enum BlockKind { Ocr, Flattened }                  // Layer::kind(), from the body
pub enum PrivateSel { Marked(MarkedSel), Streams, Resources }
pub struct Session { pub doc: Document, pub view: ViewFacts, .. }     // sheaf_ops::session
pub enum SaveTo { InPlace, File { path: PathBuf, append: bool } }
pub struct NeedsDecision(pub Ask);
```

### Exports

`run` takes an `Export` in order, and a failure anywhere writes nothing:

1. **The gates** (`preflight`, §12.1): never the document's own file
   (`Gate::OwnFile`, by `durable::same_file` against `Snapshot.file`:
   `export::not_its_file`, which the comment exports' own writers ask
   too, sheaf-ops' `comments::summary_to` and `comments::write_to`),
   never a file that's there unless `overwrite` (`Exists`, checked again
   before the write), and each transform one its actor may make
   (`Refused`: a transform's capability by `transform_cap` and
   `actor_exports`, an op's by `Tx::check` on the version).
2. **The transforms**, lowered on an `ExportTx` (`tx/export.rs`), which
   is `Document::tx_at` over the version: the binder's resolve,
   capability and lowering stages and cascades, its ids drawn from the
   document's allocator through the `LowerCx` the export carries
   (`Document::export_from(actor)`), so a `Snapshot` alone can't lower
   one. Only `ExportTx` takes a `Transform`, so nothing an export does
   becomes an edit.
3. **The plan** of a Rewrite of what they leave (`materialize::plan` over
   `SaveInput::detached`, its target the export's state), with save
   policy's form rules, so an extract that leaves out a hybrid form's
   widget pages drops its XFA as a delete does. The plan's asks are the
   export's: `Signatures`, limited to the signatures whose field the
   exported state still holds (a certification always counts), so an
   extract without the signed pages doesn't ask; and `XfaHoldsValues`.
   One the `answers` don't answer is `ExportError::Ask`.
4. **Written** by `materialize` (every object emitted, `Deferred` bodies
   made as they're written), checked as every output is (§11.9, the
   private-data check under Strip among them), then put at its path by
   `durable::write`, or handed back (`Out::Bytes`). `p` counts the pages
   and cancels between them.

The transforms (sheaf-edit's `Transform`, JSON outside the envelope:
`{"keep-pages": {…}}`, `"remove-private-data"`):

- **`KeepPages { pages, outline, labels }`** deletes the other pages with
  `DeletePages`' lowering and cascades: their marks, comments and fields
  shown only there go, a connector whose other end goes keeps no
  partner, a label range starts at the first page of it kept, and a
  generator stays on the pages it covers with the Bates numbers they had,
  so an extract of an unsaved Bates run draws and keeps the binder's
  numbers. `OutlinePolicy::Headings` (extract, split) first prunes the
  outline as the old `subset` did: a bookmark whose page goes stays as a
  heading when one under it stays; `Promote` leaves it to the delete's
  cascade; `Drop` takes every bookmark off. `labels: false` clears the
  labels. `numbering` says what a header's page tokens read: `AsKept`
  (extract, split, Remove Blanks) where the file has each page, "Page 1
  of 2"; `AsDocument` (print) where the version it keeps them from has
  it, "Page 5 of 10", and its labels (`DocState::numbered_in`, above).
  Capability `Plugin`, a delete's.
- **`RemoveMarkup { keep }`** takes off sheaf's marks and other tools'
  comments, and the links and widgets `keep` doesn't keep, and sets the
  profile's `unheld` to `Only(keep)`: the emitter writes, of what the
  records don't hold (a printer's mark, media), only those links and
  widgets (`annots::unmodeled`), so no post-pass on the graph is left.
  Capability `Approval`.
- **`RemovePrivateData`** puts `tx::content::private_filters` on every
  page and sets the profile's `private` to `Strip` when the preset left
  it `Keep` (Private data, below).
  Capability `Approval`.
- **`Op(op)`** goes through `Tx::apply` itself: `FlattenAnnotations` for
  print and Export Flattened, `AddBookmark` for Remove Blanks' rejects.

A plugin may make only `KeepPages`, and an OCR or Trim White job none.

The presets are constructors (`Export::…(from, …, out)`), each with its
profile (§12.3, proposal 10):

| Preset | Transforms | Profile (encryption, numbering, private) |
|---|---|---|
| `whole`: a save's bytes (`save::rewrite`, for tests) | none | Keep, Stable, Keep |
| `extract`: Extract Pages, a split's part | `KeepPages { Headings, labels, AsKept }` | Keep, Dense, Keep |
| `print(pages, true)`: print, page images, Save as PDF | `KeepPages { Drop, AsDocument }`, `FlattenAnnotations { Printable }`, `RemovePrivateData` | None, Dense, Scrub |
| `print(pages, false)`: the same without markup (§16 row 27) | `KeepPages { Drop, AsDocument }`, `RemoveMarkup { links, widgets }`, `RemovePrivateData` | None, Dense, Scrub |
| `flattened`: Export Flattened | `FlattenAnnotations { All }` | Keep, Dense, Keep |
| `clean_copy`: Clean Copy | `RemoveMarkup { links, widgets }`, `RemovePrivateData` | Keep, Dense, Strip |
| `rejects`: Remove Blanks' second file | `KeepPages { Drop, labels, AsKept }`, an `AddBookmark` a page | Keep, Dense, Keep |

Each is written as a Rewrite is, so it packs as one does: of a source
that used object streams, what it writes anew goes in object streams,
numbered after its objects (Dense).

A print file answers its own asks (`PRINT_ANSWERS`: signatures broken,
an XFA part dropped): it goes to a printer or a picture, never to a file
someone keeps. `export::printed(from, pages, markup, p)` is its bytes,
which `sheaf_print::submit` sends to CUPS (`lp -`) or GDI and page
images render; Save as PDF writes the preset through `Out::File`. Print
preview realizes what prints (`export::print_preview(from, page,
markup)`, §16 row 28): with markup, the version with that page's printed
annotations flattened as print flattens them (memoized a page,
`PrintContext.printed`), drawn under `ViewInputs::print`; without, the
version as it is. A check box with states and no `/AS`, which PDFium's
form filler draws as a box, prints and previews as nothing (M10 bug 10).

The app's Extract, Export Flattened, Clean Copy and the print dialog's
Save as PDF (the print preset to `Out::File`) go through
`DocView::write_export`, a job (`start_job` and `drive`, as `run_job`'s):
one at a time, its pages counted in the status line, Escape stopping it.
An ask opens the save's dialog in its export wording
(`ask_before_writing(Asking::Export(Exporting { answers, again }))`:
"Export (breaks N signatures)", "Export Without It") and runs the export
again with the answer. Split is a job of its own (`split_into`), whose
ask runs it again the same way; page images are `run_job`'s. The command line's export, split,
remove-blanks and comments summary build an `ExportFrom` from their
session. Page images and text write each file with `durable::write`; a
TIFF streams to `.tif.part` and is renamed, since 7,000 pages in memory
could take gigabytes.

### Flattening

`FlattenAnnotations { pages, which }` (`tx/flatten.rs`; capability
`Approval`, "Flatten N [printable] annotations on …") takes, page by page
in `/Annots` order, each annotation shown (neither Hidden nor NoView),
and with `Printable` printed too; never a link, which stays, nor a
pop-up, which goes with what opens it. Each goes as a delete takes it
(`drop_annots` and the cascades: a thread, a partner, a widget from its
field and the field with its last), and what it drew goes on a new
`Flattened` layer on the page's stack, `Static::Placements`, each a form
placed through a matrix:

- **Another tool's comment or a widget:** `FormRef::Base` of its `/AP
  /N` form, placed through `Affine::onto` (its matrix times its bounding
  box, onto its `/Rect`, ISO 32000-1 12.5.5). A widget draws the state
  its `/AS` names, and one with states and none named draws nothing (no
  first-state fallback, M10 bug 10); another tool's comment with none
  named draws what PDFium, the page view's engine, shows: the state its
  `/V` (or its parent's) names, else its `/Off`, else nothing. Until the
  backlog-fixes PR (B12) it drew its first, as the old flatten did, so
  print, print preview and page images showed it where the page view
  didn't. Its dictionary and form are read unkept, and its bounding box
  once.
- **A sheaf mark:** `FormRef::Pinned(PinnedAp)`, its appearance pinned
  by what it's drawn from (`MarkAp`: kind, a connector's without its
  partner, color, opacity, stroke, contents, fill, dash, cloud, turn and
  a page stamp's form), by value and naming no annotation
  (`check_invariants` says so), compared and hashed by digest, placed by
  the identity (its appearance's box is its `/Rect`). A replacement
  gives two, the caret and its struck text. Replies and statuses draw
  nothing and go.
- **`PlaceTag`:** `Artifact` on a page of a file with a structure tree,
  marked `/Artifact BMC … EMC` (no property list or type fits a
  flattened annotation), else `Untagged`.

A page whose content can't be read is `EditError::Content`. Annotations
the records don't hold stay as the page has them. Each flatten adds a
layer; `Layer::kind()` says `BlockKind::Flattened` from the body (the
kind isn't stored).

**Written** last on the page, after the generators drawn over it
(`[gen-under… QOpen base′… QClose ocr… gen-over… flat…]`), since an
annotation shows over everything on its page, a watermark over the
content included; the OCR block stays before the generators. A stream
marked `/SHEF_Block << /K /Flat /B (b…)
>>` (`ObjKey::Block`), each placement `q <matrix> cm /Sh… Do Q`, its
name from the form's digest. A Base form is referenced copy-on-write; a
pinned one is drawn again through `annot::appearance`, as emission draws
the mark, under `ObjKey::Pinned(digest)` (byte tag 21), by content, so
an Append writes it only when the file lacks it; a page stamp's page is
named in it by its form's digest, not its number, which an Append and a
Rewrite give it apart, so both write it alike. Both the block and the
pinned forms are `Deferred` bodies, made and deflated as they're written
(`Cx::push_deferred`), so the export holds none of them at once. Placing
artifacts needs PDF 1.4. The emitter lists the annotations a primary
page had that its written page leaves out (`Emission::removed_annots`),
and the Rewrite cuts the structure tree's references to them
(`keep_only(pdf, kept, removed)`), so a delete's are pruned too.
**Reopened**, a saved block is `Marker::Flat(BlockId)`, kept in its slot
after the generators (`content::intact` reads the trailing Flat blocks
off first), so a second flatten adds a layer after it at the same depth; the removal check judges it as one of the file's own
objects, and `Drawn::on` counts what its placements show. A layer gone
is `RemovedKind::Flattened` ("page's flattened annotations"). The
flattened page's pixels are the annotated page's within 2 a channel
(PDFium maps a mark's box onto its grid-snapped `/Rect`), under an opaque
watermark over the content too, flattened, exported flattened or printed
with markup.

A widget's own pinned appearance (its value and the form's `/DA`) comes
with M10 slice 15's `FlattenFields`; until then a widget flattens by
reference to its form.

### Private data

`private::KEYS` (§15) is every name sheaf writes that only sheaf reads:
where it appears and how Strip takes it out. sheaf is pre-release: it
reads only the names this build writes, and what a sheaf before the edit
API wrote (its own names, before they were second-class `SHEF_` ones)
is neither read nor recognized nor stripped. `found(pdf)` lists what of them a file holds (dictionary keys and
names anywhere, sheaf `/NM`s, and the names and inline lists' keys in
the content of pages, forms and tiling patterns; content that can't be
read whole is read a stream at a time, and content that can't be lexed
or a stream that can't be decoded but says `/SHEF_` counts, as does a
stream past the content limit), and a Strip output whose graph holds
any fails (`verify::private`, §11.9 item 4), so a clean copy can't hold
them. A Scrub output (print, with markup or without) is written as Strip
writes it, and what's left is a note (`verify::private_left`): a
printer's or a page image's file, or text read from it, mustn't fail for
what sheaf left on a page sheaf can't read. Print with markup
kept sheaf's private data until the backlog-fixes PR (B13): its header
stayed live in `/SHEF_Generators`, so the file opened in sheaf again
numbered it against its own pages ("Page 1 of 2" for pages 5 and 6 of
10). Everything else sheaf writes is standard keys.

| Name | Where | Under Strip |
|---|---|---|
| `sheaf:…`, `sheaf-anon:…` | `/NM` | written neutral (`neutral_nm`: a UUID-shaped xxh3-128 digest of the name) |
| `SHEF_Annot` | annotation | left out; marks are written anew, never as read |
| `SHEF_Generators` | catalog | left out |
| `SHEF_Block` | stream | not marked; a file's block copied without it (`PrivateSel::Streams`) |
| `SHEF_Gen`, `SHEF_OcrBlock` | property list | lists written without them (`PrivateSel::Marked`, `PrivateSel::NamedLists`); recognized text `/ShOCR BMC` with no list |

`LIST_KEYS` and `STREAM_KEYS` are derived from it at compile time
(`model::LEGACY_KEYS` went). A stamp library's collection files
(`/SheafStampText`) are the reader's own library, outside the edit API
and the table: a stamp placed from one is its page's content and
resources.

- **The profile** (`Profile.private`, `EmitBase::Rewrite { private }`)
  covers what the emitter writes anew: no `/SHEF_Annot` and a neutral
  `/NM` on marks and their StrikeOut companions (`/IRT`, `/RT` and
  `/Dest` are references, so threads and connectors hold), others'
  comments without sheaf's keys, no `/SHEF_Generators`, blocks and
  isolation unmarked, generator lists `<< /Type /Pagination /Subtype …
  >>` (`generate::layout` and `text_layer::block` take a `Private`). A
  Strip target planned as an Append is refused.
- **The filters** (`Filter::StripPrivate`, `private_filters(def)` from
  the page's blocks and resources) cover what the file holds:
  `Marked(SheafGen(g))` and `Marked(SheafOcr)` for merged blocks;
  `Streams`, when the page has a stream sheaf marked
  (`PageBlocks::sheaf_marked`), copying each with its dictionary
  stripped; `NamedLists`, when a list its resources' `/Properties` name
  holds a key of `LIST_KEYS`, which it loses there, every byte of the
  content as it was. A named list under `Marked` alone is still refused
  with a note. On the Keep
  path nothing changes order, so a save's bytes are as before. Adding
  the filters is `RemovedKind::PrivateData` ("page's private data from
  sheaf"; `OlderSettings` went).

Clean Copy and print without markup are the presets that strip. Content
sheaf can't read keeps what it holds, and a clean copy of a page whose
content can't be read but holds sheaf's names fails: it says so rather
than write them. Print without markup writes such a page with a note.

### Pages made in memory

Blank pages, images, a scan's pages, library stamps and the comment
summary are PDFs made in memory, written through sheaf's serializer
(`serialize::full::write_built`, unencrypted and deterministic, its
bodies keyed `ObjKey::Built`, byte tag 20) and interned as `Memory` blobs
(`BlobKind::memory(kind)`; `MemoryKind::{Blank, Images, Summary}` beside
`Library`). The same inputs make the same bytes, so the same blob: an
insert lowered again or replayed has the pages it made. Nothing is
written to disk, and the generated cache (`generated.rs`, the cache
directory, `SHEAF_CACHE_DIR`) is gone.

- **`InsertPages`/`ReplacePages` from `PageSource::Blank { size, count }`
  or `Images { images, fit }`** (`Fit::{Original, Paper { width, height,
  margin }}` with `LETTER` and `A4`, which replaced `images::PageFit`).
  Applying builds and interns the source (`memory::blank`,
  `memory::images`); `Tx::check` builds and reads nothing, taking an
  images insert's `pages` on trust. Blank is named "blank page" or "N
  blank pages", images by a single `{"with": NAME}`'s name or "N
  images". The app's Insert Blank and Insert Images plan these ops
  (`plan_made`), the images read off the UI thread (`Store::read_image`)
  and pinned; Combine's image items are `InsertPages { from: Images }`.
- **Stamps.** `stamp_lib::finish` writes through `write_built`; the stamps
  panel names a library stamp by its blob's stem and writes no file; an
  imported stamp is interned as a one-page PDF blob (`BlobKind::stamp()`).
  `model::find_stamp` lost its cache arm and absorbed `held_stamp`: a stem
  nothing holds isn't found. An adopted Acrobat stamp holds the file's own
  form: `ForeignAnnot.stamp` names its `/AP /N` when its conversion is a
  page stamp, and the held-stamp index lists it.
- **`memory::empty`** is a file of no pages, `Memory(Blank)`, which a
  `Session::new_empty` starts from; `Source::open` accepts no pages from
  that origin only, and a user's file with none is still `NoPages`.
  `memory::titled` is the same with an `/Info` (its title, `/Producer`
  sheaf, `/CreationDate` now), which `Session::new_titled` starts from.
- **Images to PDF** inserts one `Memory(Images)` blob made by
  `images::to_pdf` (`intake::batch_source`; `--dpi` overrides the images'
  resolution, which no op field says) into a new session, then saves it.
  **A scan** goes through the intake (`sheaf_ops::intake`, M5 slice 1),
  a page at a time, each encoded once it's cleaned: every batch of pages
  is such a blob, inserted with its pages' recognized text in one commit
  of `Actor::Job(Scan)`, the batches one `GroupKind::Scan` group, so one
  undo step. A black and white page is held as its CCITT G4 bytes from
  the moment it's made (`Pixels::G4`, encoded by `PageImage::from_image`
  and written as they are), not as 8-bit pixels. `scan::add_text`, its
  throwaway Document and the `.images.part` file went. From M5 slice 2 a
  landing makes the blob in any store (`intake::batch_source`) and puts
  the insert and the text into any transaction (`intake::apply_batch`),
  so the app's landing shares them; `made_pages`, a Session's alone,
  went.
- **The comment summary** is a session: `new_titled`, then an insert of
  its pages, a `Memory(Summary)` blob written by `write_built`. Its Pages
  layout built its pages as an eager lopdf graph until slice 20 (it bound
  resources through `ResObjects` and copied the printed pages with
  `stamp_lib::page_forms`, which trimmed a clone of an eager document);
  from slice 20 it opens its printed bytes as a source and builds its
  sheets with `serialize::built::Built`, each page form copied from the
  source (PDFs made in memory, in Plugin API 0.4). Its `/Info` is the
  empty primary's, which a save keeps: until the backlog-fixes PR (B8)
  it had none, and since then the primary is `memory::titled`'s, so the
  summary has its Title, Producer and CreationDate again.

### Sessions and saves

A `Session` is a document with no window: `open(path, pw, p)` through
`open::read` (`open::file`'s body with progress, a torn save undone and
said), `new_empty()`, `new_empty_named(name)` (an untitled document's,
M5 slice 2, so `<<file>>` says its name until it's saved) and
`new_titled(title)`, `From<Document>`; `intern`,
`hold` and `name` for the files its ops name (`{"with": NAME}`);
`tx(actor)` with those names granted; `commit`/`commit_as`
(`Actor::Cli`, or a job's); `edit` and `preview` for ops read from a
file; `save(SaveTo, answers, p)`; `export_from()` and `export`. `view`
keeps what the open read beside the state (attachments, portfolio,
layers, form facts) for a window that takes it.

- **`save_with(doc, path, SaveHow, answers, p)`** is the save core
  (`materialize::save` went), over the private `saved_as` with a planner
  and what an Append it can't make does: `Planned` as the app saves
  (`--in-place`), `Rewrite` (`forced_rewrite`, `-o`), and `Append` (`-o
  … --append`), whose planner (`materialize::appending`) is `plan()`'s
  but appends onto a copy whenever the saved file is as sheaf left it,
  signed or not. A Rewrite stands in for an Append it can't make only
  when the reader chose to break signatures (`Refusal::Chosen`);
  otherwise it stops, with the planner's ask (Append anyway offered) or
  `SaveStop::Failed("the save can't append: …")`. So the command line's
  `-o` on a signed file asks, `--append` keeps its bytes as a prefix and
  its signatures, and `--signatures break` rewrites.
- **`NeedsDecision(Ask)`** is a save's or an export's stop at an ask, as
  an op's error (`Decided::decided`, which keeps a cancel recognisable):
  the command line exits 3 and prints `NeedsDecision::json()` (kind,
  message, counts, `append_anyway`, `stays_readable`, reason; a
  signature ask's counts are `signatures`, `breaks`, `flags` and
  `already_broken`, which it doesn't ask about). combine,
  comments import, remove-blanks and split take `--signatures
  ask|break|append-anyway` and `--xfa ask|drop` (sheaf-cli's
  `decide.rs`); comments import writes with `-o OUT [--append]` or
  `--in-place`. `--force` only replaces an output: comments csv, text
  and data refuse an existing one without it, written durably.
- **Every subcommand** opens through `Session::open`; blank pages, a
  scan, Images to PDF and the summary make theirs in memory; comments
  import commits into the session (`comments::import`), matching marks by
  name, so importing the same XFDF twice updates them, keeping `/NM`,
  dates and authors. A comment export writes sheaf's marks through
  `annot::emission` (`WriteCx::held` and `form`), and an FDF through
  `write_built`; `to_pdf`, `to_pdf_in` and `Written` are fixtures only
  (`serialize::built`'s from slice 20).
- **`Document::forget_before_cursor()`** advances history's root to the
  current version (`advance_root`): nothing to undo, the saved version
  kept as an anchor so the document still knows its file. It stops at a
  sealed version.

**Combine** (proposal 35) is `Session::open` of the first file (or
`new_empty` when it's an image) and one transaction
(`Actor::Job(Insert)`): the opened outline's roots deleted and its
labels cleared (`SetPageLabels { None }`), the first file's pages left
out deleted, one `InsertPages` a further file (`File` or `Images`), and
a bookmark per file over each file's own (`add_bookmarks`, from the
source's flat outline); committed, history forgotten before it, and
saved with `SaveTo::File { append: false }`. The first file stays the
primary, so its catalog flows through and so do its generators: a header
on all its pages covers the pages combined after it (a change: the old
Combine opened the first file without them). The app's Combine dialog
runs it in the background and hands the `Session` to a window
(`CombineEvent::Created { combined }`, taken once): the origin window
when it has no document, else a new one: `DocView::adopt`, whose core
`adopting` builds the view from the session and its `view` facts without
`open::finish`, then `Workspace::adopt`, which watches it and calls
`adopted`, telling plugins `Opened` once. Nothing is reopened: the same
store, the same version, its notices and scripts state as built, history
at the combined version, `file()` the output. The dialog refuses an
output open in a window (`workspace::is_open`).

**Create from Scans** (M5 slice 2) writes nothing until the user saves.
The dialog hands its images and choices to the window
(`ScanEvent::Scan`), which shows a new untitled document of no pages
(`DocView::untitled`: `Session::new_empty_named` through `adopting`,
kept at `Place::Untitled` under the first "Untitled scan N" no view in
any sheaf claims, `untitled.rs`), in the origin window when it has no
document, else
in a new one, and scans into it (`scan_run.rs`). The intake runs as the
view's job on a background thread; its landing makes each batch's
source in the document's store (`intake::batch_source`), asks the view
for the version to plan on with the scan job's lowering, plans the batch
there (`Document::tx_at`, `intake::apply_batch`), and hands it back to
be committed on the UI thread with `Base::At` and `Relower` in the
scan's group (`GroupKind::Scan`). An insert at the end reads no order,
so no edit of the user's meanwhile conflicts with it. A view that's gone
answers nothing, which stops the intake, and the group ends whatever
stops it. Until it's saved the view has no `path()`, and what dialogs
suggest is named after it (`stem`, `folder`: its images' folder); Save
is Save As there (`DocView::save`), after which its place is the file.

Its crash-recovery sidecar is the untitled kind, the whole document, in
the state folder's `recovery/` under its name (`dirs::recovery_dir`),
which the view claims (names skip the ones claimed or with a sidecar
there), in every sheaf process, a standalone one's too. Each batch that
lands starts its write at once, or as soon as the write in flight ends
(`DocView::write_recovery_at_once`), and autosave writes it after other
edits, as a file's; it's removed when nothing is unsaved, with Don't
Save, when the view closes with nothing unsaved, and once the document
is saved to a file, whose own sidecar then follows it. A write still in
flight then removes what it wrote (the view's `sidecar_unwanted`, shared
with it), even once the view is gone, holding the name's claim till
it's done. At launch the process that owns sheaf's state
(`instance::owns_state`) offers each one no live view holds back in its
first window (`workspace::offer_untitled_recoveries` over `offered`):
Recover shows it in an empty window or a new one
(`DocView::recovered_untitled`: the body read in the background, then
shown as a file opens, its pages first and `finish_open` after them,
never saved once it's ready, so dirty, Save As its first save, starting
in the folder its head says); Discard removes it. The close prompt's
Save stops a scan running and waits for it to end before saving
(`DocView::save_to_close`), and closes only when nothing is unsaved
after the save.

### `sheaf edit` and `sheaf inspect`

`sheaf edit IN --edits FILE|-` reads an envelope (§6.1:
`{"v": "1.0", "label"?: …, "ops": […]}`; `--schema` prints its schemars
schema), applies its ops to `IN` as one version of the command line's
(`Actor::Cli`), named by `label`, else by its first line as history
names one, and saves it where its flags say.

| Flag | Means |
|---|---|
| `-o OUT` | written whole to `OUT` (a Rewrite) |
| `-o OUT --append` | `IN` copied to `OUT` and the edits appended, which keeps a signed file's signatures; it stops, saying why, when they can't be appended (`SaveHow::Append`) |
| `--in-place` | saved into `IN`, appending when the file allows (the planner's choice) |
| `--with NAME=PATH[:PASSWORD_ENV]`… | a file the ops name as `{"with": "NAME"}`: a PDF (its password read from the environment variable `PASSWORD_ENV`), an image, or a file to attach (`Session::intern` and `name`). A `PATH` that's a file there is taken whole, so `C:\a.pdf` is a path |
| `--password PW` | `IN`'s password |
| `--signatures ask\|break\|append-anyway`, `--xfa ask\|drop` | the answers to what the save asks; `ask`, the default, stops before anything is written |
| `--dry-run` | what the edits would do, writing nothing |
| `--force` | only: replace an `OUT` that's there. It answers no ask |

One of `-o`, `--in-place` and `--dry-run` is required.

- **Addressing.** Ops from a file name things as a person sees them:
  pages by `{"pos": n}` (1-based, against the document as each op finds
  it), bookmarks by `{"path": [titles from the top]}`, annotations by
  `{"nm": …}` (and `"page"` where the name is on more than one), files
  by `{"with": NAME}`; and what an earlier op in the same envelope made
  by its local, `"$1"`. A session's ids mean nothing in a file:
  `Session::edit` and `preview` set `Tx::ops_from_file`, and a page,
  bookmark or annotation named by a bare number, or a blob by
  `{"blob": …}`, is `EditError::Invalid` ("session ids aren't meaningful
  in a file"), checked or applied.
- **Output.** Standard output is one JSON object:
  `{"version", "created", "lines", "strategy"}`. `version` is the
  version's name (null when the edits changed nothing); `created` is
  `{"ops": [[made, …] per op], "locals": {"$1": made}}`, each `made`
  named as above (`{"page": 3}`, `{"bookmark": ["Part II", "Schedule
  B"]}`, `{"annot": {"nm": "sheaf:…", "page": 1}}`, null for what a
  later op took away); `lines` are history's (`{"text", "removes"}`);
  `strategy` is `"append"`, `"rewrite"`, or null when nothing was
  written. Keys are sorted, whichever binary prints it (serde_json keeps
  insertion order only where a crate turns on `preserve_order`, as GPUI
  does in the app). What the edits' commit and the save tell the reader
  (its notices, the plan's notes) goes to standard error as `note:`
  lines.
- **`--dry-run`** prints `{"version", "lines", "consequences", "pages",
  "removed_pages", "created"}` (`Session::preview`, so `Previewed`):
  `consequences` in words, what a save removes or replaces as the save
  path's judge counts it ("removes 1 page") and what appending would do
  to the file's signatures by §9.3's table; `pages` the positions, after
  the edits, of the pages they change or add, and `removed_pages` the
  positions now of those they remove.
- **Exit status.** 0 done; 1 failed (an op that can't be done, as
  `delete-pages: page 9 of 4 isn't there`, and nothing written); 2
  usage; 3 a decision is needed: the save's `Ask` is printed as JSON on
  standard output (`NeedsDecision::json`) and nothing is written; 130
  cancelled. Ctrl-C cancels the whole edit, from reading its edits to
  the save.
- **Progress.** On a terminal, a bar on standard error. Anywhere else
  (a pipe, a log), a line as each op starts (`editing…`) and a line per
  message, where there was nothing before.

`sheaf inspect IN [--password PW] --json` prints what a script needs to
write edits, its keys sorted too:

```json
{
  "bookmarks": [{"page": 2, "path": ["Part I", "Ch. 1"]}],
  "comments": [{"nm": "n1", "page": 1, "subtype": "Text"}],
  "form": {"fields": 12, "signatures": 1, "signatures_broken": 0, "widgets": 14, "xfa": "none"},
  "generators": [{"kind": "header-footer", "scope": "2-5"}],
  "marks": [{"kind": "note", "nm": "sheaf:4127", "page": 2}],
  "pages": [{"boxes": {"crop": [0, 0, 612, 792], "media": [0, 0, 612, 792]}, "label": "i", "pos": 1, "rotate": 0}]
}
```

Pages in order, with `trim`, `bleed` and `art` among the boxes when the
page has them; sheaf's marks (`kind` as Op JSON's mark kinds) and other
tools' comments (links aren't), page by page in `/Annots` order;
bookmarks in outline order; generators, one of each kind at most, by
kind: `background`, `header-footer` or `watermark`, over `all` pages
(those added later too) or positions; the form's terminal fields and
their widgets, its XFA part (`none`, `static`, or `dynamic`: the XFA is
the form) and the file's signatures, and how many of them no longer
match the bytes they signed (`signatures_broken`).

The examples below run in CI (sheaf-cli's `docs_json_examples_run`), as
a dry run and saved, on a binder of four pages labelled i, ii, 1 and 2,
bookmarked "Part I" (over "Ch. 1") and "Part II", with another tool's
note `n1` on page 1 and highlight `h1` on page 3, and
`--with appendix=appendix.pdf --with logo=logo.png`.

Tidy a scanned binder by position: once page 2 is gone, `{"pos": 2}` is
what was page 3.

<!-- sheaf-edit example -->
```json
{"v": "1.0", "label": "Tidy the scans", "ops": [
  {"op": "delete-pages", "pages": {"positions": "2"}},
  {"op": "rotate-pages", "pages": {"ids": [{"pos": 2}]}, "by": 90}
]}
```

Bookmark a schedule under "Part II", and color the bookmark the first op
made, by its local:

<!-- sheaf-edit example -->
```json
{"v": "1.0", "label": "Bookmark the schedule", "ops": [
  {"op": "add-bookmark", "parent": {"path": ["Part II"]}, "at": "last",
   "title": "Schedule B", "target": {"pos": 4}, "local": "$1"},
  {"op": "patch-bookmarks", "bookmarks": ["$1"], "patch": {"color": [200, 0, 0]}},
  {"op": "patch-bookmarks", "bookmarks": [{"path": ["Part I", "Ch. 1"]}],
   "patch": {"title": "Chapter 1"}}
]}
```

Answer a reviewer's note by its `/NM`, and make another tool's
highlight a mark to recolor it:

<!-- sheaf-edit example -->
```json
{"v": "1.0", "label": "Answer the reviewer", "ops": [
  {"op": "add-mark", "page": {"pos": 1}, "reply_to": {"nm": "n1"},
   "mark": {"kind": {"type": "reply"}, "contents": "Tied out.",
            "style": {"color": [220, 40, 40], "opacity": 1, "stroke": 1}}},
  {"op": "adopt-annotations", "annots": [{"nm": "h1"}]},
  {"op": "patch-marks", "marks": [{"nm": "h1"}], "patch": {"color": [0, 150, 0]}}
]}
```

Insert the files `--with` names: the appendix at the end, and the logo
as a background on every page:

<!-- sheaf-edit example -->
```json
{"v": "1.0", "label": "Add the appendix", "ops": [
  {"op": "insert-pages",
   "from": {"type": "file", "file": {"with": "appendix"}, "name": "appendix.pdf"}},
  {"op": "set-watermark", "z": "under", "scope": "all",
   "spec": {"content": {"type": "image", "image": {"with": "logo"},
                        "scale": 0.4, "opacity": 0.25},
            "position": "bottom-right"}}
]}
```

Produce the binder: Bates numbers in the footer, and what prints drawn
into the pages:

<!-- sheaf-edit example -->
```json
{"v": "1.0", "label": "Produce", "ops": [
  {"op": "set-header-footer", "scope": "all",
   "spec": {"texts": ["", "", "", "", "", "<<bates>>"], "size": 9, "color": [0, 0, 0],
            "margins": {"top": 36, "bottom": 36, "left": 36, "right": 36},
            "first_number": 1,
            "bates": {"prefix": "ACME-", "suffix": "", "digits": 6, "start": 1}}},
  {"op": "flatten-annotations", "pages": "all", "which": "printable"}
]}
```

### The clipboard, the lint bans and the tests

- **The clipboard** (`markup_tools::CopiedAssets`) holds what copied
  marks hold by `BlobId` (`blobs`, with `by_stem` to find them): a
  library or memory stamp's blob, an attachment's file. A stamp drawn by
  a file's form (`FormRef::Base`) or pinned is made a one-page PDF blob
  when it's copied, so the clipboard never holds a file's source alive.
  A paste interns them into the target's store before the `AddMark` is
  lowered (a target holding that stem as a file's form keeps its own), so
  a library stamp never saved pastes into another window, which saves it.
- **Lint bans** (the plan's Lint ledgers). sheaf-doc's, sheaf-ops',
  sheaf-render's and sheaf-app's `clippy.toml` disallow
  `lopdf::content::Content::{decode, encode}`,
  `lopdf::Document::{get_page_content, change_page_content,
  add_page_contents}` and `std::env::temp_dir`, each with its reason.
  Test code is allowed them (`#![cfg_attr(test, allow(
  clippy::disallowed_methods))]` at each crate root); so are
  `forms/fixture.rs`, built for other crates' tests under the `fixtures`
  feature, and the `roundtrip` example, by a module `#![allow]`, and
  each integration test that uses one (an integration test is a crate
  of its own: sheaf-ops' `append_corpus`, `blank`, `comments`, `crop`,
  `forms_open`, `gate_7k`, `progress`, `scan_blanks` and `tests/common`,
  and sheaf-render's `appended_pages`, `flatten`, `markup_appearance` and
  `ocr_layer`). Three uses outside tests carry an `#[allow]` with their
  reason: the recent list's check for sheaf's own temporary files, the
  folder an attachment the reader opens is written to (M11 slice 1
  deletes it at close), and the plugin installer's archive folder.
- **Every way out** (sheaf-ops' `tests/exports.rs`, in a child process
  whose `TMPDIR`, `TMP`, `TEMP`, `XDG_CACHE_HOME` and `HOME` are empty
  directories that must stay so, `common::own_dirs`): a filled hybrid
  1040 (a fixture without the corpus) with another file's generator's
  watermark merged into a page,
  three other tools' squares (hidden, not printed, printed) and a check
  box with no `/AS`, given a note and an unsaved Bates header, then
  written 16 ways: Save and Save As by both strategies, the recovery
  sidecar as Append sections and as Full, Clean Copy, Export Flattened,
  an extract, a split's part, Combine, print with and without markup,
  Save as PDF, a stamp made from a page and the comment summary. Each
  output's form check finds nothing the input's didn't
  (`Report::added_since`, through `common::nothing_found`); every export
  refuses the open file, the comment summary and a data file too; the
  clean copy and print without markup hold no `private::KEYS`; the
  extract draws and keeps its Bates number; flatten and print draw only
  what's shown and printed, print without markup nothing, and no
  `/AS`-less check box; an extract
  without the widget pages drops the XFA. The ignored corpus tier
  `export_round_trips` (`SHEAF_EXPORT_ONLY`) extracts each corpus form's
  first page, clean-copies and flattens it, with the same checks; its one
  allowed failure is a delete's (a radio group whose chosen button's page
  goes keeps its `/V`, forms/safedocs/0070616.pdf).
- **The command line's goldens** (sheaf-cli's `tests/golden.rs`, 18 of
  them, `SHEAF_GOLDEN_UPDATE=1` rewrites them): exit codes 0, 1, 2, 3 and
  130 (Ctrl-C after `editing…`, Unix only), each addressing, `--dry-run`,
  a signature ask, `--append` keeping a signature, `--force`, `inspect`
  and `--schema`'s head. `tests/edit.rs` runs this section's examples,
  appends to six signed files of the corpus's `signatures/`
  (`Corpus::signatures()`: a byte prefix, the same `/ByteRange`s over the
  same bytes, the signatures read back the same), and runs every
  subcommand that writes with the temp and cache directories empty.

**Deleted:** `save::{subset, paired, form_on, prune_outline, print_pdf,
extract_pages, extract_bytes, export_clean_copy, export_flattened,
flatten_annots, normal_appearance, strip_markup, strip_markup_annots,
is_link_or_field, without_markup, EXPORT, CLEAN, written, is_the_file}`
(`save::rewrite` and `rewrite_to_path` are an `Export::whole`, for tests);
`materialize::{PostPass, materialize_then, save}`;
`generated.rs` whole, `images::{PageFit, files_to_pdf}`,
`model::held_stamp` and `find_stamp`'s cache arm, combine's
`page_source`, the app's blank and image cache writes and the stamps
panel's; `model::LEGACY_KEYS` and `RemovedKind::OlderSettings` (now
`private::LIST_KEYS` and `PrivateData`); the old flatten's writer;
`scan::add_text` and its throwaway document, blank.rs's and comments
import's hand-rolled open and save, Combine's `open::start` and its
write and reopen, `comments::summary_pdf`; the app's
`ask_before_saving`, `export_flattened_to`, `export_flattened_prompt`,
`export_clean_copy_prompt` and `PrintContext::snapshot`.
`tests/retired.rs` lists them.

Outside tests, lopdf's writer wrote only a tape's picture
(`annot::tape_pdf`) until slice 20, which builds it with
`serialize::built` and writes it with `write_built`; `ResObjects` and
`WriteCx` moved there, for the PDFs sheaf builds in memory (the
summary's pages, an exchange's marks, a pinned mark's one-page PDF).

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness, each
measurement in a process of its own on a quiet machine):

| Row | Synthetic | Heavy | Scanned |
|---|---|---|---|
| Export Flattened (`FlattenAnnotations { All }`), median of five processes: time; peak RSS; RSS added | 467 ms; 88.3 MB; 50.6 MB | 1.29 s; 514 MB; 293 MB | 521 ms; 1,076 MB; 553 MB |
| the open in that process, before it: peak RSS | 37.9 MB | 221 MB | 523 MB |
| a Rewrite of the file as opened, against slice 8's writer (asserted ≤ 1.3×, flagged past 1.2×) | 146 ms, 1.09× (1.08–1.12× over five) | 768 ms, 1.09× (1.06–1.12× over five) | |

The heavy's flattened export can't meet its gate, a peak of at most
1.5× the file + 200 MB (220.5 MB) in its own process: opening the heavy
alone peaks there, and the export's Rewrite loads the primary eagerly to
copy it (a plain Rewrite of it adds 252 MB). So the row is measured, and
asserted only with `SHEAF_GATE_FLATTEN`, for Greg to rule on (the plan's
ledger has the row). Slice 2's flatten, which kept the heavy's 7,000
widgets, took 3.49 s and peaked at 420 MB. Every other gate holds where
slice 18 left it, proposal 8's accepted misses included.

## Plugin API 0.4 (M10.5 slice 20)

A plugin speaks one world, `sheaf:plugin@0.4.0` (`wit/sheaf.wit`): it
reads the document in PageSpace, a page's `to-view` and `from-view` maps
beside it, and edits with sheaf's own ops, Op JSON in the envelope,
lowered at once into the call's one transaction, which lands when the
call returns (or when it commits) as one undo step, or is taken back.
What it reads is what it decides by; it never learns the user's name
unless the user lets it; it never names a blob by id, and uploads what
its edits hold. WIT 0.2 and 0.3, their adapter and their fixture are gone
with no shim. The PDFs sheaf makes in memory have one builder,
`serialize::built`, and model-level code names no lopdf `ObjectId`
(§2 rule 7, §6.3, §6.6 to §6.8, §8.4, §8.5, §14.1, §14.3, proposals 11,
18, 19 and 20 and bugs 27 and 30 of `plan/complete/m10.5-edit-api/README.md`).

```rust
pub struct Published { pub snap: Snapshot, lowering: Lowering }      // sheaf_doc
impl Document { pub fn published(&self) -> Published; pub fn instance(&self) -> u64; }
impl Published { pub fn plugin_tx(&self, id: Arc<str>, version: Arc<str>, identity: bool) -> Tx;
                 pub fn store(&self) -> &Arc<Store>; pub fn is_user(&self, author: &str) -> bool; }
impl Tx { pub fn apply_said(&mut self, ops: Vec<Op>, p) -> Result<Preview, EditError>;
          pub fn preview_batch(&mut self, ops: Vec<Op>, p) -> Result<Preview, EditError>;
          pub fn mark_handed_out(&mut self); pub fn base(&self) -> VersionId;
          pub fn is_empty(&self) -> bool; }
pub struct CommitOpts { .., pub depth: u8 }   pub struct ChangeSet { .., pub depth: u8 }
pub struct LowerCx { .., pub identity: bool }
pub struct Applied { created, locals: BTreeMap<Local, Made>, lines }  // sheaf_edit::reply
pub struct Previewed { lines, created, locals, caps: Vec<Cap>, removes: bool }
pub struct Failure { kind: FailureKind, message: String }    pub enum Cap { Plugin, Approval, User }
pub trait Door { fn model(&mut self) -> &mut Document; fn commit(..); fn follow(..); }  // sheaf_plugin
pub trait Committer: Send + Sync { fn commit(&self, tx: Tx, label: &str, depth: u8)
                                       -> Result<Committed, EditError>; }
pub struct Landing { .. }   // commit, finish, end_group
pub struct Landed { pub failed: Option<(String, EditError)>, pub kept: bool, pub refused: Vec<String> }
pub fn follow_ups(landed: &Landed) -> Vec<DocEvent>;
pub struct ChangeInfo { version, parent, instance, cause, actor_kind, scopes: Scopes, depth, undone }
pub enum DocEvent { .., Changed(ChangeInfo), EditRefused(String) }
pub struct PageObj { pub blob: BlobId, pub id: ObjectId }  // sheaf_doc::source; Source::page_obj
pub struct Built { .. }    // sheaf_doc::serialize::built: new, pdf, page, form_page, finish
```

### One world

`sheaf-plugin` has one `bindgen!` and `sheaf-plugin-api` one
`generate!`, both of `wit/sheaf.wit`, the only file under `wit/`
(sheaf-plugin's `tests/one_world.rs` counts them). The host is split by
interface (`api/{convert, document, text, render, edits, assets, ui,
progress, storage}.rs`). A manifest names the API it was built for
(`api = "^0.4"`), which must match `API_VERSION` exactly to the minor
until sheaf's first release (`API_PREVIOUS` and the N-1 policy went),
and the version of Op JSON its edits are written in (`edit-schema =
"1.0"`): optional, since a plugin that only reads has no edits, and one
that leaves it out has every `submit` answered `invalid`; a newer minor
than sheaf reads is refused when the manifest is read ("built for edits
1.1; this sheaf reads up to 1.0"), so `discover` lists it as an error and
no plugin is made. `permissions.identity` is the one new permission
(below).

The SDK, `sheaf-plugin-api` 0.4.0, re-exports `sheaf_edit` (`Op`,
`Applied`, `Previewed`, `Failure` and everything an op holds) and adds
`Ops` (a builder: `push`, `bookmark`, `bookmark_in`, `mark`, `stamp`,
`page_stamp`, `connector`, `delete_pages`, `to_json`, `submit`,
`preview`; and `commit`, `upload`, `style`, `page`, `at`, `BLUE`) and
`view` (`to_view`, `from_view`, `box_to_view`, `box_from_view`,
`dest_from_view`, one axis at a time as `Affine::apply_axes` maps,
`shown_size`). `Batch` went. The six bundled plugins are ported, at
0.3.0 (tax-form-bookmarks appends its bookmarks; bookmark-from-pattern
names a new heading by a local; amount-crossref and duplicate-pages key
what waits between calls by path and `version.instance`; subtotal forgets
a page's numbers when `scopes.content` names it, and all of them on a
change to generators, order or labels, since 0.4's text leaves
annotations out).

### The call's transaction

The app publishes, at every change and when who the user is changes,
`Document::published()` into the view's cell (`Doc.plugin_cell`, an
`ArcSwap<Published>`): the snapshot and what lowering needs (the store,
the allocator, the identity, the file's name, whether the file is
ready). A call makes its transaction at its first read or submit
(`HostState::tx`) with `Published::plugin_tx(id, version, identity)`,
which binds `Actor::Plugin` alone; `Document::lower_cx` binds every
other actor, and never grants identity. So a call that waited in the
queue, or on a form, reads the document as it is by then, and the
transaction is on that version (`Tx::base`).

- **`submit(label, ops)`** parses the envelope (`sheaf_edit::Envelope`:
  a `label` of its own, another `v` than the manifest's `edit-schema`, or
  no `edit-schema`, is `Failure { invalid }`), lowers its ops with
  `Tx::apply_said` (all or none, on the plugin's thread), marks the
  transaction's ids handed out (`Tx::mark_handed_out`) and answers
  `Applied` as JSON: each op's `created` ids, the `locals` it named and
  its `lines`. The first submit's label names the call's edits.
- **`preview(ops)`** is `Tx::preview_batch`: a checkpoint, `apply_said`,
  and a rewind, answering `Previewed` (the batch's lines, ids, locals,
  each op's `Cap`, and `removes`, its `Flags::REMOVES`). The ids it drew
  stay drawn. Since `Tx::allowed` refuses an op the actor may not make, a
  plugin's preview of one fails as its submit would.
- **Failures** are `Failure::from(&EditError)`, whose `FailureKind`
  mirrors `EditError`'s variants. `Made`, the reply types and `Cap` (with
  its Display words) live in sheaf-edit (`reply.rs`), re-exported at
  `sheaf_doc::tx`, so a reply names a cap as sheaf does; ops-1.0.json is
  unchanged, since none of them is in the envelope.
- **Reads** go through the transaction (`tx.reader()`, `read_render`),
  so each is a decision read of the call's (§6.3), and they read the
  transaction's own state (`Snapshot { state: tx.state().clone(), .. }`
  for text and renders, keyed by render key, not version): a call sees
  its own edits, and `Reader::note` records no read of a slot the
  transaction wrote, so its own edits never refuse it.
- **The end of a call** lands what's left (`CallOutput.tx`, none when it
  applied nothing) through `sheaf_plugin::Landing`: `CommitOpts::based(
  tx.base(), Stale::Relower, label)` in a `GroupKind::PluginCall` group
  of the call's, at the call's depth. A relowered transaction whose ids
  were handed out must make what it made: `Document::relower` compares
  the original `created` (each op's ids and the locals) with the new,
  and a difference is a `Conflict` over the first lowering read whose
  value changed (`by` the edit that changed it), or, when none did, over
  the first slot the new steps wrote (`by: None`). So a user's edit
  elsewhere lands beside the call's, its marks keeping the ids `submit`
  gave, and an edit to what the call read, or one that makes its ops
  make something else, refuses it.
- **`commit()`** lands the call's edits so far now, through
  `DocContext.committer`. The app's `AppCommitter` sends a
  `CommitRequest { tx, label, depth, reply }` to the call's task on the
  UI thread, which lands it (`CommitRequest::land`, `DocView::commit_call`,
  only while the view shows the call's cell) through the call's own
  `Landing`, so a call's commits and its end are one group; the plugin's
  thread waits on the reply, and the time is credited to its deadline.
  The reply carries the version, the instance and the cell `on_change`
  published, which the call's next transaction is made on, its uploads
  granted again. A closed view or a dropped channel answers
  `EditError::Invalid { op: "commit", why: "its document was closed" }`.
  The UI thread never waits on a plugin's.
- **Rollback.** `Landing::finish` ends the group when the call returned
  `Ok` and its last commit landed. An `Err`, a trap, a stop (a stopped
  call that returned `Ok` too: its edits are dropped, "… was stopped, so
  it changed nothing") or a last commit that fails drops what's
  uncommitted and takes back what the call committed with
  `Document::rollback_group`: its only version truncated, with no redo
  entry, or, when the user edited in between, a version that reverts the
  call's alone. When the user has since changed what it committed, its
  edits stay, and the notice says so ("… its partial edits were kept").

### What a plugin may do, and see

**Caps** are per resolved target (slice 11's `cap_of` and
`actor_allows`); 0.4 adds no row and grants nothing beyond `Cap::Plugin`.
A plugin's `InsertPages` and `ReplacePages` are `Refused(User)`, deleting
another tool's comment and `FlattenAnnotations` are `Refused(Approval)`,
and a raw `{"blob": …}` is `Refused(User)` (resolve.rs). There is no
clipboard or export function in 0.4: exports stay out of plugins' reach
(D46), a `Transform` is outside the envelope, and `actor_exports`' plugin
arm has no WIT producer.

**Reads in PageSpace.** `page-info` gives each page's id, boxes,
`rotate` as it shows, its label and its `to-view` and `from-view`
matrices; `pages()` reads the order, each page's `Page` and `Geom`, and
`Labels`. `annotations(p)` lists sheaf's marks whole and other tools'
comments as read-only summaries (`comment-summary`), never a widget, a
link or an annotation the file hides. Text is the page as the file will
draw it now and leaves fields out (`TextOpts` has no `fields` until M10
slice 14, and PDFium never reads a widget's value as text). A render
draws the page as it shows (`RenderSource::render(&ViewInputs)`,
`render_delta`), its `to-page` and `region` in PageSpace; `without-markup`
is `ViewInputs::print(false)`, `plugin()` with no marks and no other
tool's comments, form fields still drawn. `ocr.recognize` gives PageSpace
words; 0.2's `page-text-layer` went with 0.2.

**Who made what** (§6.8). Authors are roles (`Roles::of`): `you`,
`plugin <id>` for an installed plugin and `other`. A plugin's marks are
authored by its id (§6.8 says its name; the id is stable, is what
History shows, and matches the role), and `<<name>>` and `<<initials>>`
in them stay literal (`<<date>>` and `<<time>>` are filled). With the
`identity` permission (the consent prompt's "Know your name",
`Grants::identity`), `LowerCx.identity` fills them, and authors are told
by name, an installed plugin's still as `plugin <id>`.

**Assets.** `assets.upload(bytes, kind)` interns into the document's
store (`Published::store`) as a paste interns the clipboard's blobs: an
image as a one-page stamp PDF (`stamp_lib::image_stamp_bytes`), a PDF
with pages as a stamp's page, a file as an attachment. It grants it in
the call's transaction (`Tx::grant`, `BlobSel::Asset(n)`, a
`Grant::Stamp` or `Grant::Blob`), pins it (`Tx::pin`), and grants it
again, in order, in each transaction a commit starts, so a handle holds
until the call returns. A call's uploads hold at most 64 MiB
(`MAX_UPLOAD_BYTES`), an image's pixels counted at four bytes each:
`image_stamp_bytes` reads the header, refuses what passes the budget
left, and decodes under `image::Limits` of those dimensions, since a
host call runs past the plugin's memory limit and deadline.

### Events and depth

A change reaches a plugin as `changed(change-info)`: `ChangeInfo::of(
&ChangeSet, instance)`, its cause, its actor's kind (user, plugin, job,
mixed; never a name), `parent` and `version`, and its scopes (`order`;
the pages whose `geometry`, `content` or `annots` changed, pages added
and removed among the content; `outline`, `labels`, `form`, and
`generators`, which no page set says). `edited` and its label went.

- **Merged while busy.** `PluginHost.changes`, a `Coalescer` keyed by
  (plugin, view), queues what waits and merges a change into the last
  waiting with `ChangeInfo::merge` (the earliest parent, the latest
  version, the union of scopes, `Coalesced` and `Mixed` where they
  differ) only when the two are alike to answer
  (`ChangeInfo::answerable`: depth 0 and not undone), else queues it
  after, so a merged change is refused an answer only when each of its
  parts would be; the unanswerable keep the deeper depth and undone if
  either. It sends the first waiting when the plugin's call ends
  (`queue_done`); a change of another instance drops what waited for
  the old one. Regions stay latest-wins per plugin in the same kind of
  queue (`Latest` and `region_done` went).
- **No echoes.** `recipients(plugins, kind, from)` leaves out the
  plugin whose call made the change.
- **Depth.** `ChangeSet.depth` is its commit's (`CommitOpts.depth`; an
  amend and a commit that changed nothing carry it; every other cause is
  0). An answer to a `changed` is one deeper (`runtime::answering`), and
  refused when the change was already an answer ("it answers a plugin's
  answer to a change, and those stop there"), so the user's edits and a
  command's are answered once and the answer isn't; or when it was an
  undo, redo, jump, rollback or revert (`ChangeInfo.undone`: "the user
  undid this"), so a plugin can't fight undo. A refused call's `submit`,
  `preview` and `commit` all answer `Failure { refused }`.
- **`edit-refused`.** `follow_ups(&Landed)` makes an
  `EditRefused(why)` of each refusal and of a last commit that failed,
  and the app sends them only to a plugin that subscribes
  (`follow_up_calls`). Edits answering one are refused ("it answers a
  refusal of its edits") and not told again, which closes a two-step
  fight with undo.
- **`opened`** comes once the file is ready, as before (`plugins::
  Waiting`, `release_plugin_calls`).

### PDFs made in memory

`content/to_lopdf.rs` is gone, and `serialize/built.rs` is the permanent
builder of every PDF sheaf makes in memory: `ResObjects` (one object a
resource a document, bound through `content::sink::bind`), `WriteCx`,
`StampForm` (with `resource()`) and `Held` (the marks as the writer
makes them, moved from annot.rs), the tests' `Written`, `to_pdf`,
`to_pdf_in` and `write`, a private `Graph` sink in place of
`sink::Lopdf`, `as_form` in place of `Appearance::write`, and `Built`,
the page builder (`new`, which numbers the page tree first, or `From<
lopdf::Document>` for a graph already parsed; `pdf()`, `page(media,
content, needs)`, `form_page(StampForm)` drawing a form as `/Stamp`, and
`finish()`, written by `write_built`). annot.rs builds dictionaries and
content alone: `emission` takes the digest of what it wrote from direct
values (`written_staleness`, `NoObjects` gone); `tape_pdf`'s picture is
built with `Built` and written by `write_built`, not lopdf's `save_to`;
`pinned_pdf` draws through `WriteCx::stamp` and `Built::form_page`; and
`WriteCx::stamp` takes a copied form's frame from its source, a pinned
one's from its appearance, a blob's from `page_form`, never reading back
the graph it built. lopdf's writer writes nothing outside tests.

- **Exchange's comments are a source.** `Comments { src, annots, file }`:
  `Comments::built(Built, annots, file)` writes the FDF or XFDF graph as
  a file of no pages and opens it as a memory source
  (`MemoryKind::Comments`, which `Source::open` lets have none, as
  Blank), and `from_pdf` opens another PDF lazily, decrypted with its
  password (`BlobKind::comments(password)`). `gather` builds through
  `WriteCx` into a `Built` too, and `fdf::write` copies each comment out
  of the source (`copy_object`), so an FDF holds only what its comments
  reach.
- **Page forms come from a source.** `stamp_lib::page_forms(&Src,
  &[u32], dst)` copies each page's content, as one stream, and its own or
  inherited resources (`remap`), where it trimmed a clone of an eager
  document and copied every other page key as orphans. The summary opens
  its printed bytes as a `Memory(Summary)` source and builds its sheets
  with `Built`; `form_to_pdf` builds through `Built` and writes the
  bytes it wrote; `Blob::source(&Limits)` opens a stamp's or a
  watermark's blob where no store is at hand (`Store::source` calls it).
  `copy_page_dict` moved from `materialize::copy` to stamp_lib, its only
  user.
- **The `Objects` trait stays** for what still loads a file whole: a
  Rewrite's graph (`materialize::record`'s `RootValues::of` and
  `TreeShape::of`, `keep_only`'s `is_page_node`; backlog F15), the form
  check's file and revisions (`forms::check`), the stamp library's
  collections and the user's PDF a stamp is made from (proposal 29), and
  tests. Every file a document reads, and every PDF sheaf builds in
  memory, is read as a source.

### No lopdf ids in the model

Rule 7's "no id is an ObjectId" is held by sheaf-doc's
`tests/object_ids.rs`: a `syn` walk from lib.rs that follows `mod`
declarations as the compiler does, leaving out what's built only for
tests or the `fixtures` feature, and fails on any path or type with
`ObjectId` as a segment, a `use` of it (renamed too) and the name in a
macro's tokens, outside an allowlist with a reason for each entry. It
sees through type aliases: an alias that names `ObjectId`, in any file,
or names such an alias, fails as `ObjectId` does. The
allowlist is the plan's directories (`source/`, `materialize/`,
`serialize/`, `content/`, `forms/`) and the file-level readers that read
a file's objects by number: the open's fact readers (`open/annots.rs`,
`open/outline.rs`, `attachments.rs`, `labels.rs`, `layers.rs`,
`portfolio.rs`, `properties.rs`, `file.rs`, `import.rs`), `realize/`'s
`alloc.rs`, `assemble.rs` and `untouched.rs`, `private.rs`,
`stamp_lib.rs`, `exchange/mod.rs` and `generate/saved.rs`. Four more
tests check that every `.rs` file is walked or built only for tests,
that each entry still names `ObjectId` and none covers `model.rs`,
`model/`, `tx/` or annot.rs, and, on probe crates in memory, that a hit
is caught where it should be and nowhere else, an alias's included. To
pass it:

- **`source::PageObj { blob, id }`** (`Source::page_obj(index)`) keys a
  page's object: `DocState::page_showing(PageObj, copy)`,
  `page_named` and `pages_showing`, and the save's maps
  (`emit::written_pages`, `copy::Pages::Written`, `copy::page_named`,
  deps.rs's `Written`), in the order they had.
- **`open::FlatOutlineItem`** moved from model.rs (still
  `sheaf_doc::FlatOutlineItem`): cached in the `Source`, it can't hold an
  `ObjRef`, which would hold its source. `holding` and `file_in` take
  `ObjRef`s.
- **`source::appearance`** (`Shows::read`, `Shows::in_slot`,
  `Shows::form`, `frame`) is what a flatten reads of an annotation, moved
  from tx/flatten.rs, which keeps `placed` and imports nothing from
  lopdf.
- **`generate::saved`** holds the reader of a file's saved generators
  (`Saved`, `read`, `Block`, `rebind`), moved as it was, re-exported at
  `generate::{Saved, read}`; generate.rs itself is checked.
- **`materialize::kept`** is `keep_only`, moved from the crate root.

`layers::LayerId` was an alias of `ObjectId` (`ViewInputs::off`, the
layers panel), which the walk couldn't see through. Since the
backlog-fixes PR (B21) it's sheaf's own, a type over the group's object
that only layers.rs reads, and the walk follows aliases.

### Docs and tests

`docs/plugins.md` is rewritten for 0.4: PageSpace and the maps, reads as
decision reads, marks and summaries, roles and identity, one transaction
per call, "What a plugin may do" (the `Cap::Plugin` ops, and what's
refused), assets, events and answers, and the edits schema, linking
`crates/sheaf-edit/schema/ops-1.0.json` and `sheaf edit --schema`. Its
three `<!-- plugin-ops example -->` blocks are run by sheaf-plugin's
`tests/docs_examples.rs` (`EXAMPLES = 3`): parsed as an envelope with no
label at `sheaf_edit::VERSION`, lowered through `plugin_tx`,
`apply_said` and `mark_handed_out` on four blank pages, and committed
with `Relower`. host.rs's `Shared` harness (the document and its
`Landing` behind a mutex, the cell, a `Committer`) and `FakeUi::editing`
(a user's edit committed and published while a form is open) run the
fixture's commands: a thread named by a local, an envelope with a label,
traps after two submits, after a commit and after a user's edit, the
tape a total read changed meanwhile, an edit beside one, the refusals,
the roles, an uploaded stamp after a commit, reads of the call's own
edits, answers to answers, an answer to an undo, and time spent waiting
on a commit.

**Deleted:** `wit/0.2`; sheaf-plugin's `api::{v02, v03, shared}`
(`Adapter`, `Said`, `More`, `page_frame`, `to_frame`, `dest_from_wit`,
`rotation_from_wit`, `pt_from_wit`, `rgb_from_wit`), `bindings_v02` and
`bindings_v03`, `API_PREVIOUS`, `runtime::{World, EditBatch, Held}`,
`CallOutput::{batches, reads, base}` and `HostState::{batches, reads,
read, page_base}`, the 0.2 fixture and its tests,
`DocEvent::Edited` and `EventKind::Edited`; sheaf-plugin-api's
`batch.rs` (`Batch`, `annot`, `at`, `BLUE`; `at` and `BLUE` are `Ops`'
now); the app's `plugins::{land, Landed, edited_label, Latest,
region_done}`; `content::{to_lopdf, ResObjects}`, `content::sink::
Lopdf`, `annot::{WriteCx, StampForm, Held, Written, to_pdf, to_pdf_in,
NoObjects}` and `Appearance::write` (moved or replaced in
`serialize::built`), `materialize::copy::copy_page_dict`,
`exchange::Comments::{pdf, new}`; `model::FlatOutlineItem` and
`tx::flatten::{Shows, read, shown, frame, unkept}` (moved). Slice 11
had retired `edit_from_wit`, `sign_marks` and `keep_unseen`, and slice
12 docview's `edit_source` and `DocContext`'s per-call snapshot.
`tests/retired.rs` lists them.

**Numbers** (AMD EPYC 7302P, release; the 7k gate harness on a quiet
machine). What the app publishes at every change, `Document::published()`,
takes 0.36–0.39 µs on the synthetic, heavy and scanned 7k (a snapshot
0.24–0.28 µs), asserted under the snapshot's 50 µs. The slice's paths
reach no other gate, and every one holds where slice 19 left it: the
geometry walk 50.0 and 77.2 ms, ready 379 ms on the heavy, realize +
open + render an edited page 12.0 and 21.5 ms, the thumbnail strip 425
and 352 pages/s, the scanned's one-mark Append 30.0 ms, and the Rewrite
1.09× and 1.12× slice 8's writer.

## Non-goals of this design

- Collaborative / concurrent editing. Single writer, single history.
- Editing page *content* in place (M-later). When it comes, a page's edited
  content stream becomes another per-`PageId` persistent map entry and
  rendering of that page goes through a scratch PDF; the history model is
  unchanged.
- Making lopdf's object graph the live model. It stays what sources are
  read through and the objects sheaf's serializer writes; a Rewrite's
  graph is the one document still loaded whole (backlog F15).

## Decisions (2026-10-11)

- **Persistent maps use the `im` crate** from the start.
- **History trimming is a policy trait**, composable:

  ```rust
  pub trait HistoryPolicy {
      /// Whether it might act on this many snapshots and bytes: when it
      /// can't, no stats are worked out (M10.5 slice 12).
      fn could_act(&self, snapshots: usize, bytes: usize) -> bool { true }
      /// How many of the oldest snapshots to drop, given current stats.
      fn trim(&self, stats: &HistoryStats) -> usize;
      /// Whether to prune a version first (M10.5 slice 12).
      fn prunes(&self, stats: &HistoryStats) -> bool { false }
  }
  pub struct FixedCount(pub usize);
  pub struct MemoryBudget(pub usize);
  pub struct AllOf(pub Vec<Box<dyn HistoryPolicy>>); // drop until all are satisfied
  ```

  `HistoryStats` reports what each kept snapshot holds that no other
  version does (`snapshot_bytes`: the `im` nodes it copied for the slots
  its version changed), what all versions' steps hold (`step_bytes`: their
  ops, lines, effect sets, cascades and prims' values, each counted once,
  by the version that made it, and an `Arc` once however many of its
  prims put it), the bytes
  of every blob the store holds, each once (`blob_bytes`), and those of
  the blobs only versions off the current line bring
  (`undone_source_bytes`: an inserted scan that was then undone, from
  the blobs each version noted as it was made). Steps name sources by
  blob, so source bytes are never step bytes.
  Snapshot and step bytes are computed when a version is made, from its
  prims and the sizes of the maps they changed. They are estimates, and
  one is coarse: the page order, the outline's top level and the form (its
  top-level fields and its instances' names) are each one value, and a
  prim that puts one counts all of it, though `im` shares all but the
  changed path with the version before (about 56 KB for any change to the
  order of 7,000 pages). So in a long session of such edits the steps
  alone can pass `MemoryBudget`, which then drops every snapshot it may,
  and undo replays from the nearest version that kept one. Every other
  record is its own slot, counted once by pointer, with the path its map
  copied (942 step bytes to rename one bookmark of 2,000, against 375 KB
  before M10.5 slice 8). A value records share is counted once a version,
  so a duplicate's page shares its original's content stack and its
  fields their props without counting them twice there, but a version
  that copies a page an earlier one recognized counts the stack again.
  Trimming frees snapshot bytes only; a version's step bytes, and the
  sources it holds, go when it is pruned (past `MAX_VERSIONS`, and from
  M10.5 slice 12 when `MemoryBudget` says: when what dropping every
  snapshot it may would leave, `untrimmable_bytes`, is over it, versions
  are pruned one at a time, weighed again after each, before snapshots
  are dropped), and a blob goes with the last record holding it (an
  engine the pool opened on it closes, and lets its bytes go, at the
  pool's next turn: M10.5 slice 13). A policy weighs undone sources but not `blob_bytes`,
  most of which is the files the document shows, which no pruning frees
  (slice 12's plan said `step_bytes + blob_bytes`, which would have
  pruned every version of a document opened on a 500 MB scan); so a
  large undone insert has its branch pruned, the oldest first, and its
  bytes freed, when it puts history over the budget.
  Default: `AllOf([FixedCount(200), MemoryBudget(256 MB)])`.
- **Deleting pages cascades to bookmarks that target them**, with a
  non-modal notice ("Deleted 3 pages and 2 bookmarks — Undo"). No modal;
  snapshot undo makes the cascade zero-risk. Rationale: for both "delete a
  section" and "delete a bookmarked page", a retargeted or orphaned
  bookmark would be misleading.
- **Replace Pages is its own edit** that keeps the `PageId` (and so every
  bookmark/link to the slot). This is the workflow where people actually
  want bookmarks preserved, and no automatic retargeting rule gets it right.
- **Set Destination** on a bookmark re-anchors it to the current page. We
  never create orphans ourselves, but files in the wild have dangling
  bookmarks (the outline panel already greys them), so they need a fix path.
- Thumbnails show a bookmark glyph on pages that are bookmark targets, so a
  cascade is visible before the delete.
- A page's rotation is its `/Rotate`, **absolute**, as a save
  writes it (M10.5 slice 5; `Geom.rotate` from slice 8, a page's `Geom`
  its own record): a page starts with its source page's own, and
  a save keeps it. A render takes `Document::render_delta`, the turn PDFium
  adds to the source page's own `/Rotate`. It was a delta over the source
  page until then, since reading the originals up front cost a page load
  per page; the page table (M10.5 slice 3) has every page's `Geom`, its
  `/Rotate` among them, from the open, so the original is free.
- `im` is MPL-2.0 (file-scoped copyleft); fine as an unmodified dependency
  of an MIT/Apache project.

## Text layers (OCR)

Added in M5. An OCR run produces, per page, a `TextLayer`: words with boxes
in PageSpace (M10.5 slice 5; see Coordinates), like marks, the frame the
text reads upright in (`upright`, the page's absolute rotation when it was
recognized) and the engine's name. It is page content (M10.5 slice 8):
an `OcrText` layer on the page's `ContentStack`, from slice 18 with its
block's id (`Layer { id, body }`), `DocState::ocr_text(page)`, put with
the page's `PageDef`.

- **A save writes it as a block of its own** (M10.5 slice 18; OCR as
  content): invisible text (text render mode 3 in `content::Glyphless`,
  Tesseract's glyphless font embedded once a file, each UTF-16 code unit
  of a word a CID and the text read back through its ToUnicode), a
  stream marked `/SHEF_Block << /K /Ocr /B (b…) >>` after the page's
  isolated content and before the generators drawn over it, its range
  `/ShOCR` with a list naming the block, its font and list under `/Sh…`
  names. A save is an output (slice 9), so the layer stays on the stack,
  the page still showing the file it opened, and the next save writes it
  again in place of what the file holds. A file opened again holds the
  block as the page's own content, kept in its place by its marker; a
  re-OCR or a Clear takes it out (the stack's `SheafOcr` filters), so a
  layer in a file is like any other, saved or not. The model reads no layer back from a
  file: its words are the page's text, as PDFium reads it. Without
  sheaf's private data (a clean copy, print without markup; M10.5 slice
  19) the range is `/ShOCR BMC` with no list, and a file's lists lose
  `/SHEF_OcrBlock`.
- **Ops**: `SetOcrText` (one page's layer) and `ClearOcrText`, which put
  the pages' `PageDef`s and nothing else, each reading the page's blocks
  for the text its file holds (a page whose content can't be read is
  `EditError::Content`). Delete removes a page's layer with the
  page, Duplicate shares it (the same `ContentStack`, its id too),
  Replace drops it (the page shows other content, under the empty
  stack), and Move, Rotate and, from M10.5 slice 15, Crop
  (`SetPageBoxes`, the page's `Geom` alone) leave it alone.
- **Coalesced undo.** Results arrive in flushes over minutes (every 10
  pages or 2 s), but a run is one undo step: from M10.5 slice 12 it's a
  group (`Document::begin_group(GroupKind::Ocr)`, which takes commits of
  `Actor::Job(JobKind::Ocr)` only, which may only set and clear OCR
  text; see One funnel and the history budget). Each flush is a
  transaction made on the version its round began on (the run's first
  version, or a later round's, below), whose snapshot its pages are
  drawn from (`ViewPages`, their content alone, slice 13),
  that reads each page as OCR read it (`Tx::read_render(p,
  &ViewInputs::content_only())`: what the page shows, from M10.5 slice
  13) and lands by value (`Stale::Validate`) in the group: it amends the
  group's version when that's the current one, has nothing after it, was
  never saved and isn't being saved (sealed, see Saving), and pushes a
  version "… (continued)" otherwise. An amend coalesces the run's prims
  and replaces the version with a new `VersionId` under the same parent
  (an id names one state), and a run whose flushes put back all they did
  leaves no version. A save or an unrelated edit mid-run splits the run
  into fragments, which History's "Undo all of …" takes back at once
  (`revert_group`); a flush during a save lands in a version after the
  sealed one, which stays unsaved. A page whose slot changed since (a
  ReplacePages, or another change that writes `Page(p)`; a header strip,
  a watermark and a crop re-pointed its base too until slices 17, 16 and
  15 made them generators and geometry, which a flush lands beside)
  is a Conflict: the flush is retried page by page, the other pages keep
  their text, and from slice 18 the changed page is read again, as it
  shows then, in a round of the run that begins when the running one
  ends, in the run's group: a new job over the pages changed since,
  still in the document, made on the current version (twice at most a
  page; after that, and for one gone or a run stopped first, the run's
  message counts it as changed). A page whose content can't be read
  (`EditError::Content`, slice 18) takes no text: the page-by-page retry
  counts it as failed, and the rest land. Undoing the
  run, or jumping away from it, makes its next flush `GroupEnded`: the
  run stops ("OCR stopped: its results were undone") rather than put
  undone results back.
- **Search.** The stack's layer is drawn into its page's realization,
  invisible, as a save writes it, so Find, Patterns, selection, plugins
  and blank detection read its words from the page as they read any text
  (M10.5 slice 14's text service, under inputs with `ocr_over`; until
  then a search of the layers merged beside PDFium's), between the page's
  own text and a header (slice 18). Hits are addressed by `PageId` and
  position, so inserted and reordered pages work too.

## Open questions

- None blocking slice 1.
