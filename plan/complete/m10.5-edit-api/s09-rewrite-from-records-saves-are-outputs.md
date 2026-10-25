# M10.5 slice 9: Rewrite from records; saves are outputs

Status: done · Part of [M10.5: the edit API](README.md)

**Rewrite from records; saves are outputs** (XL; proposals 15, 23,
24 and 37; bugs 1, 3, 14, 15, 22, 23 and 29).
- **`materialize/`:**
  - `plan` (Rewrite only, for now);
  - the emitters (`pages`, `content`, `annots`, `form`, `outline`,
    `catalog`), `copy`, `graph` and `verify`.
- **Numbering and copying:**
  - stable `ObjKey` numbering;
  - raw passthrough under its five conditions;
  - ObjStm members re-serialized as plain objects;
  - Dense numbering for exports;
  - a balanced tree.
- **Emission rules:**
  - inheritable keys written when a page is reparented;
  - the `/StructParents` rule;
  - merged field and widget pairs as one object;
  - the root ownership table.
- **`keep_only` unchanged,** returning the ids it modified. Templates
  are kept.
- **`rules::plan`,** with the new signature.
- **Post-conditions:** reachability with templates, `forms::check`,
  copy-on-write and encryption.
- **Saves become outputs:**
  - a `SaveRecord` synthesized at open and produced by every save;
  - `save_input` seals, and `record_save`/`abandon_save` unseal;
  - Save As is a Save;
  - **every save is a Rewrite,** for this slice only (slice 10 brings
    Append back). The tests that need an Append, meaning the Append
    halves of the both-strategies suites and M10 slice 5's
    signed-Append tests, are `#[ignore = "slice 10"]`;
  - recovery v2 in **Full** form, encrypted, with transforms applied,
    and the sidecar lifecycle;
  - signed files ask Rewrite or Cancel.
- **Transitional:**
  - derived pages emitted as the original object with derived
    content, re-implemented in the emitter from the registration's
    `derived` record (save.rs's `origins`, `needed` and
    `copy_derived_page` aren't called);
  - the header/footer generator drawn on Rewrite;
  - OCR layers emitted in the legacy shape through
    `text_layer::install` (`/SheafOcrLayer`, `/SheafOcr`), which also
    strips a layer the base already holds. From slice 10, the old
    layer's streams are then unlinked, so the removal check turns a
    re-OCR over a saved layer into a Rewrite; that replaces
    incremental.rs's replaced-layer bails;
  - exports built by `subset` but written by this writer, with
    `rules::plan` and the `flatten_annots` post-pass.
- **Deletes:**
  - the old rewrite path: `build_rewrite`, `rewrite_with`, `finish`,
    the `renumber_objects` use;
  - the old annotation and outline writers: `write_outline`,
    `write_page_annots`, `strip_sheaf_annots`, `link_connectors`
    (moved);
  - page and source plumbing: `LoadedSource`/`load_source` (moved
    into `graph`), `is_identity`, `reserve_pages`,
    `reserve_page_objects`, `copy_of`, and
    `origin`/`origins`/`needed`/`copy_derived_page`, whose only
    callers were the two old writers;
  - form rules: `rules::take_out` and forms/merge.rs's copy maps;
  - incremental.rs entirely, with its replaced-layer bails
    (incremental.rs:268, :322);
  - `rebase_to_saved` and its adaptation to records;
  - the legacy projection and its types;
  - the dirty tracking: `Document.baseline` (slice 8's one baseline
    state, which replaced `saved_outline`/`labels`/`annots`/`fields`/
    `removed` and `baked_header_footer`), `removes_content`,
    `annots_dirty_pages`, `fields_dirty`, `header_footer_dirty`,
    `form_changed`, and the `Snapshot` dirty flags and sources map;
  - `save_to_path_noting` (its length-only disk check went in slice
    7), `Signed`, and `WouldBreakSignatures` (now `Ask::Signatures`);
  - docview's `Signed::Append` offer path (docview.rs:1635);
  - `/SheafPart` on written companions: a replacement's StrikeOut is
    recognized by `/IRT` and `/RT /Group` to a sheaf Caret (feature
    36), and `/SheafPart` is only read;
  - the app's reopen, re-hide and read of the saved file after a
    save (what's left of it reads the saved file's annotations and
    form on the save thread for `rebase_to_saved`; its re-listing of
    other tools' comments, `reread_foreign`, went in slice 8);
  - recovery v1, its temp primary and its leaked temp file;
  - the `geom_of`/`page_geom` wrappers.
- **Checks:**
  - **Ports and identity:**
    - every save.rs rewrite test is ported;
    - after a corpus Rewrite, every surviving primary object keeps
      its number, and untouched ones are byte-identical;
    - the same input twice, and from two threads, gives
      byte-identical output.
  - **Outline, ownership and forms:**
    - outline `/A` URI actions, `/SE` and closed items survive a
      reorder plus save;
    - a duplicate moved before its original leaves copy 0 with the
      original object;
    - `forms::check` is clean on every corpus form after a
      page-delete Rewrite;
    - the M9 bug 14 reproductions pass.
  - **Emission rules:**
    - duplicating a page with a merged single-widget field gives one
      object per copy, with forms clean;
    - a page from a tagged file inserted into a tagged file has no
      `/StructParents`;
    - with an intermediate node carrying `/Rotate 90` and a
      `/CropBox`, a foreign page inserted next to it and a duplicate
      both keep their PDFium size and rotation;
    - owned keys survive set → save → undo → save → reopen;
    - `page_templates_stay` passes through the post-conditions;
    - a replace-text mark saves with no `/SheafPart`, and reopens as
      one mark whose StrikeOut is its companion; an older file's
      `/SheafPart` companions still open that way.
  - **Saves during edits:**
    - an edit and an OCR flush made during a slow save (a test hook)
      each land in a child version, stay dirty, and go into the next
      save;
    - a page delete on a hybrid form gives a sidecar with no XFA, and
      the export of a hybrid form that leaves out widget pages drops
      XFA.
  - **Tiles** aren't dropped by a save (an app counter).
  - **Gate:** a Rewrite at 7k takes at most 1.3× today's writer's
    time (flagged past 1.2×), with RSS at most 1.5×, on the synthetic
    and heavy fixtures.

## Notes

Done. A save is an output (proposals 15, 23, 24 and 37; bugs 1, 3,
14, 15, 22, 23 and 29): `save_input` seals the version, `materialize::
plan` plans the save, `materialize` emits the state's records by
`ObjKey` over the primary, numbers them stably (from 1 for exports),
prunes with `keep_only`, checks the post-conditions and writes the
file through `serialize::full`, and `record_save` keeps the written
file's `SaveRecord`. No version, state or source changes, and nothing
is reopened, re-hidden or read back after a save. Every save is a
Rewrite until slice 10, Save As is a save, a signed file asks Rewrite
or Cancel, and recovery is format 2, Full, encrypted as the file is,
with save policy's transforms applied. The old writers (save.rs's
rewrite path and its outline, annotation and page plumbing,
incremental.rs, forms/merge.rs and `rules::take_out`), their entry
point (`save_to_path_noting`, `Signed`), the legacy projection, the
baseline and every dirty flag, the renames table and `rebase_to_saved`
are deleted, and the tests that need an Append are `#[ignore = "slice
10"]` twins of their Rewrite halves.
- **Records and sealing** (`materialize/record.rs`, model.rs):
  `SaveRecord { version, path, at, strategy, stamp, xref, crypt, map,
  present, roots, tree, drawn, transforms }` with `ObjMap` (the
  primary's blob as the identity, plus the keys sheaf numbered),
  `BitSet`, `RootValues` under one static ownership table per root
  object (`CATALOG_OWNED`, `ACROFORM_OWNED`, `OUTLINES_OWNED`) and
  `TreeShape`; `SaveNote { path, at, strategy }` replaces
  `model::Saved`. `SaveRecord::opened` is synthesized at open when the
  primary was read from a file: an identity map keying only the root
  objects and page-tree nodes, every number in use present. Memory,
  blank and restored primaries have none. `Version` gains `sealed` (a
  count), `record` and `saves`; `save_input(v)` seals `v`,
  `record_save(rec)` makes `rec.version` saved and holds its record
  (the version saved before keeps only its note) and unseals,
  `abandon_save(v)` unseals, and `recovery_input()` seals nothing. A
  job's batch doesn't amend a sealed version; pruning, `remove_version`
  and `remove_subtree` keep sealed and saved versions; `keeps_snapshot`
  keeps them, and its save pins and the snapshot recount at save went
  (slice 1's note).
- **Plans** (`plan.rs`): `SavePlan { strategy, asks, transforms,
  notes }`; `Strategy { Unchanged, Append { onto }, Rewrite { why } }`
  with slice 9's `Why`s (`OtherTarget`, `NoSaveRecord`,
  `LegacyHeaderFooter`, else `AlwaysRewrite`); `Ask { FileChanged,
  Signatures { signatures, append_anyway, reason }, XfaHoldsValues {
  fields } }`; `SaveStop { Ask, Failed, Cancelled }`; `AskAnswers` and
  `unanswered`; `SaveTarget { path, same_file, profile, stamp }` with
  `Profile { encryption: Keep | None, numbering: Stable | Dense }`;
  `SaveTransform { DropXfa { tooltips_to_tu }, DropUr3, FixSigFlags }`
  from the pure, plan-time `forms::rules::plan`. `plan` always plans a
  Rewrite, asking FileChanged (the target's stamp), Signatures (the
  count `signatures_broken` finds) and XfaHoldsValues.
- **Keys** (`key.rs`): `ObjKey` (`Catalog`, `PagesNode`,
  `OutlinesRoot`, `AcroForm`, `Info`, `Page`, `Base { blob, obj, copy
  }`, `Annot`, `AnnotPart(id, Ap | Companion | CompanionAp | Popup |
  FileSpec)`, `Field`, `Bookmark`, `Labels`, `Res`, `ResPart`,
  `Shared`, `Block`, `Names`), with a fixed big-endian `bytes()` that
  AES IVs are keyed by (`Crypt::encrypt_as`). `Base` is keyed by
  `BlobId`: a page's copy is `PageDef.copy`, and what a form instance
  reaches is the instance's index across every registration of its
  blob (`FormCopies`).
- **The emitter** (`emit/`: `mod`, `pages`, `tree`, `catalog`,
  `content`, `outline`, `annots`, `form`) gives objects by key, each
  referring to others by placeholder (a reference at generation
  65,535), as `SymObject::{Raw, Built, Deferred}`, with the version
  they need and notes. A page keeps every key sheaf doesn't model and
  writes all four inheritable keys, since a Rewrite places every page
  in a new balanced tree (fanout 32); `/StructParents` stays only on
  copy 0 of a primary page, `/Thumb` goes under drawn content, and a
  derived page is written as its original page with the copy's
  `/Contents` and `/Resources` (bug 1). The legacy installers
  (`text_layer::install`, `stamps::install`) take a
  `content::sink::Sink`, which the emitter's `Cx` implements and lopdf
  documents get as `sink::Lopdf`; the header and footer is drawn where
  `stamps::base_draws` says the base doesn't draw it. The outline is
  written in place by provenance (bug 15). Marks go through the pure
  `annot::emission`, companions without `/SheafPart`; other tools'
  annotations are their source dictionaries, replies follow marks given
  fresh ids, and pop-ups go with their comments. A merged field and
  widget is one `ObjKey::Field` object with the widget's key an alias,
  an instance past a file's first gets objects of its own, `/DR` is
  merged with clashing fonts renamed, and `FormDoc` gains the owned
  keys and `XfaState`.
- **Numbering, the graph and the serializer** (`number.rs`,
  `graph.rs`, `copy.rs`, `serialize/full.rs`). Stable numbering keeps
  the primary's numbers and each key's previous number, and numbers
  new keys past every number in use, `/Size`, the cross-reference
  streams, the previous map and every primary number the emission
  names, none past `serialize::NUMBERS` (8,388,607); Dense numbers an
  export from 1 and writes identical streams once.
  `graph::rewrite_graph` loads the primary eagerly without `/Encrypt`
  and puts the emission in, `release_written` lets go of annotations a
  dropped page shared, `keep_only` runs unchanged but for returning
  `Kept { notes, modified }`, and the rest is pruned. `write_bodies`
  copies an object's raw span under the five conditions and a sound
  base, its whole span checked (`LazyPdf::raw_span_whole`), writes the
  rest anew and object-stream members as plain objects, and keeps
  `/Encrypt` at its number under Stable.
- **Post-conditions** (`verify.rs`), failing closed as
  `SaveStop::Failed`: copy-on-write, every reference resolved, the
  pages reached the tree's plus `/Names /Templates`, `forms::check`
  finding no kind of problem the input forms didn't have (when any of
  the form was written anew), and in debug builds every object read
  back as written, through the file's keys when it's encrypted.
- **Saves** (`saving.rs`, model.rs). `materialize::save(doc, path,
  answers, p)` runs `save_input` → `plan` → `write_file` →
  `record_save`, and `abandon_save` on any stop (ledger 9→19);
  `write_file`, the app's background half, refuses unanswered asks,
  materializes, writes durably with the stamp checked again before the
  rename and stamps the written file. `Document::file()` is the saved
  record's path and stamp, else `disk_base`; `save_target(path)` and
  `Snapshot.file` (the export guards) follow it. Save As is a save:
  the new file is the document's from then on, and the primary stays
  the file it opened.
- **Exports** (save.rs's API until slice 19): each a Rewrite of
  `SaveInput::detached` with `rules::plan`'s transforms; `rewrite` and
  `rewrite_to_path` Keep and Stable, extracted and split pages, Clean
  Copy and Export Flattened Keep and Dense, print None and Dense.
  `subset` keeps its state valid (a field shown only on pages left out
  goes, with the parents it empties). Clean Copy and print without
  markup filter the state and strip what the records don't hold in a
  post-pass on the checked graph; Export Flattened and print with
  markup run `flatten_annots` as that post-pass (`materialize_then`).
- **Recovery v2** (recovery.rs): `sheaf-recovery 2\nfull\n<len>\n
  <stamp hex>\n<flags>\n<bytes>`, written from `recovery_input()` with
  Keep, Stable and the plan's transforms, so it's encrypted when the
  file is and a page delete on a hybrid form leaves no XFA in it.
  `find` checks the base with `FileStamp::holds`; `restore` checks it
  again and opens the bytes from memory as `MemoryKind::Recovered`,
  with no record, and the document's `disk_base` is the sidecar's
  base. v1, its `MAGIC`, its temp primary and its leaked temp file
  (bug 22) are gone.
- **The app** (docview.rs, form_bar.rs, layers_panel.rs,
  attachments_panel.rs): seal, plan in the background, one
  `pending_ask` (FileChanged: Save As…, Overwrite or Cancel;
  Signatures: Rewrite or Cancel; XfaHoldsValues), `write_file` in the
  background, then `Doc::take_save` (`record_save`) or `abandon_save`;
  a FileChanged found at the rename asks again with the version still
  sealed, and a second save meanwhile is refused ("Already saving…").
  The reopen, re-hide, drawn-marks recompute, `refresh_facts`,
  `apply_layers` and tile invalidation after a save went
  (`Doc::tiles_invalidated` counts drops); the header overlay draws
  where `stamps::drawn_over` says. Recover restores from memory.
  Autosave writes beside the record's or `disk_base`'s file, waits for
  a save or another sidecar in flight and skips an unchanged (version,
  edits); after a save the sidecar is written at once when edits
  followed, and deleted otherwise. Password-protected documents still
  get none.
- **The command line:** `comments import` saves through
  `materialize::save`, answering Rewrite and drop XFA as its
  `Signed::Rewrite` did; its output is unchanged.
- **Deleted,** each confirmed dead by `cargo check` and clippy before
  the list was final: incremental.rs with its replaced-layer bails;
  from save.rs `build_rewrite`, `rewrite_with`, `finish`,
  `kept_encryption`, `save_to_path`, `save_to_path_noting`,
  `Strategy`, `Signed`, `WouldBreakSignatures`, `IfChanged`,
  `write_checked`, `file_changed`, `write_outline`, `outline_dest`,
  `write_page_annots` (`ConnectorRef`, `PageAnnotsWritten`),
  `strip_sheaf_annots`, `link_connectors`, `is_identity`,
  `reserve_pages`, `reserve_page_objects`, `copy_of`,
  `origin`/`origins`/`needed`, `copy_derived_page` and `geom_of`;
  `serialize::full::write`; forms/merge.rs, `rules::take_out` and
  `rules::has`, `forms::read::fields_of`; model/legacy.rs and `Field`'s
  `source`, `copy` and `under`; `Document.baseline`,
  `removes_content`, `annots_dirty_pages`, `fields_dirty`,
  `header_footer_dirty`, `form_changed` and `widgets_changed`, every
  `Snapshot` flag and seven state.rs helpers, so `snapshot()` computes
  nothing; `rebase_to_saved`, `rebind_annots`, `rebind_fields` and
  `mark_saved`; the renames table (`Renames`, both `renamed` fields,
  `renamed_on`, `set_source_renamed` and its two app callers);
  `SourceInfo.stamp`; `file::{page_geom, load_source, read_annots*,
  removed_with_threads}`; `WriteCx::owning`; `append::Base::of`;
  `content::to_lopdf::{resources_of, ResObjects::stream}`;
  `kept::{drop_dead_links, prune_fields}`; recovery v1; the app's
  `save_signed`, `ask_how_to_save_signed`, `ask_file_changed`,
  `pending_signed`, `pending_changed`, `pending_xfa`, the XFA ask's two
  methods, `refresh_facts`, `layers_changed` and `layers_default`.
- **Moved:** `copy_object`, `remap`, `is_page_node`, `copy_page_dict`,
  `inherited_attrs`, `INHERITED` and the page reservation
  (`copy::reserve`) to `materialize::copy`; `LoadedSource` and
  `load_source` to `graph::Loaded` and `graph::load`; `SaveStop`, `Ask`
  and `signatures_broken` to `materialize::plan`; `rename_in_da` to
  `emit::form`. Derived copies leave sheaf's marks out by
  `file::not_ours` (over `file::is_ours`, the emitter's check) and read
  boxes by `Geom::read`, as stamp_lib does; `file::annots_in` takes
  each page's turn from the page table; tests write marks through
  `forms::fixture::write_marks` over `annot::emission`.
- **Twins:** `both_ways!(name, name_appended: case)`, with
  `materialize::{How, appended, save_appended}` and sheaf-ops'
  `common::{rewritten, appended, written, saved, rewritten_and_saved,
  drops_xfa}`: 50 both-ways cases (forms_merge, forms_pages,
  forms_saves, forms_open, crop, interop, encrypted_corpus, the
  renderer's encrypted.rs, model.rs, kept/tests, open/tests,
  serialize/tests and rewrite_tests) and 21 more `#[ignore = "slice
  10"]` tests, M10 slice 5's signed Appends among them. save.rs's
  ported tests are `materialize/rewrite_tests.rs`.
- **Ledgers:** the retired ledger has 135 rows for slice 9, and
  `QUALIFIED_ONLY` 33 names more, 31 of them used elsewhere as a last
  segment (found by running the walk without them; `merge` and
  `needed` are kept as common words), and lost a duplicate `source`. The
  transitional ledger's rows say "deleted" once their slice has deleted
  them (the earlier slices' too), with the parts for rows deleted in
  parts; the `Objects` seam, `to_lopdf`/`WriteCx` and number-keyed IV
  rows went to their real slices; the every-save-a-Rewrite row is
  split, with new rows for Full-only recovery and `Document.disk_base`
  (9→10), the ignored Append twins (9→10), `materialize::save` (9→19)
  and `stamps::base_draws`/`drawn_over` (9→13, 17). No lint ban lands:
  `std::env::temp_dir`'s recovery user went, and the ban waits for 19.
- **Docs:** `docs/document-model.md`'s Saving opens with a save's
  flow; One writer keys AES IVs by `ObjKey`, keeps `/Encrypt` at its
  number and names `Profile`; "Saves are outputs" follows a save from
  start to end (sealing, plans, records, keys, the emitter, numbering,
  the Rewrite, post-conditions, exports, the document's file, the app,
  recovery, what went and the twins). The Core idea has no stamp on
  `SourceInfo` and saving as an output; Opening, Undo/redo (sealed
  versions, no save pins, a save puts no version), Page content (a
  mark's stamp and file written once a file) and Text layers (a batch
  during a save lands after the sealed version) say what changed. The
  README's save-model row and crash recovery say every save rewrites
  until slice 10, a recovered document is checked against its file,
  and a sidecar is the whole file for now.
- **The 7k harness:** the Rewrite asserted in its own process against
  slice 8's writer, at most 1.2× its time (`REWRITE_TIME_GATE`) and
  1.5× the RSS it added (`REWRITE_ADDED_GATE`), on the synthetic and
  heavy fixtures; the opened record's time, one mark saved as the app
  saves and the Full recovery sidecar of it, with its size, on every
  fixture; the incremental save's rows skipped until slice 10; the
  module doc says so.

Deviations from the plan (the planner's list first, as resolved):
- **Append halves are twins, not half-ignored tests:** some suites ran
  both strategies in one function, so each case became a function of
  `How` run twice by `both_ways!`, the Append name ignored, and every
  ignored test still compiles. Deleting something in sheaf-ops' tests
  (an in-place save that's a Rewrite either way) uses
  `rewritten_and_saved` and has no twin.
- **`Snapshot.sources` stays:** derive.rs, crop.rs and blank.rs read
  registrations, and the emitter needs each one's `derived` record
  until slices 11 and 17. Only the dirty flags, `renamed` and `legacy`
  went.
- **Plumbing with live users moved rather than went:** `derived_copy`
  (until 15–17) reserves pages with `copy::reserve`, copies with
  `copy_page_dict`, reads boxes with `Geom::read` and leaves sheaf's
  marks out with `file::not_ours`; derive.rs loads with `graph::load`;
  stamp_lib copies with `copy_page_dict` and reads geometry through a
  private `geom` over `Geom::read`, walking 64 parents as `page_geom`
  did (its `inherited` helper now too, where it walked 32).
- **`write_page_annots`' other users** (`source::fuzz::mark_fixtures`,
  annot.rs's identical-marks test, open/tests and annot_form_tests)
  write through `forms::fixture::write_marks` over `annot::emission`.
- **`to_lopdf`'s `ResObjects`, `ResBody::Written` and `WriteCx` stay**
  for `exchange::gather`, the comment summary and the legacy
  installers' `sink::Lopdf`; only `WriteCx::owning` (and `own`) went,
  and `WriteCx::file` returns an `ObjectId`, having no failure left.
  The `Objects` seam stays for exchange's Comments.pdf, stamp_lib,
  derive.rs and `keep_only`'s graph; `fields_of` went. Both ledger rows
  name their real slices.
- **The app's two flags:** `header_footer_dirty()` became
  `stamps::drawn_over`, shared with the emitter; `form_changed()`
  became `Ask::XfaHoldsValues` (and, in tests, `drops_xfa`, a
  `DropXfa` in the plan's transforms).
- **The document's file is the saved record's path,** or a restored
  document's `disk_base`, never the primary's name: the sidecar's
  place, the export guards and `write_recovery` follow it, and the
  primary stays the file opened after a Save As. A document with no
  record and no `disk_base` (blank pages, a memory primary) has no
  file: its save is a Rewrite (`OtherTarget`, `NoSaveRecord`) to the
  path the dialog chose, which asked about overwriting, as slice 7's
  note left it.
- **The open stamp:** `SourceInfo::read` returns `(SourceInfo,
  FileStamp)`, and `Document::new`, `loading`, `open::start` and the
  app's `opening_model`/`install` take the stamp for
  `SaveRecord::opened(version, &SourceInfo, FileStamp)`; the record
  names its version, which `replace` updates, so the opened record
  follows `ready`. A restored document keeps the sidecar's base in
  `Document.disk_base` (ledger 9→10).
- **The gate's baseline is slice 8's writer,** not slice 2's consts,
  which slice 6 found 2–4× loose: timed in its own process at slice
  8's commit (`WriterBaseline`, how below), the new writer in its own
  process in this build. Step 4 timed both writers in one build while
  both existed, as slice 2's note asked: 133 and 712 ms, under 1% from
  the separate build's 134 and 707.
- **The one-mark incremental rows** (plain and AES-256) go through
  `materialize::appended` and print as skipped until slice 10; they
  time `appended` and a durable write, not `save_appended`, which
  would record each run against a file the harness puts back.
- **`SaveRecord` and `SaveInput` are slice 9's size:** no
  `content_hash`, `pending_tail` or `path_keys` (slice 10), `drawn`
  empty until 16–17; `SaveInput` gains `primary` and `Snapshot` a
  `version`; `Why` has only the reasons slice 9 gives, `AppendAnyway`
  is empty, there's no `FillAppearances` (M10's) and `Profile` has no
  `private`; `StrategyKind` is `Opened | Append | Rewrite`;
  `Version.sealed` is a count (§8.1); `SaveTarget` gains `profile` and
  `stamp`; `Output.record` is an `OutputRecord`, stamped once the file
  is on disk.
- **`Ask::Signatures` carries a count** (`signatures_broken`), not
  slice 10's `SigImpact`s; a certification counts as one.
- **A synchronous save, `materialize::save`** (ledger 9→19), since
  `Session::save` is slice 19's: the command line answers Rewrite and
  drop XFA, as `Signed::Rewrite` did, and proposal 40's flags wait.
  `write_file`, `unanswered`, `Written`, `SaveReport`,
  `BlobKind::recovered` and `durable::same_file` (comparing paths when
  either can't be canonicalized, so a deleted document file still
  asks) are public too, and `save_appended` is the tests'.
- **Recovery encryption:** password-protected documents keep slice
  7's autosave skip until slice 10; an encrypted file that opens
  without a password gets an encrypted sidecar. The sidecar's stamp is
  the tail's hash then the time in nanoseconds, 32 hex digits each;
  its flags are space-separated names, and an unknown one rejects it.
- **Full-only recovery's cost** is measured (below): more than the
  planner's half second on the scanned 7k.
- **`subset`'s state** is made valid before the emitter: a field node
  whose widgets were all on pages left out goes, with the parents it
  empties, and the roots and calculation order follow.
- **Slice 8's four emitter jobs are done:** `Kept` bookmarks from any
  file keep their actions; unmodeled subtypes stay with their page
  (pop-ups go with their comments); the owned AcroForm keys and
  `XfaState` are read at open and written from `FormDoc`; replies
  follow marks given fresh ids.
- **The renames table went** (ledger 5→9, not in the Deletes) with its
  callers at model.rs, page_insert.rs and docview.rs: a save writes a
  mark given a fresh id anew (`FileAnnots::fresh`), and the app hides
  a renamed mark by its old name from the view's facts.
- **`Field.copy`, `Field.under` and `Field.source` went; the
  ObjectIds stay:** `Field.object` is read by `rules::form_differs`
  and the forms tests, `Widget.object` by the open's `nodes_of`.
- **The PDF version** is the emitter's: the base's (the header or the
  catalog's `/Version`), raised for what the save writes (1.2 for OCR's
  CID fonts, 1.4 for the header's marked content, 1.5 for
  cross-reference streams, 1.6 for AES); a Rewrite writes no catalog
  `/Version`. The decisions' version-bump policy stays slice 16's.
- **Unchanged plans** fail as Appends do ("an Unchanged save writes
  nothing, from slice 10").
- **`ObjKey::Base` is keyed by `BlobId`,** with a blob-wide instance
  index for what a form reaches (`FormCopies`), deterministic for a
  state but able to shift between saves as instances come and go,
  until slice 11 makes `FormInst.src` a `BlobId`.
- **Helpers left without callers:** `text_layer::has_layer` is called
  by `install` before it strips; `strip_markup_annots` and
  `is_link_or_field` serve Clean Copy's and print's strip pass
  (`save::strip_markup`), and §17's slice 19 stands for them.
- **A second save is refused** ("Already saving…"), which the seal
  makes safe; a FileChanged found at the rename asks again with the
  version still sealed.
- **Line references had moved:** docview's Append offer was
  `ask_how_to_save_signed` (~1696–1735), incremental.rs's bails were at
  :271 and :327, and the rebase's callers went past the four tests
  named.

Deviations found while building:
- **Pruning keeps the saved version,** so a branch holding it stays
  whole and the root stops there: `remove_version` returns whether it
  removed one, prune stops when it removes nothing, and two tests that
  relied on pruning a saved branch were rewritten
  (`pruning_keeps_the_saved_branch_whole`,
  `a_save_s_file_goes_with_the_last_version_holding_it`, the second
  later deleted with the saved-file source). The history property test
  gains SEAL and UNSEAL actions.
- **`keep_only` returns `pub Kept { notes, modified }`,** and marks the
  structure root, a structure element's `/K` and a page's `/B` modified
  only when they change, so `modified` is exact (a test compares it
  with an object diff); `kept` became `pub mod`.
- **The opened record keys only root objects** (catalog, info, form,
  outline root, labels and page-tree nodes); pages, annotations, fields
  and bookmarks of the primary are at their own numbers by identity.
- **Emitting:** `emit` returns `Result` (cancel and unreadable objects
  propagate), with `syms`, `notes`, `owned`, `aliases`, `form_keys` and
  `form_touched`; numbering is its own step, so `rewrite_graph` takes
  the emission, the numbers, the primary and the loaded graph. Page
  references are copy 0, shared by every copy, and only `/Annots`
  follows a copy; a copied reference to a page goes to the same copy,
  else the lowest written, else null. A page owns its object only on
  its file's first listing (`PageFacts.repeat == 0`), and two derived
  pages of one original take the lowest free copies. A page with no
  `/Resources` anywhere gets an empty one. New keys `ResPart`, `Block`,
  `Part::{CompanionAp, Popup}` and `Names(JavaScript)`; the OCR and
  header `q`s are content-addressed `Res` keys, not `Shared(QOpen)`.
- **The installers take a sink** rather than `ResObjects` becoming the
  emitter's allocator; its reads take `&mut self`, since the emitter's
  reads make placeholders. `derived_copy` gains `HeaderKept` (crop and
  watermark copies keep the source's `/SheafHeaderFooter`, remapped,
  so a cropped page of a file that draws its header isn't drawn
  twice), and `Source::header_footer()` is a memoized fact.
- **The outline:** unchanged primary items and the root are copied
  byte for byte (`SymObject::Raw`); a bookmark keeps its `/Dest` or
  `/A` only while it is the primary's and goes to the page written at
  the object they name, with the same view, else gets a new `/Dest`;
  `open::outline::Dests` exposes the open's readers so the emitter
  judges an item as the open read it.
- **Annotations and forms:** `annot::emission(&Annot, Held)` takes
  what the file holds rather than a `Mark`; companions lost
  `/SheafPart` in both writers while both existed; `rules::plan`
  takes the primary's registration; `FormDoc` holds no `/DR` (merged
  at emit); `/Q` is written only when non-zero; `rename_in_da` moved
  to `emit::form`; a test-only `Document::put_for_test` sets `/CO`. A
  form the state doesn't model is kept as the file has it. A field
  that showed as read and shows nowhere isn't written, nor the parents
  it empties, judged by the file's form as read (`Fields::showed`; by
  `/Kids`, corpus file 0982696 lost 19 objects), and a form left with
  no fields isn't written unless its file had none.
- **Post-conditions, as built:** a reference to a number the primary
  never used, past `NUMBERS`, or to an object it can't read is the
  input's damage. `forms::check` (§11.9 #3) is compared by what each
  finding names, not by kind (the review's change: by kind, one
  damaged inserted file let new damage of its kinds through
  anywhere): a widget, field or font name an input's finding of the
  kind has passes, as does `P.name` (a field under the parent sheaf
  gives an instance whose names clashed), and a damaged input's
  findings count by kind, since its tree couldn't be read whole and its
  names aren't its fields' (0982696's identity Rewrite writes its
  shared kids under one parent each, and four duplicate names come out
  named otherwise than the input's check named them). It skips the
  policy kinds (`StaleSignature`, `StaleSigFlags`, `XfaMismatch`,
  `EarlierValues`), which the plan asks about first, and `NoAppearance`
  (a page delete leaves one where the chosen button's widget went, as
  the old writer did): the narrowing §11.9 #3 doesn't say. It runs only
  when the save wrote any of the form anew, or `keep_only` or
  `release_written` changed it. Copy-on-write (§11.9 #6, §11.5) is
  ownership for emitted keys and content for the rest
  (`verify::copied`): every object at a primary number that no emitted
  key holds is compared with the source's object, but those
  `keep_only` reported, the `/Annots` arrays `release_written` changed
  (a dropped page's, letting go of annotations a kept page writes) and
  the objects `Graph::relocate` rewrote to follow a moved number
  (`Graph::followed`): two exceptions §11.5 doesn't list, both edits
  the Rewrite's own steps make. Objects copied as their bytes were
  aren't compared, being the source's. An export's post-pass runs
  after the graph's checks, and references and reachability are
  checked again after it (`verify::links`); copy-on-write isn't, a
  post-pass being free to change any object (every export is Dense).
  The debug read-back compares every object, raw copies too, with the
  checked graph (step 4 had narrowed it to the lazy source for raw
  copies, which hid the stale-revision bug below).
- **Numbering past the plan (§11.6 is silent):** no number at or past
  `serialize::NUMBERS` is kept or given; a primary object numbered
  past it is renumbered like a new key (`Graph::relocate`, references
  followed, `Graph::followed` written anew, a note); `/Size` and the
  old cross-reference stream's number are reserved only while the
  save's numbers fit; the Stable start also counts the highest number
  in use and every primary number the emission names; `keep_only`'s
  new objects go past the written ones; the cross-reference stream
  goes past every primary number (`Layout::xref_from`).
- **Vendored lopdf** (`vendor/lopdf/SHEAF.md`): `raw_span` checks
  readability with `object_unkept`, so a Rewrite caches nothing in the
  Source, and `raw_span_whole` refuses a stream whose `/Length` is a
  reference or doesn't fit its data. `Crypt` is `Clone` with
  `moved(at)` for a Dense record. `graph::eager` was added, the name
  `load_source` being retired. From the review: `raw_span_whole` also
  refuses an object without its `endobj` (copied, the output repeated
  the damage, `qpdf --check`: "expected endobj"); the eager load no
  longer keeps an object whose header names another number the table
  places itself (in 5 corpus forms an earlier revision's Info, at
  another entry's offset, replaced the current one in the eager graph
  only, so a raw copy of a stale-xref file named an image the graph
  never had and pruned; `lazy_tests`' `Shadowed` rows went); and a
  signature dictionary with no `/Type` or `/FT` but `/ByteRange` and
  `/Contents` keeps its `/Contents` undecrypted (`/Type` is optional
  there: 1263925's signature failed to decrypt when a copy read it,
  and stopped the save).
- **Speed, all checked byte for byte by the corpus runs:** raw spans
  found on up to 8 threads while the graph is built (told to stop and
  joined when the save stops first: a cancel or failure had left the
  threads reading the primary); unchanged annotations, widgets and
  fields copied without being built, only where the built path would
  write the same (the review found four places it wouldn't: another
  tool's annotation with `/RT` and no parent, an outline root with no
  `/Type`, a field with an empty `/Kids`, and a mark with another
  page's `/P`, a pop-up or a `/SheafPart` companion, which the docs
  said saves without it); a lazy
  modeled-slot set; the graph check in parallel; and (step 8) a page's
  base dictionary borrowed rather than cloned. Freeing the graph on a
  thread of its own was tried and reverted: back-to-back Rewrites got
  slower (1.32–1.42× on the synthetic), glibc's allocator contending
  across threads.
- **Saves:** the written file's stamp is `FileStamp::of(metadata,
  written bytes)`, not a read back, so a program writing right after
  the rename isn't taken for us; `keeps_snapshot` keeps the saved
  version, whose state `SavedSide` needs; the baseline isn't moved by
  `record_save`; once the reader agrees to drop XFA values,
  `Forms::xfa_values_dropped` answers that ask for the window; the
  command line prints no notes. `Ask::Signatures` isn't asked of a
  save onto the file an earlier Rewrite of the document wrote (the
  saved record's strategy `Rewrite`, the target the same file): the
  primary stays the opened file, whose signatures it counts, so every
  later save asked again of a file with none left. Save As elsewhere
  still asks, since it may overwrite the signed original.
- **The app:** `Doc::new` and `Doc::take_save` were factored out so
  the tile-counter test runs headless (GPUI on Linux only, quitting
  from a spawned task), and `DocView::unopened` so a test ends a save
  through the view's own `finish_save`; the sidecar after a save is
  written on the UI thread as the save ends (`write_recovery_now`), as
  D39's review ruled (step 6 had written it in the background, so a
  crash after the rename and before it lost the edits made during the
  save: the old sidecar is stale against the new file). It costs a Full
  sidecar's write (0.3 s on the synthetic 7k, 1.6 s on the scanned)
  only when edits followed the saved version, until slice 10's Append
  sidecars; a sidecar already being written when the save ends was
  begun against the old file and, stale, writes the new one again in
  the background. The autosave skip is keyed on (`VersionId`, edits),
  since job amends keep their id until slice 12. `refresh_facts`,
  `layers_changed` and `layers_default` went, their only users being
  the post-save path; `open::outline::{MAX_DEPTH, shown_title}` are
  private again.
- **Tests:** save.rs's ported tests are `materialize/rewrite_tests.rs`
  (materialize/tests.rs was 1.5k lines), and save.rs keeps its export
  tests; `geometry_follows_the_crop_box` is a `Geom::read` test in
  geom.rs; tests read a saved file's annotations through
  `rewrite_tests::annots_read` (`file::annots_in` on the bytes, ids as
  written), and `file::annots_in` takes `(page, Rotation)` pairs. The
  rebase-only tests became tests of saves as outputs
  (`a_save_keeps_ids_and_history`, `a_save_keeps_text_layers`,
  `a_save_writes_marks_and_their_comments`,
  `a_link_a_save_left_out_is_gone_from_its_file`); the removal flag's
  assertions were dropped or rewritten (`form_changed` as `drops_xfa`,
  `header_footer_dirty` as `stamps::drawn_over`), and
  `fields_dirty_compares_with_the_primary_s_form` was deleted. The
  two FileChanged tests moved to `saving_tests.rs`.
- **The 7k harness** times `materialize`'s Rewrite with its `Output`
  dropped outside the time, as slice 8's writer's bytes were (a save
  keeps the record), and a Rewrite row's gate column points at the
  asserting part.
- **The ledgers:** `QUALIFIED_ONLY` keeps `merge` and `needed`, which
  nothing else uses now, as common words, and not `legacy`,
  `load_source` or `LoadedSource`, which nothing uses: listing them
  would only let a definition of the name come back elsewhere. The
  transitional ledger now marks every row its slice has deleted.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The gate.** Slice 8's writer was timed at its commit (0fa5cf1), in
  that commit's own release build and target directory, its harness
  given a part that times `save::rewrite` on the fixture as opened just
  as `rewrite_part` times `materialize`; seven processes of each
  fixture, run alternately with seven of this build's part, each
  process's time the median of three runs, and the baseline the median
  of the seven (`SLICE8_REWRITE_*`):

  | | Synthetic | Heavy |
  |---|---|---|
  | slice 8's writer | 134 ms (130–137); 96.5 MB peak, 61.7 MB added | 707 ms (686–736); 468 MB peak, 251 MB added |
  | `materialize`, before step 8 | 150 ms (148–152), 1.12×; 85.5 / 50.0 MB (0.81×) | 764 ms (751–798), 1.08×; 467 / 250 MB (1.00×) |
  | `materialize`, as committed | 144 ms (142–149), 1.07×; 50.4 MB added (0.82×) | 764 ms (747–780), 1.08×; 250 MB added (1.00×) |
  | gate (asserted) | ≤ 161 ms, ≤ 93 MB added | ≤ 848 ms, ≤ 376 MB added |

  Step 4 timed both writers in one build: 133 and 712 ms for slice 8's,
  150 and 727 ms for `materialize` then. Before step 8's two changes (a
  page's base dictionary borrowed, the harness dropping the output
  outside the time) one whole-harness run failed the synthetic at 162
  ms (1.21×), straight after the 200 OCR versions' 5 GB; alone, the
  part read 148–152 ms. After them two whole-harness runs passed (145
  and 141 ms; 760 and 706 ms). Where the time goes, from a gdb sampler
  on the synthetic (278 samples): emitting 24% (pages 17%), writing 16%,
  building the graph 12% (half of it freeing the primary's page
  dictionaries the emitted ones replace), freeing the graph at the end
  12%, `keep_only` 9%, the eager load 8%, pruning 5%, numbering 5%.
- **The 7k harness,** both whole runs (slice 8's in brackets):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | geometry walk | 48.0–48.6 ms [48.1], asserted | 72.5–75.8 ms [74.5–77.7], asserted | 61.2–71.1 ms |
  | ready | 54.4–66.1 ms [54.0–65.0] | 342–357 ms [349–356], asserted | 451–454 ms |
  | ready, peak / added RSS | 35.6–35.7 / 27.0–28.7 MB [35.3 / 28.5] | 218 / 200 MB [218 / 199] | 521 / 514 MB |
  | the opened record (in ready) | 0.90 ms | 1.09–1.11 ms | 0.94–0.96 ms |
  | save, one mark (Rewrite) | 254–317 ms [incremental 57.2–63.9] | 657–747 ms [289–325] | 1,062–1,192 ms |
  | save, one mark, peak RSS | 91.8–91.9 MB | 485 MB | 1,556–1,557 MB |
  | recovery sidecar, one mark (Full) | 285–293 ms, 3.37 MB | 697–728 ms, 17.9 MB | 1,553–1,710 ms, 474 MB |
  | recovery sidecar, peak RSS | 104–105 MB | 501–510 MB | 2,055–2,079 MB |
  | Rewrite, in the fixture's process | 147–151 ms [132–135] | 629–664 ms [602–604] | 533–593 ms |
  | Rewrite, peak / added RSS | 108–110 / 39.0–41.3 MB [103–105 / 52.5–53.1] | 503–513 / 237–238 MB [488–490 / 247] | 1,602–1,607 / 981–997 MB |
  | crop all pages / the edit | 164–171 / 12.6–12.7 ms [158–162 / 12.0–12.2] | 720–736 / 12.2–12.7 ms [706–710 / 11.8–12.2] | 1,093–1,274 / 12.1–12.3 ms |
  | Bates apply / its Rewrite | 138–155 / 396–422 ms [148–157 / 372–380] | 194–203 / 916–935 ms [207–211 / 802–803] | 75.9–92.9 / 823–829 ms |
  | flatten | 284–289 ms [284] | 1,711–1,720 ms [1,369–1,390] | 528–582 ms |
  | Rewrite, AES-256 | 214–218 ms [191–219] | 1,017–1,032 ms [1,031–1,095] | — |

  The incremental rows are skipped until slice 10. Renders and
  previews are as before; the history rows hold (AnnotAdd 0.015 ms, a
  full reorder 0.720 ms; 1,000 mark adds 0.138–0.140 ms median on the
  heavy; ready on 20,000 marks 515–517 ms; their Rewrite 1,415–1,432
  ms, 264 MB added [1,141–1,142 ms, 238–239 MB]; 200 OCR versions
  4,094 MB of steps). A save of one mark is a Rewrite until slice 10,
  two to four times slice 8's incremental save; Bates' Rewrite is
  6–17% slower and the heavy flatten 24% (a Dense export, its
  post-pass graph written anew).
- **The corpus** (../pdf-corpus, sheaf-ops tests/materialize_corpus.rs),
  unchanged from step 4 through the end: an identity Rewrite of 4,389
  of the 4,392 files (3 don't open, as before), 2,270,468 primary
  objects surviving at their numbers, 919,554 of them copied as they
  were, 436 changed by `keep_only`, 0 failed; 2,901 forms written
  without their first page, 0 failed; every file
  written twice and then from two threads, 0 differ; 2932995's logo, 0
  of 2,037,600 bytes differ. encrypted_corpus's Rewrite half passes.
  After the review: 2,269,722 surviving and 919,479 copied as they
  were, asserted now (`IDENTITY`, when the manifest is these 4,392
  files): 746 objects (75 of them raw) the fast paths copied where the
  built path writes them otherwise are written anew, so the record
  lists them (614 of 0982696's fields and 11 of 1210120's with an
  empty `/Kids`, and an untyped outline root in each of 121 files);
  the page-delete run checks each written form against its file's own
  by name (0 new; 551 findings of the allowed kinds, values with no
  appearance and policy's), where it had thrown the report away; every
  file with every page duplicated writes (4,389, 0 failed; the reviewer
  counted 307 failures before the closure's fixes, 304 of them widgets
  on no page, and slice 8 failed 2 of them), and every fourth file's
  pages and five that once failed, inserted twice into the 1040, write
  (1,098, 0 failed; 85 failed before).
- **Interop** (qpdf 12.3.2, node 24.22 and pdfjs-dist 6.4.299):
  `qpdf --check` clean on 85 outputs (both bases' saves, extracted
  pages, print with and without markup, Clean Copy and Export
  Flattened, and the bookmarks test's Rewrite), and pdf.js reads that
  Rewrite as written: "Site" opening https://example.com/, "3" going
  to page 1, the link and the kept comment on page 2. The 14 Append
  outputs and the prefixed-append test are slice 10's twins.
- **Suites:** `cargo test --workspace` 878 passed and 96 ignored in 60
  binaries, 228 s (slice 8: 814 and 25), sheaf-doc's unit tests 522
  and 42 ignored in 29.8 s; fmt and clippy (`-D warnings`, all
  targets) are clean, and `cargo check --target
  x86_64-pc-windows-msvc -p sheaf-doc -p sheaf-ops --tests` passes. The
  ignored tier in release, the 7k harness apart: 15 passed (the replay
  property at 4,096 cases, both interop tests, the OCR engines, the
  lexer-vs-PDFium differential, PDFium's frames, the doc tests), and 72
  failed, 71 of them slice 10's twins and Append tests stopping on
  "Append comes in slice 10" by design and the 72nd the fuzz-seed
  writer, which needs `$SHEAF_FUZZ_SEEDS`. After the review:
  `cargo test --workspace` 892 passed and 96 ignored in 60 binaries,
  about 5 minutes; fmt, clippy and the Windows check clean; the interop tests
  (`--skip append`) pass, `qpdf --check` clean on 85 outputs; the
  replay property at 4,096 cases passes in release.
- **The review** (slice 9's step 9, 22 findings), besides what the
  bullets above say: the 7k Rewrite reads 146–147 ms on the synthetic
  (1.09×) and 770–791 ms on the heavy (1.09–1.12×), four processes
  each, against 144 and 764 ms before it, the copy-on-write content
  check and the objects the fast paths now build costing the
  difference; both gates hold. The tests' `Way::Materialize`, the
  `[false, true]` loop in `inserting_one_page` and forms_pages'
  "materialize" row went: `save::rewrite` is the same `materialize`
  with the same plan, so they wrote the same bytes twice. The history
  test's snapshot check names what `keeps_snapshot` keeps (the opened,
  current, saved and sealed versions), where it exempted any version
  with a save note, so it stopped checking after a run's first save.
  The ignored tier's slice 10 twins fail by design, so running it
  without them is `-- --ignored --skip append` (interop.rs and the
  document model say so). Rejected: the untracked
  `plan/topics/code-signing.md` (found twice) isn't slice 9's and no code
  touches it; the slice's commit stages its own paths, leaving it and
  `plan/topics/installer.md` out.

Found:
- **Freeing is a fifth of a Rewrite:** the primary's page dictionaries
  that the emitted ones replace, and the eager graph at the end (15 ms
  of the synthetic's 144, 86–187 ms of the heavy's). Freeing on
  another thread made the next Rewrite slower. Proposal 37's symbolic
  port, or loading the primary without what the emission replaces,
  would remove it; neither is budgeted.
- **A Full sidecar of the scanned 7k peaks at about 2.06 GB** and takes
  1.6–1.7 s, writing 474 MB: the eager graph, the output and
  `recovery::write`'s copy of the output behind its header. Slice 10's
  Append sidecars make it small; writing the header and body apart
  would drop the copy.
- **The synthetic's margin is the thinnest** (1.07× of 1.2×): a
  whole-harness run read 1.21× once before step 8's changes, after the
  200 OCR versions. After the slice, Greg set the time gate to the
  plan's 1.2–1.3×: past 1.3× fails, past 1.2× is flagged
  (`REWRITE_TIME_FLAG`), so a check that makes saves sounder can cost a
  little time.
- **The Rewrite of 20,000 sheaf marks** is 24% slower than slice 8's
  writer (1,415–1,432 against 1,141–1,142 ms) and adds 264 MB against
  238–239; no gate holds it.
- **Real files the corpus runs found** (steps 4 and 5, all fixed, each
  with a test): streams whose `/Length` is an object a Rewrite pruned,
  or doesn't fit their data, copied raw (`raw_span_whole`); a
  cross-reference stream or a new object taking a number the file still
  used or referred to with nothing there; a widget a deleted page shared
  with a kept one losing its field to `keep_only` (0218989); a field
  node whose `/Kids` another node's read took (0982696); marks another
  program copied, given fresh ids at open, copied byte for byte with
  their old `/NM` (`FileAnnots::fresh`); object numbers past what
  sheaf's reader takes, which an object stream can hold unlisted.

Checked by tests:
- **Ports and identity:** every save.rs rewrite test is ported:
  `materialize/rewrite_tests.rs` (22 tests and 19 both-ways cases,
  their Append halves ignored) and save.rs's export tests; after a
  corpus Rewrite every surviving primary object keeps its number and
  untouched ones are byte-identical
  (`an_identity_rewrite_keeps_every_number_and_copies_what_it_left_alone`,
  sheaf-ops tests/materialize_corpus.rs, with the unit test
  `an_identity_rewrite_keeps_every_number_and_copies_what_it_didnt_touch`);
  the same input twice and from two threads gives the same bytes
  (`the_same_document_writes_the_same_bytes_twice_and_from_two_threads`,
  every corpus form four times, and `a_rewrite_is_the_same_bytes_every_time`).
- **Outline, ownership and forms:**
  `outline_actions_se_and_closed_items_survive_a_reorder` (`/A /URI`,
  `/SE`, a closed item, a reorder and a save) with
  `a_kept_bookmark_from_an_inserted_file_keeps_its_action`;
  `a_duplicate_moved_before_its_original_leaves_copy_0_with_the_original_object`;
  `every_form_writes_without_a_page_its_form_checked` (2,901 corpus
  forms without their first page, the test's own `forms::check` of
  each output finding no widget, field or font the file's check
  didn't name, but values with no appearance and policy's kinds, and a
  damaged file's findings by kind; it had thrown the report away, so
  only the post-condition, comparing by kind, held it);
  `every_file_writes_with_its_pages_duplicated` and
  `every_fourth_file_s_pages_inserted_twice_into_a_form_write`, with
  the units `a_copy_doesnt_bring_its_file_s_catalog_or_form`,
  `a_copy_brings_no_widget_its_page_doesnt_show` and
  `an_object_a_copy_names_that_doesnt_read_is_copied_as_null`; the
  M9 bug 14 reproductions in kept/tests.rs
  (`a_deleted_page_leaves_the_file_whatever_points_at_it`,
  `a_reply_keeps_no_comment_of_a_deleted_page`,
  `fields_tags_and_threads_let_go_of_a_deleted_page`,
  `copies_hold_only_the_pages_copied`) run through `materialize`.
- **Emission rules:** `a_duplicated_merged_field_is_one_object_a_copy`;
  `a_tagged_page_inserted_into_a_tagged_file_has_no_struct_parents` and
  `..._has_no_struct_parent_on_its_annotations`;
  `pages_under_a_turned_cropped_node_keep_how_they_show` (sheaf-render
  tests/rewrite_pages.rs: a node with `/Rotate 90` and a `/CropBox`, a
  foreign page inserted beside its pages and a duplicate, PDFium's
  sizes and pixels; it fails with the written `/Rotate` or `/CropBox`
  taken out); `owned_keys_survive_set_save_undo_save_reopen` (labels,
  the outline root, `/CO`); `page_templates_stay` (forms_pages.rs)
  through the post-conditions; `a_replacement_saves_with_no_sheaf_part_and_reopens_as_one_mark`
  (and an older file's `/SheafPart` companions).
- **Saves during edits:**
  `an_edit_and_an_ocr_flush_during_a_slow_save_land_after_it` (a
  progress that blocks the save; both land in a child version, stay
  dirty and go into the next save);
  `a_page_delete_on_a_hybrid_form_gives_a_sidecar_with_no_xfa`
  (recovery.rs) and
  `an_export_without_a_hybrid_form_s_widget_page_drops_its_xfa`
  (forms_saves.rs).
- **Tiles:** `a_save_doesnt_move_the_tile_counter` (docview.rs,
  headless GPUI on Linux), through the view's `finish_save` (it had
  called `Doc::take_save` alone, where nothing ever dropped tiles), no
  tile or preview retired, by the counter or page by page; and
  `a_save_with_an_edit_since_leaves_its_sidecar_as_it_ends`.
- **The gate:** `rewrites_on_the_synthetic_and_heavy_7k` and each
  fixture's `rewrite` part assert ≤ 1.2× slice 8's time and ≤ 1.5×
  the RSS it added (ignored, release; Measured).
- **Also:** `undo_across_saves_is_exact`, both FileChanged tests,
  `save_as_makes_its_file_the_document_s`,
  `a_restored_document_s_first_save_checks_the_base`, the sidecar's
  round trip, staleness, OCR layers and
  `an_encrypted_sidecar_holds_no_plaintext`; the signed and certified
  tests in forms_saves.rs asking Rewrite or Cancel;
  `the_plan_asks_before_it_rewrites`,
  `save_policy_drops_the_rights_and_a_changed_form_s_xfa` and
  `the_post_conditions_fail_closed`, with the review's
  `copy_on_write_compares_the_primary_s_own_objects_with_their_source`,
  `the_form_post_condition_fails_closed_by_name`,
  `an_object_written_in_the_clear_doesnt_read_back` and
  `an_export_s_post_pass_is_checked_too`; the fast paths'
  `the_fast_paths_write_what_the_built_path_would` and
  `a_mark_with_a_pop_up_or_another_page_s_p_is_written_anew`, and an
  unedited older file's `/SheafPart` going; the readers'
  `an_object_at_another_entry_doesnt_shadow_the_one_the_table_lists`,
  `an_object_without_its_endobj_is_written_with_it` and an untyped
  signature's `/Contents` in the encryption round trip; a save that
  stops leaving its version unsealed (both FileChanged tests); a
  signed file asked once, not again on a save onto the file its
  Rewrite wrote (`a_save_that_would_rewrite_a_signed_file_asks`);
  numbering's
  `a_second_save_keeps_the_first_save_s_numbers`,
  `objects_numbered_past_the_last_number_are_given_new_ones`,
  `a_size_at_the_last_number_isnt_reserved` and
  `a_reference_to_nothing_stays_one`; the 2932995 logo
  (`the_2932995_logo_renders_as_it_did`, 0 of 2,037,600 bytes differ);
  and the ledger's `nothing_retired_is_back` with slice 9's 135 rows.
  Each new check the steps' notes name was run with its behavior
  switched off, failed, and passed with it back.

Left for later slices:
- **Slice 10:** Append from the saved record (in place, journaled),
  Unchanged plans writing nothing, the removal check, the full
  `FileStamp` (`startxref` window, `content_hash`), `SigImpact` and
  Append anyway for signed files, Append sidecars with `pending_tail`
  (replacing `disk_base`) and encrypted sidecars for password-protected
  documents, ObjKey IVs in an update's section, `Why::AlwaysRewrite`
  gone; the twins and the incremental gate rows come back, and the
  scanned 7k's one-mark Append is asserted < 50 ms.
- **Slice 11:** `FormInst.src` a `BlobId`, so `FormCopies`' index is
  stable across saves; the cascades that make the emitter's "a field
  shown nowhere isn't written" the model's; `Snapshot.sources` and the
  registrations.
- **Slice 12:** the O(1) `Snapshot` check.
- **Slice 13:** the overlay off `Generator` and `stamps::drawn_over`.
- **Slices 15–17:** derived pages emitted from the content stack
  (`derived_copy`, `HeaderKept`, `copy::reserve` and `copy_page_dict`'s
  derived users), the header and footer as generators
  (`stamps::base_draws`, `Why::LegacyHeaderFooter`); the version bump
  policy (16).
- **Slice 18:** OCR as blocks, not the legacy shape.
- **Slice 19:** `Session::save` for `materialize::save`, exports
  through `export::run` (`subset`, `print_pdf`, the `flatten_annots`
  post-pass, `strip_markup_annots`), `ResObjects`/`WriteCx` and the
  `Objects` seam's exchange and stamp_lib users.
- **Unbudgeted:** proposal 37's symbolic `keep_only`, which would also
  end freeing the eager graph; the sidecar's copy of its body.
