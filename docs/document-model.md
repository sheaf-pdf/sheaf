# Document model and edit history

Status: accepted 2026-10-11. Decides the shape of `sheaf-doc` for M2+.

## The problem

Three things want to look at "the document" and they disagree about what it is:

- **PDFium** (rendering, text, search) sees a *file*: bytes it parsed once.
  Reparsing is cheap for small files and ~100 ms for a 7,000-page one, so we
  don't want every edit to reserialize.
- **lopdf** (writing) sees an *object graph*: dictionaries and streams with
  numbers. Mutating it is how a PDF gets written, but it is a terrible
  structure to edit interactively — nothing has stable identity, and inverse
  operations are hard to get right.
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
                       kind: Pdf { password, origin: File(path) | Memory(Derived) } }

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

Generator            { id: GenId, spec: HeaderFooter(Arc<HeaderFooter>) }
                     what sheaf draws on pages from one definition: until
                     M10.5 slice 16, only the header and footer, read at open
                     from /SheafHeaderFooter, with its own page scope and its
                     Bates numbers counted at each Rewrite, as before.

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
                       generators: im::OrdMap<GenId, Arc<Generator>> }  // the header/footer
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
                       derived: Arc<DerivedCopies>,   // derived copies' origins,
                                                      // outside undo (until slice 17)
                       versions: HashMap<VersionId, Version>,  // a tree (see Undo/redo)
                       cursor: VersionId,            // which version is current
                       saved: Option<VersionId> }    // which version is on disk, its
                                                     // record the file's (a restored
                                                     // document's with the sidecar's
                                                     // sections pending)

Snapshot             { version: VersionId, state: DocState, primary: Src, name,
                       derived, file: Option<Arc<Path>> }  // the document's file
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
  (`RenderHandle::open_bytes`). A derived copy (cropped, watermarked, a
  header and footer stripped) is a memory blob, written nowhere; blank and
  image pages, and a library stamp nothing else holds (see Page content),
  are read back from the generated cache once, into blobs.
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

## Opening a file (M10.5 slice 4)

A file opens once, the same way in the app, on the command line and in
every other headless caller (`sheaf_doc::open`; `open::file` takes the
three steps in a row):

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
  another tool has edited since sheaf wrote it, whose `/SheafAnnot` digest
  no longer matches, is that tool's comment; copies of marks another program
  made, which say ids earlier marks have, get new ones; other tools' get ids
  no mark has, drawn from an rng the file's hash seeds, so the same bytes
  open with the same ids), the form (`Source::form`, also read once a
  source: every node of the field tree with the object it was read as, each
  terminal's widgets by page, and those no page lists, a page template's),
  the page labels, the header and footer sheaf drew (its settings in
  `/SheafHeaderFooter`, read as written: no page's content is lexed to find
  it), attachments, the portfolio schema, layers, and the outline with its
  colors, styles and closed state (`/C`, `/F`, a negative `/Count`), each
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
  given new ids, whose pages the next save writes. A second call is
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
  an eager document (a stamp collection's, a derived copy's) from its
  dictionary and what it inherits; no other conversion is left.
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
  mark made on a page as it shows reaches the model (the plugin adapter's
  too); `Annot::to_frame` is its inverse, what a drag moves, a handle
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
- **A crop moves nothing.** A crop still shows the page from a derived
  copy, until slice 15 makes it a box edit, and the copy only sets
  `/CropBox`, the kept area as shown mapped through `from_view`; it keeps
  the page's user space. So the flow's page rebase
  (`Tx::set_page_base`) only re-points pages: no mark, OCR word or
  bookmark moves.
- **`/SheafAnnot` version 2** (`annot::Payload`) is the mark's JSON in
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
- **Reading a mark** (`file::sheaf_mark`): version 2 is a mark while its
  digest matches. Version 1, from before slice 5, held its geometry in
  the page's displayed space, which on a page sheaf had turned wasn't the
  `/Rotate` the save wrote, so its geometry comes from the standard keys,
  already PageSpace (a box is `/Rect` less `/RD` or the old writer's pad;
  text markup is `/QuadPoints`; lines and paths are `/L`, `/Vertices`,
  `/InkList` and `/CL`; a replacement's struck text is its grouped
  StrikeOut's, found through an index of the page's StrikeOuts made
  once), its `turn` is the page's `/Rotate`, and it's a mark while
  the JSON's `modified` is `/M`. A mark that fails either check was
  edited by another tool, and opens as that tool's comment, a record as
  other tools' comments are: shown, deletable and adoptable, never
  reverted. `file::is_ours` asks the same reader, so a save keeps its
  dictionary as the other tool wrote it.
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
- **Plugins under API 0.3 keep their frame:** a page's coordinates are
  its source page as the file displays it, after the file's own
  `/Rotate` and before any turn sheaf adds. The adapter (`sheaf-plugin`'s
  `api/shared.rs`) maps marks, OCR text, bookmark positions and region
  events into that frame on the way out, and back into PageSpace on the
  way in, a mark's `turn` the frame's rotation. `page-info`'s size is the
  page as shown, sheaf's turn included, while its coordinates aren't;
  API 0.4's to-view and from-view matrices end that (slice 20).
- **PDFium's text is still in the source page's displayed space**
  (`page_chars`, `search_page`, `pattern_search_page` and
  `text_boxes_in_rect`, through a device 16 times the page's size) until
  slice 14's text service. The app maps it into PageSpace where it
  arrives, through the source page's `from_view` (`PageChars::from_engine`,
  `merge_source_hits`); the plugins' text source passes it on as it is,
  since that is 0.3's frame.
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
meet. The watermark's install and read (until slice 16) and the flatten
(until 19) are their last users, and a clippy ban lands when the flatten
goes (the plan's lint ledger).

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
  sheaf's blocks by stream marker (`/SheafBlock`) and by marked-content
  marker (`/SheafGen`, `/SheafOcrBlock`), the isolating streams
  (`/SheafBlock << /K /Isolate >>`), the OCR layer sheaf wrote before
  (its `q` and its layer, marked `/SheafOcrLayer`), the balance, and
  whether a form the page draws outside its pagination artifacts has one
  inside, which removing the page's would leave in place. A source reads
  each XObject its pages draw once, whatever names and pages draw it
  (`DrawnForms`); a form with no resources of its own draws with its
  page's, so it's read for each page. Forms are read within what the
  page's streams leave of `Limits.content.decompressed`: one past the
  cap, or met once that's spent, counts as having an artifact inside.
  Who drew an artifact (`PagOrigin`):
  - **`Sheaf`:** it's in a `/SheafBlock` stream or its list has
    `/SheafGen` (nothing writes these until slices 16 to 18);
  - **`SheafLegacy`:** sheaf before M10.5: `/SheafStamp` or
    `/SheafSettings` in its list, drawn in the `/SheafHF` font, or in a
    `/SheafOcrLayer` stream;
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
- **Removing the header and footer** (`stamps::strip`, until slice 17's
  generators) splices the page's streams, writes each it changed as a
  new object and rewrites `/Contents`, so a stream another page shares is
  never changed under it. It removes the asked-for kinds that sheaf or
  Acrobat drew and leaves an author's own (proposal 31,
  `stamps::stripped`); removing those too is slice 17's
  `include_authored`. Acrobat opens the first artifact on a page with
  the `Q` that ends the `q` it put before the page's content: that run
  of `Q`s stays (`Pagination::cuts`), the artifact's start and the rest
  of it cut either side, as sheaf's own header keeps its `Q` outside its
  range. The pages to strip are the ones whose blocks have such
  artifacts (`derive::strip_stamps` and the watermark's), so a source
  with none is never loaded, and the scan that decompressed every page
  to look is gone.
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
  written again keeps its list's name. The legacy names, `/SheafHF`
  (until slice 17) and `/SheafOcr` (until 18), are given, and never made
  for anything else. The emitter writes a content's resources by digest,
  one object a resource a file; `content::ResObjects` writes them into a
  lopdf document (exchange's Comments.pdf, the comment summary's pages),
  one object a resource a document.
- **Fonts** (`content::Font`: `encode`, `advance`, `wrap`, `covers`,
  `resource`). `Std14` is the 14 standard fonts with Adobe's Core 14 AFM
  widths (`crates/sheaf-doc/assets/afm`, with Adobe's notice) and exact
  encoders: WinAnsi as cp1252, and Symbol's and ZapfDingbats' own, glyph
  names read by the Adobe Glyph List; text is composed (NFC) first.
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
  and footer, and the app's watermark), and a commit tells it once a
  document: "Some characters can't be drawn in the built-in fonts and show
  as “?” in the file; the text itself is kept". A tape's minus and subtotal sign, which
  Helvetica lacks, draw in Symbol. M10 slice 7's Noto fallback ends it.
- **The words a page draws** (`text_bag`) are decoded through its fonts
  (`FontDecoders`: a `/ToUnicode` CMap, else a simple font's encoding
  and `/Differences`, else Symbol's or ZapfDingbats' own, else the code
  as it is) and counted as NFC words, so the same text in two subsets
  with different codes gives equal bags. Slice 10's removal check
  compares them.
- **Marks hold their stamps and attachments** (M10.5 slice 8). A mark
  names its stamp's page and its attachment's file by a stem, a blob's
  name (`BlobId::stem`), and holds them: `Mark.stamp` is a `FormRef`,
  the form a source's saved appearance draws (`Base`) or a one-page PDF
  blob (`Blob`), and `Mark.file` a `BlobRef` to the file's bytes, an
  `Attachment` blob. So a stamp or file lives as long as a version holds
  its mark, and the store counts it. At ready, and for the marks of files
  whose pages come in, each holds what its object holds there
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
  names nothing is `NotFound`; lowering never decodes bytes. A save writes each once a file, by digest
  (`ObjKey::Res`), and a mark it writes as the file holds it keeps the
  objects the file has (see Saving).
  Nothing is made again in the cache at open, so a file opens
  with its stamps and attachments on a machine that never saw them. A
  page stamp whose appearance draws no page, or an attachment with no
  embedded file, opens as another tool's comment, as a stale mark does,
  and a save keeps it so. The stamp library's page form bakes its page's
  rotation and visible box into the form's `/Matrix`, so a stamp from a
  cropped or turned page lands as PDFium shows that page.
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
takes 141 ms (synthetic), 203 ms (heavy) and 77 ms (scanned), which is
most of what a first Bates apply costs there.

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
`cursor = saved`.

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
and committed as one version (§6 of `todos/m10.5-edit-api.md`). The old
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
    // OCR text, header and footer
    SetOcrText { page: PageRef, layer: Arc<TextLayer> },
    ClearOcrText { pages: PageSel },
    SetHeaderFooter { spec: HeaderFooterSpec, scope: ScopeSel },
    RemoveHeaderFooter { pages: PageSel },
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
  default. The exceptions are the record types `/SheafAnnot` stores
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
     generated cache, reading and interning it while it lowers.
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
  Cli, Job(Ocr | Import | Insert)}`, bound by `Document::lower_cx`, the
  only way to make a lowering context. `Cap::Plugin` is what any plugin
  may do (pages moved, turned, deleted or copied, labels, bookmarks,
  sheaf's own marks); `Approval` changes or removes what the user didn't
  make in sheaf (another tool's comment, link or widget deleted or made a
  mark, OCR text cleared, the header set or removed); `User` brings
  content in (pages inserted or replaced, OCR text set, a derived page
  base, an import's names and dates). A plugin gets `Plugin` only, until
  plugin API 0.4's permissions; an OCR job sets and clears OCR text and
  nothing else.
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
  none is left. `Commit.cascades` counts each rule's work.
- **Fail, never repair** (§6.4). `EditError::{NotFound, Invalid,
  Ambiguous, Refused(Cap), NotReady, Cancelled, Mismatch { version },
  Internal}`, each naming the op as Op JSON does. `check_invariants`
  (debug builds, after every op and every commit, at open and after a
  save) also says that label ranges and the header's pages name pages
  that are there, that connectors pair both ways, and that a sheaf reply
  is to a comment on its own page. The open clears a connector's partner that doesn't
  name it back, and `save::subset` (print, extract) keeps what it writes
  so: partners left out cleared, labels re-anchored, the header's pages
  pruned.
- **Steps, lines and effects.** A step is `{ op: Arc<ResolvedOp>, prims,
  effects, line, cascades }`. describe reads the resolved op as it's
  lowered, its page numbers by `pos_of`, so a line costs what the op
  names; `describe::lines` shows a run of one kind as one line ("Recognize
  text on 7000 pages"), and the internal page base's line names its
  flow (crop, watermark, header stripped). A line says whether its op
  removed something (`Line.removes`), which the History panel marks with
  a "Removes" badge. `StepEffects` is a step's `Effects` (§7.1's flags and
  signature class) with the pages and keys it touched, judged on the step
  alone (`ClassifyCx::of_step`), so it too costs what the op touched; a
  save still judges its whole path (`judge`, slice 10).
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
  A derived copy's origin is in a table by blob (`Document::derived`,
  until slice 17), which `Snapshot` and `SaveInput` carry; the copy's
  catalog names its source blob and pages (`/SheafCopyOf`), so copies of
  different pages, however alike, are different blobs. Each version
  notes once the blobs its steps bring, so `undone_source_bytes` walks no
  step.
- **Callers.** The app commits through `DocView::commit(tx, opts) ->
  Result<Commit, EditError>`, from slice 12 the only way it changes the
  document but for OCR's flushes (below): a drag is `MoveMarks`, a handle,
  style or text change a `PatchMarks` naming only what changed
  (`mark_changes`). Crop, Remove Header/Footer and the watermark show
  pages of derived copies through the internal `Tx::set_page_base(pages,
  Derive)`, no `Op` variant (until slice 17). Combine is one transaction
  (`Actor::Job(Insert)`) on the first file's document: the first file's
  pages left out deleted, one `InsertPages` a further file, its bookmarks
  added; it's still written and reopened, until slice 19. Plugins speak
  WIT 0.2 and 0.3 until 0.4: `api::shared::Adapter` says each edit as
  ops (an update a `PatchMarks` naming only the record's fields, so what
  the API doesn't show stays), the host resolves a mark's stamp or file
  stem into a grant, and the batch is one transaction of
  `Actor::Plugin` (from slice 12 a call's batches are one group, the
  first landed on what the call read). A comment import is `PatchMarks`, `AdoptAnnotations`
  and `AddMark` with `Imported` (`Actor::Job(Import)`; `Actor::Cli` on
  the command line).

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
refresh), its panels and plugins follow the document. An OCR run and a
plugin call are each a group of versions that undo as one step. History
is kept to a byte budget, pruning the oldest versions, the saved one
too (§8 and §14 of `todos/m10.5-edit-api.md`).

```rust
pub enum Base { Current, At(VersionId) }          // the version a Tx was made on
pub enum Stale { Validate, Relower }              // how it lands on another
pub struct CommitOpts { label, base: Base, stale: Stale, group: Option<GroupId> }
pub enum GroupKind { Ocr, PluginCall { plugin: Arc<str> } }

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
    per kind and place (`GenSpec::same_place`; a header and footer has
    no z, so a document has one), a source page's copy shown once,
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
- **Groups** (§8.4). `begin_group(GroupKind::Ocr | PluginCall { plugin
  })` starts a group of versions that undo as one step. It takes commits
  (`CommitOpts.group`) from the actor its kind names, the OCR job or the
  plugin called (another's is a bug), until `end_group`. A commit joins
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
  the change touched them, and the images of pages now shown from
  another source page go. The panes lay the pages out again
  (`Doc.layout`) only on an order, a geometry or such a page, so a
  mark's edit lays nothing out. Then the plugin cell takes the new
  snapshot, autosave is scheduled (not for a save or the opening), and
  listeners hear once: `DocEvent::Opened` when the file opened, else
  `DocEvent::Changed { change }`. Plugins hear it as `edited` ("Undo:
  …", "Redo: …", "Go to: …" or the label), never of their own edits (by
  the change's actor) or of saves. A recovery sidecar is keyed by its
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
  and replacing pages, read the file into the store, open its engine
  and lower `InsertPages` or `ReplacePages` on the snapshot off the UI
  thread (`plan_insert`, `plan_replace`), then land with `Relower`
  (`CommitOpts::based(v, stale, label)`), keeping the engine only once
  committed. Crop, Watermark / Background and Header & Footer resolve
  their pages to ids on the version their dialog opened on, make the
  copies off the UI thread, show them (`Tx::set_page_base` on
  `tx_at(opened)`), commit with `Validate`, and only then keep the
  copies; their dialogs show no preview of the document until slice
  13's rendered previews. The labels dialog previews its op lowered on
  the current version (`Tx::preview`), again at each change, and
  commits on the version it opened on with `Validate`; once the labels
  changed since it opened, which that commit would find, it says it
  can't apply. Blank review's
  delete reads each confirmed page (`Tx::read(Key::Page(p))`, until
  slice 13's `read_render`) and lands with `Relower`, so a page whose
  content changed since is a Conflict, and one given a bookmark, a
  connector or a header since has the cascades applied. A comment
  import is planned off the UI thread (`Actor::Job(Import)`) and lands
  with `Relower`, reading what its matching decided by
  (`Imported.reads`, read by `ops_in`): the order, each page it brings
  comments onto (its `Geom` and `PageAnnots`) and each comment it
  updates, so two imports of one file planned together don't both add
  its comments: the second is a Conflict.
- **OCR** is a group (see Text layers): flushes made on the run's first
  version, deciding by each page as it was rendered, landed with
  `Validate`.
- **Plugins** (§14.3; WIT 0.2 and 0.3 until 0.4). Each document has a
  cell (`Doc.plugin_cell`, an `ArcSwap<Snapshot>`) that `on_change`
  stores the new snapshot in. A call loads its base from it at its first
  read or submit (`HostState::base`), so a queued call, or one that
  showed a form first, starts from the document as it is then, and its
  reads record the slots it decided by (`HostState::read`,
  `CallOutput.reads`): `pages()` the order and each page's `Page` and
  `Geom`; `annotations(p)` `PageAnnots(p)` and each mark shown; the
  outline its roots and each bookmark; text, search, words, recognition
  and renders `Page(p)`; an update the mark it keeps parts of. When the
  call returns, `plugins::land` begins a `GroupKind::PluginCall` group
  and commits the first batch on the base (`tx_at`, each read,
  `Relower`) and later ones at the current version in the group
  (`EditBatch::lowered(tx, doc)`). The first failure rolls the group
  back; if the user changed meanwhile what the call wrote (a Conflict),
  its edits stay, and the notice says "its partial edits were kept". A
  call whose cell is no longer its view's (the view opened another file)
  changes nothing.

At 7,000 pages a snapshot takes about 0.2 µs (asserted under 50 µs on
every fixture). A crop of every page but one, landed by value after the
user deleted that page, takes 27–31 ms, against 14–15 ms on the current
version. An insert of 10 pages lowered again after a move commits in
1.5 ms. One transaction of 7,000 `SetOcrText` lowers in 36 ms and
commits as an OCR group's version in 9.5 ms, about 4.5 ms of it pruning
the previous run's version, which doesn't fit the budget beside it (the
bench gives every page one shared layer, which each step counts).

## What the viewer changes

- Caches (previews, tiles, sizes) key by `PageId`, not display index, so a
  reorder costs nothing visually.
- `Job` gains `rotation`: the page's `render_delta`, the turn on top of
  the source page's own `/Rotate` (which PDFium applies itself), plus the
  pane's rotate view. PDFium renders rotated bitmaps, so tiles at a
  different rotation are just a cache miss.
- `DocView` holds a `Document` instead of a bare `RenderHandle`; sidebar
  thumbnails become the primary editing surface (drag to reorder,
  multi-select, keyboard rotate/delete).
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
appearance and, in `/SheafAnnot`, its PageSpace JSON and staleness
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
stamp's page is written once a file, as a form drawn under such a name
(`/SheafStamp` is only read), and an attachment's file once. Both come
from what the marks hold (a form a source's mark draws, a library
stamp's page and an attached file from their blobs), not this machine's
cache. The header and footer's marked content names its property list
in the page's `/Properties`.

A save is an output (M10.5 slice 9, below): the document doesn't change,
nothing is reopened or read back, and what the screen shows stays.

### One writer (M10.5 slice 7)

Saves, Save As, extracted pages, Clean Copy, Export Flattened, print and
recovery sidecars reach their bytes through `serialize/` (§11.7 of
`todos/m10.5-edit-api.md`). lopdf's writer still writes:

- the derived copies, in memory, until slices 15 to 17 delete them;
- the comment summary and FDF files, until slice 19's exports;
- PDFs made from images and blank pages (`images::to_pdf`,
  `generated::blank_pages`): Images to PDF and a scan without OCR, at
  the path the reader picked, and inserted image and blank pages and
  Combine's image items, through the generated cache, until slice 19's
  memory sources;
- the stamp library's files (`stamp_lib`: a collection, a stamp's
  one-page PDF, an image stamp), the reader's own and outside the edit
  API (§15), until slice 19;
- pictures PDFium draws from memory (a tape, the layers panel's copy).

- **Objects** (`object`): a direct object as PDF syntax, and an indirect
  object's `n g obj` frame. Reals are f64 in their shortest round-trip
  form, always with a point and never an exponent: a NaN or an infinity
  is refused and nothing is written, magnitudes are clamped to ±3.4e38,
  and nonzero ones under 1.175e-38 are 0 (Annex C). Strings keep their
  format, names are escaped, and a stream's `/Length` is its data's.
- **Cross-references** (`xref`), as a table or a stream in the base's
  kind: a whole file's one subsection `[0, Size)`, every number it
  doesn't list free and the free entries linked from entry 0; an
  update's runs of the numbers it lists, and nothing freed (lopdf's
  readers keep an earlier section's entry for a number a later one frees,
  so an object an update unlinks stays in use and its number is never
  given again; slice 10). A stream lists itself, with `/Index` and a `/W` as
  narrow as its fields allow, uncompressed. Positions are written from
  the header, so a file with bytes before `%PDF-` appends as PDFium,
  pdf.js and qpdf read it; an offset a table's ten digits can't hold is
  refused.
- **A whole file** (`full`): the header at the document's version,
  every object in number order, and one section. Object streams,
  cross-reference streams and linearization dictionaries are left out
  (an object stream's members are written as objects of their own), and
  the trailer keeps its keys but its old sections' own, `/Encrypt` and
  `/ID`. A stream the save made was compressed where it was made; a
  source's is written as stored, never re-encoded, so a Rewrite of an
  uncompressed file stays uncompressed.
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
  number it's written as, through vendored lopdf's `encrypt_object_with`
  (`vendor/lopdf/SHEAF.md`): strings in stream dictionaries too, an
  embedded file by `/EFF`'s filter, and the `/Encrypt` dictionary,
  cross-reference streams, metadata when `/EncryptMetadata` is false and
  a signature's `/Contents` left as they are. An AES IV is the first 16
  bytes of HMAC-SHA256, under the file key, of the object's `ObjKey`
  (`ObjKey::bytes`, see Saves are outputs) and what it encrypts
  (`Crypt::encrypt_as`, slice 9), so a save is deterministic, an object
  keeps its IV whatever number it's written at, and no IV serves two
  plaintexts, an update's section's too (`indirect_as`, slice 10; the
  number-keyed `serialize::indirect` and `Crypt::encrypt` went). An
  encrypted string keeps its format, a literal
  escaped as any is, so it takes about its own length, not twice it. The
  `/Encrypt` dictionary, its `/P` with it, is written as stored and
  `/ID[0]` kept, so the file opens with the same passwords and
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
  comment exports aren't yet (slice 19).

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

A save is an output, never an edit (§9 of `todos/m10.5-edit-api.md`): it
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
  the change) until slice 19's `Session::save`. `save_appended` and
  `save_rewritten` are the both-ways tests' halves.
- **Sealing.** `Document::save_input(v)` gives what a save writes from (a
  `SaveInput`: the version's state, the primary, the derived copies'
  table (see Ops and transactions), and the saved
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
  `/PageLabels` from the `LabelSpec`s, the header's settings; no
  `/Version`, which the header gives), every other key the primary's,
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
- **Derived pages, content and the outline** (`emit::{pages, content,
  outline}`). A derived copy's page (a crop, a header stripped, a
  watermark) is written as the page its `derived` record (the
  document's table by blob, from M10.5 slice 11) says it was made from, through copies of copies (`emit::Shown`): that
  page's object (copy 0 of a primary page keeps its number), keys and
  annotations, with the copy's `/Contents` and `/Resources` copied in
  and the state's boxes, until slice 17's content stack. What sheaf draws
  over a page goes through the legacy installers with the emitter as their
  `content::sink::Sink` (a lopdf document's is `sink::Lopdf`, the
  document and its `ResObjects`): an OCR layer through `text_layer::install`
  (`[q, base…, layer]`, `/SheafOcrLayer`, `/SheafOcr`), which first takes
  out a layer the page holds already (`text_layer::has_layer`), so a
  re-OCR leaves one; the header and footer through `stamps::install`,
  Bates numbers counted, on each page it covers whose base doesn't draw it
  already (`stamps::base_draws`: its file's catalog says sheaf drew these
  settings on that page; `Source::header_footer` reads it once, and a
  derived copy says what it kept, `save::HeaderKept`). The page's own
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
  dictionary with `/SheafAnnot`, its appearance (an `AnnotPart(id, Ap)`
  form, its resources by digest through the emitter's sink) and a
  replacement's StrikeOut (`AnnotPart(id, Companion)`), which carries
  `/IRT` to its Caret and `/RT /Group` and nothing else: `file::is_ours`
  knows a companion by that group under a sheaf Caret, and still reads
  `/SheafPart` in older files. A page stamp draws the form its
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
  shows (widgets on page templates) stay in its `/Kids`. A field the state
  shows nowhere that showed as its file was read (a widget of it, or of a
  node under it, on a page or a page template) isn't written, nor the
  parents it leaves with none, as `keep_only` takes such a field out of a
  file (an edit that takes a node's last widget takes the node too, by
  the `widget.field` cascade of M10.5 slice 11); a node whose
  `/Kids` another node's read took stays as it is. A form left with no
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
  generation is its header's, and the base is sound. An object stream's
  members are written as objects of their own, encrypted each on its
  own; on a stable save the `/Encrypt` dictionary stays at its number,
  and a cross-reference stream takes a number past every one the primary
  and the previous save used.
  An AES IV is HMAC-SHA256 under the file key of the object's `ObjKey`
  and the plaintext (`Crypt::encrypt_as`), an update's section's too
  from slice 10. The header gives the version needed: the
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
  mark's `/P` its page's and no pop-up or `/SheafPart` companion; another
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
  the graph at the end.
- **Pruning's changes.** `kept::keep_only` is unchanged but for what it
  gives back: its notes, and every object it changed or added
  (`Kept.modified`), which a Rewrite re-serializes while copying the
  rest as the file had them.
- **Exports** (save.rs's API until slice 19's `export::run`). Each is a
  Rewrite of `SaveInput::detached`, planned as one, so save policy's form
  transforms apply on every way out: `save::rewrite` and
  `rewrite_to_path` keep the encryption and the numbers
  (`Profile::default()`: `Keep`, `Stable`); extracted and split pages,
  Clean Copy and Export Flattened keep the encryption and number from 1
  (`Keep`, `Dense`); print writes none (`None`, `Dense`). The old rewrite
  path (`build_rewrite`, `rewrite_with`, `finish` with its prune and
  renumber, `kept_encryption`) and `full::write` are gone. An export
  writes what `subset` keeps of the state, held to its invariants as the
  cascades of deleting the other pages would leave it: a field node that
  showed only on pages left out goes, with the parents it leaves with no
  fields, and the form's roots and calculation order follow; a connector
  whose other end is left out has no partner; a label range starts at
  the first page of its range kept; and the header/footer is on the pages
  kept of its own. Clean Copy, and print without markup, filter sheaf's marks and
  other tools' comments out of the target, keeping links and widgets, and
  strip what the records don't hold in a post-pass on the checked graph
  (`materialize_then`'s `PostPass`); Export Flattened, and print with
  markup, draw marks into their pages with `flatten_annots` as that
  post-pass. Nothing of a graph a post-pass ran on is copied as its
  source had it. So an export that leaves out a hybrid form's widget page
  drops its XFA copy, as a page delete does.
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
  A second save while one is sealed is refused ("Already saving…"), since
  its base would be the first's file. Nothing is reopened, re-hidden or
  re-read after a save, and no rendered image is dropped
  (`Doc::tiles_invalidated` counts every drop). The header and footer is
  drawn over each page whose base doesn't draw it
  (`stamps::drawn_over`, shared with the emitter), before a save and
  after. Autosave writes a sidecar 3 s after the last edit (at least
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
  from memory (`MemoryKind::Recovered`) under the file's name. Format 1,
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
  lived on moved: derived copies leave sheaf's marks out with the
  emitter's filter (`file::not_ours`, over `file::is_ours`) and read
  their pages' boxes as `Geom::read` does, stamp collections read theirs
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
  shows it, an OCR layer only when no target page shows that layer; a
  field rename, and a tooltip, items, scripts or a default value
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
  pending, before `BaseUnsound` and `LegacyHeaderFooter`; then
  `BaseUnsound` (`SaveRecord::takes_updates`: a rebuilt cross-reference
  table, an unsound base or a Full restore), `LegacyHeaderFooter`,
  `RecoveredRemovals`, strict removal's `Removes` and `ReplacesText` (by
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
  reason }`. Append anyway (`AppendAnyway { stays_readable }`, what the
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
  object; the outline by place; labels; the catalog for the header's
  pages, labels that came or went, a transform the file hasn't had
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
  stamp by hash), a page's blocks on its OCR words and header text, a
  resource as its user's. Other objects are judged as a `TextBag`:
  strings, what content draws through its fonts (pages with inherited
  resources, forms, tiling patterns, Type 3 glyphs), scripts, rich text,
  XFA and XMP as words, images and embedded files by hash; their bag must
  be within the bag of everything the section writes. A string in codes
  its font doesn't map is its raw bytes' hash, so only the same bytes
  match it (`FontDecoders` says when it falls back), and what can't be
  read counts as removed. A veto on a derived object is a
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
  take an update (a Full restore's too), the legacy header is on, or the
  section is refused or fails to be made (logged), so a fault in the
  Append never leaves a document without one. A restored document opens from the file's bytes on
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
  rewrites (`Chosen`) when they don't, until slice 19's flags.
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
  4,389 append, 360 ask about signatures, 259 rewrite), a page, a mark, a
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
(`todos/m10.5-edit-api.md`, §10.1 and §10.3).
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
(`/XRefStm`), linearized, damaged (lopdf rebuilt the xref, or lopdf and
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

## Non-goals of this design

- Collaborative / concurrent editing. Single writer, single history.
- Editing page *content* in place (M-later). When it comes, a page's edited
  content stream becomes another per-`PageId` persistent map entry and
  rendering of that page goes through a scratch PDF; the history model is
  unchanged.
- Making lopdf's object graph the live model. It stays a serialization
  backend.

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
  engine opened on it keeps its bytes until M10.5 slice 13's engine
  pool closes it). A policy weighs undone sources but not `blob_bytes`,
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
an `OcrText` layer on the page's `ContentStack`, `DocState::ocr_text(page)`,
put with the page's `PageDef`.

- **A save writes it, and it stays.** A save writes every layer
  into its page as an invisible text layer (text render mode 3 in
  `content::Glyphless`, Tesseract's glyphless font embedded, each UTF-16
  code unit of a word a CID and the text read back through its
  ToUnicode, written by `ContentWriter` under the legacy name
  `/SheafOcr`; `/Contents` turned into `[q, original…, layer]`, the `q`
  and the layer marked `/SheafOcrLayer`, until slice 18's blocks). A save
  is an output (slice 9), so the layer stays in the page's content, the page
  still showing the file it opened, and the next save writes it again,
  in place of one its base already holds. The model never reads layers
  back from a file: in a file opened again, PDFium owns the text.
- **Ops**: `SetOcrText` (one page's layer) and `ClearOcrText`, which put
  the pages' `PageDef`s and nothing else. Delete removes a page's layer with the
  page, Duplicate shares it (the same `ContentStack`), Replace drops it
  (the page shows other content, under the empty stack), Move and Rotate
  leave it alone, and Crop re-points the page's base and keeps its
  content.
- **Coalesced undo.** Results arrive in flushes over minutes (every 10
  pages or 2 s), but a run is one undo step: from M10.5 slice 12 it's a
  group (`Document::begin_group(GroupKind::Ocr)`, which takes commits of
  `Actor::Job(JobKind::Ocr)` only, which may only set and clear OCR text;
  see One funnel and the history budget). Each flush is a transaction
  made on the version the run began on, which its pages were rendered
  from, that reads each page's slot (`Tx::read(Key::Page(p))`, until
  slice 13's `read_render`) and lands by value (`Stale::Validate`) in the
  group: it amends the group's version when that's the current one, has
  nothing after it, was never saved and isn't being saved (sealed, see
  Saving), and pushes a version "… (continued)" otherwise. An amend
  coalesces the run's prims and replaces the version with a new
  `VersionId` under the same parent (an id names one state), and a run
  whose flushes put back all they did leaves no version. A save or an
  unrelated edit mid-run splits the run into fragments, which History's
  "Undo all of …" takes back at once (`revert_group`); a flush during a
  save lands in a version after the sealed one, which stays unsaved. A
  page whose slot changed since (a crop, until slice 15 makes crop
  geometry) is a Conflict: the flush is retried page by page, the other
  pages keep their text, and the run's message counts the changed ones.
  Undoing the run, or jumping away from it, makes its next flush
  `GroupEnded`: the run stops ("OCR stopped: its results were undone")
  rather than put undone results back.
- **Search.** PDFium can't see unsaved layers, so Find and Patterns run the
  same literal/regex search over the pages' `OcrText` layers
  (`text_layer::search`, `pattern_search`, until slice 14's text service)
  and merge the hits with PDFium's, dropping PDFium's for any page that
  has a layer. Hits are addressed by `PageId`, so inserted pages and
  reordered pages work too.

## Open questions

- None blocking slice 1.
