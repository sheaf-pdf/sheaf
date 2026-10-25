# M10.5 slice 13: Realizer, pool, render keys, view inputs

Status: done · Part of [M10.5: the edit API](README.md)

**Realizer, pool, render keys, view inputs** (XL; proposal 16; bugs
22 and 24).
- **Overlays:**
  - group overlays with slice 2's caps;
  - the per-source overlay allocator;
  - symbolic deltas per page, deduplicated at assembly;
  - encryption at assembly, or the fallback;
  - standalone groups for damaged bases and failed base classes;
  - catalog-only view patches;
  - `RotatePolicy::KeepBase`;
  - box edits on the base engine, if spike C passed.
- **`EnginePool`:**
  - one worker;
  - visible engines pinned;
  - LRU by count and bytes;
  - a background budget.
- **`RenderKey`,** with generators keyed on `needs()` and ViewInputs
  on what is drawn.
- **`ViewInputs`,** including `content_only()`, and
  `Tx::read_render`.
- **Background previews** for dialogs.
- **Deletes:**
  - `RenderHandle`'s thread per source and `open(PathBuf)`, and
    `Doc.handles` per source;
  - `hide_annots`/`set_annots_hidden` and their replays, and
    docview's `drawn_marks` and `foreign_hidden` (hiding is data,
    `ViewInputs.hidden`);
  - the layers panel's decrypted temp copy;
  - the GPUI header/footer overlay;
  - `drop_stale_images` and the global `invalidate_tiles` calls.
- **Checks:**
  - **Pixels and correctness:**
    - over the corpus (every form, and a sample of its pool), each
      realized page renders as the saved file (appearances only),
      within 16 pixels of anti-aliasing a page (Greg, 2026-10-20),
      every page that differs at all printed and counted;
    - encrypted bases render;
    - a page rotated with an unsaved OCR layer renders once,
      rotated;
    - two edited pages in one group sharing a font, plus a
      duplicate, render correctly with no duplicate xref numbers;
    - an unsaved header is visible under view rotation.
  - **Invalidation:**
    - hiding a comment leaves print preview and the OCR input
      unchanged;
    - a 7k reorder re-renders nothing without live-token generators,
      and with "Page n of N" only the covered pages;
    - dragging a mark under `ByHost` changes no `RenderKey`.
  - **Engines and threads:**
    - no new temp files after using layers;
    - the thread count is constant across 20 inserts and crops;
    - engine memory stays under its cap after 200 groups;
    - background OCR doesn't evict visible engines.
  - **Gates:**
    - realize + open + render takes under 15 ms on the synthetic
      file, and under 25 ms on the heavy one (Greg, 2026-10-20: the
      planned 1.7× the warm base render isn't met by any design that
      opens an engine);
    - after a crop and the legacy header on all 7,000 pages, the
      thumbnail strip fills at slice 2's rate within the cap.

## Notes

Done. Realizer, pool, render keys, view inputs (proposal 16; bugs 22
   and 24): PDFium draws the model. Each consumer passes what a page
   should draw (`ViewInputs`: the screen's, print preview's, OCR's
   `content_only`, a plugin's), and a page's `RenderKey` is a digest of
   what it draws under them, computed only for the pages drawn. The
   realizer sends a page that draws as its base page does (`untouched`,
   checked against the emission) to its source's base engine, and any
   other to its group's overlay: an in-memory append section over the
   source's bytes, scoped to the group's pages, written by the save
   emitter in a new overlay mode and the save serializer, encrypted with
   the source's key, numbered by an allocator a source; a damaged base's
   group is a file of its own, and a layers choice a catalog-only patch.
   One `EnginePool` thread serves every engine of the process (visible
   engines pinned, 6 group engines in 96 MB, a background budget of 2 in
   32 MB, liveness, failures remembered), in place of a `RenderHandle`
   thread a source. The app's tiles and previews are found by render key,
   a page keeping its last image until its new one lands; hiding is data;
   print preview, OCR, plugins, text and search, opening, inserts and the
   Header & Footer dialog's new preview draw through views. The hide
   replays, `drawn_marks`, `foreign_hidden`, the layers panel's decrypted
   copy, the GPUI header overlay, `drop_stale_images`, the global
   `invalidate_tiles` and `RenderHandle` with `Doc.handles` are deleted.
   The new 7k gates pass, the heavy's realize gate at the < 25 ms Greg
   accepted in place of the planned 1.7× its warm render (2026-10-20), the
   Rewrite is 1.13× and 1.11× slice 8's writer, and the corpus counts
   hold. Greg's rulings of 2026-10-20 are in (deviations): that gate,
   the pixel tier's anti-aliasing allowance, and a base PDFium refuses
   drawn from its realizations.
   - **Render keys and view inputs** (step 1, `sheaf_doc::realize`).
 `ViewInputs { marks: MarkDraw, generators, ocr_over, widgets,
 foreign, hidden, dragging, ocg_off }` with `screen()`, `print(markup)`
 and `content_only()`; `MarkDraw::{ByHost, ByEngine, Off}`;
 `OcgOverride { off: BTreeMap<BlobId, BTreeSet<LayerId>> }` (an entry:
 every layer but those). `render_key(st, page, inputs)` hashes through
 a private `Feed` over each record destructured whole: the inputs'
 flags; `PageDef::digest()` (base and content, not copy); the boxes
 without rotate (with it for a page with an undrawn header); each drawn
 annotation by content (marks under `ByEngine`, others' with `foreign`
 and whether each link leads nowhere, widgets with their `Terminal`),
 hidden and dragged flags; `FormDoc::digest()` when a widget is drawn;
 each generator's id, recipe, whether the file draws it and, when not,
 `CtxNeeds` (position, count, Bates); the source's `ocg_off` entry.
 `render_reads(st, page, inputs)`, the slots a page draws from, and
 `Tx::read_render(page, inputs)`, which notes them; digests kept in
 `DigestCell`s (`PageDef::new`, `FormDoc`'s cell); per-state tables in
 `Indexes` (`header` for `header_covers` and `bates_of`, `shows` for
 `page_showing`/`page_named`/`pages_showing`, a B-tree), checked by
 `check_invariants` and counted by `snapshot_bytes`;
 `DocState::render_tables()` destructures `DocState` whole (the
 registry rule). OCR's flush, blank review, gate_7k's OCR run and
 plugin text read `render_reads` under `content_only`, a plugin's
 render under `ByEngine` (the transitional row 12 → 13 done).
   - **The emitter's overlay mode** (step 2). `emit(st, scope, base,
 transforms, p)` with `EmitScope::{All, Keys, Pages, Frame}` and
 `EmitBase::{Rewrite{primary, derived}, Saved{..}, Overlay(&Overlay)}`,
 `Overlay { src, inputs, rotate: RotatePolicy::{Model, KeepBase} }`;
 `Cx` holds the state and its primary; setup O(scope) through the
 state's tables, `claim`, `owned_pages` and `form::prepare` given the
 pages (`Plan::scoped`); `copy::Pages::{Written, Overlay}`. The overlay
 writes the group root `PagesNode(0)`, each page under it with its
 four inheritable keys, the source's catalog at its root's number (its
 `/Pages`, `/AcroForm` and patched `/OCProperties`), the scoped form
 (`/Fields` the roots in id order, `/Kids` cut, a one-root form's
 dictionary Raw and its root rebuilt), and filters by the inputs
 (`Cx::draws`, `draws_unmodeled`, `hides`); `layers::ocg_patch`. Saves
 write the same bytes (the corpus identity, determinism and four-times
 runs).
   - **The realizer** (step 3). `GROUP` 128, `GroupKey`, `EngineKey::{Base,
 Group, SourcePatch}`, `JobXform`, `PageView`, `Realization`,
 `Realizer::{view, open}` (Send + Sync, each cache behind its lock, the
 groups memoized per state by `DocState::draws_alike`);
 `untouched.rs` (`untouched`, and `emission_is_base` for tests and
 fixtures); `alloc.rs` (`Numbers` over the new
 `serialize::append::Base::over(&Source)`); `assemble.rs` (`Part::page`
 and `Part::frame`, `numbered`, `section`, `standalone`, `patch`); the
 deltas cache, `DELTAS_BYTES` 32 MB. `render_appearances` keeps a
 page's inherited box through the flatten. Ten pixel tests
 (`crates/sheaf-ops/tests/realize.rs`, 13 tests with steps 6 and 7's,
 14 with the refused base's) and `realize_corpus` (ignored).
   - **The engine pool** (step 4, `sheaf_render::pool`). `EnginePool<K>`,
 `Target`, `Opener::{base, overlay}`, `Budget::{Screen, Background}`,
 `Req`, `Owner` and `pin`, the queries and `search`, `open_engines`,
 `weighed`, `worker_task`; `Segments`; `SCREEN_ENGINES` 6,
 `SCREEN_BYTES` 96 MB, `BACKGROUND_ENGINES` 2, `BACKGROUND_BYTES`
 32 MB, `ENGINE_BYTES` 16 MB, `OPEN_BYTES` 4 MiB, `PAGE_BYTES` 75 KiB.
 `Draw { annots, forms }`, `PdfiumEngine::{open_segments, render_with,
 render_appearances}`, `Job::turned`. `sheaf_ops::views::Views {
 realizer, pool }` with `view`, `render`, `draw`, `target` and `base`.
 Twelve pool tests (`crates/sheaf-render/tests/pool.rs`; 14 with step
 7's failed open and the review's panic test, 15 with the refusal's).
   - **The screen** (step 5). The process's pool (`docview::engines()`),
 `Doc.views`, `Doc.inputs` and `Doc::set_inputs`, `Doc.drawn` and
 `Doc::image_key` (the last-drawn fallback, by page and turn),
 `Want` and `Doc::request`, `Pins` (each pane's, `Doc.strip_pins`),
 `Doc.layers_chosen`; tiles by `(RenderKey, Job)`, previews by
 `(RenderKey, Rotation)`; `pane::Shown { id, geom }`, the layout made
 again only on the order or a geometry. The layers panel sets
 `ocg_off` and the notice; Hide Comments and the filter set `hidden`;
 a comment drag sets `dragging`.
   - **Print preview, OCR and plugins** (step 6). `PrintContext.views`,
 `preview` and `render_preview` under `ViewInputs::print(markup)`,
 pinned; `sheaf_ocr::PageSource::{text_len(&OcrPage), render(&OcrPage,
 width)}`, one source a run, and `ViewPages`; plugins' `TextSource`
 (`&Src`, index), `OcrSource::recognize(&Snapshot, PageId, ..)`,
 `RenderSource::render(&Snapshot, PageId, ..)` over `Views`
 (`EngineText`, `EngineOcr`, `EngineRender`).
   - **Text, search, opening, inserts and the header preview** (step 7).
 `Doc::search` and text selection on base engines; `ask_form_type` at
 install and `doc.forms.form_type`; the first failure as the notice;
 `Worker.failed` (`Unopened`); page_insert without engines
 (`header_footer_op` shared by the apply and the preview);
 `HeaderFooterContext { lower, previewer }`, `Preview::{begin, take}`,
 `derive::strip_stamps_of`; `RenderHandle`, `Msg`, `worker_loop`,
 `SearchWalk` and `DocInfo` deleted.
   - **Gates, ledgers, docs** (step 8). Three gate rows and asserts, the
 delta cache's weight (Found) and the derived copy's form (Found);
 retired.rs gains 39 rows and `QUALIFIED_ONLY` nine names; the
 transitional, lint and gate ledgers; docs/document-model.md's
 "Rendering edited pages (M10.5 slice 13)" and its forward references,
 README's Document edits and Rendering strategy rows, docs/plugins.md's
 render reads.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and listed in retired.rs: `RenderHandle` (with
 `open_bytes`, `info`, `search`, `pattern_search`, its thread), the
 root `Msg` with `HideAnnots` and `SetHidden`, `worker_loop`,
 `SearchWalk`, `DocInfo`; `Renderer::{hide_annots, set_annots_hidden}`
 and `PdfiumEngine`'s; `Doc::take_in` (and its call in page_insert),
 ready's `drawn_marks`/`hide_renamed`/hide/invalidate block, the
 `drawn_marks` field and function, `marks_by_page` (and
 `renamed_marks_are_hidden_by_their_old_names`, its notice assertions
 kept as `one_notice_counts_the_renamed_marks`), `hide_renamed`,
 `Doc.foreign_hidden` and `comments_panel`'s
 `sync_foreign_visibility`; the layers panel's `load_pdf_mem` /
 `set_hidden` / `save_to` / `open_bytes` path and `layers::set_hidden`;
 `header_footer_elements`; `drop_stale_images`, `invalidate_tiles`,
 `tiles_invalidated`, `Doc.shown`, `Doc.layers_generation` and the
 rebase relayout; `pane::Shown`'s `source`, `index` and `render`;
 `Doc.handles`, `Doc::primary`, `PlannedInsert.handle`, `with_engines`,
 `Copies`; `PrintContext.handles`; `OcrPage`'s `source` and `render`
 and sheaf-ocr's `impl PageSource for RenderHandle`;
 `PageRenderer.base`, `EngineText::handle`; emit's `Cx.input`;
 gate_7k's `engines_on`. `RenderHandle::open(PathBuf)` went in slice 3;
 `PdfiumEngine::open(path)` stays for print until slice 14.

   Deviations from the plan (the planner's list first, as resolved):
   - **Where the types live.** sheaf-render doesn't depend on sheaf-doc and
 sheaf-doc mustn't pull in PDFium, so `EngineKey`, `GroupKey`,
 `PageView`, `RenderKey`, `ViewInputs` and `Realizer` are sheaf-doc's
 (`realize`), `EnginePool<K>` is generic over its key, each request
 carries an opener and a liveness probe instead of a `Weak<Source>`,
 and the glue (`Views`, `ViewPages`) is sheaf-ops'. `PageView.index` is
 a `u32`. `(key, opener, budget)` are bundled into `Target<K>` (flat
 parameters put `search_page` past clippy's argument limit); the opener
 is an `Arc<dyn Fn>` a search walk can open again, and says whether
 its engine is a base engine (the pool can't read sheaf-doc's key).
   - **`Base::of` is retired** (slice 9); the overlay's base is the new
 `serialize::append::Base::over(&Source)`.
   - **`EmitScope::Pages` over a state.** `emit()` takes `&DocState` and
 the primary and derived table moved into `EmitBase::Rewrite` and
 `Saved`, rather than a detached `SaveInput` with a meaningless
 version; an overlay refuses a page of another source.
   - **The overlay always writes its pages** (a new `/Parent`), so the
 base-or-overlay choice is a model predicate (`untouched`) checked
 against the emission compared modulo placement, not "the emission
 writes nothing".
   - **Derived copies** (crops, watermarks, stripped headers until 15–17):
 an overlay over a copy takes the page as the copy has it, passing no
 derived table; the records draw its widgets, the copy's widget
 entries are left out (a widget outside the form on the original page
 isn't drawn there, which the Rewrite keeps: transitional, row 5660),
 and, from step 8, its form lists only the records' fields (Found).
   - **Spike outcomes:** A passed, so sections are encrypted with the
 source's key and no decrypted copy is made; B failed on damaged bases
 only, so standalone groups are for `!base_sound()` bases; C failed, so
 no box edit is drawn on a base engine and `JobXform` carries only the
 turn.
   - **Scoped groups, written out:** each page's four inheritable keys
 written and `/Parent` the group root, as §11.3 and §11.6 do; the
 AcroForm scoped more tightly than "the top-level fields with a widget
 on the pages": each node's `/Kids` cut to the scope, so a one-root
 form loads only the group's fields, in field id order.
   - **Deltas by `(PageId, RenderKey)`,** not `RenderKey` alone: a delta
 names its page's own keys, which a duplicate with the same key
 doesn't share; duplicates still share images. A group is emitted as
 per-page deltas plus a frame (`EmitScope::Frame`), since the group's
 root, catalog and form are built over all its pages; a source number
 claimed twice goes to the first, the other renumbered and read again.
   - **The heavy's realize gate is < 25 ms** (Greg accepted it,
 2026-10-20), asserted in the gates run as the synthetic's < 15 ms is
 (`REALIZE_GATE_HEAVY`, `Kind::realize_gate`); the row prints the
 ratio to the warm render as information. The plan's ≤ 1.7× the warm
 base render isn't met by any design that opens an engine for an
 edited page (slice 2: the heavy's scoped open and cold render alone
 are 2.9×). Greg: "it's more important to get it correct. we can
 optimize performance once the algorithm works properly" (Left for
 later). `the_heavy_realize_gate_as_planned`, `REALIZE_RATIO_HEAVY`
 and `REALIZE_PROPOSED_HEAVY`, which held the plan's ratio until he
 ruled, are deleted.
   - **The pixel tier's anti-aliasing allowance** (Greg, 2026-10-20):
 `realize_corpus`'s pixel tier passes a page when at most 16 pixels
 differ from the Rewrite (`ANTI_ALIASING_PX`). Its differences were 1
 to 9 pixels of PDFium's edge anti-aliasing, on pages whose dictionary
 and content streams are the same in both files, and which pages
 differ changes with what the process drew before (PDFium's caches),
 so they aren't sheaf's output. Past 16 fails as before; every page
 within it is printed (file, page, engine, count), and the class table
 says "N of M pages differ, K of them within the anti-aliasing
 allowance". `SHEAF_REALIZE_ONLY` (corpus paths, comma-separated)
 narrows a run by hand; unset, as the tiers run, every file is checked.
   - **A base PDFium refuses draws from its realizations** (Greg,
 2026-10-20). Why PDFium refuses 4422502: the file is a first revision
 whole (its catalog 2248, trailer `/Root 2248 0 R /Size 2250`, its
 table at 599,350 and its `%%EOF` at 644,504), then the tail of a
 second revision's table, its trailer (`/Root 2252 0 R /Size 2254`)
 and `startxref 600541`: the revision's objects 2250–2253 and the head
 of its table were cut out. `startxref` leads into the middle of the
 first table, so PDFium rebuilds the cross-references, takes the last
 trailer, finds no object 2252 and fails with `FPDF_ERR_FORMAT` (qpdf
 says the same: "recovered trailer has no /Root entry"); sheaf's
 reader rebuilds them and finds the catalog the file has. Moving that
 `startxref` to 599,350, or that `/Root` to 2248, makes PDFium open it
 (5 pages). `serialize::fixture::revision_cut_out` builds the same
 shape (a lopdf file, a trailer naming a catalog past every object, its
 `startxref` 20 bytes into the first table), which PDFium refuses the
 same way, so no test seam was needed. A file sheaf reads as sound can
 be refused too: more than 1,024 bytes of junk before `%PDF-` (sheaf
 finds the header anywhere, PDFium in the first 1,024 bytes alone), so
 a section or view patch over its bytes may be the first engine to
 fail. The behavior: the pool tells a source when an engine over its
 bytes fails to open with a load error (its base engine, its view
 patch, or a group's section over a sound base;
 `sheaf_render::OpenError::Refused`: any PDFium load error but a
 password, `Opener::on_refused`), at the open and at each request told
 of the remembered failure after (a second document on the same bytes
 learns it); the source keeps it for its life (`Source::base_refused`,
 `refuse_base`), costing nothing for files PDFium opens. Then no page of
 it is on its base (`on_base`): every page renders from its group, a
 file of its own (`assemble::standalone`, `realize::standalone`), the
 group's digest and the found-groups cache keyed by it, so an engine
 the pool remembers as failed isn't asked again under its old key. A
 render already asked of an engine over the source's bytes renders
 from the page's view found again (`Req::fallback`, a `Fallback<K>` the
 pool runs once on a refusal, which `Views::draw` gives every such
 view), by the same job; such an overlay asked for after the refusal
 by a view made before it fails as refused without opening
 (`Views::target`), so its fallback runs too. A section PDFium refuses
 over a base it reads would be drawn standalone too, unseen in the app;
 `realize_corpus`'s pixel tier fails on one instead. The consumers:
 the screen, the strip, print preview, OCR, plugins' renders and the
 Header & Footer preview all draw through `Views::draw`, so none sees
 the failure; a render key is what a page draws, not where from, so the
 image lands under the key the next frame's view has, and no blank
 image is kept. The notice at the form type's ask says the pages are
 drawn from sheaf's copy and Find and selection don't work on it.
 What reads the base engine or the file in PDFium still fails: its
 queries, printing, Remove Blank Pages and Trim White (For Greg).
   - **No `form_draw`; `foreign: bool` added; `print(markup: bool)`.** No
 consumer draws appearances alone on a shared engine (the flatten
 mutates the page), there is no `PrintOpts`, and print preview without
 markup and `content_only` draw no annotations, which no listed field
 could say. Whole classes go by render flags (`Draw { annots, forms }`)
 not overlay filtering: widgets draw only through `FPDF_FFLDraw`, and
 the matrix path with neither places pixels as the `FFLDraw` path does
 (`annotation_flags_draw_widgets_alone`, every rotation, tiles, A4).
 Overlays still write widgets whatever the flags.
   - **`content_only`** is the page's own content at its shown box, no
 annotations, generators or OCR; "over the whole MediaBox" is Trim
 White's (15). Blank detection and Trim White keep their own engines
 (`sheaf_ops::engine_for`, no threads) until 14 and 15.
   - **OCR's `PageSource`** takes the `OcrPage` (`text_len(&OcrPage)`,
 `render(&OcrPage, width)`, no rotation: `ViewPages` turns the page by
 its view), with one source a run; the adapter is a named struct
 (`ViewPages`), as a tuple impl breaks the orphan rule. `OcrPage` lost
 `source` and `render`; `OcrSource::recognize` and `RenderSource::render`
 take the call's snapshot and page; `TextSource` takes `&Src` (the
 pool's base target needs the source to open). sheaf-render moved to
 sheaf-ocr's dev-dependencies.
   - **Plugin text, search and recognize** read base-engine text until
 slice 14, so their reads are `render_reads(p, content_only)`
 (`Page(p)`); `page_text_layer` keeps `Key::Page(p)`; `render_reads` is
 a free function, since `HostState` isn't a transaction.
   - **The layout** holds no source page (`pane::Shown { id, geom }`) and
 is made again only on the order or a geometry; a same-size rebase
 changes keys only.
   - **Keys from per-Arc digests,** not `ChangeSet.content`: keys are
 computed for the pages drawn, and no cache is invalidated by a
 ChangeSet; position, count and Bates come from lazily built per-state
 tables (bug 24: the GPUI overlay computed every Bates number for each
 page drawn).
   - **Stamp and tape pictures** stay as they are (stamp_preview.rs renders
 a one-page in-memory PDF on an engine it drops; M10 slice 12 deletes
 them, D53): slice 6's "through the realizer" is met as not needed.
   - **The Mark record** isn't restructured before render keys read marks
 (slice 11's For Greg): keys digest marks only under `ByEngine`, as the
 record is, through its `AnnotKind`'s `Debug`.
   - **The form type** comes from a pool query on the primary's base engine
 at install (`ask_form_type`, `doc.forms.form_type`), and `open_time`
 went; a PDFium-only open failure is the document's notice at that
 first open ("The pages can't be drawn: …"), and `needs_password` asks
 only the model's reader.
   - **retired.rs** gains `QUALIFIED_ONLY` `handle`, `handles`, `primary`,
 `take_in`, `shown`, `index`, `render`, `base` and `input`, each with
 its reason, checked with probe files. The root `Msg` has no row: the
 ledger matches a one-segment row by a path's first segment, and the
 pool's own `Msg::Render` would hit it; `Msg::HideAnnots` and
 `Msg::SetHidden` are rows, and `impl PageSource for RenderHandle` is
 covered by `RenderHandle`'s.
   - **Pages listing sheaf's marks** in their own `/Annots` realize as
 overlays under `ByHost` even unedited, as §10.4 has it; the docs say
 so.
   - **Weights are an estimate**, the pool's (`OPEN_BYTES` + `PAGE_BYTES` a
 page loaded + the segments an engine alone holds), and the base
 engine's bound is cap-driven: it closes, pinned or not, only when the
 screen's engines pass 96 MB with nothing unpinned left and it weighs
 over 16 MB (a fixed 16 MB bound would reopen the heavy's about every
 166 pages, 0.7 s each). The gates measure RSS.
   - Further choices, each in its step's report: `Views::draw` (a view the
 caller holds, by a job as its engine draws it) and `Views::target`;
 `Doc.drawn` keyed by `(PageId, Rotation)`; image keys by the composed
 turn; `ocg_off` only once the Layers panel has chosen
 (`layers_chosen`); the pool a `OnceLock` in docview; the strip unpinned
 while its panel is hidden; `set_inputs` also in `follow`;
 `show_all_but` inlined into `ocg_patch`; a source's dead links in the
 key, and an `Order` read and the target page's `Page` read for a page
 drawing another tool's link (`LinkTo`); the
 predicate ignores a field's value (no save writes `/V` before M10
 slice 9) and compares boxes without the turn; `SourcePatch` only for a
 source with `/OCProperties`, an untouched page of a damaged layered
 base going to its group; standalone groups keeping four catalog keys;
 an inserted form's `DA`/`Q` pushed down in overlays as saves push
 them; the liveness probe a `Weak<Source>` (`Src::downgrade`,
 `upgrade`), the realizer's `Realization` without it; failed opens
 remembered in the pool (`Unopened`); Find,
 Patterns and selection in the screen's budget (the background's would
 reopen a 7k base engine every ~380 pages); print preview without
 markup leaving others' comments out (as `print_pdf` prints); the
 header preview's own realizer, no pins, marks not drawn; the thread
 test's baseline taken after a first insert and crop (lopdf's rayon
 pool starts once); `RenderRequest` kept as `Rendered.request`, its
 page and job alone (the review retired its format and priority).

   Found while building:
   - **The delta cache wasn't bounded** (step 8, step 3's code): `Part::
 weight` counted 128 bytes an object and nothing for its dictionaries'
 entries, the placeholders or the owned map, about 680 bytes a header
 page's delta against 11.1 KB of RSS each, so 32 MB "by estimate" held
 about 48,000 deltas, some 520 MB. The 200-groups gate's RSS row found
 it (317 MB added on the synthetic). `weight` now counts each object's
 allocations (`held`, `dict_held`: entries and index grown by doubling,
 each key and value, a chunk header an allocation), the vector's
 capacity, the placeholders and owned keys: 11.6 KB a delta by the
 estimate (14.7 KB on the heavy), and the cache at its cap holds about
 its 32 MB of RSS. `an_object_weighs_its_entries_and_what_they_hold`.
   - **A derived copy's group form listed every field of the file** (step
 8, step 2's code): over a crop's copy, `form::prepare` found the
 copy's form unmodeled (the records model the original's), so the frame
 kept the copy's whole `/Fields` (7,000 on the heavy) beside the
 group's own, and PDFium's form environment loaded all of them at each
 group's first page (0.6 s a group: the strip filled at 108 pages/s).
 An overlay over a derived copy now treats its form as the records'
 (`modeled`), listing only the scoped fields: 344 pages/s, the realize
 of a group 240 → 44 ms, and the cropped pixel test unchanged.
 `a_derived_copy_s_page_is_emitted_as_the_copy_has_it` checks the
 form's `/Fields`, and fails without the fix (fault-checked).
   - Steps 1–7's own (each in its report): the dead-link flag in the key
 and reads; `render_appearances`' inherited box; the overlay push-down
 of an inserted form's defaults; the forms-off matrix path needing no
 truncation fix; failed opens tried once.
   - **The predicate missed a widget's `/Parent`** (the review, step 3's
 code): `realize_corpus`'s predicate tier, run over every corpus form
 and under each consumer's inputs (the sample's 545 pool files had
 checked only a plugin's), found 136 page-and-inputs disagreements in
 9 forms (1822407, 0050247, 0056356, 2429543, 1538204, 1525600,
 1312748, 1062413, 0262114), each "untouched" where the overlay wasn't
 the base: a field a parent's `/Kids` lists whose own `/Parent` is
 missing or names another object (or one the parent lists under no
 `/Parent` at all). The model's tree comes from the `/Kids`, the
 emitter writes each `/Parent` from it, and PDFium draws a widget with
 what it inherits up the `/Parent`s, so such a page showed its base
 engine's drawing instead of the overlay's. `untouched` now checks
 each drawn widget's `/Parent` chain (merged, its field's) against the
 model's parents (`tree_kept`): 0 of 242,055 disagree.
 `the_predicate_agrees_with_the_emission_in_every_case` has the two
 shapes and fails without the check (fault-checked).

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The new gates** (`gates_on_the_*_7k`, two whole runs of the harness
 and one of the heavy fixture alone, release, at a load of 1.5–4; the
 heavy strip only after step 8's fix):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | realize an edited page (emit + assemble), a fresh realizer | 2.00 ms | 2.38 ms | 2.01 ms |
 | realize + open + render 1,024 px, the edited page (asserted < 15 ms and < 25 ms) | 12.1–12.3 ms, 2.3× the warm render (5.3 ms) | 21.7–23.4 ms, 4.4–4.7× (5.0 ms) | 33.5–34.0 ms, 1.5× (21.9 ms) |
 | engines after 220 groups: weighed at most, RSS added (asserted ≤ 96 MB each) | 28.8 MB, 37.5 MB | 28.8 MB, 62.1 MB | 28.8 MB, 51.1 MB |
 | group engines open after them (asserted ≤ 6) | 6 | 6 | 6 |
 | thumbnail strip after a crop and the header on every page, 320 px (asserted ≥ 310 pages/s and ≤ 96 MB weighed; slice 2's run in brackets) | 406–410 pages/s (389), 80.8 MB, 32.5 MB RSS added | 344–346 pages/s (318), 81.4 MB, 80 MB | 164 pages/s, 80.8 MB, 81.5 MB |

 Before step 8's two fixes the 200-groups row added 317 MB of RSS on
 the synthetic, and the heavy's strip filled at 108 pages/s (222 MB
 added); Found has both. After the review (the synthetic and heavy
 harnesses once, release, load about 1): realize + open + render 12.1
 and 21.0 ms (2.3× and 4.3×); 220 groups 28.8 MB weighed, 37.5 and
 62.1 MB added; the strip 410 and 345 pages/s, 80.8 and 81.4 MB
 weighed, 32.5 and 79.6 MB added (now asserted ≤ 96 MB); the heavy's
 Rewrite 1.06× slice 8's constant; `the_heavy_realize_gate_as_planned`
 21.4 ms against a 5.08 ms warm render, 4.2×, failing as it should
 (deleted since, Greg's ruling). After Greg's rulings (one whole run of
 the harness, release, at a load of 14 falling to 2; 9 of 9 pass):
 realize + open + render 12.2 ms on the synthetic (< 15 ms, 2.3× its
 5.27 ms warm render), 22.3 ms on the heavy (< 25 ms, asserted now;
 4.5× its 4.91 ms) and 32.3 ms on the scanned (1.5×); 220 groups 28.8
 MB weighed, 37.5, 62.1 and 51.1 MB added; the strip 415 and 345
 pages/s, 80.8 and 81.4 MB weighed, 32.5 and 80.0 MB added; the
 Rewrite 1.10–1.12× (synthetic) and 1.09× (heavy) slice 8's constant.
 The final verification's run (one whole run, release, started at a
 load of 12 falling to 2 behind the workspace tests; 9 of 9 pass):
 realize + open + render 21.1 ms on the heavy (4.2×) and 32.3 ms on
 the scanned (1.5×), and 14.8 ms on the synthetic (2.8×), 0.2 ms
 under its gate; the synthetic's alone twice after, at a load of 1.5,
 12.1 ms both times (2.3× its 5.3 ms warm render), so the synthetic's
 margin is thin on a loaded machine (For Greg).
   - **Where a group's time goes** (a diagnostic, since deleted: group 20
 of the heavy 7k, a fresh realizer, 128 previews at 320 px): with the
 header alone, the view 1.1 ms, the realize (128 deltas, the frame,
 the section) 11 ms for a 66 KB section, the group engine's open
 8.9 ms and the renders 306 ms; after a crop and the header, 1.9, 43
 ms (206 KB), 7.4 and 320 ms, and 367 ms for the first group, which
 reads the copy's objects first. Before the derived copy's form was
 scoped: 240 ms and 930 ms. The base engine's warm 1,024 px render is
 5.0 ms on the heavy and 5.3 ms on the synthetic.
   - **The delta cache:** uncapped, a header page's delta held 11.1 KB of
 RSS (78 MB for each pass over the synthetic's 7,000 pages, 350 MB
 after four); `weight` said 0.68 KB before step 8 and says 11.6 KB
 (synthetic) and 14.7 KB (heavy) now, and a realizer at its cap holds
 32 MB (synthetic) and 30 MB (heavy) more RSS than a fresh one.
   - **The Rewrite against slice 8's writer**
 (`rewrites_on_the_synthetic_and_heavy_7k`'s part; seven processes of
 each writer, run alternately, slice 8's from its own build's binary at
 `target/slice8-gate/`, each process's time the median of three runs;
 load 2.9–4.5):

 | | Synthetic | Heavy |
 |---|---|---|
 | slice 8's writer today | 133.3 ms (131.5–137.2) | 688 ms (578–700) |
 | `materialize`, slice 13 | 151 ms (143–154); 49.8–51.1 MB added | 787 ms (752–803); 252–254 MB added |
 | ratio to the gate's constant (134 and 707 ms) | 1.13× | 1.11× |
 | ratio to today's slice 8 median | 1.13× | 1.14× |
 | RSS added, to slice 8's 61.7 and 251 MB | 0.81–0.83× | 1.00–1.01× |

 No flag: both under 1.2× the gate's constant, the gate at 1.3×
 (slice 12: 1.11× and 1.11×). The emitter's overlay mode moved
 nothing on the Rewrite's path but `Cx`'s fields and setup.
   - **The corpus** (../pdf-corpus), every count as slice 12 left it:
 materialize_corpus's identity 4,389 of 4,392 written, 2,269,722
 primary objects surviving, 919,479 copied as they were, 436 changed by
 `keep_only`, 0 failed; 2,901 forms without their first page, 0 failed
 (551 findings of the allowed kinds); every page duplicated, 4,389, 0
 failed; inserted twice into the 1040, 1,098, 0 failed; four times, 0
 differ; the 2932995 logo, 0 of 2,037,600 bytes differ. append_corpus:
 4,389 marked, 3,770 appended, 259 rewritten (211 RemovalCheck, 34
 BaseObjectsMissing, 14 BaseUnsound), 360 asked about signatures, 0
 failed, 0 derived vetoes and 211 on the files' own objects; the oracle
 finds text gone from 2,899 of 2,901 page-delete Rewrites (none from
 0016064 and 0112654). encrypted_corpus: 573 encrypted files, the
 Rewrite keeps 573 and the Append 572 (0630899 declined). The ignored
 tier, deleted means deleted over every file, six features both ways,
 0 failed: the first Append declined [28, 68, 68, 68, 0, 30], a page's
 content run found in 2,031 Rewrites and 2,022 Appends (837 s at a load
 of 20–30, beside realize_corpus and builds).
   - **realize_corpus** (ignored), as step 8 ran it (291 s): 545 files
 (the 402-file pool sample, 50 encrypted, 50 with leading junk, 50
 whose `startxref` leads nowhere), 0 that don't open, 0 failed, 48
 realized standalone, a field filled in 195 and a comment deleted in
 29; 0 of 2,148 pages differ from the saved Rewrite (appearances only,
 1,024 px), and the predicate, under a plugin's inputs only, disagrees
 with the emission on 0 of 24,991 pages.
   - **realize_corpus after the review,** over every corpus form and the
 pool's sample: 4,935 files, 3 that sheaf doesn't open (1252465,
 2615560, 6220579, as the corpus suite has them), 78 realized
 standalone, a comment deleted in 262, a field's value changed in
 3,006. The predicate tier (7 s): the predicate disagrees with the
 emission on 0 of 242,055 pages and inputs (136 before the `/Parent`
 check, Found); by class, classic table 1,973 files (0 of 128,370),
 xref stream 2,959 (0 of 113,685), hybrid 198 (0 of 8,530), linearized
 2,523 (0 of 111,825), damaged 78 (0 of 2,460), leading junk 55 (0 of
 770), encrypted 646 (0 of 22,200). The pixel tier (1,106 s, PDFium's
 lock serializing the renders) fails: 6 of 13,289 pages differ from the
 Rewrite, by 1 to 9 pixels each (0067241 page 16, 0106273 page 0,
 0171994 pages 0, 1 and 12, 0267257 page 0), single pixels of
 anti-aliasing at a picture's or a shape's edge, ≤ 14 levels; alone,
 0106273's and 0171994's page 0 differ by one pixel every run and
 0267257's by none, so some vary with what else the process drew
 (PDFium's process-wide caches); 0171994's page has the same
 dictionary and content streams in the overlay and the Rewrite. And
 4422502 fails: PDFium can't open its own bytes (a damaged file sheaf
 repairs), so its untouched page, on the base engine, draws nothing,
 while its edited ones, realized standalone, draw as saved. The pool
 sample's files all match, as in step 8 (For Greg). The final
 verification's run (debug, 1,114 s) found the same six pages and
 4422502, and one more: 0054344's page 0 by 2 pixels, 7 of 13,289,
 the variance again.
   - **realize_corpus's pixel tier on those six files, after Greg's
 rulings** (`SHEAF_REALIZE_ONLY`, the test profile, 10 s; the whole
 tier waits for the pre-merge run): passes, 20 pages compared, 3
 differing, each within the 16-pixel allowance and printed. 0054344,
 0067241 and 0267257: none differ this run. 0106273: page 0 (edited,
 its group) by 2 pixels. 0171994: page 0 (edited) by 9 and page 1 (left
 alone, so its base engine: not a realization at all) by 1. 4422502:
 refused by PDFium and realized standalone, its 4 pages (3 edited, 1
 left alone, every one from its group) identical to the Rewrite. With
 the allowance at 1 pixel the run fails (0106273 and 0171994), and with
 the tier not telling the refused source, it fails on 4422502. The
 final verification's run of the same six (2 s of renders) passes with
 6 of 20 pages differing, all within the allowance, the variance
 again: 0054344 page 0 by 2 pixels, 0106273 page 0 by 1, 0171994
 pages 0, 12 and 23 (edited) by 1, 4 and 1 and page 1 (left alone, its
 base engine) by 1; 0067241, 0267257 and 4422502 (refused, its 4
 pages from their group) none.
   - **Suites:** `cargo test --workspace` 1,318 passed and 32 ignored
 (slice 12: 1,246 and 31; realize_corpus is the new ignored test); after
 the review 1,324 and 34 (realize_corpus's two tiers and
 `the_heavy_realize_gate_as_planned`), and materialize_corpus's counts
 as slice 12 left them; after Greg's rulings 1,328 and 33 (the
 refusal's four tests added, `the_heavy_realize_gate_as_planned`
 deleted); after the review of the rulings' changes 1,329 and 33
 (`a_sound_base_pdfium_refuses_draws_from_its_groups`). fmt and
 clippy (`-D warnings`, all targets) are clean, and clippy with
 sheaf-app's and the pool's Linux-only gates switched off (as macOS
 and Windows build them) is clean once
 `PlannedInsert`'s test accessors are gated to Linux, the fix PR 3
 took as `5cb0a78` after this branch was cut; `cargo check
 --target x86_64-pc-windows-msvc` passes for sheaf-doc, sheaf-edit and
 sheaf-util with their tests and for sheaf-render, sheaf-print,
 sheaf-ops and sheaf-ocr with all targets (sheaf-plugin, sheaf-app and
 sheaf-cli can't be checked here, as before). The release properties:
 Append ≡ Rewrite at 1,024 cases (64 s) and the replay property at
 4,096 (16 s) pass. The interop tier (qpdf; pdf.js 6.4.299 on node
 24.22): 6 passed, `qpdf --check` clean on 84 Rewrite and 14 Append
 outputs, three Appends onto each of 14 bases and a delete saved and
 undone on each of 14, pdf.js reading the records file both ways; and
 `qpdf --check` is clean on two realizations written out (group 20 of
 the heavy 7k, its bytes and section: with a header, and after a crop
 and the header), each 128 pages, the cropped one's form 128 fields.
   - **The other gates** (the same runs): the geometry walk 47.9 and 79.0 ms
 (< 100 and < 250 ms); ready on the heavy 360 ms (< 1 s); a snapshot
 0.24–0.27 µs (< 50 µs); `Tx::check` of a delete of every page 0.39–0.40
 ms (< 1 ms); the scanned's incremental save of one mark 21.0 ms (< 50
 ms); AddMark 0.020 ms and a full reorder 0.981 ms at 20,000 marks (< 1
 and < 2 ms); 7,000 `SetOcrText` lowered in 37.6 ms and committed in a
 group in 10.2 ms; an OCR run's 700 flushes 91.1 ms (an amend's median
 0.118 ms, the last 50's 0.139 ms); an insert lowered again after a
 move, 1.48 ms; 200 OCR versions of 1,000 pages at most 256 MB (≤ 256
 MB; 12 versions, 235 MB of steps, RSS growth 327 MB); 1,000 mark adds
 on the heavy 0.033 ms (the first 100 0.030, the last 0.027) and on
 blank pages 0.021 ms (0.019 and 0.021); ready on 20,000 sheaf marks 542
 ms (< 1 s), their Rewrite 1,300 ms and 263 MB added; the Rewrite part
 in the whole harness 157–160 ms on the synthetic (1.17–1.19×, under the
 1.2× flag) and 770–777 ms on the heavy (1.09–1.10×). The crop rows open
 no engine now (160 and 698 ms), and the heavy's flatten peaks at 490 MB
 against slice 19's 221 MB.

   - **The reviews:** the plan was reviewed before the steps started, and
 its findings are the deviations above, as resolved. Each step checked
 its new checks by switching their behavior off (Checked by tests).
 The review of the whole slice found 24 (three of them one issue, the
 inputs' digest), all fixed:
 - Fixed, behavior: the pool isolates panics (an opener, a render, a
   query or a search step that panics fails its own request; one bad
   file had killed the process's only worker, every page then blank);
   renders asked for together are answered each, `format` compared
   (two documents on one file, or print preview beside the screen, had
   one's request cancelled; `Raster` and `Rendered` are `Clone`); a
   drawn link reads the page it leads to (`LinkTo`; a replace of that
   page didn't conflict with a decision from the link's page); a read
   with generators drawn reads every generator (a header widened onto
   the page didn't conflict); a header's tokens expand in one pass
   (`<<bates>>` in a file name drew the Bates number under a key and
   reads without it); the realizer keeps its inputs' digests by their
   sets (Hide Comments sorted every hidden id at every view), with
   `Doc::set_inputs` keeping the sets it had when they're equal; a
   group's engine key holds the inputs' flags and its source's layers,
   not the hidden and dragged sets, which its pages' keys hold (a hide
   on one page reopened every group's engine); the predicate's
   `/Parent` check (Found).
 - Fixed, tests and gates: `realize_corpus` covers every corpus form
   beside the pool's sample, its predicate tier under each consumer's
   inputs (seconds), and its pixel tier, which failed on 7 files
   before Greg's rulings (now within the allowance, or realized
   standalone; Measured); the strip's RSS added
   asserted ≤ 96 MB on the synthetic and heavy; the heavy's realize
   gate put back as planned until Greg's ruling (now < 25 ms,
   deviations); the delta cache's reuse, cap
   and bookkeeping tested (`Realizer::capped`, test builds); the idle
   worker test allows a tick (it read the worker's time before the
   worker was back in its channel); test helpers shared
   (`realize::fixture`) and `ViewInputs::plugin()` for a plugin's
   render and reads alike; the docview tests' child process run by one
   `alone` helper; `realize_corpus`'s field fill kept, its doc and
   printed line saying it checks only that a value change leaves the
   page on its base until fills write `/V` (M10 slice 9).
 - Fixed, docs and dead code: the 0.3 WIT's `render` and `recognize`
   say what they draw (wit/0.2's is frozen until slice 20 deletes it,
   as slice 12 left its batches); stale "until slice 13" comments
   (history.rs, source/pages.rs, source_corpus.rs);
   `derive::strip_stamps_of` in the transitional ledger (13 → 17); the
   test counts above; `Realization.source` and `live()` and
   `RenderRequest`'s `format` and `priority` deleted (retired.rs rows,
   `QUALIFIED_ONLY` `format` and `priority`); the gate constants'
   comment.

   Checked by tests:
   - **Pixels:** `realize_corpus` (above); `encrypted_bases_render` (RC4-128,
 AES-128, AES-256 against the Rewrite read with the password, no edit
 text in plain) and `two_segments_open_encrypted_bases`;
 `a_page_turned_with_unsaved_ocr_renders_once_turned`;
 `two_edited_pages_sharing_a_font_and_a_duplicate_render_as_saved`
 (section numbers unique, above `/Size` and the xref streams);
 `an_unsaved_header_shows_under_view_rotation` (engine and app level);
 `a_damaged_base_realizes_standalone`,
 `a_base_pdfium_refuses_draws_every_page_from_its_realization` (every
 page, untouched or not, as the Rewrite draws it, through the pool and
 its segments; a second document on the same bytes learning the
 refusal from its first query),
 `a_sound_base_pdfium_refuses_draws_from_its_groups` (2,000 bytes of
 junk before the header: an edited page's section the first engine to
 fail, telling the source and drawing from its group; the untouched
 page's base view made before it drawing from its group too; a view
 patch viewed before the base engine's refusal failing as refused
 without opening, and drawing from its group with the layer off),
 `a_refused_base_s_groups_are_files_of_
 their_own` (sheaf-doc: another engine key, one segment),
 `a_hidden_layer_s_patch_hides_its_content`,
 `hiding_a_comment_hides_it_on_its_page_not_its_duplicate`,
 `a_cropped_page_s_comments_render_once_as_saved`,
 `renamed_marks_are_left_out_under_by_host`,
 `allocator_numbers_above_size_and_xref_streams`,
 `ocr_reads_a_turned_page_as_it_shows_its_content_alone`.
   - **Invalidation:** `hiding_a_comment_changes_only_the_screen_s_key` and
 `hiding_a_comment_leaves_print_preview_and_the_ocr_input_alone`;
 `a_reorder_of_7000_pages_changes_no_key_without_live_tokens` and
 `with_page_n_of_n_a_move_changes_only_the_covered_pages_keys`;
 `dragging_a_mark_under_by_host_changes_no_key`;
 `hiding_a_comment_rerenders_only_its_page`,
 `an_edit_keeps_the_old_image_until_the_new_one_lands`,
 `a_mark_edit_lays_nothing_out`, `a_save_keeps_every_image`; key_tests'
 eighteen and untouched_tests' cases; `read_render`'s sets,
 `a_link_s_page_reads_the_page_it_leads_to` and
 `a_header_coming_onto_a_page_conflicts_with_its_render_read`;
 `a_token_in_a_value_is_drawn_as_it_is`;
 `inputs_are_digested_once_and_a_hide_changes_only_its_group` and
 `deltas_are_kept_by_page_and_key_under_their_cap`.
   - **Engines and threads:** `using_layers_writes_no_temp_file` (a child
 process, an empty `TMPDIR`); `the_thread_count_is_constant_across_20_
 inserts_and_crops` and `one_thread_serves_every_engine` (child
 processes, Linux); `engine_bytes_stay_under_the_cap_after_200_groups`
 (weighed, and RSS added ≤ 96 MB); `background_walks_never_close_screen_
 engines` and `background_ocr_doesn_t_evict_visible_engines`;
 `group_engines_count_against_the_screen_s_six`;
 `an_engine_that_fails_to_open_is_tried_once`;
 `segments_fill_a_read_across_the_boundary`;
 `annotation_flags_draw_widgets_alone`;
 `renders_asked_for_together_render_once_for_each` and
 `a_panic_fails_only_its_own_requests`;
 `a_refused_engine_tells_its_opener_and_renders_from_the_fallback`
 (told at the failed open and each request after, the fallback's page
 drawn, a password failure telling nothing and falling back to
 nothing); the pool's fifteen.
   - **Emission:** overlay_tests' twelve (each overlay naming only what it
 writes or its base has, never its base's own objects), among them
 `an_overlay_visits_only_its_scope` (a visit counter on the state's
 accessors) and `keep_base_keeps_the_base_rotate_after_a_turn`.
   - **The app:** `find_and_patterns_search_through_the_pool`,
 `the_form_type_comes_from_the_primary_s_engine`,
 `the_header_dialog_previews_the_header_and_drops_a_stale_result`,
 `a_plugin_render_draws_sheaf_marks_and_the_unsaved_header`,
 `a_file_pdfium_refuses_draws_its_pages` (the first frame's base view
 drawn from the group, the next frame's view the group's under the
 same key and image, the notice).
   - **Fault checks** (each step's, restored from its own copy and checked
 with `cmp`; no proptest seed added): step 1's 32 (31 caught; dropping
 the hashed "the file draws it" flag can't be, since it follows from
 the base page and the recipe, both keyed; ignoring it is caught), step
 5's 11 (10 caught; tiles drawn at any rotation have no headless test,
 checked by eye), step 6's 13, step 7's 16 and step 8's derived form,
 all caught; the review's 17, all caught: the link's and the
 generators' reads, the inputs' digest kept and kept out of the group's
 key, the delta cache's reuse, eviction and byte count, the one-pass
 expansion, the opener's panic guard, coalescing by format and
 answering every receiver, the retired `RenderRequest` field, and the
 `/Parent` check (dropped, and only the root's). Greg's rulings'
 (2026-10-20) 11, all caught (`cmp`-checked, no proptest seed added):
 no fallback in `Views::draw`; the opener's hook not noting the
 source; `on_base` ignoring the refusal; the remembered failure not
 telling the opener; the found groups not keyed by it; the group
 digest without the standalone flag; `standalone` ignoring the
 refusal; PDFium's load errors not classed `Refused`; a fallback on
 any failure (a password); and on the six files' pixel tier, the
 allowance at 1 px, and the tier not telling a refused source.
   - **Manual runs** (Xwayland, `--script`, a two-page file with two
 layers, another tool's square and a text field): the Layers panel
 turning "Red layer" off redraws both pages from the source's patch
 engine with the square and field kept, the status saying "1 layer
 hidden"; the Comments filter's type unchecked hides the square on the
 page ("1 hidden by the filter"), the field kept, and Hide Comments
 likewise ("Comments hidden"); the Header & Footer dialog's preview
 shows the typed footer ("UNSAVED HEADER 1") on the page's picture
 before applying; once applied, the unsaved footer shows on both pages
 under a 90° view rotation, turned with them. Idle with the file open,
 the process took 7 ticks in 10 s (0.7% of a core): GPUI's main
 threads 4, Mesa's Vulkan `[vkps] Update` thread 3, the render thread
 none. Not seen by eye: a field edit (M10.5 has no fill op; the
 headless `an_edit_keeps_the_old_image_until_the_new_one_lands` covers
 the fallback), print preview, an OCR run on a form page, a PDFium-only
 open failure (headless since Greg's ruling:
 `a_file_pdfium_refuses_draws_its_pages`). Screenshots in the session
 scratchpad
 (`s13/step8/ui`).

   For Greg:
   - **Decided: the heavy's realize gate is < 25 ms** (Greg accepted
 < 25 ms, 2026-10-20), asserted beside the synthetic's < 15 ms; the
 plan's ≤ 1.7× its warm render is no design's that opens an engine.
 Making it faster is a later perf pass's (Left for later).
   - **The synthetic's < 15 ms has little room on a loaded machine:**
 12.1 ms at a load of 1.5, 14.8 ms in a whole run started behind the
 workspace tests (Measured). Run the gates on a quiet machine; the
 same perf pass would widen it.
   - **`ViewInputs.foreign`** (others' comments and links) is new, **and
 there's no `FormDraw`**: whole classes are left out by PDFium's render
 flags; nothing draws appearances alone on a shared engine.
   - **`EnginePool` is generic** over its key, in sheaf-render, with the
 realizer's types in sheaf-doc and the glue in sheaf-ops.
   - **Deltas are cached by `(PageId, RenderKey)`,** images by
 `RenderKey`.
   - **Render flags rest on widgets drawing only through `FPDF_FFLDraw`,**
 proved pixel for pixel; `content_only` and `print(false)` draw no
 annotations by flags, not by smaller overlays.
   - **OCR reads a page's content alone** now: no widgets, comments or
 unsaved header (the app's OCR and plugin `ocr.recognize`), so OCR of
 form pages may change.
   - **Plugin renders draw sheaf's marks, the unsaved header and other
 tools' comments,** as the saved file will, in the file's orientation
 (docs/plugins.md); a render's reads follow what it drew.
   - **Print preview without "Comments and markups"** leaves other tools'
 comments out, as printing does.
   - **PDFium no longer gates the open:** a file lopdf reads and PDFium
 can't opens with a notice; the XFA-only form bar waits for the first
 engine's answer (milliseconds). When PDFium refuses the file (a load
 error, not a missing library), its pages draw from sheaf's copy
 (deviations; Greg, 2026-10-20), and the notice says so: "PDFium can't
 read this file as saved, so its pages are drawn from sheaf's copy,
 and Find and text selection don't work on it". Wording yours to
 change.
   - **A source PDFium refuses has no text yet:** Find, Patterns,
 selection, plugin text and OCR's "skip pages with text" read the base
 engine, which fails (OCR with that option on fails each page's text
 check; off, it recognizes them). Slice 14's text service reads text
 from realizations. Printing such a file fails too while print opens
 the file's path in PDFium (`PdfiumEngine::open(path)`, slice 14), and
 so do Remove Blank Pages and Trim White, which open the source's
 bytes in an engine of their own (`sheaf_ops::engine_for`; "PDFium
 can't read the file") until slices 14 and 15.
   - **The base engine reopens** once the screen's engines pass 96 MB with
 nothing unpinned to close (about every 1,256 distinct pages by the
 estimate), a 0.7 s pause on the heavy 7k; the heavy's base engine
 weighs about 35 MB once its form loads, against the 4 MB counted.
   - **One process-wide pool** counts every document's group engines
 against 6, and pinned ones are never closed, so two split documents
 can hold more; a `Base(blob)` engine is shared by documents opened on
 the same bytes.
   - **Two changes still don't conflict with a render read,** though they
 change the key: a generator added after the read (its id is drawn at
 random, so no read names it; adding the first header while a plugin
 decides from pixels lands beside it), and a link leading nowhere that
 a replace makes lead to a page again. Each needs a table-level key
 (all generators; the page map), which §6.3 doesn't have.
   - **wit/0.2's `render` doc** still says renders leave sheaf's markup
 out; frozen until slice 20 deletes it, as slice 12 left its batches.
 The 0.3 doc says what renders draw now, and 0.3 renders have no way to
 leave markup out (the `without-markup` option is 0.4's, §14.3).
   - **`realize_corpus`'s pixel tier runs over every corpus form,** as the
 rule for corpus tiers says (the plan said "a corpus sample"), in 18
 minutes. Decided (Greg, 2026-10-20): a page passes within 16 pixels
 of anti-aliasing, each such page printed and counted apart, and a
 source PDFium refuses realizes every page standalone (deviations).
 Its whole run, and the predicate tier's, wait for the pre-merge run
 (Greg, 2026-10-20); the six files it failed on pass (Measured).
 Decided too (Greg, 2026-10-20): the tier stays over every corpus
 form, and the ignored corpus tiers run in full only before a PR
 merges.
   - **Field values** aren't in the untouched predicate until fills write
 `/V` (M10 slice 9).
   - **Crop's first use starts rayon's 32-thread pool** (lopdf's parallel
 load in crop's copy) until slice 15.
   - **The header preview** doesn't draw sheaf's marks (GPUI draws them on
 the page, and the dialog has no overlay for them).
   - **lopdf WARNs** during autosave come from recovery's form check
 (`append.rs:408` → `forms::check_latest`), as before this slice.
   - **target/** was 364 GB at the start of step 8 (the memory's 300 GB
 rule): `cargo clean --profile dev` freed 383 GiB; release and the 7k
 fixtures were kept.

   Left for later slices:
   - **Slice 14:** the text service on realizations (an edited page's text
 through its `PageView`, unsaved OCR text), replacing base-engine text
 by index, `CharsKey`, `layer_hits` and `merge_source_hits`; print,
 print preview and page images from bytes, and with them
 `PdfiumEngine::open(path)`'s last use (sheaf-print's gdi.rs); blank
 detection off its own engine.
   - **Slice 15:** crop as geometry, `content_only` over the whole
 MediaBox for Trim White (off its own engine), the crop dialog's
 rendered preview.
   - **Slice 16:** the watermark dialog's preview (on the header preview's
 pattern), watermarks as generators in the key and the strip gate's
 watermark.
   - **Slice 17:** derived copies gone with their realizer part (row 5660).
   - **M10 slice 12:** PDFium draws sheaf's marks; `MarkDraw::ByHost` and
 the stamp and tape pictures go.
   - **Slice 17:** `derive::strip_stamps_of`, the header preview's strip
 (transitional ledger), goes with derive.rs.
   - **Slice 20:** the `without-markup` render option (§14.3): until 0.4,
 a plugin's render always draws sheaf's markup and the unsaved header;
 wit/0.2's render doc goes with it.
   - **A later perf pass:** make realize + open + render faster on the
 heavy 7k (21.0–23.4 ms against its 5.0 ms warm render, gated < 25 ms)
 once the algorithm is settled (Greg, 2026-10-20: correct first,
 then fast); the plan has no perf slice yet.
   - **Slice 14:** text of a source PDFium refuses (Find, selection,
 plugin text, OCR's text check) from its realizations, printing it
 from bytes, and Remove Blank Pages on it (blank detection off its own
 engine). **Slice 15:** Trim White on it (`content_only`, off its own
 engine).
   - **Follow-ups:** the Mark record's restructuring (slice 11's For
 Greg); the `Closure::stale` probe (slice 11's Unprobed), untouched by
 this slice: no step changed that walk; `Doc::set_inputs` still walks
 every annotation for the Hide Comments set on each change (`follow`),
 though it keeps the sets it had when they're equal, so the realizer
 digests them once.
