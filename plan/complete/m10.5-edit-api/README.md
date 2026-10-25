# M10.5: The edit API

Status: **in progress** (started 2026-10-15). Slices 1–20 are done.
M10.5 comes between M10 slice 5 and M10 slice 6: M10 paused after slice
5 so that its remaining slices are built on this API, and M10 resumes
after slice 20. A few decisions were made while planning (below); the
proposals were taken as recommended, as Greg asked. One commit per
slice, with notes per slice at the end, as in M7–M10. How each slice is
built, and how the work reaches `main` in six PRs, is in "How each
slice is built" before the Slices. Why the design is what it is (the
competing designs, every fork's alternatives, the 136 review findings
and their rulings) is in [`m10.5-decisions.md`](decisions/README.md).

- **PR 1, foundations** (the plan, slices 1–7, the `in-progress` swap
  and the move to gpui-pre): merged as #17 (`539cdab`), 2026-10-17.
- **PR 2, model and saving** (slices 8–10): merged as #20 (`121d4a3`),
  2026-10-19, and the pre-commit review's fixes as #21 (`99c7685`).
- **PR 3, the edit vocabulary** (slices 11–12): merged as #22
  (`69212e3`), 2026-10-20.
- **PR 4, rendering** (slices 13–14): merged as #23 (`5804615`),
  2026-10-21.
- **PR 5, the feature moves** (slices 15–18): merged as #26 (`bea4e15`),
  2026-10-22.
- **PR 6, the consumers** (slices 19–20): branch `m10.5-p6`, slices 19
  and 20 done.

Research for this plan, all on 2026-10-15:
- sheaf's code at `a0955fc` (M10 slices 1–5 and the pause commit, on
  `m10`);
- a survey by ten readers of the code, the future milestones and prior
  art: model and history, save and materialize, content features, forms,
  the app and CLI, plugins and the agent, rendering, the future
  milestones (M11–M15 and Pending), PDF libraries, and editor
  architectures;
- the M10 worker's notes after slices 3–5 (also at the end of
  [`m10.md`](../../m10-forms/README.md));
- measurements of PDFium on a synthetic 7,000-page file and on 402 real
  files of 4 MB or more;
- five competing designs (consumer-first, minimal delivery, model
  definition, object delta, risk-first), judged under three lenses
  (correctness and fit to the future; cleanliness, simplicity and
  deletion; feasibility, performance and delivery) and merged into one;
- a six-lens adversarial review of the merged design: 136 findings, each
  with a ruling, and the accepted ones built in below;
- outside sources, listed at the end.

## The plan

- [The design](design/README.md), by section (§1–§20)
- [Why](why.md)
- [Scope](scope.md)
- [Decided while planning (Greg, 2026-10-15)](decided-while-planning.md)
- [Proposed (to confirm at kickoff)](proposed.md)
- [Found while planning](found-while-planning.md)
- [How each slice is built](how-each-slice-built.md)
- [M10 slices 6+ on the new API](m10-slices-6-new-api.md)
- [Changes to other plans](changes-other-plans.md)
- [Risks](risks.md)
- [Asks](asks.md)
- [Not in M10.5](not-m10-5.md)
- [Sources](sources.md)
- [The decisions and the adversarial review](decisions/README.md)

## Slices

**Rules for every slice:**
- It is one commit on the M10.5 branch for its phase (see above), cut
  from `main`, and the app works at that commit.
- It deletes something, and adds its retired names to
  `tests/retired.rs`.
- It updates its own section of `docs/document-model.md`.
- Before the slice's list is final, a `cargo check` on a branch confirms
  that nothing live still calls a symbol it deletes.
- It extends the 7k gate harness where it touches a gated path.

**Sizes:** S = 1, M = 2, L = 3, XL = 5. **About 68 points over 20
slices.**

| # | Slice | Size | Headline deletion |
|---|---|---|---|
| 1 | History stores what changed; the retired ledger | L | `Ids`, seeds, `replay_step`, deep-equality no-op, in-place amend |
| 2 | Spikes, heavy fixtures and the gate harness | M | the spike code (verdicts and harness stay) |
| 3 | Lazy sources and the Store | L | `SourceInfo`'s bytes, sizes and password, `set_source_sizes`, the layout size walk |
| 4 | One open, gated | M | `set_initial_*`, `opened_state_mut`, six late loaders, `sheaf_ops::open` |
| 5 | PageSpace and absolute rotation | L | `Rebase.shift`, `TextLayer::shifted`, `annot::PageGeom`, the rotation delta |
| 6 | Content toolkit and fonts | L | four encoders, `has_artifacts`, `strip_ops`' round trip, cache-stem stamps, `recover_cached` |
| 7 | The serializer | L | lopdf `save_to` in `finish`, the `/Encrypt` strip, `write_atomic` without fsync, the length-only disk check |
| 8 | Records | XL | `removed_comments`, `Doc.foreign_annots`, `Document.forms`, linear scans, slice 1's keys |
| 9 | Rewrite from records; saves are outputs | XL | `build_rewrite`, incremental.rs, `rebase_to_saved`, the projection, dirty flags, `Signed` and its Append offer |
| 10 | Append, the planner and the removal check | XL | the always-Rewrite rule, Full-only recovery, slice 9's Rewrite-or-Cancel ask for signed files |
| 11 | Ops and Tx | XL | `Edit`, `compute`, `EditOutcome`, `SourceId(u32)` registrations, id guessing, `form_notice`, `insert_runs` |
| 12 | One funnel and the history budget | XL | three app funnels, the job API, `reveal_new_bookmarks`, per-edit relayout, pinned saved versions |
| 13 | Realizer, pool, render keys, view inputs | XL | per-source threads, hide replays, layers temp file, GPUI header overlay |
| 14 | Text service; print and images from bytes | M | `CharsKey`, `layer_hits`, `merge_source_hits`, print/export temp files |
| 15 | Crop as geometry | S | `cropped_copy`, crop's derived file |
| 16 | Watermark, background and strip | L | `watermark::install/read`, `SheafSettings`, derive::watermark |
| 17 | Header/footer and Bates; derived sources gone | L | `stamps::install` and `strip`, `Derived`, `derived_copy`, `copy_form`, derive.rs |
| 18 | OCR as content | M | `text_layer::install/strip`, the legacy OCR shape, plugin OCR overrides |
| 19 | Exports, memory sources, Session, `sheaf edit` | XL | `subset`, `print_pdf`, `flatten_annots`, `export_*`, generated.rs, write-then-reopen Combine |
| 20 | Plugin API 0.4 | L | WIT 0.2/0.3, `World`, `Batch`, fixture wasm, the 0.3 adapter |

**Why this order:**
- **Exact replay and the ledger come first (1),** since every later
  slice relies on them.
- **Measured risks come second (2),** as their own commit with written
  criteria, so the group size, the caps and the box-edit path are
  decided by numbers before the records slice.
- **The Store comes before anything stores handles (3).**
- **Open (4), PageSpace (5) and the content toolkit (6) run on today's
  DocState,** so records (8) and ops (11) are written once, in final
  coordinates and with the final appearance code. Op JSON never changes
  meaning.
- **The serializer (7) comes before records,** so the old writers prove
  it, encryption included, before they are deleted. It depends on
  nothing in the model.
- **Records (8), then Rewrite from records (9) at once.** Saves become
  outputs in slice 9, which deletes the projection, incremental.rs and
  `rebase_to_saved`. The projection, and `rebase_to_saved` adapted to
  records, live for one slice and cross no coordinate or appearance
  change.
- **Append (10) right after,** so saves are always Rewrites for one
  slice only. The planner needs keys and records, not ops.
- **Ops (11) against the final records and the final emitter.**
  `compute`, adapted to the new tables in slice 8, lives until here and
  writes the same records lowering will. `SourceId` becomes `BlobId`
  here, once form instances replace registrations per insert.
- **The funnel (12) after Append and ops,** so `Snapshot` is O(1) by
  then.
- **The realizer (13–14) before any content feature moves (15–18),** so
  crops and generators show on screen.
- **Consumers last (19–20).** M10 resumes after slice 20 (proposal 11).


Every transitional piece has the slice that adds it and the slice that
deletes it, and `tests/retired.rs` gains its names in the deleting
slice. A row its slice has deleted says so ("deleted"), and a row
deleted in parts says which.

| Transitional piece | Added | Deleted |
|---|---|---|
| Prim keys over today's DocState fields (Pages, Outline, AnnotsOf, TextLayer, RemovedOf, Labels, HeaderFooter, Field, WidgetsOf) | 1 | 8 (deleted) |
| Job amends through `apply_in_job` with id-minting, then through a Tx (slice 11's part done: `apply_in_job(job, tx, label)` commits a Tx as the job's version or amends it; OCR's runs, the app's and `scan::add_text`, lower `SetOcrText` as `Actor::Job(JobKind::Ocr)`) | 1, 11 | 12 (deleted: groups, `GroupKind::{Ocr, PluginCall}` and `CommitOpts.group`, an amend a new version under a new id, `GroupEnded` by ancestry, fragments "(continued)", `revert_group` and `rollback_group`; the job API's names retired) |
| `SourceId(u32)` registrations over `Src` (`{ src, name, derived }`); from slice 8 the records name them too: `PageBase.source` beside `PageBase.src` (render handles, `OcrPage.source`, `SourceInfo.derived` and the old writers' source maps key by registration, and two inserts of the same bytes are two registrations of one `Src`) and `FormInst.src` | 3 | 11 (deleted: `SourceId = BlobId`, records name their `Src`, an insert names its pages by `BlobSel` and its instance by the op's `name`, `FormInst { src: BlobId, copy }` one per insert op) |
| Derived copies as interned blobs with a `derived` record (on the registration, then, from slice 11, a side table keyed by `BlobId`: `Document.derived`, `Arc<HashMap<BlobId, Derived>>`, filled by `DerivedSource::note` and carried by `Snapshot` and `SaveInput`; `MemoryKind::Derived`) | 3 | 15 (crop's, deleted: a crop is a box edit and makes no copy), 16 (the watermark's, deleted: a watermark or background is a generator drawn at emit, and no copy keeps a header, so `save::HeaderKept` went; `Source::marked` reads a copy's stream markers, as it keeps a file's streams but not its catalog), 17 (deleted: no flow makes a copy; `Derived`, `DerivedCopies`, `Document::derived`, `MemoryKind::Derived`, `save::derived_copy` and `copy_form` gone, and `Source::marked` reads the catalog alone) |
| Generated-cache files interned by path: blank pages and images (3); from slice 6 the stamps and the never-saved attachments `assets.rs` falls back to by stem (attachments stay cache stems until slice 8 makes them blobs); from slice 8 only a stamp whose stem nothing else holds (a plugin's, an adopted comment's), read by compute's stem resolver (`model::find_stamp`'s cache arm), interned and held by its mark as a `FormRef::Blob`; from slice 11 by what resolves a stem before lowering (the WIT adapter's `EditBatch::lowered` and comment import, into a Tx's grants; an insert's `resolve::incoming`, pinned) and by `AdoptAnnotations`' lowering | 3, 6 | 8 (attachments, deleted), 19 (deleted: blank and image pages, a scan's, library and imported stamps and the comment summary are memory sources, `Memory` blobs written by `serialize::full::write_built`; `generated.rs`, the cache directory, `SHEAF_CACHE_DIR` and `find_stamp`'s cache arm gone, an adopted stamp holding its file's form through `ForeignAnnot.stamp`) |
| `assets.rs`: `MarkAssets`, a document's table of the stamps and attachments marks name by cache stem (outside history, shared with its snapshots): what each file's marks hold (`SourceAssets` from `file::annots_in`, registered by `Document::ready`, `set_source_assets` for inserted, replacing and combined files and the app's re-read after a save), stamps placed and marks pasted (the app's clipboard blob cache), and last the generated cache by path (stamps a plugin or adopted comment names until 19, attachments never saved until 8); `annot::WriteCx` writes each once a save from it | 6 | 8 (deleted: marks hold `Mark.file: BlobRef` and `Mark.stamp: FormRef`; the stamps' cache fallback moved to `model::find_stamp`, deleted 19) |
| The `attachments::Objects` seam over `LazyPdf` and the eager `Document` (`resolve` and `forms::xfa::read` generic over it; from slice 4 the open's readers too: labels, `stamps::from_catalog`, attachments, `portfolio::collection`, `layers_in`, `file::{annots_in, page_geom}`, import's `convert_page`, and `stamp_lib::form_to_pdf` through `save::remap`/`copy_object`), and `forms::read::fields_of` for `forms::rules`' eager reads. `convert_page` keeps an eager user past slice 9, exchange's FDF/XFDF-built `Comments.pdf`, so slice 9 keeps the seam for it or builds that document as a source. Slice 9 deleted `fields_of` and `page_geom`, and kept the seam for its eager users: exchange's `Comments.pdf`, stamp_lib's page forms, derive.rs's derived copies and `keep_only`'s graph | 3 | 9 (`fields_of` and `page_geom`, deleted); 17 (derived copies and `stamps::from_catalog`, deleted with derive.rs and `derived_copy`), 19 (deleted in parts: exchange writes sheaf's marks through `annot::emission`, `to_pdf_in` and `Written` fixtures only; still eager through the seam, exchange's FDF/XFDF-built `Comments.pdf` read by `import::convert_page`, and stamp_lib's `page_forms`, which trims a clone of an eager document, for the comment summary's Pages layout and a stamp made from a page), 20 (deleted in parts: exchange's `Comments` is a source, `Comments { src, annots, file }`, its FDF/XFDF graph built by `serialize::built::Built` and opened as a `MemoryKind::Comments` memory source and another PDF's comments read from it lazily with its password; `stamp_lib::page_forms(&Src, &[u32], dst)` copies a page's content and resources from a source; annot.rs reads no graph it built (`written_staleness`, `NoObjects` gone). Kept, the one reader left, for what still loads a file whole: the trait and `impl Objects for Pdf` serve a Rewrite's eager graph (`materialize::record`'s `RootValues::of` and `TreeShape::of`, `keep_only`'s `is_page_node`; backlog F15), `forms::check`'s file and its revisions, the stamp library's collections and the user's PDF a stamp is made from (proposal 29), and tests) |
| `PdfiumEngine::outline` loading every page dictionary before it reads destinations, while the outline still comes from PDFium | 3 | 4 (deleted) |
| `file::{read_annots, read_annots_mem, read_sheaf_annots, read_sheaf_annots_mem}`, eager reads of a loaded file kept `#[cfg(test)]` for save.rs's and exchange's tests of the old writers | 4 | 9 (deleted) |
| `save::geom_of` / `file::page_geom` as wrappers over `Geom` for the old writers: `geom_of(dict, rotate)` (crate-private) for save.rs's header/footer and derived copies, `page_geom(pdf, page)` for stamp_lib's page forms and the turn of a page's v1 marks in `file::annots_in` | 5 | 9 (deleted: derived copies and stamp_lib read `Geom::read`, `file::annots_in` takes each page's turn from the page table) |
| Stale sheaf marks (a v2 digest or a v1 `/M` that no longer matches) shown through today's foreign-comment list: `Facts.foreign` from `file::annots_in`, converted for adopting as any other tool's comment is; so, from slice 6, a sheaf page stamp whose appearance draws no page and an attachment with no embedded file | 5 | 8 (deleted) |
| Copied marks' renames for the old writers: `Document::set_source_renamed` (called by `ready` and the app's insert and replace), `Snapshot::renamed_on` and `write_page_annots`' `renamed`, through which other tools' replies to a copy follow it to its new `/NM` | 5 | 9 (deleted: a save writes a mark given a fresh id anew, `FileAnnots::fresh`, and the app hides it by its old name from the view's facts) |
| `content/to_lopdf.rs`: the old writers write a `ContentWriter`'s `ResourceNeeds` into lopdf (`ResObjects`, one object per `ResId` a save, and the isolating `q`s), binding the legacy names `/SheafHF` (until 17) and `/SheafOcr` (until 18) as given names; `ResBody::Written`, a form or image the old writers wrote, by its number; `annot::WriteCx`, a save's `ResObjects` and the stamps' pages and attachments' files it wrote, which `to_pdf_in`, `save::write_page_annots`, the savers and `exchange::gather` pass along. From slice 9 no save writes through them: `exchange::gather`, the comment summary and the legacy installers' `sink::Lopdf` keep `ResObjects` and `WriteCx`, whose trust of the file's own stems (`owning`) went | 6 | 9 (`owning`, deleted); 17 (the header's installer, deleted: `stamps::install` gone, the header drawn as generator blocks), 18 (the OCR layer's, deleted: `text_layer::install` gone, no installer writes through `sink::Lopdf`, which keeps the marks' appearance forms `annot.rs` builds; the sink's `add` and `stream_dict` went with it, and no name is given), 19 (deleted in parts: `exchange::gather` writes through `annot::emission` with `WriteCx::held` and `form`, `to_pdf_in` and `Written` fixtures only, and every PDF built with them is written by `write_built`; still binding resources for PDFs made in memory, `WriteCx` (gather, `annot::pinned_pdf`) and `ResObjects` with `sink::Lopdf` (`WriteCx::form`, the summary's pages, `annot::tape_pdf`, whose picture lopdf's writer still writes)), 20 (deleted: content/to_lopdf.rs is gone, and what PDFs made in memory need is the permanent `serialize/built.rs`: `ResObjects`, `WriteCx`, `StampForm` and `Held` moved there, `Appearance::write` is its `as_form` and `sink::Lopdf` its private `Graph` sink, and `Built` builds pages (`page`, `form_page`) and writes them with `write_built`; the summary's sheets, exchange's comments, a tape's picture, a pinned mark's page and `form_to_pdf` are all built so, and lopdf's `save_to` writes nothing outside tests) |
| The legacy resource names `ContentWriter::finish` is given and never generates: `/SheafHF`, the header and footer's font (`stamps::to_content`), which `PageBlocks` reads a range as `SheafLegacy` by, and `/SheafOcr`, the OCR layer's (`text_layer::to_content`) | 6 | 17 (`/SheafHF`, deleted: no header writes it, and `PageBlocks` still reads a range in that font as `SheafLegacy`), 18 (`/SheafOcr`, deleted: `ContentWriter::finish` and `named` take no given names, the glyphless font and the block's list take `/Sh…` names, and `PageBlocks` still reads the legacy layer by `/SheafOcrLayer`) |
| The old writers (`finish`, incremental.rs) writing through `serialize/` | 7 | 9 (deleted) |
| `save::{SaveStop, Ask}` with only `Ask::FileChanged`, and `save::IfChanged` (Overwrite skips both stamp checks) on the old entry point `save_to_path_noting`; the app's `pending_changed` and the `IfChanged` its `pending_signed` carries | 7 | 9 (deleted: moved to `materialize/`, with the plan's asks, `AskAnswers` and `write_file`'s stamp check) |
| `SourceInfo.stamp`, the minimal `FileStamp { len, mtime, tail }` on the primary's registration, re-stamped by the app's re-read after a save | 7 | 9 (deleted: `SaveRecord.stamp`; slice 10 added the `startxref` window, `FileStamp.window`, and `SaveRecord.content_hash`, done) |
| `save::kept_encryption` and `finish`'s `Option<&Crypt>`: each caller keeps the primary's encryption or passes None (print) | 7 | 9 (deleted: `Profile.encryption`), 19 (the presets, deleted: `Export::{whole, extract, print, flattened, clean_copy, rejects}`, each with its profile; save.rs's `EXPORT`, `CLEAN` and `written` gone) |
| AES IVs keyed by the number and generation an object is written as (`Crypt::iv`); from slice 9 only an update's section (`serialize::append::section`, its unit tests) keys them so | 7 | 10 (deleted: an update's section encrypts through `indirect_as`, its IVs keyed by `ObjKey`; `serialize::indirect` and `Crypt::encrypt` retired) |
| `serialize::append::Base::of(&Source)`: an update's base read from the Source, and the old entry point's in-place save as the bytes read at open plus the section, replacing the whole file through `durable::write` | 7 | 9 (deleted, with incremental.rs; slice 10's Append takes its base from the `SaveRecord` and appends in place) |
| The legacy projection: `LegacyDocState { text_layers, header_footer, removed_comments, fields }` (`Snapshot::legacy()`, built the first time an old writer asks, never in `snapshot()`), `removed_comments` the base page's fact's names less the page's records', and legacy `forms::Field { source, object, copy, under }` from the records (`legacy_field`); the old writers read widgets from the records (`forms::merge::page_copy` reads a page's first widget's `FormInst`), so no widgets-by-page map or legacy `Widget` was made | 8 | 9 (deleted) |
| The baseline: `Document.baseline: DocState`, the opened version's state and then each save's, which the `Snapshot` dirty flags compare the records with, pointers first; it replaced `saved_outline`, `saved_labels`, `saved_annots`, `saved_fields`, `saved_removed` and `baked_header_footer` | 8 | 9 (deleted, with the dirty flags and the `Snapshot`'s) |
| `rebase_to_saved` adapted to records: pages re-pointed by position to the saved file (copy 0, its `Geom`, OCR dropped), `Mark.src` and `Mark.stamp` rebound by `/NM`, `Foreign` by name and subtype on the page (one the save left out removed), `FieldNode` by full name in tree order, `Widget` by its place among its field's on the page, `Bookmark` by pre-order (none deeper than the reader reads); the app reads the saved file's annotations and form on its save thread for it, and its re-listing of other tools' comments (`reread_foreign`, `set_saved_assets`) went in 8 | 8 | 9 (deleted) |
| `compute` adapted to the new tables (`bring_annots`, which `import_marks` became, `take_in_fields`, `copy_fields`, `copy_annots`, `drop_annots`, `scripts_apart`; `InsertPages` and `ReplacePages` still carry sheaf's marks in the `Edit`, and other tools' comments, links, widgets and fields come from the source's facts) | 8 | 11 (deleted: lowering in `tx/`, an insert's marks and outline from its source's facts by `Bring`; `Edit` and its Edit→Op shim, which lived within slice 11, went with it) |
| The GPUI header/footer overlay and `text_layer` search / `layer_hits`, reading `Generator` and `ContentStack` | 8 | 13 (the overlay, deleted: the realizer draws the header into the page's overlay), 14 (search, deleted: the text service reads each page from its realization, which draws the stack) |
| The header/footer generator with legacy semantics (drawn on Rewrite; `Why::LegacyHeaderFooter`) | 8, 9 | 17 (deleted: a §3.5 generator drawn as `/SheafBlock` blocks, its Bates numbers fixed at apply and kept in `/N`; `Why::LegacyHeaderFooter` gone) |
| Every save a Rewrite (`plan`'s `Why::AlwaysRewrite`; `materialize` fails an Append or Unchanged plan); signed files ask Rewrite or Cancel (`Ask::Signatures` with a count, no Append anyway) | 9 | 10 (deleted: the planner, `Ask::Signatures { impact, append_anyway, reason }`, and the app's Append Anyway, Rewrite or Cancel) |
| Recovery Full only (`full`, every sidecar flagged `removes`; about 0.5 s and the file's size written per autosave on the scanned 7k), none for password-protected documents, and `Document.disk_base`: a restored document's file and stamp, which its first save checks and its sidecars name, since a Full sidecar restores with no record | 9 | 10 (deleted: Append sidecars, encrypted as the file is, password-protected documents' too; a restored document's record is `SaveRecord::restored`, the disk file's stamp with the sidecar's sections as `pending_tail`, and a Full restore's `pending_tail.bytes` is None, so `knows_file()` is false and its save is NoSaveRecord's Rewrite) |
| A recovery sidecar is Full while the legacy header and footer is on (`plan::sidecar` returns None): the header is drawn by a Rewrite | 10 | 17 (deleted: `plan::sidecar`'s header rule; an unsaved header's sidecar is an Append section, and so is a live redraw's) |
| §9.3's conservative mapping (`forms::sigs`): `/P` 2 allows only filling in, an approval signature with no MDP filling in and comments, usage rights only what they grant, a signature that can't be read `/P` 1, and FieldMDP's includes lock the nodes over a name too | 10 | M12, or before, once Greg's Acrobat cross-check (Asks) says what Acrobat allows |
| An Append refuses a form-instance index shift (`deps::Closure::form_shift`, `append::Refused::FormShift`, `Why::BaseObjectsMissing`): `FormCopies` indexes the instances the state holds, so an instance held before another's (a registration earlier in order) coming or going moves the other's `Base { copy }` keys off the objects the saved file holds for it | 10 | 11 (deleted: `FormInst.src` a `BlobId` and its `copy` fixed when the insert makes it, which the emitter uses as the index; `FormCopies`, `form_shift` and `FormShift` gone) |
| The Append halves of the both-ways tests as ignored twins (`#[ignore = "slice 10"]`): `both_ways!(name, name_appended: case)`, `materialize::{How, appended, save_appended}`, sheaf-ops' `common::appended`, M10 slice 5's signed-Append tests, and the 7k harness's one-mark Append rows printed as skipped | 9 | 10 (deleted: every ignore, the "until slice 10" wording and the skipped rows; `both_ways!`, `How`, `appended`, `save_appended` and `common::appended` stay as the harness of the tests that save both ways, with `save_rewritten` beside them, so nothing of them is retired) |
| `materialize::save(doc, path, answers, p)`, a synchronous save (`save_input` → `plan` → `write_file` → `record_save`) for the command line and the tests; the CLI answers `break_signatures` and drop XFA, asking nothing. From slice 10 the planner decides: a save the signatures allow keeps them, so a signed input saved to another path copy-appends (the CLI table's `-o` would rewrite and `-o --append` copy-append, until slice 19's flags; a deviation for Greg), and one they don't, answered `break_signatures`, rewrites (`Why::Chosen`) | 9 | 19 (deleted: the save core is `save_with(doc, path, SaveHow, answers, p)`, under `Session::save` and proposal 40's flags; `-o` rewrites, `-o --append` copy-appends through `materialize::appending`, and a signed input asks, exit 3) |
| `stamps::base_draws` and `stamps::drawn_over`: whether a page's base draws the legacy header and footer (its file's catalog, `Source::header_footer`), shared by the emitter and the GPUI overlay, which draws the header over each page whose base doesn't; from slice 13 the emitter's alone, an overlay's included, and the render key's and the untouched predicate's `HeaderFooter::draws` | 9 | 13 (the overlay's use, deleted), 17 (the emitter's, deleted: `content::intact` decides for every generator, the header's included) |
| Derived-page emission in the emitter (slice 9) and the realizer (slice 13: an overlay over a derived copy's bytes takes the page as the copy has it, with no derived table; the records draw its widgets, the copy's own widget entries are left out and its form lists only the records' fields; a derived page is never untouched) | 9, 13 | 15 (crop's, deleted: a cropped page is its source's page, emitted and realized with its new boxes), 16 (the watermark's, deleted: a watermarked page is its source's page, its blocks drawn around its own content), 17 (deleted: `Shown` is `{ src, index, copy }`, and `emit::origin`, `annots::derived` and `EmitBase`'s `derived` went) |
| OCR layers emitted in the legacy shape through `text_layer::install` (`/SheafOcrLayer`, `/SheafOcr`) | 9 | 18 (deleted: OCR is a `/SheafBlock << /K /Ocr /B (b…) >>` block, `text_layer::block`, its range `/ShOCR` with a list naming the block; `install`, `strip`, `has_layer`, the emitter's `replaced` and `fresh_balance` gone; a saved layer of either shape is taken out by `DropStreams(SheafOcr)` and `DropMarked(SheafOcr)`) |
| `subset`-built exports written by the new writer, with `rules::plan` and the `flatten_annots` post-pass | 9 | 19 (deleted: `export::run`, the transforms lowered on an `ExportTx`; `subset`, `PostPass`, `materialize_then` and `flatten_annots` gone) |
| The WIT 0.2 and 0.3 edit variants → `Op` adapter (`api/shared.rs`, included by both): `api::shared::Adapter` says each WIT edit as ops (an update as a `PatchMarks` naming only the record's fields, its `Annot(id)` a read), a mark's stamp or file stem goes in `EditBatch.held`, and `EditBatch::lowered(tx, doc)` (from slice 12) resolves the stems into the Tx's grants and lowers it into the call's Tx, of `Actor::Plugin`. From slice 12 the 0.2/0.3 reads are recorded as the call's decision reads (`CallOutput.reads`, the base the call's first read or submit loaded from the document's cell in `CallOutput.base`), and the app commits a call's first batch on that base with `Stale::Relower`, later ones at the cursor, all in one `GroupKind::PluginCall` group, rolled back on the first failure | 11 | 20 (deleted: `sheaf:plugin@0.4.0` is the only world, its edits Op JSON in the envelope lowered into the call's one Tx (`submit`, `preview`, `commit()`), its reads that Tx's decision reads, landed by `sheaf_plugin::Landing`; `api::{v02, v03, shared}`, both bindings, `World`, `API_PREVIOUS`, `EditBatch`, runtime's `Held` and `CallOutput.{batches, reads, base}` gone) |
| An internal `SetPageBase` for derived flows: `Tx::set_page_base(pages, Derive)` (`Derive::{Crop, Watermark, RemoveWatermark, StripHeaderFooter}`), no `Op` variant, its line naming the flow | 11 | 15 (`Derive::Crop`, deleted: a crop is `SetPageBoxes`), 16 (`Derive::Watermark` and `RemoveWatermark`, deleted: `SetWatermark` and `RemoveWatermark`), 17 (`StripHeaderFooter`, deleted, with `Derive`, `Tx::set_page_base`, `ResolvedOp::SetPageBase` and `tx::Entry`'s base variant) |
| Commits only at the cursor (`Document::commit` refuses a Tx made on another version as `Internal`); background planners apply on the UI thread | 11 | 12 (deleted: `Base::At` with `Stale::{Validate, Relower}`, `Document::tx_at`; planners lower off the UI thread through `run_planner` and land with a base, dialogs commit on the version they opened on) |
| `Tx::read(Key::Page(p))` standing in for `read_render(p)`: OCR's flushes and blank review's deletes decide by the page's `Page` slot (what it shows), as rendered when the run or search began, and a plugin's text, search, recognize and render reads record `Page(p)` | 12 | 13 (deleted: `Tx::read_render` and render keys; OCR's flushes, blank review and plugin text read `render_reads` under `content_only`, which is `Page(p)`, and a plugin's render under its own inputs; from slice 14 plugin text reads under `plugin()`, as the file will draw the page) |
| An OCR flush conflicts with a crop of its pages made during the run: a crop re-points the page's base (`Page(p)`, through `Tx::set_page_base`) rather than its boxes, so the flush's `Page(p)` read no longer holds; `page_by_page` keeps the other pages, and the run's message counts the changed ones | 12 | 15 (deleted: `SetPageBoxes` reads and writes `Geom(p)` alone, so the flush's read holds and it lands; `an_ocr_flush_lands_after_a_crop_of_its_page`) |
| Combine writes its file and the app reopens it (from slice 11 one Tx, `Actor::Job(JobKind::Insert)`, on the first file's document) | today | 19 (deleted: `Session::open` of the first file and one transaction, history forgotten before it, saved and handed to a window as made, `DocView::adopt`) |
| Remove Header/Footer as a derived source: `derive::strip_stamps` and `stamps::strip` (on the splice from slice 6), applied with `SetPageBase` (from slice 11 `Tx::set_page_base` with `Derive::StripHeaderFooter`, then `RemoveHeaderFooter` in the same Tx); from slice 16 `stamps::strip` and `stripped` take the header and footer's kinds alone (`stamps::WATERMARK` and `BACKGROUND` deleted) | today | 17 (deleted: `RemoveHeaderFooter` takes others' headers by origin through Remove Watermark's lowering, the app's Remove a planner; derive.rs, `stamps.rs` and the app's derived flows gone) |
| The watermark generator carries its own z, `Scope` and `replaces` (`GenSpec::Watermark(Arc<WatermarkGen>)`), and `Generator::{covers, z, scope, replaces}` read them as §3.5's fields would be, the header's made from `HeaderFooter.pages` (a `Cow<Scope>`), `Over` and none; `Generator` is still `{ id, spec }` | 16 | 17 (deleted: `Generator { id, spec, z, scope, frozen, replaces }`, `WatermarkGen` its content and position, `PageCtx` and `CtxNeeds.label` added) |
| The Header & Footer dialog's preview strips the previewed page's drawn header through `derive::strip_stamps_of` (one page's strip, which `strip_stamps` runs over the order), then `Tx::set_page_base` (`Derive::StripHeaderFooter`) and the dialog's `SetHeaderFooter` in a Tx at its snapshot, drawn by its own realizer | 13 | 17 (deleted, with derive.rs: the preview lowers `SetHeaderFooter` alone, whose `replaces` takes the drawn header off the pages it covers, not `RemoveHeaderFooter` + `SetHeaderFooter` as planned) |
| The legacy OCR layer installed after the generator blocks (`text_layer::install` leaves `3 Tr` set without isolating it), so a page with OCR and a header reads its own text, the header, then the recognized words; `page_res` writes the page's resources inline when the installer runs after the stack | 17 | 18 (deleted: OCR is a block of its own, in a `q` of its own, after `QClose` and before the generators drawn over the page, §11.4, so the page reads its own text, the recognized words, then the header; `page_res`' inline case gone) |
| Proposal 8's accepted misses not asserted (`gate_7k`'s `ACCEPTED_MISSES`, asserted only with `SHEAF_GATE_PROPOSAL_8`; Greg kept live page numbers and accepted them, 2026-10-22): on the synthetic and the heavy, the first page drawn of a group every page of which is laid out anew (set unsaved, moved, one deleted, in the session and opened again; 19–27 ms on the synthetic against 15, 32–42 ms on the heavy against 25 against the realize gate's 15 and 25), and the reorder's Append on the file opened again (1.57 s on the heavy, 0.89–1.07 s on the synthetic over two runs against 1 s; its section is asserted) | 17 | option A, a lead window, revisited (Greg) |
| The flattened export's RSS gate not asserted (`gate_7k`'s flatten row, asserted only with `SHEAF_GATE_FLATTEN`): `FlattenAnnotations { All }` on the heavy 7k peaks at 514 MB (513–515 MB over five runs, 293 MB added) against 1.5× the file + 200 MB (220.5 MB), and opening the heavy alone peaks at 221 MB, so no flatten in that process meets it: the export's Rewrite loads the primary eagerly to copy it | 19 | Greg's ruling (slice 19's For Greg) |
| WIT 0.2's `ocr.page-text-layer`, kept for 0.2 plugins (the frozen 0.2 fixture wasm imports it) and answering none (`api/v02.rs`'s `ocr::Host`, `recognize` shared with 0.3's through `api::shared::recognized`): recognized text is the page's content, saved or not, which `text` reads, and 0.3 has no such function | 18 | 20 (deleted, with WIT 0.2, `api::v02` and the 0.2 fixture; 0.4 has only `ocr.recognize`) |
| Spike code | 2 | 2 (deleted) |

### Lint ledgers

**Clippy `disallowed-methods`** lives in per-crate `clippy.toml` files
for sheaf-doc, sheaf-ops, sheaf-render and sheaf-app. Each ban lands in
the slice that removes its last user. Test code is allowed through
`#![cfg_attr(test, allow(clippy::disallowed_methods))]`. Two pieces of
test code build without `cfg(test)` under CI's
`cargo clippy --workspace --all-targets` and get a module-level
`#[allow]` too: `forms/fixture.rs` (the `fixtures` feature other crates'
tests enable) and `examples/roundtrip.rs`. Both bans landed in slice 19;
an integration test is a crate of its own, so each one that uses a
banned method carries its own `#![allow]` (slice 19's notes list them).
Slice 20 lands no ban. The ledger planned none for it, and the two
candidates it leaves are better held otherwise. lopdf's writer
(`Document::{save, save_to, save_modern}`) lost its last user outside
tests and fixtures with `annot::tape_pdf`'s `save_to`, but it is how 29
integration tests, the forms fixtures and the fuzz helpers
(`source::fuzz`, built without `cfg(test)`) make their input files, so a
ban would be an `#[allow]` in each of them and keep nothing out.
Rule 7's lopdf `ObjectId` is a type, allowed in some modules and not in
others, which `disallowed-types` can't scope; `tests/object_ids.rs`
holds it (Tests, fixtures and gates).

| Ban | Lands | Last users removed |
|---|---|---|
| `lopdf::content::Content::{decode, encode}`, `lopdf::Document::{get_page_content, change_page_content, add_page_contents}` | 19 (landed) | `stamps::strip`'s round trip (6; `stamps::strip` deleted 17), `forms::check`'s `da_font` (forms/check.rs:760–761, 6), stamp_lib.rs's page-to-form (stamp_lib.rs:408, 6), `watermark::install` and `read` (16, deleted), `flatten_annots` (19) |
| `std::env::temp_dir` (non-test) | 19 (landed) | the recovery temp primary (docview.rs:1024, 9), the layers panel (3; the decrypted in-memory copy that replaced its temp file went in 13, which lands no ban: the print and export temp files remain), the print and export temp files (14, deleted: print sends its job to CUPS and GDI from memory, and export's flatten already read its print file from memory, so 14 lands no ban either), the generated cache (19). Three legitimate uses get an `#[allow]` with a reason: `recent.rs`'s path check; the attachments panel's folder for opened attachments (the one user-requested file; M11 slice 1 deletes it at close); and the plugin installer's archive folder (`install_archive`, plugins.rs:529), which holds no document content and is removed after the install. |
| — | not needed | sheaf-instance's `socket_path` fallback, which kept that crate without a `clippy.toml`: the crate is gone since, replaced by instance-handoff, whose fallback for a long socket path is its own (a private per-user folder, not the shared temp folder) |

**The retired-symbols ledger** (`tests/retired.rs`, from slice 1) lists
qualified paths and is matched with a `syn` walk. `generated::*` and
other deleted modules are covered by it, not by clippy.

### Tests, fixtures and gates

| Runs | Tests |
|---|---|
| **Default (`cargo test`)** | the replay property (64 cases); invariants after every op; one test per cascade rule; removal-check unit tests; **Append ≡ Rewrite reopen facts (32 cases)**; determinism (emit twice, two threads); the RC4/AES and `/EFF` fixtures; the header-offset fixture; CLI goldens (slice 19, sheaf-cli's `tests/golden.rs`); the retired ledger; the private-keys scan (slice 19, `private::found` and `sheaf_names`); the registry destructure compile checks; the schemars snapshot; the `ObjectId` grep (slice 20, sheaf-doc's `tests/object_ids.rs`: no lopdf `ObjectId` in model-level code, a reasoned allowlist, and its walk and probe self-tests); `docs/plugins.md`'s Op examples lowered and committed as a plugin's (slice 20, sheaf-plugin's `tests/docs_examples.rs`); one plugin world (slice 20, sheaf-plugin's `tests/one_world.rs`: one `bindgen!`, one `generate!`, `wit/` holding only `sheaf.wit`) |
| **Corpus, skipped without `$SHEAF_CORPUS` / `../pdf-corpus`** | lazy-vs-eager object equality; lex + splice byte identity; classification of pagination origins; `forms::check` clean on every output of every op family; deleted-means-deleted (each feature × both strategies, removed strings absent from the decoded bytes); overlay against saved-file pixel equality (appearances only); signed and certified fixtures (picked by script from the 322 `/DocMDP` corpus files) |
| **Ignored (`--ignored`, run per slice and before merge)** | `qpdf --check`; pdf.js open; the 402-file pool realizer benchmarks; the lexer-vs-PDFium boundary differential; the 7k gates; the replay property at 4,096 cases; Append ≡ Rewrite at 1,024 cases |
| **Nightly** | fuzz targets (the lazy reader, page tree, lexer, splice, `/SheafAnnot` v2, `/SheafGenerators`, Op JSON) |
| **Manual, recorded in the slice notes** | Acrobat round trips (open, edit our header and watermark; signed files' "changes since signing") |

**Fixtures:**
- the **synthetic 7k** (3.5 MB);
- the **heavy 7k**: embedded subset fonts, an xref stream with object
  streams, about 20k annotations including widgets, 2k bookmarks,
  labels;
- the **scanned 7k** (about 500 MB), generated by `bench/gen.py` and
  never committed.

**Gates** (the ignored test `tests/gate_7k.rs`, run per slice):

| Gate | Threshold |
|---|---|
| geometry walk | < 100 ms synthetic, < 250 ms heavy |
| ready (open → edits enabled) | < 1 s heavy |
| realize + open + render one edited page | < 15 ms synthetic; < 25 ms heavy (Greg, 2026-10-20; was ≤ 1.7× the warm base render) |
| crop commit, all pages | < 200 ms |
| Bates apply, all pages | < 50 ms |
| live-token reorder: all pages of the heavy 7k under "Page n of N", after a save (proposal 8's verdict) | the next Append ≤ 8 MB and ≤ 1 s; each visible page re-renders within the realize gate (slice 17: the Append passes in the session that saved the header; on the file opened again, and the first draw of a group laid out anew, it misses, and Greg kept live numbers and accepted the misses, 2026-10-22) |
| Rewrite | ≤ 1.3× today's writer's time (flagged past 1.2×) and ≤ 1.5× its RSS, synthetic and heavy |
| incremental save of one mark | < 50 ms on the 500 MB scan |
| engines after visiting 200 groups | within slice 2's byte cap (96 MB weighed, and RSS added beyond the base engine's) |
| thumbnail strip after crop + watermark + header on all pages (slice 13: crop + header, the watermark from 16) | ≥ slice 2's fill rate (310 pages/s), within the cap (96 MB weighed, and RSS added), synthetic and heavy |
| export `FlattenAnnotations { All }`, heavy | peak RSS ≤ 1.5× file size + 200 MB (slice 19: unattainable by construction, the open alone at the limit; measured, asserted only with `SHEAF_GATE_FLATTEN`, for Greg) |
| history: 200 OCR versions on a 1,000-page scan | within the 256 MB policy |
| a `Snapshot` of each fixture as opened (slice 12), and what the app publishes for plugins, `Document::published` (slice 20) | < 50 µs, the median of 21 |
| 1,000 mark adds on blank pages, each a version (slice 12) | the last 100's median ≤ 1.5× the first 100's |
| idle CPU | 0% |

1. [History stores what changed; the retired ledger](s01-history-stores-what-changed-retired-ledger.md)
2. [Spikes, heavy fixtures and the gate harness](s02-spikes-heavy-fixtures-gate-harness.md)
3. [Lazy sources and the Store](s03-lazy-sources-store.md)
4. [One open, gated](s04-one-open-gated.md)
5. [PageSpace and absolute rotation](s05-pagespace-absolute-rotation.md)
6. [The content toolkit and fonts](s06-content-toolkit-fonts.md)
7. [The serializer](s07-serializer.md)
8. [Records](s08-records.md)
9. [Rewrite from records; saves are outputs](s09-rewrite-from-records-saves-are-outputs.md)
10. [Append, the planner and the removal check](s10-append-planner-removal-check.md)
11. [Ops and Tx](s11-ops-tx.md)
12. [One funnel and the history budget](s12-one-funnel-history-budget.md)
13. [Realizer, pool, render keys, view inputs](s13-realizer-pool-render-keys-view-inputs.md)
14. [Text service; print, preview and page images from bytes](s14-text-service-print-preview-page-images.md)
15. [Crop as geometry](s15-crop-as-geometry.md)
16. [Watermark, background and strip](s16-watermark-background-strip.md)
17. [Header/footer and Bates; derived sources gone](s17-header-footer-bates-derived-sources-gone.md)
18. [OCR as content](s18-ocr-as-content.md)
19. [Exports, memory sources, Session and `sheaf edit`](s19-exports-memory-sources-session-sheaf-edit.md)
20. [Plugin API 0.4](s20-plugin-api-0-4.md)

### Transitional code ledger

## About the notes

in-progress 0.1.** sheaf's progress crate is published as `in-progress`
(sheaf-pdf/in-progress), and sheaf takes it from crates.io;
`crates/sheaf-progress` is gone, and the notes above name it as it was.
For later slices:
- **Dropping a child fills its share** unless it was cancelled: on a
  return, `?`, a panic or the end of a temporary like
  `&*p.child("reading", 0.7)`. Keep no long-lived clones of a child
  tracker, and never `let _ = p.child(..)`, which fills it at once.
  Children are `#[must_use]`.
- **`cancel()` stops a subtree only.** Hold the root to stop a job; every
  cancel in sheaf is on a root `Tracker::new()`.
- **Weights are `f64`.** Count pages in `sheaf_doc::PAGES` (the noun is
  compared, so it's spelled once) and other things in `Unit::ITEMS`.
- **Siblings add up,** a count after a child no longer pulls the bar
  back, and a dropped step hands the snapshot's step back to its parent.
- **Classify errors with `sheaf_ops::was_cancelled`,** which also finds a
  `Cancelled` inside an `io::Error`, what `p.check()?` makes in code
  returning `io::Result`: slice 7's serializer needs it for "stopped"
  and exit status 130.
- **Plugins:** a task past `MAX_TASKS` gets its detached tracker before
  anything is placed (`has_task_room`), since a placed one would fill
  its share when swapped out; the poll loop clears the last progress
  line it showed, which the snapshot no longer makes once the call's
  tasks are freed; and the WIT now says a second task doesn't move the
  bar (it never started where the first left off).

**Between 6 and 7 (own commit, 2026-10-17): sheaf-progress replaced by
