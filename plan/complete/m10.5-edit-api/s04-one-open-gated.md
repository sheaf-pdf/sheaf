# M10.5 slice 4: One open, gated

Status: done · Part of [M10.5: the edit API](README.md)

**One open, gated** (M; proposal 28; bug 10).
- `open::{start, finish}` builds today's DocState shape: geometry,
  sheaf marks, fields, the outline with `/C`, `/F` and closed state,
  labels, and the header/footer. It never lexes content.
- The Loading state:
  - edit and plugin commands are grayed out;
  - `commit` returns `NotReady`;
  - plugin calls are queued until ready, and `opened` fires after.
- The CLI and headless code use the same open.
- **Deletes:**
  - `set_initial_outline`, `_outline_styles`, `_labels`,
    `_header_footer`, `_annots` and `_fields`;
  - `opened_state_mut`;
  - the six "arrived after edits; ignored" loaders;
  - `load_sheaf_annots`' async path;
  - `sheaf_ops::open`.
- **Checks:**
  - Over the corpus, the root's facts equal the old loaders' (counts
    per manifest row).
  - App and headless opens produce equal states.
  - First paint is unchanged.
  - **Ready within 1 s on the heavy 7k.** If that's missed, edits
    wait with progress, and no late facts are added.
  - An edit before ready returns `NotReady`.
  - A plugin's `opened` never comes before the facts (a slow-loader
    race test).
  - A queued plugin call runs after ready.
  - A grep finds no "arrived after edits".

## Notes

Done. A file opens once, the same way in the app and headless:
`open::start` makes the document from the page table, `open::finish`
reads every other fact from the source alone (the outline with `/C`,
`/F` and its closed state among them, with no eager load and no
PDFium), and `Document::ready` makes them the opened version once,
under a new id. Until then edits are `NotReady`, the app greys out
what edits or writes the document and every plugin command, and
plugin calls wait for `opened` (proposal 28, bug 10).
- **One reading seam:** `attachments::Objects` gains lopdf's names,
  `get_object`, `get_dictionary`, `dereference` and `catalog`, each
  the type's own method for `lopdf::Document` and `LazyPdf`, with
  `object` and `dictionary` as defaults over them, so a reader's body
  reads the same over either. The readers the open ran eagerly are
  generic over it (`<P: Objects + ?Sized>`), their bodies unchanged:
  `labels::label_ranges` and its helpers, `stamps::from_catalog`,
  `attachments_in` and `attachment_bytes`, `portfolio::collection`,
  `layers_in`, import's `convert_page` and its helpers,
  `measure::scale_of`, `file::{annots_in, page_geom, recover_cached}`,
  and `stamp_lib::form_to_pdf` with the `save::{remap, copy_object}`
  it copies through. A reader that walks pages takes their objects by
  index (`impl IntoIterator<Item = ObjectId>`): the page table's from
  the new `Source::page_ids()`, or `page_iter()`, `get_pages`' order,
  for an eager document.
- **`open::finish(&Source, &dyn Progress) -> Facts`**
  (`sheaf-doc/src/open/`): `Facts { marks, foreign, labels,
  header_footer, form, attachments, collection, layers, outline }`,
  every page by its page-table index. One pass over the page table's
  annotations (`file::annots_and_cached`, its pages counted to the
  progress) finds the marks, the other tools' comments and the marks
  that keep cache files (FileAttachment, PageStamp) with their
  dictionaries, which `recover_cached` makes again where they're
  missing, with no second page walk; then `forms::read(src)`, the
  catalog's readers and the outline. It can't fail once the source has
  opened, it never reads a page's content stream, and it reads every
  tree a node at a time: `name_tree`, the labels' number tree, the
  layers' `/Order` and a portfolio's folders keep a set of the nodes
  they reached by reference (`attachments::once`, forms/read.rs's
  before), as the outline and the form tree did. `file::{read_open,
  facts_in, FileFacts}`, the eager open load, are gone.
  `read_attachment(&Source, &AttachmentRef)` numbers pages by the page
  table, as the listing does.
- **`open::outline(&Source) -> Vec<FlatOutlineItem>`**
  (`open/outline.rs`): `/Outlines` in pre-order, with a seen set and
  PDFium's caps (32 deep, 200,000 items). A destination is a `/Dest`
  array, or a name or string looked up in `/Names /Dests` and then
  the catalog's `/Dests` (a value may be a dictionary's `/D`), else a
  GoTo action's `/D`; attachments' `named_files` became
  `name_tree(pdf, key)` for both trees. A page reference maps through
  the page table (a page listed twice is its first index), and an
  integer is an index when it's in range. `/XYZ` goes to displayed
  space through `page_geom`'s frame (nulls are None, a zoom of 0 is
  none, an XYZ with nothing set is None); the fit kinds are
  `DestView::Other` with as many params as the kind takes. `/C` (black
  is no colour) and `/F` as before, and a negative `/Count` is closed.
  Titles decode as PDFium's `GetTitle` does (deviations).
- **The closed state, end to end:** `OutlineNode.open` and
  `FlatOutlineItem.open` (serde default true, skipped when true),
  carried by `outline_from_flat` and `prune_outline`; a bookmark an
  edit makes is open. `write_outline` writes `/Count` as the number of
  items shown below an open node, and that number negated for a
  closed node with children, so a tree with nothing closed is written
  as before. The outline panel starts with the top-level nodes the
  file leaves open expanded, where it expanded every top-level node.
- **`Document::ready(facts) -> Result<ViewFacts>`** (`model.rs`): one
  private builder, `opened(facts)`, maps each page index to its
  `PageId` once and builds the outline, the marks, the labels
  (`labels::to_specs`), the header/footer with its page scope, and the
  fields (their `source` the primary) and widgets, then
  `ids.fields_from`; it sets the `saved_*` baselines,
  `baked_header_footer` and `forms[primary]` from them. `ready` takes
  the loading root out and puts the built one in its place with
  `replace`, under a new `VersionId`, as slice 1's amend does: root,
  cursor and saved follow, so the document isn't dirty and there's
  nothing to undo. A second call is an `Err`. The crate-private
  `Document::loading` makes the pages-only root `open::start(store,
  primary, policy)` returns; `Document::new` is `loading` plus the
  builder with only an outline, at `VersionId(0)`, for a file sheaf
  has just made. `ViewFacts { foreign, attachments, collection,
  layers, form: FormFacts }` is what the view shows beside the model.
- **`NotReady`** (thiserror, "the file is still opening", exported
  from `sheaf_doc`): `apply`, `apply_batch` and `apply_in_job` check
  readiness first, so a refused edit makes no version and draws no
  ids; `rebase_to_saved` debug-asserts it.
- **`open::file(path, password) -> Result<Opened { doc, view }>`, the
  headless open:** `SourceInfo::read` into a new store, then `start`,
  `finish` and `ready`. The CLI's split, export and blanks,
  `comments::open`, the gate harness and the sheaf-ops tests (a
  `common::open` helper) open through it. sheaf-ops' open.rs (`open`,
  `open_with_foreign`, `describe`) is gone, and `engine_for` moved to
  sheaf-ops' lib.rs. `combine::build` reads each file with
  `SourceInfo::read` and `open::finish` (an image's facts are
  `Facts::default()`) and takes its outline from `facts.outline`, read
  once where `describe` and `finish` read it twice; the first file
  goes through `start` and `ready(Facts { marks, form, .. })`.
- **The app opens through the same three,** by two plain functions
  of docview.rs: `install` builds the model with `opening_model`
  (`open::start` and the user's name as author) and the rest of `Doc`
  as before, so first paint goes the same way, and `finish_open` runs
  `open::finish` on the background executor with a `Tracker` the
  `Doc` keeps (`reading`), redrawing every 100 ms, so the status bar
  says "Opening: page N of M…" during the first pass and "Opening…"
  after it. `DocView::ready` takes the facts with `take_facts`, which
  drops a result unless the view's primary has the `BlobId` it was
  read from, or when the model refuses it, and marks a recovered file
  unsaved; then the view takes `ViewFacts` (other tools' comments,
  attachments and the portfolio, layers, the damaged-form notice and
  `forms.primary`), seeds `outline_expanded`, prints one
  `[timing] ready` line, hides the drawn marks in PDFium (the new
  `drawn_marks(&DocState)`, which the save path uses too), gives a
  recovered file its notice, calls `touched()` and
  `refresh_history()`, emits `DocEvent::Opened`, and only then runs
  the dev hooks that edit or save. Deleted: `fetch_outline`,
  `load_sheaf_annots` and their "arrived after edits" branches,
  `Doc::{outline_loaded, facts_loaded}` (the outline, attachments and
  layers panels show their loading text while the model isn't
  ready), the labels-cache reset, form_bar's `take_form`, and with
  them the six `set_initial_*` and `opened_state_mut`.
  `Incoming::read` (insert and replace) uses `finish`, `insert_file`
  takes `incoming.outline` rather than waiting on the renderer, and
  the combine dialog's `inspect` uses `SourceInfo::read`. Insert from
  a file or images and Replace refuse with `STILL_OPENING` before they
  read the file, and again before they register it, so a refused one
  leaves no source, engine or form behind (found in review: the
  registration went into the next version's `sources`).
  `attachments_panel::refresh_facts` and `reread_foreign` read the
  saved primary's Src.
- **The Loading state's gates** (`availability.rs`): `When::Ready`, not
  a manifest word, reads `Facts.ready` (`is_ready()`), and
  `Facts.opening` (`DocView::is_opening()`: the file still being read,
  or shown with the model not ready) turns plugin commands off.
  `[Document, Ready]` gates Save and Save As, the export copies, the
  comment summaries, exports and import, Export Pages and Split, the
  page commands (insert blank, from images and from a PDF, replace,
  duplicate, extract, delete, rotate, crop), page labels,
  header/footer, Bates, watermark and background, the bookmark
  commands, Set and Reset Status, Find Blank Pages, Print, Undo and
  Redo, reply, the checkmark, OCR, every markup and measure tool with
  its cycles, Paste Marks and the group navigation; viewing,
  navigation, zoom, the panels, Find, the select tool, Copy,
  Properties, Close and Cancel OCR keep `[Document]`. Keys run a
  command the menus grey out, so `DocView::still_opening` refuses the
  saves, Extract, the exports, comment import and export, Split and
  Print with "Still opening the file — try again in a moment"
  (`STILL_OPENING`), `Workspace::still_opening` refuses the dialogs
  drawn over the document, and `failed(what, e)` turns a `NotReady`
  from the model into the same notice wherever an edit's error is
  shown. The status bar says "Opening…" until ready, with the page
  the first pass is on.
- **The plugin queue** (`plugins.rs`): `Waiting`, a plain type, with
  `hold(plugin, call)`, `release(&Document, subscribers)`, which
  gives nothing while the document is opening and, once it's ready,
  `Opened` for each subscriber, then the held calls in order, and
  `opened()`, whether a release has sent `Opened`. A held region event
  takes the place of the plugin's one before it, as the host's
  coalescer keeps only the newest. `plugins::run` holds every call (a
  command, a panel action, an event) but `closed` while the view
  `is_opening()`, saying "{label} runs once the file is open" for a
  command, and is done with a held region event at once
  (`region_done`), so the coalescer doesn't keep the plugin busy for a
  view that may close first; `closed` while opening goes out only when
  `opened()`, as after a recovery reopens a file, and is dropped
  otherwise, with the held calls (found in review). `dispatch_event`
  makes one list of calls, the released queue on `Opened` or the
  event to each subscriber, offers its region events to the
  coalescer, and runs each through `run`, so each builds its context
  from the document as it is then.
- **PDFium reads no outline:** `OutlineItem`, `Renderer::outline` and
  the PDFium impl with slice 3's page-dictionary preload (the
  transitional ledger's 3-to-4 row, closed), `RenderHandle::outline`
  and `Msg::Outline` are gone. The walk lives on as the `#[cfg(test)]`
  module `sheaf_render::pdfium::bookmarks`, the reference the source
  outline is checked against, holding the PDFium lock for a whole
  document; `tests/bookmark_positions.rs` moved there as
  `positions_round_trip_through_pdfium_and_the_source`.
- **The gate harness:** the ready row times `open::file` and asserts
  < 1 s on the heavy after the table prints; the flatten opens with
  it, and the history bench makes its document with `start` and
  `ready`.
- **The ledger:** 30 rows for slice 4. `file::{read_open, facts_in,
  FileFacts, outline_styles, OutlineStyle, style_flat_items}`;
  `labels::page_labels`; the seven `model::Document` functions
  (`set_initial_outline`,
  `set_initial_outline_styles`, `set_initial_labels`,
  `set_initial_header_footer`, `set_initial_annots`,
  `set_initial_fields`, `opened_state_mut`); `open::{open,
  open_with_foreign, describe}`; `docview::DocView::{fetch_outline,
  load_sheaf_annots}`, `docview::Doc::{outline_loaded, facts_loaded,
  pending_outline_styles}`, `docview::Incoming::outline_styles`,
  `docview::flat_item` and `form_bar::Doc::take_form`;
  `Renderer::outline`, `pdfium::PdfiumEngine::outline`,
  `RenderHandle::outline`, `Msg::Outline` and `OutlineItem`.
  `QUALIFIED_ONLY` gains `outline`, `Outline`, `OutlineItem`,
  `describe` and `page_labels` (`DocState::page_labels` is a method).
  Run over `crates/` as of b75e59a (slice 3), every row hits.
- **Docs:** `docs/document-model.md`'s Core idea says the open reads
  every fact from the source and the old writers load eagerly until
  slice 9; a new section, "Opening a file", has start, finish and
  ready, the Loading state, no late facts and ready's gate; the
  Undo/redo bullet on VersionIds counts ready among the ways a version
  is put in another's place, with no exception left; and "What the
  viewer changes" says the outline is read from the source. "Opening a
  file" has trees read a node at a time, the page count in the status
  bar, the refusals before a file is read, and region events and
  `closed` while a file opens. `docs/plugins.md` has `opened` once the
  file is read, calls waiting while it opens, only the newest region
  event waiting, and `closed` only after `opened`. The plan's
  transitional ledger: the `Objects` seam row names the open's readers
  and `convert_page`'s eager user past slice 9, and a new row (4 to 9)
  the `#[cfg(test)]` `read_annots*`.

Deviations from the plan:
- **`open::{start, finish}` aren't §14.2's shape.** The app needs a
  `Document` while `finish` runs: the first paint, thumbnails,
  selection and the tile caches go through `doc.model` and key by
  `PageId`, and those ids must survive ready. So `start(store,
  primary, policy)` returns a document that isn't ready, whose root
  holds the pages only; `finish(&Source, &dyn Progress) -> Facts` is
  a pure read on any thread; `Document::ready(facts)` builds the
  complete root and puts it in the loading root's place with
  `replace`, under a new `VersionId` (slice 1's amend, not an
  in-place change); and
  `open::file(path, password) -> Opened { doc, view }` is the
  headless whole. `ready` refuses a second call with an `Err` in every
  build rather than a `debug_assert`, which would panic in the tests
  the plan asks for; a second call is a caller's bug, and refusing
  leaves the document as it was. Since edits are refused until ready,
  "after an edit" and "twice" are one case.
- **"The page count and the first page's geometry in O(1)" was met
  already:** since slice 3 every page's size comes from the page table
  `SourceInfo::read` builds before the first paint, and `start` reads
  nothing of its own.
- **`finish` counts only the pages of its first pass** (found in
  review: an earlier version took no progress, though §14.2's
  `finish(loading, progress)` takes one and a 40,000-page file keeps
  edits waiting for seconds). It takes sheaf-progress's
  `&dyn Progress`, a direct dependency of sheaf-doc now (sheaf-image
  had it already), and counts the pages of the annotation pass, the
  longest: on the 40,000-page heavy (release) 0.73–1.10 s of it, the
  form 0.39–0.66 s, the catalog's readers 0.09–0.17 s and the
  outline 35–55 ms. The status bar says "Opening: page N of M…" in
  that pass and "Opening…" after it, with no progress bar: one that
  counted pages would sit full while the form is read. `open::file`,
  combine and `Incoming::read` pass `NoProgress`. The heavy 7k's
  ready is 391–416 ms against the 1 s gate, so the readers didn't
  need to run concurrently.
- **There's no `commit` before slices 11 and 12:** `NotReady` is what
  `Document::{apply, apply_batch, apply_in_job}` return while
  loading, a typed error callers downcast (`e.is::<NotReady>()`). Its
  message is lowercase, since the app showed edit errors as
  "{label} failed: {e:#}"; `failed()` gives the one notice for it now.
- **Bug 10's line references had moved:** the six loaders were at
  docview.rs:1155 (outline), :1817 (bookmark colours), :1826 (labels),
  :1831 (header/footer), :1874 (annotations) and form_bar.rs:71
  (fields); `opened_state_mut` at model.rs:822, and the `set_initial_*`
  at :999–1182.
- **The closed state was new to the model:** neither `OutlineNode`
  nor `FlatOutlineItem` had one, and `write_outline` always wrote a
  positive `/Count`. `open: bool` is §3.8's `Bookmark.open`, and a
  closed node's negative `/Count` is a step ahead of bug 15 (slices 8
  and 9), whose other losses on a rewrite (`/A` actions, `/SE`, named
  destinations) remain. The panel's first expansion follows the file.
- **Every outline read moved to the source, beyond the slice's
  Deletes:** sheaf-doc doesn't link PDFium, so `finish` reads the
  outline from the source; inserts (`insert_file` awaited
  `handle.outline()`) and combine (`describe`) read PDFium's too, and
  dropping slice 3's preload with the outline left behind would have
  made big inserts slow again. `Renderer::outline`,
  `RenderHandle::outline`, `Msg::Outline`, `OutlineItem` and the impl
  went, and PDFium's walk is test code.
- **Titles aren't read through `pdf_text`.** On the corpus 56 files'
  titles then differed from PDFium's: `pdf_text` reads a string with
  no BOM as Latin-1, so a bookmark would show `\u{90}` where PDFium,
  and so the app until now, showed ’, and likewise dashes and quotes;
  and a UTF-16BE title with a dangling odd byte read a NUL. The
  reader's own `title()` reads UTF-16BE or LE without a dangling last
  byte, UTF-8 after its BOM and PDFDocEncoding otherwise (a small
  table for 0x18–0x1F and 0x80–0xA0, and NUL for the three bytes it
  leaves undefined, 0x7F, 0x9F and 0xAD, as PDFium's table has them:
  found in review, testing titles byte by byte against PDFium), with
  control characters as spaces, as `GetTitle` does. `pdf_text` is
  unchanged, since `anon_name` and the comment readers depend on its
  exact output (Found); lopdf's PDFDocEncoding table is private and
  drops control bytes.
- **The outline reader is more lenient than PDFium in four places,**
  each deliberate: it follows an indirect `/S` on a GoTo action, where
  PDFium finds no page (the one listed corpus difference,
  forms/safedocs/1260393.pdf); an `/XYZ` with fewer than three params
  reads the missing ones as null, where PDFium gives no view; a fit
  kind's null param is None, where `GetFloatAt` gives 0; and a title
  of malformed UTF-8 or UTF-16 shows U+FFFD where the bytes don't
  read, where PDFium drops them or, at an unpaired surrogate, shows no
  title. No corpus file has the last three.
- **The eager readers became generic over the existing seam,** a
  mechanism the plan didn't name. `Objects` is `pub`, not
  `pub(crate)`: the generic readers are public, with users outside
  sheaf-doc (sheaf-ops' crop calls `page_geom`; the app calls
  `attachments_in`, `collection`, `layers_in` and `annots_in`), and a
  public function bounded by a crate-private trait trips
  `private_bounds` under `-D warnings`; `resolve` and `xfa::read` stay
  crate-private. Generic beyond the list, because listed readers call
  them: `measure::scale_of`, `portfolio::walk_folders`,
  `layers::walk_order` and, until step 2 deleted it,
  `file::outline_styles`; labels' own `resolve` went for
  attachments'. `annots_in` became `pub` for `reread_foreign`.
  `read_annots`, `read_annots_mem` and `read_sheaf_annots*` stay
  eager, numbering by `page_iter()`, but only save.rs's and
  exchange's tests use them now, so they're `#[cfg(test)]
  pub(crate)` (a transitional-ledger row, 4 to 9), and
  `labels::page_labels`, which only its own tests used, is deleted
  (they read `label_ranges` and `labels_for`), a ledger row; found in
  review. The transitional ledger's seam row (3 to 9) says what joined
  it and that `convert_page` keeps an eager user past slice 9,
  exchange's FDF/XFDF-built `Comments.pdf`, so slice 9 keeps the seam
  for it or builds that document as a source.
- **`recover_cached` takes the marks the annotation pass found**
  (`&[(Annot, &Dictionary)]`, crate-private), not page ids, so there's
  no second page walk and direct dictionaries in `/Annots` count too.
  Its two eager callers, save.rs tests, recover through `open::finish`
  on a source of the saved bytes.
- **Ledger names:** sheaf-doc's module defines no `open::open`, or the
  row retiring sheaf-ops' would hit it, so the headless entry is
  `open::file`. `QUALIFIED_ONLY` alone couldn't keep the one-segment
  row `OutlineItem` off the WIT's `w::OutlineItem`, since a qualified
  path hit any row whose segment it held, so a one-segment row (a
  crate-root item) is now named only by a path that starts with it
  after its anchor (`sheaf_render::OutlineItem`,
  `crate::OutlineItem`), documented, with
  `a_root_item_is_named_from_its_root`; slice 3's `SizeWalk` and
  `SIZE_CHUNK` were root items and still hit as intended. The test
  walker is `pdfium::bookmarks::bookmarks`, no item at
  `pdfium::PdfiumEngine::outline`, which is a row for the deleted
  trait-impl method.
- **Output commands are gated as well as edits.** "Edit and plugin
  commands are grayed out" left out Save As, the exports, print,
  extract and split, which read the model too and before ready would
  write a pages-only root, dropping the file's marks, form and
  outline (and `rebase_to_saved` needs a ready document). They're
  `[Document, Ready]`, with a guard in each handler, since keys bypass
  menu state. Save, Undo and Redo are `Ready` too, though `can_save`,
  `can_undo` and `can_redo` are false until ready. Import Comments
  refuses while opening: it matches what comes in against the
  document's comments, so a pages-only state would duplicate them.
  The dialogs drawn over the document refuse at their openers, since
  they start from what the file says (Header & Footer would start
  empty; Split and Print plan from a snapshot); the commands that ask
  for a file first (Insert from File or Images, Replace, Save As,
  Extract, the export copies, comment export and import) refuse when
  what they lead to runs, an insert or a replace before it reads the
  file it was given. History jumps aren't gated: until ready only the
  opened version exists, and `jump_to` returns early for the current
  one. OCR's dialog refuses and its flush uses `failed()`, so its
  start has no guard of its own.
- **Calls are held, and plugin commands grey, while the view is still
  reading the file** (`State::Loading`, before the pages show) as well
  as while the model isn't ready: before, such calls ran with no
  document context. Locked and Failed views don't hold. The queue type
  puts `Opened` first and the held calls after it, and
  `dispatch_event`, which the workspace's subscription calls, hands
  it the event's subscribers for `Opened` and runs what comes back,
  so the rule (Opened first, then the calls, never before ready) lives
  in a gpui-free type its tests check directly. Two rules the plan
  doesn't state, both found in review: a held region event frees the
  host's region coalescer at once (it marked the plugin busy, and a
  view closed before it opened left it busy for the session, in every
  window), and the queue keeps only a plugin's newest region event;
  and `closed` is never held (it died with the view, so a plugin that
  had `opened`, before a recovery reopened the file, never heard
  `closed`).
- **`sheaf_ops::open` was a module** of `open`, `open_with_foreign`,
  `describe` and `engine_for`. `engine_for` has live users (crop.rs,
  blank.rs), so it moved to sheaf-ops' lib.rs, still `pub`, rather
  than being retired.
- **Combine's first file goes through `start` and `ready(Facts {
  marks, form, .. })`,** as it went through `Document::new`,
  `set_initial_annots` and `set_initial_fields` (§1): the full open
  would put the file's own outline in the root beside the per-file
  bookmark `add_bookmarks` adds, and bring its labels and
  header/footer in. That's today's behavior, until slice 19 reworks
  Combine (proposal 35).
- **`refresh_facts` and `reread_foreign` read the saved primary's
  Src** through the same readers, not an eager load of the path, until
  slice 9 deletes the re-read; `reread_foreign` runs `annots_in` and
  `refresh_facts` the three catalog readers, all each uses.
  `read_attachment` read the file eagerly and numbered pages with
  `get_pages()`, while the listing uses the page table, so it reads
  through the source too.
- **form_bar's `take_form` is deleted, not renamed:** without
  `set_initial_fields` it kept two lines, the damaged-form notice and
  `forms.primary`, which moved into `DocView::ready` (a ledger row).
- **The app's ready checks the bytes, not the open:** a result lands
  only when the view's primary has the `BlobId` it was read from, and
  `Document::ready`'s `Err` covers a document that has opened. Facts
  are by page index and come from the bytes alone, so if a reopen of
  the same bytes gets the earlier open's facts first, they're the
  right facts, and its own result is refused; a recovery opens a
  different file, and a close drops the view.
- **The "Recovered unsaved changes" notice is set at ready,** after
  the damaged-form notice: set at install, it was replaced by a
  damaged-form notice that came later. The four timing lines (the
  outline, third-party comments, sheaf annotations and the recovered
  notice's) are one `[timing] ready: N sheaf annotations, N
  third-party, N bookmarks, N fields in T, T since launch`;
  `recovered unsaved changes in` stays in `recover`.
- **No gpui test harness:** sheaf-app has no `#[gpui::test]` or
  `TestAppContext`, so what the Checks need of the app is in plain
  code its tests run: the model's side of its open in `opening_model`
  and `take_facts`, which `install` and `DocView::ready` call and the
  equal-opens test checks against `open::file`, and the plugin queue
  in `Waiting`. What stays in gpui code is checked by hand
  (Measured), as the plan's "Manual, recorded in the slice notes" row
  allows, or not at all: `DocView::ready` emits `Opened` after
  `take_facts`, which the probe plugin saw (and if it came first,
  `release` would give nothing, so `opened` would never go out rather
  than come early); `run` freeing the coalescer for a held region
  event and delivering `closed` only after `opened` weren't run, a
  Shift+drag or a close needing to land while a big file opens.
  There's no race to test without threads of the app's: the facts
  come back to the view's thread whole, so the queue tests (found in
  review: they had held `finish` at a `Barrier` on a thread that
  changed no assertion) read the facts on the test's thread. The
  probe plugin can't see fields (0.3's `document`
  interface has no field reads), so fields are checked by
  `opened_goes_out_with_the_facts` and the `[timing] ready` line
  printed just before `Opened`.
- **`Document::new` stays,** built complete at once with only an
  outline (`VersionId(0)`, ids as before), for about 20 test sites and
  scan.rs, which wraps a file it has just made. It reads no facts, so
  it isn't a second open.
- **The loading window was checked on a 40,000-page heavy**
  (`bench/gen.py heavy 40000`, with 11 sheaf marks added by
  `--selftest-annot`): the `--script` harness starts 1.5 s after
  launch, and the heavy 7k is ready at about 0.95 s, so nothing can be
  sent while it loads. The `opened` probe also ran on the heavy 7k.
- **The equal-opens test is sheaf-app's,**
  `the_app_opens_as_the_command_line_does`, so it runs the app's own
  model code (found in review: in sheaf-doc it compared `open::file`
  with a copy of its own three calls, and a change to `install` or
  `ready` passed it). For it `DocState` and `ViewFacts` derive
  `PartialEq` under the `fixtures` feature as well as `cfg(test)`, and
  `every_fact`, `outline_tree` and `note` moved from `open::tests` to
  `forms::fixture`. Dropping the labels in `take_facts` fails it on
  every-fact.pdf. sheaf-doc keeps
  `a_file_opens_headless_with_every_fact`.
- **The no-lex check sees what's read:** `LazyPdf::is_read(id)`,
  added to sheaf's lopdf patch (`vendor/lopdf/SHEAF.md`), says whether
  a read of an object was kept, and `finish_never_lexes_content`
  asserts no page's content stream was (found in review: every reader
  passes over what it can't read, so facts alone couldn't show a
  content stream unread). A reader that dereferences each page's
  `/Contents` fails it.
- **Smaller choices:** the equal-opens test also compares the state
  after one identical DuplicatePages edit, so the ids drawn next match
  too, and writes its fixtures to a temporary directory, since
  `open::file` takes a path; the threaded corpus loop is one helper,
  `forms::corpus::each_entry`, for sheaf-doc's corpus test, the
  equal-opens test and sheaf-render's outline test (sheaf-ops' and
  `source::lazy_tests`' keep theirs: they run PDFium or the opens on
  the test's thread beside the workers, or count into several maps);
  the facts corpus test compares other tools' comments whole (found
  in review: it compared five of their fifteen fields);
  `finish_never_lexes_content` starts from the shared `every_fact()`
  fixture and asserts the header/footer scope and the outline too;
  `no_fact_comes_after_the_first_edit` is in open/tests.rs on the
  public API, reaching the first edit's parent by undoing it;
  `annotations_read_from_a_source_as_loaded` gives its Stamp the same
  content every run (found in review: a random one left a new file in
  the generated cache each run);
  `fields_dirty_compares_with_the_saved_form` makes its dirty fields
  through the test module's access to the root's state rather than
  `opened_state_mut`, since there are no field edits yet. The facts
  corpus test's EXPLAINED holds only the 12 field-count files; unparsed
  manifest rows aren't compared on pages or fields, since pypdf
  recorded no counts for them (26 would differ, the manifest giving
  them 0 fields). The one-offs against `read_open` and against
  b75e59a's app were built from `git archive` copies in the scratchpad;
  the working tree was never touched.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The 7k harness's ready rows,** one full run at the end (step 6;
  slice 2's baselines in brackets, then slice 3's):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | ready | 50.8 ms [77.1; 77.7] | 391 ms, asserted [724; 965–1,090] | 539 ms [853; 592] |
  | ready, peak RSS / added | 29.1 / 22.0 MB [47.2 / 40.4; 59.6 / 52.4] | 278 / 264 MB [320 / 312; 449 / 436] | 514 / 507 MB [1,011 / 1,005; 1,039 / 1,032] |

  The heavy's ready was 732 ms after step 1 (the eager loads gone;
  285 / 272 MB), 409 ms after step 2 (PDFium's outline gone; 282 /
  267 MB) and 406 ms after step 3 (`open::file`; 278 / 263 MB). The
  scanned's was 547 ms and 517 MB at step 1 and 545 ms and 515 MB at
  step 3: its 474 MB blob and little else, where slice 3's eager parse
  was the other 500 MB. A probe of the heavy at step 1:
  `SourceInfo::read` 77–144 ms, the annotation pass (7,000 other
  tools' comments, a fresh source) 120–182 ms, `forms::read` after it
  60–92 ms, the catalog's readers, attachments and 2,030 outline
  styles 20 ms, and `finish` with the read 357–528 ms; the rest of the
  732 ms was `describe`'s PDFium open and outline.
- **The harness's other rows** are as in slice 3 or faster: the walks
  48.2 and 72.5 ms (asserted); on the heavy the incremental save
  140 ms [166], Rewrite 2,922 [3,082], crop 2,197 [2,326], Bates'
  Rewrite 3,731 [3,979] and flatten 3,617 [3,489]; on the scanned
  Rewrite 865 [1,201], crop 1,605 [2,256] and Bates apply 87.4
  [428]; history AnnotAdd 0.695 ms [0.686] and a full reorder 0.950 ms
  [0.945] (both asserted), and the 200 OCR versions as in slice 2.
  Later rows peak as in slice 3 or lower (the heavy's incremental save
  509 MB, Rewrite 555 MB and flatten 564 MB) but add more than slice 3
  did, the heavy's incremental save 223 MB [216] where slice 3's added
  164, presumably because the open leaves less freed memory behind
  for the old writers' eager loads to reuse (inferred). The scanned's
  incremental save read 992 ms [742] (813–891 in slice 3; it writes
  the whole file, slice 10's gate) and its warm render 21.7 ms [16.5],
  as in slice 3.
- **The app** (Xwayland, `--timing`, the dev profile; b75e59a's
  sheaf-app built from `git archive` against step 4's): first pixels
  on the synthetic 7k 0.463–0.529 s (6 runs) against 0.482–0.507 s
  (3; slice 3's notes 0.510–0.529); on f1040 1.327–1.392 s in 6 runs of
  10 and 0.392–0.435 s in 4, the ones whose page preview came before
  the first tile, against 1.357–1.386 s (3; slice 3 1.35–1.41); on the
  heavy 7k 1.51–1.55 s (6) against 2.56–2.58 s (3). The heavy's ready
  in the app is 426–473 ms after the open and 944–989 ms after launch,
  where b75e59a's late loaders took 431 ms for the outline and
  1.01–1.03 s for the third-party comments; at step 5, ready 0.95 s
  and first pixels 1.50–1.69 s.
- **The open's progress, by hand** (Xwayland, `--timing`, the dev
  profile, the 40,000-page heavy, `bench/gen.py heavy 40000`, no
  marks): ready 3.54 s after the open and 4.82 s after launch; the
  status bar, redrawn when the busy main thread could, said "Opening:
  page 636 of 40000…", then 3,021 and 32,895 within the first 1.0 s,
  then "Opening…" for the rest. Headless (`open::file`, release): the
  heavy 7k 384–511 ms, the 40,000-page heavy 2.06–2.76 s. The 7k
  harness after the review's fixes: ready 416 ms on the heavy
  (asserted), 449 ms on the scanned and 48.7 ms on the synthetic.
- **The app by hand:** the selftests pass (f1040: insert f1040s1, 4
  pages, with the "fields are under f1040s1" notice, and a Rewrite
  save; the synthetic 7k: an insert, 7,002 pages in 29 ms, and a
  Rewrite save, 7,000 pages in 1.02 s). After `--selftest-annot` on
  f1040, reopening the saved file brings its 11 sheaf marks in at
  ready, each drawn once (a screenshot), and Bookmarks says "No
  bookmarks". Recover gives 4 pages and "Recovered unsaved changes —
  Ctrl+S to keep them"; corpus 0619933 says "This form can't be
  filled: the field tree is more than 32 deep". A hand-built file's
  closed top-level bookmark starts collapsed and red, the open one
  expanded, and "What’s" (PDFDocEncoding) reads as written. On the
  40,000-page heavy (ready about 5.7 s after launch): `r` while it
  loads says `STILL_OPENING`, and after ready Edit > Undo is grey, the
  title isn't dirty and the page isn't rotated, so no version was
  made; the Document menu is grey, with "Opening…" in the status bar,
  until ready, and enabled with its labels after; a probe plugin
  (events `opened` and `selection-changed`, a command on ctrl-alt-p)
  whose command was pressed while loading was held and ran after
  `Opened`, both logging 40,000 pages, 11,600 bookmarks and 11 marks.
  On the heavy 7k the probe's `opened` logged 7,000 pages, 2,030
  bookmarks and 0 marks, and on f1040 with 11 marks 2 pages and 11
  marks (199 fields by the ready line).
- **The corpus** (`../pdf-corpus`, 4,392 manifest files):
  - **The open's facts from the source against the eager reading:** 3
    sources don't open, and every other file loads eagerly; no file
    numbers its pages otherwise in the page table than in `get_pages`,
    and slice 3's Unlisted and Shadowed files show no fact difference.
    Every fact compared eagerly is equal on every file. 12 files
    differ, all in their field count against the manifest and all
    with fewer than the row: 11 Fewer (pypdf counts twice, or lopdf
    loses an object) and 1 Damaged (0619933, past 32 deep), the files
    sheaf-ops' manifest test lists by count. Totals: 943 other tools'
    comments, 89 attachments, 906 layers, 416 files with labels, 21
    with bookmark styles; no sheaf marks, header/footers or portfolios
    (the fixtures have them). 6.8 s (the test profile).
  - **A one-off against `read_open` itself,** built from b75e59a's
    sheaf-doc: per-row dumps of its facts and of `finish`'s are
    byte-identical on all 4,392 rows (3 unopened both ways); `finish`
    took 2.15 s and `read_open` 7.20 s (release, 16 threads).
  - **The source outline against PDFium's:** PDFium opens 4,388 files,
    865 have bookmarks, 12,840 bookmarks, and one listed difference
    (1260393's indirect `/S`). Before the title decoder, 56 files'
    titles differed (PDFDocEncoding, and one dangling UTF-16 byte). A
    one-off before `outline_styles` went: the new reader's colours and
    styles equal its on every styled file (21), the one difference a
    title with a control character. 17.0 s, about 10 s of it PDFium's
    serialized walk and page loads; it stays in the default suite,
    beside slice 3's Geom-against-PDFium check.
  - **A one-off of the root against the old loaders'** (step 3, before
    open.rs went): `open::file`'s root against
    `open_with_foreign`'s on every row (pages, marks by index, the
    outline's titles, pages, depths, views, colours, styles and open
    state, labels, the header/footer, fields, widgets by index,
    foreign names, dirty and undo, and the ids themselves). Every row
    is equal, ids included; the same 3 files don't open either way:

    | kind | files | unopened | differ | pages | marks | bookmarks | closed (with kids) | styled files | with labels | with header/footer | fields | widgets | foreign |
    |---|---|---|---|---|---|---|---|---|---|---|---|---|---|
    | acroform | 3,786 | 0 | 0 | 22,272 | 0 | 12,396 | 172 | 21 | 394 | 0 | 270,872 | 291,349 | 925 |
    | hybrid | 267 | 0 | 0 | 768 | 0 | 31 | 0 | 0 | 17 | 0 | 26,386 | 26,981 | 13 |
    | unparsed | 30 | 3 | 0 | 441 | 0 | 202 | 0 | 0 | 4 | 0 | 1,189 | 1,320 | 5 |
    | xfa-only | 309 | 0 | 0 | 775 | 0 | 211 | 3 | 0 | 1 | 0 | 0 | 0 | 0 |
    | total | 4,392 | 3 | 0 | 24,256 | 0 | 12,840 | 175 | 21 | 416 | 0 | 298,447 | 319,650 | 943 |

  - **App and headless opens:** 4,392 files, 3 open neither way, none
    otherwise in the app; 4.9 s.
- **Suites:** `cargo test --workspace` 625 passed, 17 ignored after
  the review's fixes (620 in 101 s before them; 594 at slice 3);
  sheaf-doc's unit tests, the corpus's among them, 308 in 19.1 s (306
  in 23.0 s before). The ledger walks 241 files in 1.4 s.

Found:
- **`pdf_text` reads a string with no BOM as Latin-1,** not
  PDFDocEncoding, and a dangling UTF-16 byte as a NUL, so other tools'
  comments' text and authors with curly quotes or dashes read
  wrongly, as bookmark titles would have (deviations). Left as it is:
  `anon_name`, the name of a comment with no `/NM`, depends on its
  exact output.
- **The heavy 7k's first pixels were 2.56–2.58 s on b75e59a,** behind
  the late loaders and their `touched()` calls; 1.51–1.55 s now.
  f1040's first pixels fall into two groups before and after, about
  1.33–1.39 s when a tile is drawn first and 0.39–0.44 s when the
  preview is.
- **PDFium finds no page for a GoTo action whose `/S` is indirect**
  (1260393); the source reader follows it.
- **Fixed on the way:** a damaged-form notice that came after the
  recovered one replaced it; a plugin call made before the pages
  showed ran with no document context.
- **A tree that lists a node more than once never finished reading**
  (found in review, fixed): `walk_names` (`/Names /Dests` and
  `/EmbeddedFiles`), the labels' `collect_nums`, the layers'
  `walk_order` and the portfolio's `walk_folders` capped only the
  depth, so a node listing itself twice in `/Kids` (or an `/Order`
  holding itself twice) took 2^32 visits, and a folder that's its own
  `/Next` looped 10,000 times a level. Before this slice the late
  loaders ran them, and only those facts were lost; with one open,
  the file never became ready, and `open::file`, so every CLI
  command, hung. Each now reads a node it reached by reference once
  (the folders' 10,000 cap went with it);
  `a_tree_that_lists_itself_is_read_once` timed out at 10 s before
  the fix and takes under a millisecond after.
- **PDFium reads PDFDocEncoding's undefined bytes 0x7F, 0x9F and
  0xAD as NUL,** so a title shows a space there (found in review);
  the title reader showed DEL, U+009F and a soft hyphen. Fixed.
- **Before this slice, and outside the 7k gates:** on a 40,000-page
  heavy the main thread is slow while the file loads (each script
  step 0.5–2 s), and first pixels come at about 20 s, as on b75e59a
  (22.3 s). `cargo clippy -p sheaf-app --no-default-features
  --all-targets -- -D warnings` fails on
  `contrib::Contributions::add_panel`, dead in test builds without
  plugins (CI runs clippy with the default features). The outline
  panel draws `/C` but not bold or italic.
- **The `--script` harness's fixed 1,500 ms start can't reach the
  heavy 7k's loading window,** so a check of the loading state needs
  a bigger file.

Checked by tests:
- over the corpus, the root's facts equal the old loaders':
  `the_open_s_facts_from_the_source_equal_the_eager_reading` (each
  manifest row's pages and fields against the row, every other fact,
  other tools' comments whole, against the eager reading, the 12
  listed files by kind; it fails on a difference not listed or a
  listed one gone) and
  `the_source_outline_is_pdfium_s` (sheaf-render; the outline against
  PDFium's, the old loader's, one listed file), with step 3's one-off
  (Measured);
- app and headless opens produce equal states:
  `the_app_opens_as_the_command_line_does` (sheaf-app; the app's
  `opening_model` and `take_facts` around `finish` against
  `open::file`: whether it opens, the `DocState` with its ids, the
  `ViewFacts`, the version, dirty and undo, and the state after one
  DuplicatePages; on `every_fact()`, every_kind and the corpus; a
  recovered file the same but unsaved; facts from other bytes, or a
  second time, not taken); dropping the labels in `take_facts` fails
  it on every-fact.pdf; and `a_file_opens_headless_with_every_fact`;
- first paint is unchanged: by hand (Measured), `start` reading
  nothing beyond the page table;
- ready within 1 s on the heavy 7k: gate_7k's ready row asserts
  (391 ms; 416 ms after the review's fixes); past it, edits wait with
  the pages read shown: `the_status_bar_counts_the_pages_read`,
  `a_file_opens_headless_with_every_fact` (`finish` counts 3 of 3
  pages) and by hand on the 40,000-page heavy;
- an edit before ready returns `NotReady`:
  `an_edit_before_ready_is_not_ready` (`apply`, `apply_batch` and
  `apply_in_job`, with no version and no ids drawn, then taken after
  ready), `ready_replaces_the_opened_version` (a new id, the loading
  one gone, the same pages, every fact, not dirty, nothing to undo, a
  second ready refused), and in the app `rules_follow_the_facts`'
  loading case (Rotate, the Note tool, Save As, Print and a plugin
  command off; Zoom, Next Page and Find on; a plugin command off
  before the pages show too) and by hand;
- a plugin's `opened` never comes before the facts:
  `opened_goes_out_with_the_facts` (every_fact's mark and field
  through the app's `opening_model` and `take_facts`; calls held while
  the facts are read and after, nothing released until they're
  taken; then `opened` first and the snapshot has the mark and the
  field), `no_fact_comes_after_the_first_edit` and the probe plugin
  by hand;
- a queued plugin call runs after ready:
  `nothing_leaves_the_queue_before_ready` and
  `a_queued_call_runs_after_opened` (`Opened` first, then the held
  calls in order); taking out the ready check fails the first and
  `opened_goes_out_with_the_facts`;
  `a_held_region_gives_way_to_the_newest` (a plugin's newest region
  event or clearing, other calls and other plugins' regions in
  place);
- a grep finds no "arrived after edits": `rg 'arrived after edits'
  crates` finds nothing;
- the open never lexes content: `finish_never_lexes_content` (no
  page's content stream is read, by `LazyPdf::is_read`, and pages
  whose `/Contents` is Flate garbage still give their mark, labels,
  form, header/footer scope and outline);
- trees that list a node again end:
  `a_tree_that_lists_itself_is_read_once` (`/Dests` named by a
  bookmark, `/EmbeddedFiles`, `/PageLabels`, the layers' `/Order` and
  a portfolio folder that's its own child and sibling, each read
  once, under a 10 s timeout);
- lazy equals eager on each reader's own fixture: labels, layers,
  portfolio, the header/footer save test,
  `a_source_lists_and_reads_as_the_loaded_file` (attachments listed,
  then each read) and `annotations_read_from_a_source_as_loaded` (a
  mark, a companion, another tool's comment and its reply, another
  tool's Stamp through `form_to_pdf`, a FileAttachment);
- the outline reader: `an_explicit_dest_names_its_page`,
  `named_destinations_are_looked_up`, `a_goto_action_is_followed`,
  `an_integer_is_a_page_index`,
  `a_page_listed_twice_is_its_first_index`,
  `positions_are_in_displayed_space`,
  `positions_sheaf_writes_come_back`,
  `every_fit_kind_keeps_its_numbers`, `colors_styles_and_titles`,
  `titles_decode_as_pdfium_shows_them` (literal bytes: PDFDocEncoding's
  table whole, its accents, quotes and undefined bytes, control
  characters, UTF-16 either way with a dangling byte, UTF-8 after its
  mark, each checked against PDFium in a one-off),
  `a_negative_count_is_closed`, `a_cycle_reads_each_item_once` and
  `depth_past_32_is_cut`, with
  `positions_round_trip_through_pdfium_and_the_source` and
  `bookmark_styles_come_from_the_file`;
- the closed state through a save:
  `a_closed_bookmark_stays_closed_through_a_rewrite` (Rewrite and
  incremental: Closed −2, the one under it 1, Open 1, the root 3);
- the ledger: `nothing_retired_is_back` with the 30 rows, and
  `a_root_item_is_named_from_its_root`.

Left for later slices:
- **Slice 5:** bookmark views and marks in PageSpace through `Geom`,
  and `page_geom` over `Geom` (the outline reader's `/XYZ` goes
  through its frame today).
- **Slice 8:** bookmark records with `src` and `Target::Kept`, and
  other tools' comments as records (where `pdf_text`'s encoding and
  `anon_name` can be settled together). Bug 15's other losses on a
  rewrite, `/A` actions, `/SE` and named destinations, in slices 8
  and 9.
- **Slice 9:** the `SaveRecord` synthesized at open (§9.1); the
  `Objects` seam and the eager test helpers (`read_annots*`,
  `#[cfg(test)]`) with the old writers, but for
  `convert_page`'s eager user, exchange's `Comments.pdf`, which keeps
  the seam or is built as a source; `refresh_facts` and
  `reread_foreign` go with save's re-read.
- **Slice 11:** `NotReady` becomes `EditError::NotReady`.
- **Slice 12:** `DocView::commit` returns it, and ready gives a
  `ChangeSet`.
- **Slice 13:** `hide_annots` at ready goes with the realizer.
- **Slice 19:** `Session::open` over `open::file`, and combine's first
  file.
- **Slice 20:** WIT 0.4's `opened (when ready)`.
- **Any slice:** `Incoming::read` (insert and replace) runs the whole
  `finish`, reading labels, attachments, layers and a header/footer
  it throws away, which costs little; counting the form's read too,
  if a file ever needs it.
