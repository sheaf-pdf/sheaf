# M10.5 slice 19: Exports, memory sources, Session and `sheaf edit`

Status: done · Part of [M10.5: the edit API](README.md)

**Exports, memory sources, Session and `sheaf edit`** (XL; proposals
10, 35 and 40; bugs 16, 21, 22 and 29).
- **`export::run`:**
  - preflight, with the signature and `XfaHoldsValues` asks;
  - an `ExportTx`;
  - `rules::plan`;
  - `Deferred` bodies;
  - progress and cancel.
- **Transforms:** `KeepPages`, `RemoveMarkup`, and
  `RemovePrivateData` with `StripLegacy`.
- **`FlattenAnnotations`,** with `PinnedAp`.
- **The presets.**
- **Memory sources:** blank pages, images-to-PDF, scan, the summary
  and library stamps.
- **`Session`.**
- **Combine** on `Session`, with the app handing the same Document
  to a new window.
- **`sheaf edit`:**
  - its flags and exit code 3;
  - JSON addressing;
  - `-o --append`.
- **`sheaf inspect --json`.**
- **Every subcommand** runs on `Session`.
- **Print** goes to GDI and CUPS from bytes.
- **The clipboard blob cache.**
- **The content-round-trip and `temp_dir` clippy bans** (Lint
  ledgers).
- **Deletes:**
  - **The old exports:**
    - `subset` with its fake `Snapshot`, and `prune_outline`;
    - `print_pdf`, `extract_pages`/`extract_bytes`,
      `export_clean_copy`, `export_flattened`;
    - `flatten_annots` and `normal_appearance`'s widget fallback;
    - `strip_markup_annots`.
  - **The generated cache:** generated.rs and the cache directory,
    once its last callers move:
    - page_insert.rs:83–84 and :129 (`blank_pages`, `store`): blank
      and image pages are `InsertPages { from: Blank | Images }`
      memory sources;
    - sheaf-ops combine.rs:99 (`page_source`): Combine's images are
      `PageSource::Images`;
    - stamps_panel.rs:125–127, and stamp_preview.rs:114's read of a
      stamp's PDF by stem: library stamps are `Memory(Library)`
      blobs;
    - import.rs:358–362: an imported stamp's appearance is a
      one-page PDF blob (`FormRef::Blob`).

    Earlier slices moved the rest: stamp_preview.rs's pictures and
    annot.rs's and file.rs's cache reads (6), attach.rs,
    attachments.rs and import.rs's attachments (8), crop.rs:150
    (15), and derive.rs:154 (16) and :71 (17).
  - **Hand-rolled flows:**
    - scan's `add_text` and its throwaway Document;
    - blank.rs's and comments import's hand-rolled open and save;
    - Combine's write-then-reopen.
  - **`/SheafFlat1`.**
  - **The transitional export path built on `subset`.**
- **Checks:**
  - **Ports:** the extract, split, print, clean and flatten tests
    are ported.
  - **One parametrized every-way-out test** over M10 proposal 7's
    list:
    - `forms::check` is clean;
    - the open file is refused as a target by every export;
    - a clean copy has no `private::KEYS`, including a legacy
      watermark's `SheafSettings`;
    - an unsaved Bates keeps its numbers in an extract;
    - flatten honors Print and Hidden, with no first-state widget
      fallback;
    - a hybrid form's XFA is dropped when widget pages are left out;
    - nothing is written to the temp or cache directories, and no
      `generated::` path remains (the retired ledger).
  - **Combine:** the 1040, Schedule 1 and Schedule B give one
    version, unique names, and the notices plus the scripts state in
    the new window.
  - **Other flows:**
    - split by MaxBytes reproduces today's parts;
    - inserting blank or image pages writes nothing to disk;
    - importing the same XFDF twice updates its marks instead of
      duplicating them, keeping their `/NM`, dates and authors, and
      replies still thread by name
      (`imports_match_by_name_and_report_the_rest`, ported).
  - **The CLI:**
    - CLI goldens pass, with exit codes 0, 1, 2, 3 and 130;
    - `--force` only overwrites;
    - `--dry-run` prints lines;
    - a signature ask exits 3 with the Ask as JSON;
    - `-o out.pdf --append` on a signed file keeps its signatures;
    - JSON addressing by position (1-based), title path and `--with`
      works, and a bare-number `Id` ref is `Invalid`;
    - the docs' JSON examples run in CI.
  - **Pasting** a stamp mark into another window works, because the
    blob travels.
  - **Gate:** `FlattenAnnotations { All }` on the heavy 7k exports
    with peak RSS under 1.5× the file size plus 200 MB.

## Notes

Done. Exports, memory sources, Session and `sheaf edit` (proposals
   10, 35 and 40; bugs 16, 21, 22 and 29): every way out of a document
   but a save goes through one writer, `sheaf_doc::export::run`, an
   `Export` of a version, its transforms lowered on an `ExportTx` that's
   never committed (`KeepPages`, `RemoveMarkup`, `RemovePrivateData` and
   ops), written by `materialize` with a save's checks, its asks the
   plan's. `FlattenAnnotations` is an op and a content layer of forms
   placed by reference (`Static::Placements`, `FormRef::Pinned` for
   sheaf's marks), drawn as a `/SheafBlock << /K /Flat >>` block after
   the page's content. sheaf's private data is a table (`private::KEYS`,
   §15), and a Strip output holding any of it fails. Blank pages,
   images, scans, library stamps and the comment summary are made in
   memory and written through sheaf's serializer, so the generated cache
   is gone. A `Session` in sheaf-ops runs every subcommand, saves
   through `materialize::save_with` (`materialize::save` gone) with
   proposal 40's `-o`/`--append`/`--in-place`, `--signatures` and
   `--xfa`, and exit 3; Combine is one transaction on the first file's
   session, handed to a window as made; `sheaf edit` and `sheaf inspect
   --json` are new, with JSON addressing, CLI goldens and the docs'
   examples run in CI. The clipboard holds blobs by `BlobId`, and the
   content-round-trip and `temp_dir` clippy bans landed. The flattened
   export's RSS gate is unattainable by construction (the open alone is
   at the limit): measured, and asserted only with `SHEAF_GATE_FLATTEN`
   (For Greg). Every other asserted gate holds, and the corpus counts are
   #26's.
   - **Memory sources** (step 1). sheaf-edit: `PageSource::Blank { size,
 count: u32 }` and `Images { images: Vec<BlobSel>, fit: Fit }`;
 `Fit::{Original, Paper { width, height, margin }}` with `LETTER` and
 `A4`, in place of `sheaf_doc::images::PageFit`; `every_op` gained
 two memory-source inserts and an images replace; ops-1.0.json
 regenerated in place. serialize/full.rs: `write_built(&lopdf::
 Document)`, unencrypted and deterministic over `write_bodies`, keyed
 by `ObjKey::Built(ObjectId)` (tag 20); `images::to_pdf` and
 `stamp_lib::finish` write through it. memory.rs (new):
 `blank_pages(w, h, count)` (0 refused), `blank`, `images`, and from
 step 5 `empty`, interning `Memory(Blank | Images)` through
 `BlobKind::memory(kind)`; store.rs: `MemoryKind::{Blank, Images}`,
 `Store::read_image`. tx/resolve.rs: `Resolver.build` (apply builds
 and interns the source; `Tx::check` builds and reads nothing), Blank
 validating its size and count, Images resolving each `BlobSel` to
 image bytes, the names ("blank page", "N blank pages", a `With`
 image's name, "N images"). App: `insert_blank` and `insert_images`
 plan the ops through `plan_made`, the images read off the UI
 thread and pinned; `insert_made` became `insert_planned`;
 `load_stamps` names a stamp by its blob's stem and writes no file.
 Combine: an image item is `InsertPages { from: Images }` (from step
 5, a first one into a `new_empty` session); `page_source` went.
 import.rs: an imported stamp interned as `BlobKind::stamp()`;
 `find_stamp` lost its cache arm and absorbed `held_stamp`;
 `ForeignAnnot.stamp` (an adopted page stamp's `/AP /N`) in the
 held-stamp index. Tests: tests/common's `own_cache` became
 `own_dirs` (a child process whose `TMPDIR`, `TMP`, `TEMP`,
 `XDG_CACHE_HOME` and `HOME` are empty and must stay so);
 `SHEAF_CACHE_DIR` went from common, crop.rs and gate_7k (three
 `set_var`s); the fixture users moved to `memory::blank_pages` or
 blobs.
   - **`FlattenAnnotations`** (step 2). sheaf-edit:
 `Op::FlattenAnnotations { pages, which: FlattenSet::{All, Printable}
 }`. records.rs: `Static::Placements(Arc<[Placement]>)`, `Placement {
 form, matrix, tag }`, `PlaceTag::{Artifact, Untagged}`,
 `FormRef::Pinned(Arc<PinnedAp>)`, `PinnedAp { inputs: MarkAp, digest
 }` (equal and hashed by digest, `PinnedAp::new`), `MarkAp` (kind with
 a connector's partner cleared, color, opacity, stroke, contents,
 fill, dashed, cloudy, turn, a page stamp's form), `BlockKind::{Ocr,
 Flattened}` from `Layer::kind()`, `ContentStack::with_layer`,
 `FormRef::digest`; prim.rs' heap sizes, realize/key.rs'
 `pinned_digest` and `form_ref_digest`, `check_invariants` (no pinned
 mark names an annotation), effects tracking shown layers by
 `Static::addr` with `RemovedKind::Flattened`, untouched.rs (a page
 with placements is never untouched). tx/flatten.rs (new): page by
 page in `/Annots` order with progress and cancel; links, Hidden,
 NoView and (Printable) unprinted skipped; a mark's `Pinned`
 placements by the identity (a replacement two: the caret and its
 StrikeOut, through the new `annot::companion`), replies and statuses
 none; others' comments and widgets `Base` of their `/AP /N` through
 `Affine::onto`, a widget by its `/AS` only, a comment falling back to
 its first state; read unkept; a layer tagged Artifact when the source
 has a structure tree; `drop_annots` and the cascades; an unreadable
 page `EditError::Content`; cap Approval, "Flatten N [printable]
 annotations on …". emit/content.rs and emit/annots.rs: over layers
 after `QClose` in stack order, a Flat block `/SheafBlock << /K /Flat
 /B (b…) >>` (`ObjKey::Block`), `q <m> cm /Sh… Do Q` a placement,
 `/Artifact BMC … EMC` when tagged (`ContentWriter::begin_tagged`),
 `form_of` (from `stamp_form`) for every `FormRef`, a pinned form
 drawn again through `annot::appearance` under `ObjKey::Pinned(u128)`
 (tag 21), both `Deferred` and deflated (`Cx::push_deferred`,
 `emit::deflated`). Reopen: `Marker::Flat(BlockId)`, kept by `intact`
 and `rebuilt` as saved OCR is; removal.rs judges a saved Flat block
 as the file's own and a pinned form through its page (`Drawn::on`).
 OBJRs: `Emission::removed_annots`, passed to `keep_only(pdf, kept,
 removed)`. `AnnotKind::flags()` and `annot_flags::{NO_ZOOM,
 NO_ROTATE}`.
   - **`RemovePrivateData`** (step 3). private.rs (new): `KEYS` (a row
 per §15 name: places, exact or prefix, older or written, how Strip
 takes it out), `LIST_KEYS` and `STREAM_KEYS` derived at compile
 time (`model::LEGACY_KEYS` retired), `is_private`,
 `is_resource_name`, `is_sheaf_nm`, `neutral_nm`, `in_annot`,
 `strip_annot`, `found(pdf)` (failing closed on unlexable content
 that says `/Sheaf`), `sheaf_names`. materialize: `Private::{Keep,
 Strip}` in `Profile`, through `EmitBase::Rewrite { private }` to
 `Cx.private`; `verify::private`, the post-condition on the Rewrite's
 graph; a Strip target planned as an Append refused. The emitter under
 Strip: marks always written anew, no `/SheafAnnot`, neutral `/NM`s;
 others' comments' sheaf keys out, `/Name SheafTick*` as `/Tick`; no
 `/SheafGenerators`; blocks and isolation unmarked;
 `generate::layout` and `text_layer::block` take a `Private`.
 records.rs: `LegacySel::{Marked, Streams, Resources}`;
 emit/content.rs: `Streams` copies each marked stream dict-stripped,
 `Resources` strips named lists (a copy-on-write `PageRes` edit) and
 renames legacy names `/Sh…` (`Reserved::rename`, the new
 `content::renames` over the content's operands), the Keep path's
 object order as before. tx/content.rs: `private_filters(def)`;
 `PageBlocks::sheaf_marked`, `Reserved::names`,
 `PropertiesDict::lists`; `RemovedKind::OlderSettings` became
 `PrivateData`.
   - **`export::run`** (step 4). sheaf-edit transform.rs (new):
 `Transform::{Op, KeepPages { pages, outline: OutlinePolicy::
 {Headings, Promote, Drop}, labels }, RemoveMarkup { keep:
 MarkupKeep { links, widgets } }, RemovePrivateData}`, outside the
 envelope. tx/export.rs (new): `ExportTx::{new, apply, state,
 private, unheld, flattened}`, `transform_cap` and `actor_exports`
 (caps.rs). export.rs (new): `ExportFrom { snap, cx }` and
 `Document::export_from(actor)`, `Export`, `Out`, `Gate`,
 `preflight`, `ExportError`, `Exported`, `run`, the presets
 (`Export::{whole, extract, print, flattened, clean_copy, rejects}`),
 `PRINT_ANSWERS`, `printed`, `print_preview`, and for tests
 `Document::export_bytes`. materialize: `PostPass` and
 `materialize_then` gone, `verify::links` private, `Profile.unheld`
 (`Unheld::{Keep, Only(MarkupKeep)}`) through `EmitBase::Rewrite` to
 `Cx::keeps_unheld` in `annots::unmodeled`. save.rs: the old exports
 deleted (Deletes), `rewrite` and `rewrite_to_path` an
 `Export::whole`, taking `&Document`. sheaf-ops: `export::run` and
 `flattened` take an `ExportFrom`, page images and text through
 `durable::write`; `split::split(from, …, answers, p)`, its parts and
 MaxBytes probe `Export::extract`; `blank::write` exports two presets
 and commits nothing; the summary reads `export::printed`. App:
 `DocView::write_export`, `ask_before_writing(Asking::{Save,
 Export})` with `Exporting { answers, again }`, Split a job
 (`split_into(Splitting, answers)`), Save as PDF through `Out::File`,
 `send_print` through `printed`, `PrintContext.from`; methods renamed
 (`flattened_copy_to`, `flattened_copy_prompt`, `clean_copy_prompt`).
   - **Session** (step 5). sheaf-doc: `save_with(doc, path, SaveHow::
 {Planned, Rewrite, Append}, answers, p)` (public, over the private
 `saved_as` with a planner and a `Refusal`; `materialize::save`
 deleted, about 60 test calls ported); `materialize::appending` (the
 planner of `SaveHow::Append`: `CopyWhen::Whenever`);
 `Refusal::Chosen`; `open::read(path, pw, p)` (`open::file` calls it
 with `NoProgress`); `memory::empty` and `Source::open`'s zero-page
 rule; `MemoryKind::Summary`; `Document::forget_before_cursor()`
 (`advance_root` to the cursor); `exchange::gather` through
 `annot::emission` with `WriteCx::held` and `form`, `fdf::write`
 through `write_built`; `annot::{to_pdf, to_pdf_in, Written, write}`
 fixtures only; save.rs' `whole` module. sheaf-ops session.rs (new):
 `Session { doc, held, named }` (`view` from step 7), `open`,
 `new_empty`, `intern`, `hold`, `name`, `tx`, `commit`, `commit_as`,
 `save(SaveTo::{InPlace, File { path, append }})`, `export_from`,
 `export`, `From<Document>`; `NeedsDecision` and `Decided`.
 combine.rs: `build` and `combine(items, params, out, answers, p) ->
 (Session, CombineReport)` (Combine, below), the `open::start` and
 `Facts` path deleted. scan.rs: `made_pages`, `add_text` and the
 `.images.part` file gone; images.rs on the same path; blank.rs on a
 `&Session`; split returns `NeedsDecision`; comments: `open` a
 Session, `import(session, bytes, pw)`, `summary` a Session
 (`summary_pdf` gone), written by `write_built`. sheaf-cli: decide.rs
 (new, `--signatures`, `--xfa`), comments import's `-o [--append]` and
 `--in-place`, `run` mapping `NeedsDecision` to exit 3 with its JSON,
 `ANSWERS` deleted, csv/text/data export refusing an existing output
 without `--force` and writing durably, every subcommand opening
 through `Session::open`. App: the comments panel saves the summary
 through its Session, data exports durably.
   - **`sheaf edit` and `sheaf inspect`** (step 6). sheaf-doc:
 `Tx::ops_from_file()`, which resolve.rs enforces (a bare-number page,
 bookmark or annotation, or `{"blob": …}`, is `EditError::Invalid`,
 checked or applied, carried through `TxParts::relowered` and the
 ExportTx's resolver); `Corpus::signatures()`, `Entry.extra` and
 `Entry::column`. sheaf-ops: `Session::edit(Envelope, p)` (labelled
 by the envelope, else by its first line) and `Session::preview(&
 Envelope, p) -> Previewed { preview, state, removed, signatures }`.
 sheaf-cli: edit.rs, inspect.rs and names.rs (new); `SUBCOMMANDS`
 and `Command` with `edit` and `inspect`; `print_json` with sorted
 keys; progress.rs' plain lines off a terminal. tests/golden.rs and
 18 goldens, tests/edit.rs, tests/common; docs/document-model.md's
 `sheaf edit` section with five run examples; README's command line;
 `.gitattributes` pinning the goldens to LF.
   - **The app** (step 7). sheaf-ops: `Session.view` (the open's
 `ViewFacts`). combine_dialog.rs: the background work runs `combine`
 and emits `CombineEvent::Created { path, combined: Combined, params
 }` (`Combined` a `Cell<Option<Box<(Session, CombineReport)>>>`,
 taken once); the write and reopen went. workspace.rs: `show_made`
 (the origin window when it's empty, else a new one),
 `Workspace::adopt` (`watch`, then `DocView::adopted`), `is_open`.
 docview.rs: `DocView::adopt` and its core `adopting`, `adopted`
 (`ChangeSet::opened`), `ready()`'s view facts factored into
 `take_view`. markup_tools.rs: `CopiedAssets { blobs, by_stem }`,
 `of(marks, &Document)` making a Base or Pinned stamp a one-page PDF
 blob at copy, `give` interning into the target store. sheaf-ops
 tests/exports.rs (new): `every_way_out_keeps_the_form_and_its_rules`
 and the ignored `export_round_trips` (`SHEAF_EXPORT_ONLY`);
 `nothing_found` and `told` moved to tests/common.
   - **Gates, lint bans, retired.rs, docs** (step 8). gate_7k: the flatten
 part measures the open in its process first (`open, peak RSS`, `open,
 RSS added`), and on the heavy asserts the flatten's peak ≤ 1.5× the
 file + 200 MB only with `SHEAF_GATE_FLATTEN` (`FLATTEN`,
 `UNATTAINABLE`), the module doc saying so. clippy.toml of sheaf-doc,
 sheaf-ops, sheaf-render and sheaf-app: the six bans with reasons;
 `#![cfg_attr(test, allow(clippy::disallowed_methods))]` at each crate
 root; module allows on forms/fixture.rs and examples/roundtrip.rs;
 `#![allow]` in 12 integration tests and sheaf-ops' tests/common
 (Deviations); reasoned `#[allow]`s on recent.rs' `is_sheaf_temp`,
 attachments_panel.rs' `temp_dir` and plugins.rs' `install_archive`.
 forms/check.rs' `Content::decode` (in its test module),
 content/corpus.rs (`#[cfg(test)]`) and source/fuzz.rs' `temp_dir`
 calls (its test module) are test code, as clippy confirms; each ban
 was checked to fire by a probe calling all six from sheaf-doc's
 library code, removed and the file checked with `cmp`. retired.rs: 47
 slice-19 rows, in groups with their comments (step 8 added
 `page_insert::DocView::insert_made`). The ledgers (above), the
 slice-19 section of docs/document-model.md and its forward
 references, README's flatten and clean copy rows. Two fixes the
 verification found (Found): annot.rs' `StampForm.res` and
 `StampForm::written`, emit/annots.rs' `pinned` naming a stamp's page
 by its form; split.rs' size test's limits.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy, the names in retired.rs:
 `save::{subset, paired, form_on, prune_outline, print_pdf,
 extract_pages, extract_bytes, export_clean_copy, export_flattened,
 flatten_annots, normal_appearance, strip_markup,
 strip_markup_annots, is_link_or_field, without_markup, EXPORT,
 CLEAN, written, is_the_file}` (with them `/SheafFlat1`'s writer and
 the widget first-state fallback); `materialize::{PostPass,
 materialize_then, save}`; `generated.rs` whole
 (`cache_dir`, `store`, `store_in`, `name_of`, `is_stem`, `path_of`,
 `stem_of`, `write_new`, `blank_pdf`, `blank_pages`) and
 `SHEAF_CACHE_DIR`; `images::{PageFit, files_to_pdf}`;
 `model::held_stamp` and `find_stamp`'s cache arm;
 `combine::page_source` and Combine's `open::start` path and
 write-then-reopen; page_insert.rs' blank and image cache writes
 (`insert_made`) and the stamps panel's; import.rs' stamp cache
 store; `model::LEGACY_KEYS` and `RemovedKind::OlderSettings`;
 `scan::add_text`, its throwaway Document and the `.images.part`
 file; blank.rs' and comments import's hand-rolled open and save;
 `comments::summary_pdf`; the app's `ask_before_saving`,
 `export_flattened_to`, `export_flattened_prompt`,
 `export_clean_copy_prompt` and `PrintContext::snapshot`; sheaf-cli's
 private `ANSWERS` (not retired: `save.rs` and exports.rs name
 consts so). `rg 'generated::|SHEAF_CACHE_DIR' crates` finds only
 retired.rs.

   Deviations from the plan (the planner's list first, as resolved):
   - **Export carries its `LowerCx`:** a `Snapshot` alone can't lower,
 so `ExportFrom { snap, cx: Arc<LowerCx> }` (`Document::export_from`)
 travels with the export, and what its transforms make draws ids
 from the document's allocator, not scratch ids. `save::rewrite` and
 `rewrite_to_path` take `&Document` for it; realize_corpus and the 7k
 AES row, which hold a modified snapshot, build an `ExportFrom`. The
 whole-document Rewrites answer what they ask as print does, as the
 old `written()` ignored the plan's asks; their bytes are unchanged
 (encrypted_corpus and the determinism tests).
   - **`BlockKind` is derived:** `Layer { id, body }` stays, and
 `Layer::kind()` gives `BlockKind::{Ocr, Flattened}` from the body.
   - **`PinnedAp` covers marks only,** as `MarkAp` (no AnnotId, author,
 dates or `/NM`; an attachment's file isn't drawn and isn't
 captured); a widget's pinned appearance is M10 slice 15's
 `FlattenFields`, and a flattened widget is a `Base` placement of its
 form. A pinned form is drawn by `annot::appearance`, the drawing
 emission uses, not by emission (which would build the dictionary
 and `/SheafAnnot` too); the identity matrix places it, its box its
 `/Rect`, and PDFium's grid snap leaves pixels within 2 a channel.
   - **`save_with` is the save core:** `save_with(doc, path, SaveHow,
 answers, p)` over the private `saved_as`; `SaveHow::Append` isn't
 `forced_append` (which asks nothing, so `-o --append` would silently
 break signatures) but the production planner `appending`, with
 `Refusal::Chosen` letting a Rewrite stand in only when the reader
 chose to break signatures. sheaf-ops' `common::planned`,
 forms_saves' `save` and gate_7k's reorder save call
 `save_with(Planned)`, the app's planned Save As, which `SaveTo` can't
 say by design.
   - **save.rs keeps `rewrite` and `rewrite_to_path`** as a fixtures-only
 `whole` module over `export::run(Export::whole)`, beside the live
 `text_string` and `validate` (Found: `validate` has no caller):
 materialize_corpus's IDENTITY, encrypted_corpus and the both-ways
 harness read a save's bytes through them.
   - **`KeepPages`' Headings rule is `subset`'s outline rule,** not
 DeletePages' promotion: before the delete, a bookmark whose target
 isn't a kept page (`Target::Kept` included) stays only as a heading
 over kept descendants. Connectors, fields shown only on pages left
 out and labels follow the delete's cascades, as `subset` did; the
 recorded MaxBytes parts and the CLI's part sizes are unchanged (the
 sheaf-ops fixture's three rows confirmed in review against main
 3c72df2's split, ten runs alike).
   - **RemoveMarkup's emission option is a Profile field,**
 `unheld: Unheld { Keep, Only(MarkupKeep) }`, set by the `ExportTx`
 and copied by `run` (§12's Profile has three fields); two
 RemoveMarkups keep only what both keep. No post-pass and no model
 field.
   - **The asks come from the plan,** in `run`, not `preflight`: `Gate`
 holds refusals only (`OwnFile`, `Exists`, `Refused { transform, cap
 }`). The Signatures ask is limited to signatures whose field the
 exported state still holds (a certification always counts).
 **Print answers its own** (`PRINT_ANSWERS`: break signatures, drop
 XFA). **Print without markup gets `RemovePrivateData`** (§16 row 27)
 and `Private::Scrub` (review): written as Strip writes it, but what
 the filters can't take out (a page whose content can't be read, a
 list that can't be written again) is a note (`verify::private_left`),
 not a failure, so printing, page images and text never fail on an
 older sheaf's leftovers; Clean Copy keeps Strip, failing closed. A
 transform's Strip raises only a Keep profile. With markup print keeps
 them.
   - **The preview doesn't stay on view inputs alone:** the plan kept
 print preview as `ViewInputs::print(markup)`, with a test that it
 equals the Print preset's bytes. That test found 136 px different
 with markup (PDFium's form filler draws a box for a check box with
 states and no `/AS`, which the print flatten draws as nothing, M10
 bug 10), so with markup the preview realizes the print's state,
 the page's printed annotations flattened as print flattens them
 (`export::print_preview`, memoized a page in `PrintContext.printed`),
 under print's inputs; without markup it's the version under
 `ViewInputs::print`. 0 px differ both ways; the dialog no longer
 paints marks with GPUI over the preview.
   - **The schema grew in place** at 1.0 (`SHEAF_SCHEMA_UPDATE=1`):
 `PageSource::{Blank, Images}`, `Fit` and `FlattenAnnotations`, safe
 because nothing persisted Op JSON before `sheaf edit`. From this
 slice an addition bumps the minor. `Transform` is outside the
 envelope and its schema.
   - **Combine's outline and labels; generators extend:** on a full open
 the opened outline's roots are deleted and its labels cleared in the
 transaction, then each file's bookmarks re-added from its source's
 flat outline, so the output's outline and labels are today's. The
 generators stay, so a first file's All-scope header covers the pages
 combined after it (For Greg). The app fills the origin window when
 it has no document, else a new one; its dialog refuses an output
 open in a window (`workspace::is_open`). The hand-off is a
 `Cell<Option<Box<…>>>` taken once, not `Arc<Mutex<Option<
 Document>>>`: events go by reference on the UI thread, and the
 Session carries its view facts (`Session.view`), without which the
 adopted window would lose the first file's attachments, portfolio,
 layers and form facts.
   - **`open::read`:** `open::file` has 104 callers; `open::read(path,
 pw, p)` holds its body with progress, `Session::open` calls it, and
 `open::file` calls it with `NoProgress`.
   - **The integration-test lint allows:** an integration test is a
 crate of its own, and the crate root's `cfg_attr(test, allow(…))`
 doesn't reach it, so each one that calls a banned method (all
 `std::env::temp_dir`, for scratch files) has its own
 `#![allow(clippy::disallowed_methods)]`: sheaf-ops' `append_corpus`,
 `blank`, `comments`, `crop`, `forms_open`, `gate_7k`, `progress`,
 `scan_blanks` and `tests/common` (a module allow covering every test
 that includes it), and sheaf-render's `appended_pages`, `flatten`,
 `markup_appearance` and `ocr_layer`. sheaf-cli has no `clippy.toml`
 and needs none.
   - **No text export exists** as an export: slice 14's "print and text
 export" left print and page images; text (`sheaf export -f text`)
 reads the text service and writes durably. The app's exports run as
 jobs (review): `DocView::write_export` is `start_job` and `drive`,
 one at a time with page progress and Escape, for Extract, Export
 Flattened, Clean Copy and Save as PDF; Split and page images were
 jobs already.
   - **The writer is `write_built`,** not `full::write`, a retired name
 (slice 9's old writer); `ObjKey::Built(ObjectId)` (tag 20) names its
 bodies and is never used for encryption.
   - **`Fit` moved into sheaf-edit** (`images::PageFit` retired), the
 types ops carry living there (§6.1); `images::files_to_pdf` lost its
 only caller and went.
   - **`Tx::check` builds nothing:** a memory source's check stands the
 primary in for it; Blank's range is checked against `count`, and an
 images insert's `pages` are taken on trust (a TIFF's page count is
 known only once decoded), so a check of an undecodable image passes
 where applying it is `Invalid` (tested). The resolved op's `Src`
 holds the blob from resolve into the step, so there's no separate
 `Tx::pin`; the callers pin the image blobs.
   - **Images are accepted by their bytes** (`BlobKind::image(bytes)`), as
 a watermark's image is, since a blob keeps the kind it was first
 interned as.
   - **Adopted stamps hold their file's form:** without the cache an
 adopted Acrobat stamp would be `NotFound`, so `ForeignAnnot.stamp`
 names its `/AP /N` when its conversion is a page stamp and the
 held-stamp index lists it; `find_stamp` finds `FormRef::Base`, and
 nothing is interned.
   - **`LegacySel::Streams` and `Resources`,** not `Stream(StreamSel)`: a
 strip takes every marker, never a subset; `StreamSel` would need
 variants `DropStreams` never takes; resource names belong to the
 page; and the old flatten's `/SheafFlat<n>` stream has no marker to
 select it by. `private_filters(def)` is pure, rather than
 `strip_private(lw, page)`. Markers and lists the file holds are the
 filters'; the profile covers what the emitter writes anew, so Strip
 without the filters fails its post-condition.
   - **`sheaf-anon:` is a KEYS row** (the `/NM` sheaf gives a comment with
 none when a reply made in sheaf names it), which §15 doesn't list.
 The neutral `/NM` is a UUID-shaped xxh3-128 digest of the sheaf name.
   - **OCR and generator lists under Strip** are `/ShOCR BMC` with no
 list and `<< /Type /Pagination /Subtype … >>`, the header's layout
 digest not cached.
   - **The post-condition reads the Rewrite's graph,** not the bytes:
 plaintext, and what a Dense export writes. Deferred streams are
 empty there; the rest of their page is read (tested on a flattened
 page).
   - **Artifacts are `BMC`, not `BDC`:** no property list or artifact
 `/Type` fits a flattened annotation; `PlaceTag::Artifact` when the
 page's source has a `StructTreeRoot`. They need PDF 1.4, so
 `needs_after` walks pages for files below 1.4 (it walked below 1.2).
   - **Removed annotations reach `keep_only` through
 `Emission::removed_annots`,** which the emitter computes, so a
 delete's OBJRs in tagged files are pruned too.
   - **What flatten takes:** Hidden and NoView (and under Printable the
 unprinted) are neither drawn nor removed; annotations the records
 don't hold (printer's marks, media, widgets outside the form) stay,
 where the old flatten drew them; a frame with no area draws nothing
 (it drew at scale 1); an unreadable page is `EditError::Content`.
   - **Deferred bodies are always deflated,** their dictionary saying
 `/FlateDecode` before the data exists (lopdf's `compress` skips
 data that doesn't shrink). They are the codebase's first `Deferred`
 producers; the Rewrite, Append and realizer took them unchanged.
   - **`RemovedKind::Flattened`** is new ("page's flattened
 annotations"), and `PrivateData` replaced `OlderSettings`.
   - **The presets are constructors** carrying `from` and `out`; the
 app's ask dialog generalized to `Asking::{Save, Export}` with export
 wording; a split checks every part's asks before it writes any
 (`export::check`, `run`'s plan without the write, review), so one
 that asks writes nothing and runs again with the answer, and Remove
 Blanks checks its rejects file before it writes the kept one, as
 proposal 40 says (`sheaf split` exited 3 with parts written); Remove
 Blanks' rejects file is an export (Dense, outline
 dropped, labels kept), the document unchanged. A TIFF export still
 streams to `.tif.part` and renames (7,000 pages in memory could take
 gigabytes); page images and split parts give the export a
 zero-weight child progress.
   - **Transform caps** (the caps table has no row): `KeepPages` is
 Plugin (a delete's), `RemoveMarkup` and `RemovePrivateData`
 Approval; `Transform` is `non_exhaustive`, so a later one needs User
 until it has a row. Transform JSON is externally tagged.
   - **A scan inserts one `Memory(Images)` blob** made by `images::to_pdf`
 (`scan::made_pages`), not an image blob a page: 18.0 ms made once
 against 23.2 ms written to PNG and read back (a 2550×3300 bitonal
 page, release, median of 5), and nothing held twice. Images to PDF
 takes the same path, since `--dpi` overrides the images' resolution
 and no op field says that.
   - **The summary** is a Session (`new_empty` and an insert of a
 `Memory(Summary)` blob), its `/Info` the empty primary's; its Pages
 layout still builds an eager graph through `ResObjects` and
 `stamp_lib::page_forms` (which trims a clone of an eager document),
 written by `write_built`. So the to_lopdf.rs and Objects-seam rows'
 slice-19 parts are deleted in parts (the ledger says what remains,
 for slice 20).
   - **Session's API beyond §14.2's sketch:** `From<Document>`, `hold`,
 `tx(actor)` with the names granted, `commit_as`, `export_from`
 (`from()` clashed with `From::from`), `view`; `export` returns
 `Exported` (no `ExportReport`). `Session::edit` and `preview(&
 Envelope)` take envelopes and alone set `ops_from_file`; Combine,
 scan, Images to PDF and the summary commit by id. There's no
 `Preview.consequences`: the dry run's are computed in sheaf-ops
 (`effects::judge` and `sig_impact`), as putting `judge` in every
 `Tx::preview` would cost the app's dialogs at 7k pages.
   - **`Tx::ops_from_file()`,** not `from_file()`: clippy's
 `wrong_self_convention` fails a `from_*` method taking self.
   - **Output shapes and CLI JSON:** `created` is `{"ops": […], "locals":
 {…}}`, `version` the label or null, `strategy` "append", "rewrite"
 or null; commit notices and save notes go to stderr as `note:`
 lines. Every CLI JSON is printed with sorted keys (`print_json`):
 GPUI turns on serde_json's `preserve_order`, so the app's `sheaf
 edit` and `sheaf-cli` printed keys in different orders and the
 goldens failed under `cargo test --workspace`.
   - **Progress off a terminal prints plain lines** (`editing…`, a line a
 message) where indicatif printed nothing, for every subcommand: the
 130 golden needs a first line, and warnings were lost on a pipe.
 The Ctrl-C handler cancels before it prints "stopping…".
   - **The 130 golden holds its edits on standard input:** SIGINT after
 `editing…`, then the edits written, so it doesn't race a fast edit
 (20,000 blank pages finished first under the workspace run).
   - **`--with NAME=PATH[:PASSWORD_ENV]`:** a value naming an existing
 file is taken whole, so `C:\a.pdf` stays a path. `--dry-run`
 conflicts with `-o`, `--in-place` and `--force`; `--append` needs
 `-o`; `--schema` is exclusive; `inspect` needs `--json`.
   - **`--force` audit:** comments csv, text and data export gained
 `--force` (they overwrote silently); every other subcommand already
 replaced only with it.
   - **Clipboard:** a Base or Pinned stamp becomes a one-page PDF blob at
 copy, not paste, so the cache never holds a file's `Src`; `by_stem`,
 since `stems` is a retired bare name.
   - **Every way out:** Save As both ways uses `How::save` (the Session's
 `SaveHow::Append` correctly refuses this document: the Bates header
 drops `/Perms /UR3`); the Full sidecar is forced unsound; Combine is
 the file and a blank page (a self-combine's renamed copy repeats the
 file's own findings under new names); the form check runs last, so
 each behavior's fault trips its own assertion. `export_round_trips`
 asserts its one known failure, as the watermark tier asserts 0112530.
   - **Renamed tests:** `a_stamp_resolves_from_the_store_a_source_or_the_
 cache` is `…_from_the_store_or_a_source`, and `stamps_and_
 attachments_survive_an_empty_cache(_appended)` is `stamps_and_
 attachments_are_read_from_the_file(_appended)`; earlier notes name
 the old ones.
   - **Flattened annotations draw last** (review): §11.4's `[… QClose
 static-over… gen-over…]` drew a flatten's block under the
 generators over the page, so a flattened page put its annotations
 under an Over watermark and the header, where every viewer draws
 annotations over all of a page's content (ISO 32000-1 12.5) and the
 old flatten drew them last. Recognized text keeps the static-over
 slot; Flat blocks, the file's and new ones, go after the generators:
 `[gen-under… QOpen base′… QClose ocr… gen-over… flat…]`, and
 `content::intact` reads the trailing Flat blocks off first
 (`flats_last`), so a page opened again is written as it is and
 nesting doesn't grow as it's flattened again.
   - **The comment exports refuse the open file** (review): §12.1's gate
 is `export::not_its_file`, which `preflight` asks, and so do
 sheaf-ops' `comments::summary_to` and `comments::write_to`, which the
 Comments panel's exports and `sheaf comments summary|csv|text|export`
 write through (they replaced the open PDF given its path and
 `--force` or the OS's overwrite prompt).
   - **Blank pages stop at the page tree's limit** (review):
 `PageSource::Blank`'s count is refused past
 `Limits::DEFAULT.page_tree.pages` (0xFFFFF − 1), what `Source::open`
 reads back, in the shared resolve, so `Tx::check` and apply refuse
 it before any is made (`sheaf edit --dry-run` of 2,000,000 took 24.6
 s and 4.8 GB, debug, before failing; now 0.10 s and 18 MB). An
 insert's total with the document's pages isn't
 checked: a replacement adds none.
   - **The private-data read fails closed a stream at a time** (review):
 `private::found` reads a page whose content can't be read whole one
 stream at a time, so one stream that can't be decoded no longer
 hides an older sheaf's list in another; a stream that can't be
 decoded counts when its bytes say `/Sheaf`, and one past the content
 limit counts. lopdf hands back what of a broken deflated stream
 inflates, so only other filters and the limit reach that path.
   - **The print preview keeps a page it can't make** (review):
 `PrintContext.printed` holds `Option<DocState>`, so a page whose
 flatten fails is lowered and logged once; the lowering stays on the
 UI thread (one page's flatten on an `ExportTx`).

   Found while building:
   - **A radio group whose chosen button's page is removed** keeps its
 `/V`, which no button left shows, and `forms::check` reports
 `NoAppearance` (forms/safedocs/0070616.pdf `Gruppieren4`, and
 `StaleSigFlags` on the delete): `DeletePages`', so an extract's;
 `export_round_trips` asserts it as its one failure. Backlog B9.
   - **A stamp made from a page drops the page's `/Annots`** (Add from PDF
 Page…, `stamp_lib::one_page`), widgets included, and reads the
 file, not the unsaved state: against M10 proposal 7's "stamps made
 from a page show the field values, unsaved ones included". M10 slice
 12's (slice 20 carries it into the rewritten `m10.md`).
   - **Create from Scans still writes and reopens** (`open_paths`),
 though scan builds a Session; it could hand it over as Combine does.
 Backlog B11 (M5's scanner intake reworks this path).
   - **Exports in the app ran unjobbed** (no progress, no cancel); §12
 says exports run as jobs. Fixed in review (Deviations).
   - **Printing a page subset** evaluated live header tokens against the
 pages printed ("Page 1 of 2"), as §12 said and the old print did,
 while the print preview showed the document's numbering, so the two
 differed there. Fixed at the wrap-up, per Greg's ruling (2026-10-22:
 "for print it should say page 5 of 10. that's a bug. (correct
 behavior for extraction as you say)"): `KeepPages` says how the
 pages it keeps are numbered (`numbering: PageNumbering`, `AsKept` an
 extract's, a split part's and Remove Blanks' files, `AsDocument`
 print's). Kept `AsDocument`, the export's state holds the version it
 kept them from (`DocState::numbered_in`, never an edit's), and
 `generate::page_ctx` and the render key read a page's place, the
 count and its label there, so the printer, page images and the print
 preview's Save as PDF say "Page 5 of 10" with markup and without, the
 document's labels too (print keeps none), its Bates numbers as
 fixed. A version's own state has none: one `Option` checked a page.
 Tests, each failing with its read switched off:
 `a_print_numbers_its_pages_as_the_document_does_and_an_extract_as_it_has_them`
 (materialize/header_tests.rs: 10 pages, print and extract of pages 5
 and 6, of a document not saved and of one saved and opened again),
 `print_preview_of_pages_from_the_middle_draws_what_prints` (sheaf-ops
 realize.rs: each previewed page against the print file's, 0 px, with
 markup and without), and
 `a_page_kept_as_the_document_numbers_it_keys_as_in_the_document`
 (realize/key_tests.rs). Not fixed: a print file with markup keeps
 sheaf's private data, its header live in `/SheafGenerators`, so the
 print preview's Save as PDF, opened in sheaf again, numbers its
 header as that file has its pages ("Page 1 of 2", and so does a save
 of it) where other viewers show "Page 5 of 10"; without markup the
 header is plain content. Backlog B13.
   - **`save::validate` had no caller** in the workspace: deleted at the
 wrap-up, and retired (retired.rs, slice 19's rows).
   - **A tape's picture** (`annot::tape_pdf`) is still built with
 `ResObjects` and written by lopdf's `save_to`. Slice 20's (the
 `ObjectId` grep).
   - **No automated test covers `scan --ocr`** (tesseract may be missing
 in CI); checked by hand. Backlog F14.
   - **README's subcommand table had no `form` row:** added at the
 wrap-up (`form check`); its `edit` and `inspect` rows were there.
   - **A pinned page stamp wasn't the same in an Append and a Rewrite**
 (step 2's; fixed in step 8, which the release property
 `append_rewrite_reopen_alike_1024` found: patch a mark, duplicate a
 page, flatten, then an Append forced onto the file): a pinned
 appearance drew its stamp's page under a name made from the form's
 object number (`Res::written`), which a library stamp's form is given
 apart by the two strategies, so the reopened pages' resources
 differed by that name (`/ShTIUIYP` against `/ShSVPKPP`). `StampForm`
 carries the name it's drawn by (`res`, `StampForm::written` for a
 number's, as before), and `emit::annots`' `pinned` names the stamp's
 page by its `FormRef::digest`; every other appearance is written as
 before. The property's seed is kept (`crates/sheaf-doc/proptest-
 regressions/materialize/append_rewrite_tests.txt`, a real failure),
 so the default 32-case run replays it; it fails without the fix.
 Marks written outside a flatten still name a library stamp's page by
 its number, which differs by strategy but is never compared or seen.
   - **Step 4's size split test sat on a limit** (fixed in step 8): the
 extract of pages 1–7 is 11,998–12,008 bytes, its marks' random ids
 moving it, against a 12,000-byte limit, so the parts were [7, 5] in 2
 of 60 runs and [6, 6] otherwise (the workspace run caught it); and
 pages 8–10, 8,011–8,017 bytes, sat 11 bytes over 8,000. The limits
 are 7,500 and 12,500 now, each extract some hundreds of bytes from
 them (probed over 60 runs, then 60 runs of the test without a miss).

   What review found and fixed: the plan was reviewed before the steps
   started, and its findings are the deviations above, as resolved. Each
   step checked its new checks by switching their behavior off (Checked
   by tests). The slice's own review (22 findings, each checked against
   the code; none rejected):
   - **Fixed, with a test each:** flattened annotations drawn under an
 Over watermark and the header (Deviations); the comment summary and
 the comment data, CSV and text exports writing over the open file;
 the app's exports unjobbed; a split or Remove Blanks writing some
 files and then asking (exit 3 with parts on disk, against proposal
 40), now `export::check`ed first; print without markup and text
 export failing on an older sheaf's leftovers the filters can't
 reach (`Private::Scrub`); one undecodable content stream hiding
 another's private data from the Strip check; a blank-page count
 past the page tree's limit built before it failed; the print
 preview lowering and logging a failing page every frame; the
 cancelled-export test cancelling only before the export started
 (now at every check, 12 for an extract); the every-way-out test's
 print without markup unchecked; the pinned stamp's fix guarded by a
 seed alone (now a fixed case too, which fails without the fix); the
 docs examples counted loosely (now exactly five, and a mistyped
 marker fails).
   - **Fixed in place:** `private::sheaf_names` test-only; the stale link
 to `ask_before_saving`; the to_lopdf.rs ledger row's stale "19"
 entry; the docs on the app's exports (Split is a job of its own, Save
 as PDF is the print preset to `Out::File`); `flatten`'s `/AP /N`
 read in place, not cloned; `gate_7k`'s clean copy row (measured,
 no gate); doc paragraphs reflowed and `PathBuf` in jobs.rs' std
 imports.
   - **Confirmed:** the size split fixture's rows (3,000, 7,500, 12,500)
 are what main 3c72df2's split makes, run there in a tree of its own
 (ten runs alike: `[2,1,1,1,2,1,1,1,2]`, `[4,3,2,3]`, `[7,5]`; its
 extracts of pages 1–7 11,431–11,443 bytes, of 8–10 7,655–7,663), so
 the comment and the notes stand, reworded to say so.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, each test in a run of its own,
 each started once the 1-minute load was under 4 on two checks 30 s
 apart: load 1.1 to 3.2 at each start; 11 of 11 pass, and the flatten
 part five more times on each fixture):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | Export Flattened (`FlattenAnnotations { All }`), five processes of its own: time; peak RSS (the heavy's gate ≤ 221 MB, asserted only with `SHEAF_GATE_FLATTEN`); RSS added | 357–467 ms, median 467 ms; 88.3 MB (87.9–89.0); 50.6 MB (50.4–51.2) | 1.25–1.52 s, median 1.29 s; 514 MB (513–515); 293 MB (292–295) | 466–697 ms, median 521 ms; 1,076 MB (1,076–1,077); 553 MB (553–554) |
 | the open in that process: peak RSS; RSS added | 37.5–37.9 MB; 32.8 MB | 221 MB; 216 MB | 523 MB; 518 MB |
 | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 51.6 ms; 54.9 ms | 86.0 ms; 352 ms | 61.4 ms; 457 ms |
 | snapshot, `Tx::check` of a delete of every page (asserted < 50 µs, < 1 ms) | 0.250 µs, 0.415 ms | 0.281 µs, 0.416 ms | 0.240 µs, 0.421 ms |
 | realize + open + render 1,024 px, an edited page, a header and recognized text on it (asserted < 15 and < 25 ms) | 11.8 ms (slice 18: 12.0) | 21.2 ms (22.7) | 31.7 ms (32.2) |
 | engines after 220 groups: weighed, RSS added, open (asserted ≤ 96 MB, ≤ 96 MB, ≤ 6) | 28.9 MB, 41.6 MB, 6 | 28.9 MB, 65.3 MB, 6 | 29.0 MB, 62.2 MB, 6 |
 | thumbnail strip, crop + watermark + "Page n of N" header, 320 px (asserted ≥ 310 pages/s, ≤ 96 MB weighed and added) | 420 pages/s, 80.7 MB, 25.9 MB | 351 pages/s, 80.7 MB, 65.4 MB | 164 pages/s, 80.8 MB, 81.9 MB |
 | crop all pages, and validated after a delete (asserted < 200 ms) | 13.2, 15.1 ms | 9.97, 16.1 ms | 12.7, 14.8 ms |
 | Bates apply, all pages (asserted < 50 ms), and the fonts its Rewrite adds (asserted 1) | 0.616 ms, 1 | 0.644 ms, 1 | 0.692 ms, 1 |
 | census of a watermark and a PNG background (asserted 1, 1, 1, 0; page resources) | 1, 1, 1, 0; 1 of 1 | 1, 1, 1, 0; 70 of 70 | 1, 1, 1, 0; 7,000 of 7,000 |
 | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 9.81 s | 1, 7,000; 10.05 s | 1, 7,000; 10.13 s |
 | one-mark Append on the scanned (asserted < 50 ms) | | | 26.4 ms (slices 14–18: 19.3–30.3) |
 | "Page n of N", saved, every page moved: the Append (asserted ≤ 1 s), the section (asserted ≤ 8 MB) | 469 ms, 1.36 MB | 690 ms, 1.47 MB | 469 ms, 1.37 MB |
 | the same move on the file opened again: the Append (asserted ≤ 1 s on the scanned; accepted misses on the others), the section (asserted ≤ 8 MB) | 1,023 ms, 3.31 MB | 1,578 ms, 3.33 MB | 886 ms, 3.32 MB |
 | proposal 8's draws: set unsaved, a moved page, one deleted, one deleted on the file opened again (accepted misses on the synthetic and heavy) | 22.9, 19.4, 19.3, 29.7 ms | 36.0, 32.7, 33.7, 43.9 ms | 34.0, 32.7, 31.2, 40.6 ms |

 Proposal 8's accepted misses are where slices 17 and 18 left them
 (slice 18: 22.4, 20.5, 18.7 and 29.4 ms on the synthetic, 39.1, 34.9,
 32.2 and 42.6 ms on the heavy, the Appends on the file opened again
 888 ms and 1,555 ms; slice 17's runs 0.89–1.07 s and 1.43–1.57 s);
 this slice's paths don't reach them. The rest: history's 200 OCR
 versions of 1,000 pages at most 236 MB (≤ 256 MB, asserted; slice 18:
 236); AddMark 0.020 ms and a full reorder 0.969 ms at 20,000 marks (<
 1 and < 2 ms); 7,000 `SetOcrText` in one transaction lowered in 48.2
 ms, the first 67.1 ms with every blank page's blocks read cold,
 committed in 13.5 ms, an OCR run of 700 flushes 107 ms; 1,000 mark
 adds on the heavy 0.033 ms (blank 0.021 ms); ready on 20,000 sheaf
 marks 513 ms (< 1 s); their Rewrite 1,639 ms (slice 18: 1,523; no
 gate), through `save::rewrite`, which is `export::run` now; Trim
 White 40.5, 36.0 and 93.1 s (no gate; slice 18: 46.4, 40.2 and 95.4
 s); Find Blank Pages over the synthetic 12.2 s, and Find with a
 header on every page past six pinned groups 12.2 and 9.2 s (no gate).
 With `SHEAF_GATE_FLATTEN` set the heavy's flatten fails ("peaked at
 513.8 MB, over 1.5× the file's 13.7 MB + 200 MB (220.5 MB)"), and the
 others pass, as they assert nothing.
   - **After the review's fixes** (release, each test started once the
 1-minute load was under 4 on two checks 30 s apart, load 1.3 to
 2.2): the three fixtures' gates, the Rewrite against slice 8's
 writer and history's 200 OCR versions pass; the emitter's order
 change touches only pages with flattened layers, and the corpus
 counts are unchanged. The scanned's reopened Append tripped once
 at 1,067 ms and passed on the reruns (940, 887 and 934 ms), as it
 sits near its gate. The Rewrite, five processes each with the gates'
 own part: synthetic 147–158 ms, median 149 ms (1.11×), heavy 753–809
 ms, median 785 ms (1.11×), under the 1.2× flag. Export Flattened:
 284 ms and 89.1 MB peak synthetic, 1,253 ms and 513 MB heavy (the
 open 221 MB), as before. The new clean copy row (no baseline, no
 gate): synthetic 319 ms, 93.2 MB peak, 55.6 MB added, the
 private-data read alone over the copy 121 ms; heavy 1,103 ms, 474 MB,
 253 MB, the read 225 ms. That read is of what the copy writes, every
 page's content once a page (pages sharing streams read them again);
 not memoized, as at 225 ms of 1.1 s it doesn't show. History's 200
 OCR versions 236 MB (≤ 256 MB). The release properties pass (Append
 ≡ Rewrite at 1,024 cases 61.4 s, the replay at 4,096), no seed
 added; the interop tier 19 of 19 with qpdf and pdf.js; the export
 tier on six of its seven files, its one known failure alone; `cargo test
 --workspace` 1,654 passed, 0 failed, 56 ignored (+10, the review's
 tests); clippy (`-D warnings`, all targets) on Linux, with the
 Linux-only gates off, and for `x86_64-pc-windows-msvc` (sheaf-doc,
 sheaf-ops, sheaf-render, sheaf-print with all targets, sheaf-cli
 without its OCR engines), and fmt, clean.
   - **The Rewrite against slice 8's writer**
 (`rewrites_on_the_synthetic_and_heavy_7k` five times, each fixture
 in a process of its own, against slice 8's 134 and 707 ms):
 synthetic 145–150 ms, median 146 ms, 1.09× (1.08–1.12×); heavy
 749–793 ms, median 768 ms, 1.09× (1.06–1.12×); none past the 1.2×
 flag (slice 18: 1.11× and 1.10×). The gates' own Rewrite parts: 152
 ms (1.14×) and 770 ms (1.09×). RSS added 50.1–51.6 and 252–253 MB (≤
 1.5× slice 8's 61.7 and 251 MB).
   - **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
 no corpus" line), every count as #26 left it, after step 8's fix as
 before it: materialize_corpus's identity 4,389 of 4,392 written,
 2,269,688 primary objects surviving, 919,465 copied as they were, 432
 changed by `keep_only`, 0 failed; 2,901 forms without their first
 page, 0 failed; every page duplicated, 4,389, 0 failed; inserted
 twice into the 1040, 1,098, 0 failed; four times, 0 differ; the logo,
 0 of 2,037,600 bytes differ. append_corpus: 4,389 marked, 3,770
 appended, 259 rewritten (211 RemovalCheck, 34 BaseObjectsMissing, 14
 BaseUnsound), 360 asked, 0 failed, 0 derived vetoes and 211 on the
 files' own objects; the oracle finds text gone from 2,899 of 2,901
 (none from 0016064 and 0112654). encrypted_corpus: the Rewrite keeps
 573 of 573 and the Append 572 (0630899 declined). The new
 `export_round_trips` tier (`SHEAF_EXPORT_ONLY`) on seven files (the
 1040, 0025448, 0070616, 0187215 signed and PDF/A, 0442193, 0630899,
 0861328): 7 of 7 extracted, clean-copied, flattened and opened again,
 the one failure its allowed one (0070616's extract,
 `NoAppearance(["Gruppieren4"])`); step 7's run over every form: 4,389
 of 4,392 files, the same one failure, 56 s. The full ignored tiers
 (deleted means deleted, realize_corpus, the watermark, header, OCR
 and export tiers) are the pre-merge run's (Greg, 2026-10-20).
   - **Suites:** `cargo test --workspace` 1,644 passed, 0 failed and 56
 ignored (#26: 1,581 and 51; ignored +5: the export tier, the pdf.js
 tests of flattened pages, clean copies and exports). Its first run
 failed step 4's size split test once (Found);
 `nothing_retired_is_back` caught a wrong retired row of step 8's,
 taken out. The release properties: Append ≡ Rewrite at 1,024 cases
 failed once on step 2's pinned page stamp (Found, fixed), then passed
 twice (61.2 s, 58.7 s), and the replay property at 4,096 passes
 (14.8 s). The interop tier (qpdf; pdf.js 6.4.299 on node 24.22): 19
 pass with `--include-ignored` (#26: 15; the flattened pair, the clean
 copy and the exports), `qpdf --check` clean on every output (84, 14
 and the rest), and on two signed corpus files appended by `sheaf edit
 -o --append` (signatures/safedocs/0000020.pdf and
 pyhanko/pdf/minimal-two-fields-signed-twice.pdf, each the file's
 bytes as a prefix). fmt and clippy (`-D warnings`, all targets)
 clean, and clippy with sheaf-app's Linux-only gates switched off
 (`nonlinux-clippy.sh`); clippy (`-D warnings`) on
 `x86_64-pc-windows-msvc` passes for sheaf-edit, sheaf-doc, sheaf-util
 and sheaf-image with their tests, for sheaf-render, sheaf-print,
 sheaf-ops and sheaf-ocr with all targets, and for sheaf-cli with all
 targets without its OCR engines (`--no-default-features`: oar-ocr's
 tokenizers build oniguruma with a C compiler); sheaf-plugin and
 sheaf-app can't be checked for Windows here, as before.

   Checked by tests:
   - **Memory sources:** `tx::tests::blank_and_image_pages_are_memory_
 sources` (origin, `write_built` bytes, the same blob again, a check
 making and reading nothing), `blank_pages_replace_pages`,
 `memory::tests::blank_pages_are_the_same_bytes_each_time`,
 `memory::a_file_of_no_pages_is_sheaf_s_own`, the history replay
 drawing Blank and Images sources, `model::tests::a_stamp_resolves_
 from_the_store_or_a_source` (an adopted Acrobat stamp), sheaf-ops'
 `blank::inserted_blank_and_image_pages_write_nothing_else` (in
 `own_dirs`), `combine::tests::images_are_pages_made_in_memory`, the
 app's `a_made_page_lands_lowered_again`.
   - **Review's:** flatten_tests' `a_flattened_layer_draws_over_the_
 watermark` (both ways: `[… QClose Ocr Gen Flat]`, reopened intact,
 flattened again `[… Gen Flat Flat]`) and sheaf-render's
 `flattened_annotations_draw_over_the_watermark` (an opaque Over
 watermark: flattened, exported flattened and printed with markup,
 the annotated page's pixels); exports.rs' own-file block with the
 summary and an XFDF, and print without markup drawing nothing and
 holding no private data; the app's `exports_run_as_jobs_one_at_a_
 time` and `a_print_preview_that_cant_be_made_is_tried_once`; the
 CLI's `a_split_that_asks_writes_no_part` and sheaf-ops blank's
 `a_removal_that_asks_writes_neither_file`; private.rs'
 `a_stream_that_cant_be_decoded_fails_closed` and private_tests'
 `private_data_a_page_s_content_hides_fails_a_clean_copy_alone`
 (Strip refuses, Scrub notes); tx's blank count past the page tree
 (check and apply `Invalid`, nothing interned); export's
 `a_cancelled_export_writes_nothing` at every check; append_rewrite's
 `a_flattened_library_stamp_draws_alike`; tests/edit.rs' five
 examples. Each fault-checked by switching its fix off (the emitter's
 old order, `flats_last` at 0, the summary's gate, the split's and
 Remove Blanks' checks, the per-stream read, Strip for print, the old
 blank limit, the stamp's number name): each caught, each file
 restored from its own copy and checked with `cmp`, no seed kept.
   - **Flatten:** materialize/flatten_tests.rs (12: Print, Hidden, NoView
 and links; no widget fallback; a comment's first state; fields gone;
 undo and redo; cancel; the layer after the content, tagged and
 untagged, OBJRs pruned; reopened in its slot and rewritten as it is;
 a second flatten at the same depth; deterministic bytes; an
 unreadable page refused; an Append over a saved Flat block vetoing
 nothing; a pinned form's `to_pdf`), effects' `flattened_layers`,
 key_tests' `flattening_changes_a_page_s_key_under_every_input`, the
 caps totality (26 kinds), sheaf-render's tests/flatten.rs (pixels
 within 2 a channel), interop's `a_flattened_page_reads_in_pdfjs`.
   - **Private data:** private.rs' unit tests; materialize/
 private_tests.rs (`no_sheaf_name_but_the_private_keys_is_written`
 both ways over marks, replies, a replacement, a dynamic library
 stamp, a connector, a watermark, Bates, OCR, a flatten and a form;
 `a_file_without_private_data_holds_none`;
 `a_file_holding_private_data_is_not_written_without_it`;
 `a_flattened_page_s_own_content_is_read`); `renames`,
 `Reserved::rename` and the Strip lists' unit tests; sheaf-ops
 tests/private.rs (an older header, a legacy OCR layer, OCR blocks,
 a named list, `/SheafFlat1`: no KEYS left, the same pixels and
 characters); interop's `a_clean_copy_reads_in_pdfjs`.
   - **Exports:** export/tests.rs (the ported `clean_copy_strips_markup_
 only` with the existing-file refusal, `extract_pages_keeps_marks_
 and_relevant_bookmarks`, `print_flattens_or_drops_markup`,
 `flattened_copy_burns_marks_into_content` with a widget,
 `extract_flatten_and_clean_keep_layer`; new `kept_pages_keep_their_
 bookmarks_as_the_outline_policy_says`, `a_cancelled_export_writes_
 nothing`, `an_export_asks_only_for_the_signatures_it_exports`,
 `a_transform_needs_its_capability`); the ports across tx/header_
 tests, materialize's header, recognized, others, annot_form, kept,
 saving and serialize tests and sheaf-ops' interop, forms_saves,
 forms_pages, comments, realize, watermark, private, progress, blank
 and realize_corpus; realize.rs' `print_preview_draws_what_prints` (0
 px both ways); interop's `the_exports_read_in_pdfjs`; the MaxBytes
 parts recorded before the port (`[2,2,2,1]` on the CLI, a sheaf-ops
 fixture); sheaf-edit's `transforms_round_trip_and_name_themselves`.
   - **Session:** `model::history_before_the_cursor_is_forgotten`,
 `session::a_document_made_in_memory_takes_pages_and_saves`,
 `session::a_stop_at_an_ask_needs_a_decision`, `combine::the_first_
 file_s_header_covers_the_pages_combined_after_it`, forms_saves'
 `a_session_saves_a_signed_file_as_its_flags_say` and
 `a_session_appends_only_what_it_can`, comments'
 `importing_the_same_xfdf_twice_updates_it` (the port), the CLI's
 `a_signed_file_asks_before_its_signature_breaks`, forms_merge's 1040
 (one version, nothing to undo, the notices, scripts On).
   - **`sheaf edit`:** `tx::tests::ids_mean_nothing_in_ops_read_from_a_
 file`; the 18 goldens; tests/edit.rs' `docs_json_examples_run`,
 `a_signed_file_appended_keeps_its_signatures` (six files of
 `signatures/`, both subfilters, two certified with P=3),
 `the_command_line_writes_nothing_but_its_outputs`,
 `the_schema_is_the_committed_one`, `edits_come_from_standard_input`;
 the forms corpus reader's tests for `signatures()`.
   - **The app:** `docview::tests::a_combined_document_opens_in_its_
 window_as_made` (a fixture always, the corpus 1040 + Schedule 1 +
 Schedule B when there), `markup_tools::tests::a_library_stamp_
 pastes_into_another_window`, `copied_marks_take_what_they_hold`.
   - **Every way out:** `every_way_out_keeps_the_form_and_its_rules`
 (16 ways), and the `export_round_trips` tier.
   - **Fault checks** (each restored from the step's own copy and
 checked with `cmp`; no fault run's proptest seed kept; the one kept
 is a real failure, Found): step 1's adoption without the held-index
 entry (its test fails); step 7's Hidden skip removed, the widget
 first-state fallback on, Printable as All, the OwnFile gate
 removed, the clean copy without RemovePrivateData, DropXfa off,
 Bates drawn by position, a `temp_dir` write in `export::run`, the
 clipboard not carrying Blob stamps, and a reopen in `adopting`
 (each caught by its own assertion); step 6's goldens with an exit
 code and an output altered (both caught); step 8's six bans, each
 called from sheaf-doc's library code (all six flagged by clippy),
 its retired row `page_insert::DocView::insert_made` (a probe
 defining it there is hit), `SHEAF_GATE_FLATTEN` set on the heavy's
 flatten part (it fails at 513.8 MB) and on the synthetic's (it
 passes, asserting nothing), and the pinned stamp's name (the
 property's seed fails without it).

   For Greg:
   - **The flattened export's RSS gate is unattainable by construction**
 (`FlattenAnnotations { All }` on the heavy 7k at a peak of at most
 1.5× the file + 200 MB, 220.5 MB, in the flatten's own process). Five
 runs, each in a process of its own on a quiet machine: opening the
 heavy alone peaks at 221 MB (216 MB added, all still held when the
 export starts), so any flatten in that process starts at the limit;
 the export then peaks at 513–515 MB (median 514 MB; 292–295 MB added,
 median 293). Most of that is the Rewrite's: a plain Rewrite of the
 heavy adds 252 MB (the gates' own Rewrite part), the primary loaded
 eagerly to copy it (`materialize::graph::eager`), which no slice of
 M10.5 makes lazy. It takes 1.25–1.52 s (median 1.29 s; slice 2's
 flatten, which kept the 7,000 widgets, 3.49 s and 420 MB). What the
 export adds misses too (293 MB against 220.5). The row is measured on
 every run with the open's peak printed beside it, and asserted only
 with `SHEAF_GATE_FLATTEN` (a ledger row, as proposal 8's misses
 are). Meeting it needs a Rewrite that copies the primary's objects
 without loading the file whole (a streaming copy, its pruning and
 `keep_only` included) and a gate on what the export adds rather
 than the process's peak; your ruling: keep the gate and plan that
 work, or restate it.
   - **B5 reaches the exports' and the command line's signature ask:**
 an export asks about every signature in what it exports whenever
 the opened file has one, judged by `/V` and permissions
 (`sig_impact`), not by whether it validates, so a signature already
 invalid when opened still asks. In the corpus,
 signatures/safedocs/0008830.pdf and 0010094.pdf, whose signatures
 pyHanko finds neither intact nor valid, exit 3 on a mark add (37
 unencrypted files have such a signature), and a file written with
 `--signatures break` still asks on its next edit (`append_anyway:
 true`, "… removed since the last save"). Not fixed here (B5).
   - **The Op schema grew in place at 1.0** (`PageSource::{Blank,
 Images}`, `Fit`, `FlattenAnnotations`): nothing was persisted
 before `sheaf edit`, which persists from now on, so a later addition
 bumps the minor.
   - **The neutral `/NM` is an unkeyed digest** of the sheaf name:
 anyone holding both files can match a clean copy's marks to the
 source's ids, which are random and say nothing else.
   - **`new_empty`'s primary is a file of no pages** (`Memory(Blank)`,
 the only origin `Source::open` accepts with none); a scan, Images to
 PDF and the summary insert into it. The summary's `/Info` (Title,
 Producer, CreationDate) is gone with it: no op sets document info.
   - **Combine keeps the first file's generators,** so its All-scope
 header covers the pages combined after it (pinned by a test); it
 fills the origin window when that's empty; the adopted window keeps
 the first file's form facts as opened, and `xfa_values_dropped`
 isn't set from Combine's `PRINT_ANSWERS`, so a hybrid first file
 whose XFA held values could still show the "XFA part holds values"
 bar after the combine dropped it.
   - **Print answers its own asks** (`PRINT_ANSWERS`: signatures broken,
 XFA dropped) for print, page images and Save as PDF; print with
 markup flattens widgets by their `/AS`, so a check box with states
 and no `/AS` prints nothing (M10 bug 10), where print kept widgets
 live before.
   - **A non-widget with states and no `/AS`:** PDFium shows nothing (it
 falls back to `/V`, then `/Off`), but flatten keeps the plan's
 first-state fallback for non-widgets, as the old flatten did, so
 flattening such a comment draws what no viewer showed.
   - **The command line's `-o` on a signed file asks** (exit 3) where it
 copy-appended; `--append` copy-appends any file on request, and
 `--signatures break` rewrites.
   - **A clean copy fails closed** ("it would hold sheaf's private data:
 …") where sheaf's private data sits on a page whose content can't be
 read, or in another tool's appearance stream (a stale old stamp's
 `/SheafStamp` form name); the corpus sweep found none. Print without
 markup, and with it page images, text and Save as PDF without markup,
 writes such a file with a note instead (`Private::Scrub`, review), so
 a print file may hold what an older sheaf left where sheaf can't
 read; your call whether Save as PDF, a file someone keeps, should
 fail closed as a clean copy does.
   - **Flattened annotations draw over the watermark and header**, after
 the generators, not in §11.4's static-over slot (review, Deviations):
 the plan's order changed how a watermarked page looked once
 flattened.
   - **What 19 left of the to_lopdf.rs and Objects-seam rows** (the
 summary's eager pages, exchange's eager `Comments.pdf`, `WriteCx`
 and `ResObjects` for PDFs built in memory, the tape's picture) is
 put on slice 20, whose `ObjectId` grep reaches them; your call.
   - **Wording, yours:** "an export can't replace the open file; choose
 another name"; "Export (breaks N signatures)" and "Export Without
 It"; "The result can't replace a file open in sheaf; close it
 first"; "the save can't append: …"; "a decision is needed: …";
 "page 17 is named by id, and session ids aren't meaningful in a
 file: name it {"pos": n}"; `RemovedKind::Flattened`'s "page's
 flattened annotations" and `PrivateData`'s "page's private data from
 sheaf".

   Left for later slices:
   - **Slice 20:** WIT 0.4's clipboard and assets (`assets.upload`), and
 plugins' flatten and exports (`FlattenAnnotations` and the
 transforms in 0.4, under its caps); what 19 left of the to_lopdf.rs
 and Objects-seam rows; the final docs pass.
   - **M10 slice 15:** `FlattenFields`, which owns a widget's pinned
 appearance (`MarkAp` for widgets: value, display, state, `/DA`, `/Q`,
 `/DR`).
   - **M11:** per-export security (encryption chosen per export,
 proposal 10's), and the redaction resolve stage.
   - **Greg:** the flatten gate (For Greg); option A for proposal 8's
 misses, as before.
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed),
 untouched: no step changed that walk. The Found items above.

   Earlier slices' "Slice 19" bullets, as met:
   - **Slice 2's and 13's** (assert the flatten's peak ≤ 1.5× file + 200
 MB in its own process; switch the row to `FlattenAnnotations { All
 }`; 490 MB at slice 13):
 switched; measured with the open's peak beside it, and asserted only
 with `SHEAF_GATE_FLATTEN`, unattainable by construction (For Greg,
 a ledger row).
   - **Slice 3's and 4's** (blank pages, images and Combine's image PDFs
 as memory sources; `Session::open` over `open::file`, and Combine's
 first file): memory sources; `Session::open` over `open::read`;
 Combine's first file a full open with its outline and labels taken
 off in the transaction.
   - **Slices 6, 8 and 11's** (the generated cache, the stamps' cache
 fallback and the panel's and attach.rs' cache writes, the stem arm;
 the content-round-trip bans): the cache is gone, `find_stamp` has no
 cache arm, adopted stamps hold their file's form, and the bans
 landed. attach.rs' cache write went in slice 8.
   - **Slice 7's** (export presets and profiles; print, page images,
 comment exports and the summary through the serializer and durable
 writes; Images to PDF, scans, image and blank pages and the stamp
 library off lopdf's writer): done, but for a TIFF's `.tif.part`
 stream and a tape's picture (Found).
   - **Slices 9 and 10's** (`Session::save` for `materialize::save`;
 exports through `export::run`; `-o --append` and `--signatures`):
 done, the ledger rows deleted.
   - **Slice 9's helpers** (`strip_markup_annots`, `is_link_or_field`):
 deleted with the old exports.
   - **Slices 11 and 12's** (`Session`, `sheaf edit` as the envelope's
 first producer, `FlattenAnnotations`, `PageSource::{Blank,
 Images}`, `Transform` and `ExportTx`, Combine without its write
 and reopen): done; the schema grew in place (Deviations).
   - **Slice 14's** (an `ExportTx` for print and text export; the
 `temp_dir` ban): print is the Print preset; there's no text export
 to move (Deviations); the ban landed.
   - **Slice 15's** (Op JSON regenerated in place until `sheaf edit`
 persists it): `sheaf edit` persists it from this slice.
   - **Slice 16's** (`LegacySel::Stream`, named lists' settings in
 `/Properties`, renaming `/SheafWM*`): `LegacySel::Streams` and
 `Resources` (Deviations).
   - **Slice 17's** (RemovePrivateData of `/SheafHeaderFooter`,
 `/SheafHF` and `/SheafStamp` in old files): KEYS rows; the catalog's
 key owned and never written, `/SheafHF` renamed, `/SheafStamp` out of
 lists and its stamp written anew.
   - **Slice 18's** (RemovePrivateData of `/SheafOcr` and
 `/SheafOcrLayer`, `/SheafOcrBlock` under Strip; `BlockKind` with
 `Flattened` and `Static::Placements`): done.
