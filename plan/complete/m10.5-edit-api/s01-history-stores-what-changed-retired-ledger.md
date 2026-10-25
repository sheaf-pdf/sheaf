# M10.5 slice 1: History stores what changed; the retired ledger

Status: done · Part of [M10.5: the edit API](README.md)

**History stores what changed; the retired ledger** (L; proposals 12
and 13; bugs 6–9).
- `Prim::Put { key, before, after }` over today's DocState fields, as
  transitional keys: Pages, Outline, AnnotsOf(page), TextLayer(page),
  RemovedOf(page), Labels, HeaderFooter, Field(id), WidgetsOf(page).
- `compute` writes through a recording context, and `Step` becomes
  `{ edit, line, prims }`.
- Checked replay: a `Mismatch` shows as "History is damaged at
  version N".
- One `IdAlloc`, never rewound.
- A no-op is an empty prim list after coalescing per key.
- describe runs at apply, against the pre-edit state, and is stored.
- **VersionIds name immutable states:** `apply_in_job`'s amend
  replaces the cursor version with a new id under the same parent,
  and so does `rebase_to_saved`, which lives until slice 9.
- `snapshot_bytes` and `step_bytes` replace the 256-byte guesses.
- `tests/retired.rs`, matching qualified paths (`syn` over `crates/`,
  `vendor/` excluded).
- **Deletes:**
  - `Ids { next_page, next_outline, next_field, seed }`, the reseed
    per step, and `replay_step` with its max-merge;
  - the replay-failed warn + `None` branches in undo, redo and jump;
  - deep DocState equality as no-op detection;
  - describe-against-parent and `VersionView`'s count fallback;
  - `marginal`'s guesses (model.rs:1798–1814);
  - the in-place amend.
- **Checks:**
  - A proptest over all 21 `Edit` kinds, plus batches, jobs,
    branches, undo, redo and jump, and random trims at a snapshot
    keep-rate of 0: every version rebuilt from prims equals the
    original under a test-only deep `Eq`, with identical ids.
  - One corrupted before-value yields `Mismatch`.
  - The batch "move pages 3–4 before 1, then delete page 4" describes
    the right page numbers.
  - Each kind's no-op pushes nothing, and a batch x → y → x pushes
    nothing.
  - An OCR amend gives the cursor a new VersionId, and the old id no
    longer resolves.
  - A bench on 7,000 pages with 20,000 marks: AnnotAdd under 1 ms,
    and a full reorder under 2 ms.
  - The ledger test passes.

## Notes

Done. History stores what each edit changed, as prims with the values
they replaced, and rebuilds a trimmed version from them, checked,
without running an edit again; the retired-symbols ledger starts.
- **`prim.rs`:** `Key` (the nine transitional keys), `Val`
  (`Field(Box<Field>)`), `Prim { key, before, after }` and
  `Mismatch { key }`. `same()` is `ptr_eq` (or `Arc::ptr_eq`) first,
  then `==`. `DocState::get` and `DocState::set` are one exhaustive
  match on `Key` each, and `DocState::apply` checks the before-value
  and leaves the state alone on a mismatch.
- **`Rec`, the recording context:** reads through `Deref`, writes only
  through `put`, and `finish()` gives the prims in key order, without a
  slot that ended as it started. `compute(&self, rec, edit)` and its
  helpers (`take_in_fields`, `copy_fields`, `bring_fields`,
  `import_marks`) write only through it. `Step` is
  `{ edit, line, prims: Arc<[Prim]> }`.
- **Coalescing** (`coalesce`), at a batch's commit and at an amend:
  each key's net prim, from the first before to the last after, goes on
  the last step that wrote the key, and a prim that nets to nothing is
  dropped. A no-op is an empty prim list: `apply`, `apply_batch` and
  `apply_in_job` push only with prims, and the three whole-state
  comparisons are gone. `DocState` derives `PartialEq` only under
  `cfg(test)`.
- **Checked replay:** `replay(id)` starts from the nearest ancestor
  that kept a state and applies every step's prims on the way, oldest
  first. A `Mismatch` is logged with its key and becomes
  `Damaged { version }` ("History is damaged at version N").
  `ensure_state` marks the version damaged (`Version.damaged`,
  `VersionView.damaged`), logs, and panics in debug builds. `undo` and
  `redo` return `Result<Option<&str>, Damaged>` (`Ok(None)`: nothing to
  undo or redo); `jump` passes `Damaged` up. The History panel shows
  the error as a damaged version's meta line, in `theme::ERROR`, and
  the app's undo, redo and jump say so in a notice.
- **One `IdAlloc`** (`page`, `outline`, `field` as `AtomicU64`,
  `fields_from` by `fetch_max`) replaces `rng`, `next_page`,
  `next_outline`, `next_field`, `Ids` and the reseed per step. Mark ids
  come from `fastrand` below 2^53. A replay draws nothing.
- **Stored lines:** `Document::step` runs `describe(&edit, &state)`
  against the state before the edit, and `view()` reads `Step.line`.
  describe's `Option<&DocState>` and its count fallback for a trimmed
  parent are gone; `pages_text` makes one pass over the pages.
- **Immutable versions:** `replace(old, v)` mints a new id for `v` and
  puts it in `old`'s place: among the parent's children and in its
  `redo_to`, as its children's parent, and as the root, cursor or saved
  version. `apply_in_job`'s amend and `rebase_to_saved` both go through
  it. An amend that nets to nothing removes the version and the cursor
  goes back to the parent.
- **Accounting:** `Version.snapshot_bytes` and `step_bytes`, computed at
  push, at amend and at save by `accounting(steps, state)`.
  `step_bytes` is each step's `size_of::<Step>()` with its edit's and
  line's heap, plus each prim's `size_of::<Prim>()` and its
  after-value's heap, an `Arc` (text layer, header/footer) once per
  version by pointer. `snapshot_bytes` is `size_of::<DocState>()` plus,
  for each per-page or per-field map the version changed,
  `path_copies(touched, len, fanout)`: at depth l, min(touched,
  fanout^l) nodes of fanout entries, so k slots never count more than
  the map. Whole-value keys add nothing (their values are the steps').
  `HeapSize` (prim.rs) estimates what a value holds beyond its
  `size_of`, with exhaustive matches over `Val`, `Edit` and `AnnotKind`.
  `HistoryStats` has `snapshot_bytes: Vec<usize>` and
  `step_bytes: usize`; `MemoryBudget` drops snapshots against the total
  but subtracts only snapshot bytes. `marginal` is gone.
- **The page splice** (`splice`: one `split_off`, an `extend`, one
  `append`) replaces the per-page `Vector::insert` loops in MovePages,
  InsertPages, ReplacePages' extra pages and OutlineMove's span.
- **The ledger** (`crates/sheaf-doc/tests/retired.rs`): `RETIRED` rows
  (a path from the crate root, and the slice) and `QUALIFIED_ONLY`
  (the plan's `Mode`, `Step`, `PageRef`, `LabelSpec`, `Widget`, and
  `ids`, `rng`, `next_page`, `next_outline`, `next_field`). Every `.rs`
  under `crates/` but not under a `target` is parsed with `syn`; a
  file's module comes from its path below its package (the nearest
  `Cargo.toml`), and inline `mod`s nest. A row fails on a definition
  at its path (an item; an impl's or trait's item, a field or a
  variant as `Type::name`); on a path, in code or a `use`, that holds
  the row's last two segments in a row after a leading `crate`,
  `self`, `super` or `sheaf_*`; and, unless the last segment is
  qualified-only, on that name as a method, a call's callee, a type, a
  struct literal's or pattern's path, or a field (accessed, in a
  literal or in a pattern). A local variable is never a hit. Macro
  bodies count where they parse as comma-separated expressions. Each
  hit prints as `file:line: row (retired in slice n)`. Slice 1's rows:
  `model::Ids`, `model::Step::ids`, `model::Document::{replay_step,
  rng, next_page, next_outline, next_field, fresh_page_id,
  fresh_outline_id, marginal}`, `model::Version::marginal_bytes` and
  `history::HistoryStats::marginal_bytes`.
- **Docs:** `docs/document-model.md`'s "Undo/redo" section describes
  versions, steps and prims, no-ops, checked replay, immutable
  VersionIds, `IdAlloc`, stored lines and the save pins, with the
  costs measured below; the Document sketch is a tree of versions with
  a `VersionId` cursor (bug 30); the Decisions bullet on
  `HistoryStats` and the Text layers "Coalesced undo" bullet are
  current.

Deviations from the plan:
- **The ledger is `crates/sheaf-doc/tests/retired.rs`,** sheaf-doc's
  first integration test, walking `CARGO_MANIFEST_DIR/../../crates`:
  the root `Cargo.toml` is a virtual manifest, so a root `tests/`
  builds nothing. Slice 2's `tests/gate_7k.rs` has the same problem.
  It needs `proc-macro2`'s `span-locations` beside `syn` (both
  dev-dependencies) for the hits' lines.
- **The ledger matches a run of the row's last two segments anywhere
  in a path,** not only at its end, so `incremental::Mode::Append`
  hits the row `incremental::Mode` (slice 9's). Trait items, pattern
  field names and macro bodies are matched too; the plan names none of
  them, and without them `assert_eq!(d.marginal(), ..)` or
  `let Version { marginal_bytes, .. }` would pass.
- **The ledger reads an `include!`d file as the module of its own
  path:** sheaf-plugin's `api/shared.rs`, `v02.rs` and `v03.rs` are
  `api::shared` and so on, not the modules that include them. Slice
  20's rows for them must use those paths.
- **`Ids`'s fields have no rows of their own:** they can't be defined
  without `model::Ids`, which has one. No other named symbol went in
  steps 1–6; the in-place amend, describe's `Option` parameter and the
  parent lookup in `view()` were code inside functions.
- **The bench needed fixes the slice's bullets don't list:** `splice`
  (the reorder was 262 ms, 189 ms of it the per-page insert loop) and
  a one-pass `pages_text` (describe of 3,500 ids was 24 ms, an
  `index_of` per id), required once describe runs at apply.
  `pages_text` keeps a sorted, deduplicated Vec and `binary_search`
  rather than a HashSet: faster at every size measured at 7,000 pages
  (one id: 35–48 µs against 142–196 µs; 3,500 ids: 172–175 µs against
  276–281 µs). A list naming a page twice now reads as its pages; the
  count stays for an unknown id (bug 11), more than 4 runs and an
  empty list, and counts each page once.
- **Prims are stored per step, coalesced across the version:** §8.1
  keeps prims on the Step and §6.2 coalesces at commit, so each key's
  net prim goes on the last step that wrote it. Every step keeps its
  edit and line, with or without prims. Each key is then in one prim
  per version, so the order of replay doesn't matter, and a 7,000-op
  batch stores one Pages value, not 7,000. Coalescing runs at amend
  too (§6.2), so a job's version can net to nothing.
- **`Mismatch { key }` and `Damaged { version }`:** `DocState::apply`
  returns the first, as §6.2 has it; the history-level error carries
  the version and becomes `EditError::Mismatch { version }` in slice
  11. `DocState::set` returns `Result<(), Mismatch>` too, so the check
  that a value fits its key is in the one match; `Rec::put` treats a
  misfit as a bug in `compute` and panics.
- **Five kinds have no no-op:** OutlineInsert, OutlineDelete,
  AnnotAdd, AnnotDelete and ConnectorAdd always make or remove
  something, or fail. "Each kind's no-op pushes nothing" covers the
  other 16; the x → y → x batches (rotate, labels, header, move) are in
  `a_batch_that_puts_things_back_pushes_nothing`.
- **`undo` and `redo` return `Result<Option<&str>, Damaged>`;** the
  `Option<ChangeSet>` is slice 12's. Tests that undid to check
  something was undone still check it (`.unwrap().unwrap()`, or
  `Ok(Some(..))`).
- **`IdAlloc` has atomics but no `Arc`:** nothing else owns it until
  slice 11's `LowerCx`. `compute` takes `&self`.
- **A Field prim's values are copies** (`Val::Field(Box<Field>)`):
  `DocState.fields` holds fields by value until slice 8. Boxed, or
  clippy's `large_enum_variant` fires. Its snapshot copy is counted in
  `snapshot_bytes` too.
- **`rebase_to_saved` gets a new id under the same parent,** and its
  steps' prims still lead to the state before the save, so it's never
  replayed: `keeps_snapshot` pins every version with saves, and (a fix
  the plan doesn't list) the children of one. A child made from the
  cursor before a save has prims whose before-values are the pre-save
  state, so undo, save, then redo mismatched once replay was checked
  (`replay_step` used to recompute another state silently, bug 6).
  `rebase_to_saved` now rebuilds the cursor's children's states before
  re-expressing it. A save at the opened version replaces the root's
  id. The property test saves too; only its fold from the root skips a
  version with a save on its path, since that fold mismatches across a
  save.
- **`set_initial_*` (through `opened_state_mut`) still change the
  opened version's state in place** before the first edit: the one
  exception to immutable versions until slice 4 deletes them.
- **Two parts of §8.4's amend rule come now:** the guard gains
  `saves.is_empty()` (a replaced id mustn't keep save notes for content
  it doesn't have; `saved != cursor` alone allowed it) and
  `parent.is_some()` (found in review: pruning past `MAX_VERSIONS` can
  make the job's version the root, as after 999 edits, a save at the
  tip, a jump back to the opened version and an OCR batch; the next
  batch then panicked on the missing parent, in release too), and an
  amend that nets to nothing removes the version. The parent becomes
  the cursor without a replay (`p.state.get_or_insert(state)`, the
  state just computed, equal to the parent's by value), and its
  `redo_to` is cleared if it named the removed version. A no-op job
  batch no longer relabels the version or adds a step to it.
- **An amend moves `unlogged_sources` into the new version** (found
  here): an amend left a source `add_source` registered for the next
  version, though `add_source` says the next version records it.
- **`CommentsDelete` writes `RemovedOf(page)` only when a name is new**
  (found here): names that were all deleted already, or none, put
  `Some(empty)` over `None`, pushing a version and marking the page
  dirty.
- **Accounting beyond the plan's formula:** `path_copies` shares the
  nodes near the root (min(touched, fanout^l) a level, not touched ×
  depth), or an OCR job over 1,000 pages would count about 2.3 MB
  against a map of about 25 KB; fanout is im's own, 32 for HashMap and
  64 for OrdMap (`fields`), not 32 for all. `rebase_to_saved`
  recomputes its version's `snapshot_bytes`, since the pages and
  fields it re-expresses are new allocations no prim shares.
  `Edit::SetTextLayers` counts only its Vec's buffer (its Arcs are its
  prims'), a small under-count when a later step of the job takes that
  prim back. `AnnotKind` has 25 variants, not 26.
- **A whole-value prim over-counts** (found in review, documented, not
  fixed): a Pages, Outline or Labels prim counts its whole value,
  walking it, though im shares all but the changed path with the
  parent's, so `step_bytes` isn't the plan's "uniquely retained" bytes
  for those keys: 168,740 bytes for a one-page rotate at 7,000 pages,
  375,627 for renaming one bookmark of 2,000. About 690 such renames
  put the steps alone over 256 MB, and `MemoryBudget`, which can only
  subtract snapshot bytes, then drops every snapshot it may on each
  trim (undo still works, replaying from the nearest kept one).
  Counting the changed chunks would be transitional code for keys
  slice 8 deletes, so `docs/document-model.md` and `accounting` say
  so instead.
- **`HistoryStats.undone_source_bytes` stays 0** (slice 3), and
  `Edit` payloads don't count source bytes.
- **The bench is an ignored sheaf-doc test,**
  `history_at_7k_pages_and_20k_marks`, run with `--release`: the gate
  harness comes in slice 2. Medians over 21 AnnotAdds and 5 reorders.
- **The app:** `DocView::refresh_history()` clears the History panel's
  row cache on a damaged undo, redo or jump, since its key
  `(version, dirty)` doesn't change when a version is found damaged. A
  failed jump says "Can't go there: History is damaged at version N".
  The panel keys `expanded` and `open_branches` by VersionId, so an
  expanded row collapses when an amend or a save replaces its version;
  accepted.
- **Tests replaced:** `replaying_the_log_rebuilds_every_version` (7
  kinds) by the property test, and `noop_edit_adds_no_version` by the
  no-op table's RotatePages row. `marginal_counts_layer_bytes` became
  `history_counts_the_layer_a_step_holds`.
- **The property test's runs:** a job action sends 1–3 batches with
  one job (a new one 1 time in 4), half of them SetTextLayers or
  ClearTextLayers, so 64 runs give about 69 amends and 2 jobs that net
  to nothing. A save action (found in review: the save path changed
  most and had one fixed scenario) calls `rebase_to_saved` and records
  the version it makes; versions whose path from the root has a save
  skip the fold from the root but not the jump, and the "snapshots
  were dropped" check needs a run without saves. Inserted and
  replacing ranges are at most 4 pages: a saved file has all the
  document's pages, and inserting all of them doubled it each time (a
  2,600-action run reached 3.5 million pages). Taking out
  `rebase_to_saved`'s rebuild of the cursor's children fails the 4,096
  cases (not always the 64); taking out either save pin in
  `keeps_snapshot` fails the 64.
  `prims_rebuild_every_version_past_max_versions` is one seeded run of
  2,600 actions with no jumps (with them, pruning only ever takes
  branches) that moves the root forward past `MAX_VERSIONS`, with
  saves on the way. Each run seeds `fastrand` (mark ids), and the
  builder never iterates im's HashMaps, so a failure reproduces.
  `a_run_applies_every_kind` counts a kind only when its edit made a
  version. Beyond the plan, the runs check that verification's replays
  and jumps draw no ids (the `IdAlloc` counters are unchanged), and
  each jump follows a `trim()`, so it's a real replay. Making
  `coalesce` keep each step's own before-value fails the 64 cases (a
  seed for it is in `proptest-regressions/model/history_tests.txt`;
  it still fails it with the save action, and shrinks to one batch).
- CI needed no change: `cargo test --workspace` runs the ledger.

Measured (release, 7,000 pages, 20,000 marks, `default_policy`):
- **AnnotAdd:** 1.1 ms before, 0.69–0.70 ms after (median; limit
  1 ms). `marginal`'s walk over the marks was 0.34 ms of it, and
  `find_annot`'s 20k scan in `fresh_annot_id`, about 0.57 ms, is the
  floor until slice 8.
- **A full reorder** (MovePages of the second half before page 1):
  262 ms before, 0.95–1.02 ms after (limit 2 ms), 0.2 ms of it
  describe.
- **describe of 3,500 ids:** 24 ms before, 0.17 ms after.
  `timeline()`: 72 ms over 22 versions before, 27 µs over 63 after.
- **One AnnotAdd at 2,000 pages and 20,000 marks** counts 3,484
  step bytes and 7,224 snapshot bytes; the 256-byte guesses counted
  about 5 MB.
- **The replay property:** 64 cases in 0.05–0.08 s (dev profile,
  0.05 s release); 4,096 cases in 2.2 s (release); the 2,600-action
  run in 0.7–0.8 s.
- **The ledger** walks 227 files in 0.77 s (dev profile).

Checked by tests:
- every version rebuilt from prims equals the original under the
  test-only deep `Eq`, with identical ids: `prims_rebuild_every_version`
  (64 cases; `model/history_tests.rs`) and the ignored
  `prims_rebuild_every_version_4096`, over the 21 kinds, batches,
  `apply_in_job`, undo, redo, jump, saves and trims under
  `FixedCount(1)`, each recorded version checked by a fold from the
  root (with no save on the way) and by `trim()` then `jump`;
  `a_run_applies_every_kind`;
  `prims_rebuild_every_version_past_max_versions`;
- a corrupted before-value: `a_corrupted_before_value_is_a_mismatch`
  (`Damaged` from replay, `Mismatch { key: Pages }` from apply) and
  `undo_over_damaged_history_says_so`, in both profiles (found in
  review: it was `should_panic` in debug, so CI checked nothing after
  the panic): the undo panics in debug and is `Err(Damaged)` in
  release, and either way the cursor is unmoved, only that version is
  shown damaged, a replay of it is refused even with its prims put
  right, and so is a jump;
- the batch's lines:
  `a_batch_describes_each_edit_against_the_document_before_it`
  (["Move pages 3, 4", "Delete page 4"]; the old code said "Delete
  page 2") and `a_trimmed_version_keeps_its_lines`;
- no-ops: `each_kind_s_no_op_pushes_nothing` (17 rows over 16 kinds,
  `CommentsDelete` with names already deleted and with none),
  `a_batch_that_puts_things_back_pushes_nothing`,
  `a_batch_keeps_each_slot_s_change_on_its_last_step`;
- immutable versions: `an_ocr_amend_gives_the_cursor_a_new_version`,
  `a_save_gives_the_cursor_a_new_version`,
  `a_job_that_nets_to_nothing_leaves_no_version`,
  `a_save_keeps_what_was_undone_before_it`;
- jobs: `a_job_s_amend_takes_its_sources_and_its_no_op_changes_nothing`
  (both found here, untested until review) and
  `a_job_s_version_pruned_to_the_first_is_not_amended` (the review's
  repro, through real pruning);
- describe: `pages_read_as_numbers_and_runs` with a page named twice,
  alone ("page 3") and beside an unknown id ("2 pages");
- accounting: `history_counts_the_layer_a_step_holds`,
  `an_added_mark_costs_its_page_not_the_document`,
  `memory_budget_counts_steps_but_drops_only_snapshots`,
  `heap_size_counts_buffers_and_what_they_hold`;
- the bench: the ignored `history_at_7k_pages_and_20k_marks`;
- the ledger: `nothing_retired_is_back`,
  `the_ledger_matches_definitions_calls_and_paths` (a definition, an
  impl item, a field, a bare method call, one in `assert_eq!`, a `use`
  and a variant path hit; a bare qualified-only type and field, a
  local named like a row and a nested module's namesake don't),
  `a_file_s_module_comes_from_its_path`. Run over model.rs and
  history.rs as they were before the slice (`44a2e6c`), it finds every
  slice 1 row.

Left for later slices:
- **Slice 2:** fold `history_at_7k_pages_and_20k_marks` into
  `tests/gate_7k.rs` (in a package, as above). This slice touches a
  gated path, but the harness doesn't exist yet.
- **Slice 3:** a real `undone_source_bytes`, and source bytes in step
  accounting.
- **Slice 4:** delete `set_initial_*` and `opened_state_mut`, the
  in-place change to the opened version.
- **Slice 8:** Arc'd records and an O(1) `find_annot` (AnnotAdd's
  remaining 0.57 ms); delete the nine keys, and rewrite `Val`'s
  `HeapSize`, `accounting`'s match on `Key` and `path_copies` against
  the records' maps (the exhaustive matches fail to compile until
  then). Field prims stop being copies. OutlineMove's `rest_state`
  literal and prim.rs's test `DocState` literal go with the old
  `DocState`. The whole-value over-count narrows to `Order`,
  `OutlineRoots` and `Labels`; `docs/document-model.md` says so.
- **Slice 9:** delete `rebase_to_saved`, and with it the save pins in
  `keeps_snapshot` and the snapshot recount at save.
- **Slice 11:** `ResolvedOp`, and `EditError::Mismatch { version }`
  for `Damaged`; `IdAlloc` in an `Arc` for `LowerCx`; one line for a
  batch of 7,000 ops of one kind (§8.3; each op's line walks the pages
  once today, 35–50 µs). history_tests.rs's `kind()`, its per-kind
  builder, the no-op table and the five kinds without one follow the
  edit set.
- **Slice 12:** drop the root's prims and prune by `step_bytes`,
  knowing an `Order`, `OutlineRoots` or `Labels` prim counts its whole
  value (im's sharing with the parent unmodelled); keep a per-step
  byte count if re-accounting every step at amend (about 2 ms per OCR
  amend at 7,000 pages) matters; the rest of the amend rule (sealed
  versions) and `Option<ChangeSet>` from undo and redo.
