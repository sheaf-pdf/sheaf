# M10.5 slice 12: One funnel and the history budget

Status: done · Part of [M10.5: the edit API](README.md)

**One funnel and the history budget** (XL; proposals 13 and 19;
bugs 9, 25 and 26).
- **`Document::commit` with `Base` and
  `Stale::{Validate, Relower}`:**
  - value validation, and the post-merge cascade and uniqueness
    check;
  - recomputed lines and coalescing per key.
- **ChangeSets** from every transition.
- **Groups:**
  - `GroupKind::{Ocr, PluginCall}`, with amends that mint ids;
  - `GroupEnded` by ancestry, and interleaved fragments;
  - `revert_group` and History's "Undo all of …";
  - `rollback_group` that truncates or reverts.
- **`DocView::commit` and `on_change`** are the only mutation and
  the only refresh.
- **Background planners** commit with a base, concurrently with
  OCR's own run. `run_job` is one-at-a-time for file-writing jobs
  only.
- **Dialogs** resolve to ids at open and re-lower their preview on
  each ChangeSet.
- **`Snapshot`** is O(1).
- **The plugin cell** (`ArcSwap`).
- **The history budget:**
  - the byte budget prunes versions, with the detached saved anchor;
  - only saved and sealed versions are pinned;
  - older records are reduced to `SaveNote`s.
- **Deletes:**
  - `DocView::apply`/`apply_batch`/`finish_edit`/`after_model_change`;
  - the hand-written `touched()` + `after_model_change` copies in
    `insert_file`, page_insert.rs and blank_review.rs;
  - `begin_job`/`apply_in_job`/`job_is_current`/`job_is_undone`/`Version.job`;
  - `reveal_new_bookmarks`' outline diff;
  - the global relayout per edit;
  - the `NoProgress` paths in page_insert.rs;
  - the transitional job-amend path;
  - `keeps_snapshot` for every version with saves, and "the root
    never passes the saved version".
- **Checks:**
  - **The funnel:**
    - a `--script` run emits exactly one ChangeSet per transition;
    - a mark edit triggers no relayout;
    - `Snapshot` takes under 50 µs at 7k.
  - **Relower and Validate:**
    - an `insert_file` whose anchor page is deleted first gives an
      error notice and applies nothing;
    - an `insert_file` during a page move elsewhere is re-lowered,
      and both land;
    - a crop planned while the user deletes another page commits,
      and one planned while the user crops the same page conflicts;
    - a dialog based on V, then an OCR amend of V, then the dialog's
      commit keeps the amended layers;
    - a stale blank-review `DeletePages` against a concurrently
      added generator, bookmark and connector is re-lowered with the
      cascades applied;
    - a `Validate` commit whose cascades would add prims conflicts,
      and so does one that would leave two sheaf watermarks of the
      same z.
  - **Groups:**
    - undo during OCR stops the flushes;
    - OCR flushes interleaved with user edits fragment the group,
      and "Undo all" removes every OCR fragment while keeping the
      user's edit;
    - a plugin rollback with an interleaved user edit reverts only
      the plugin's versions;
    - a rollback after the plugin's version was saved keeps it, with
      a notice.
  - **The history budget:** 200 OCR versions on a 1,000-page scan
    stay within 256 MB, and a save after pruning past the saved
    version still appends correctly.

## Notes

Done. One funnel and the history budget (proposals 13 and 19; bugs
   9, 25, 26, and 27's version check): a transaction made on an earlier
   version lands on the current one, checked by value against what it
   read and wrote and then by the cascades and the uniqueness and
   reference rules (`Stale::Validate`), or lowered there again with the
   ids it drew (`Stale::Relower`), or is a Conflict naming the edit in
   the way and changes nothing. Every transition gives a `ChangeSet`,
   made from the slots it changed. An OCR run and a plugin call are
   groups of versions that undo as one step: edits between split them
   into "(continued)" fragments, an undo stops them (`GroupEnded`),
   History undoes all of one (`revert_group`), and a failed call rolls
   back (`rollback_group`). Every change reaches the app's one refresh,
   `on_change`; the user's, planners', dialogs' and plugin calls' commits
   go through `DocView::commit`, OCR's flushes commit on the model in
   their run's group (Deviations); and a mark's edit lays nothing out.
   Inserts, crop, watermark, header and footer, labels, blank review and
   comment import plan on a snapshot, off the UI thread or in their
   dialog, and land on its version. A plugin call reads the document
   from a cell that swaps at every change, and its batches land as one
   undo step on what it read. History is pruned to its 256 MB
   budget: the saved version detaches into an anchor, the first version
   kept drops its prims, and a push walks no history. The job API, the
   app's three funnels and relayout per edit, `reveal_new_bookmarks`'
   outline walk, the `NoProgress` paths and slice 11's refusal of commits
   off the cursor are deleted. 200 OCR versions of a 1,000-page scan hold
   at most 256 MB (4,254 MB of steps before); a snapshot takes 0.2 µs at
   7,000 pages; the last 100 of 1,000 mark adds take what the first 100
   do; the Rewrite is 1.11× slice 8's writer on both fixtures; the
   corpus counts hold.
   - **ChangeSets and version keys** (step 1). `change.rs`: `ChangeSet
 {from, to, cause, actor, label, keys, order, pages_added,
 pages_removed, geometry, content, annots, fields, outline, labels,
 form, generators }` and `Cause::{Opened, Commit, Amend, Undo, Redo,
 Jump, Saved, Rollback, Revert}`, re-exported from the crate's root.
 `ChangeSet::of` reads the keys only, a slot lookup or two a key
 (`Page(p)`: None→Some added, Some→None removed, Some→Some content;
 `Geom` geometry; `PageAnnots` and an `Annot`'s page in either state
 annotations; the rest their flag or list); `empty` (a no-op commit, a
 save) and `opened` (every flag, every page and field, no keys).
 `Version.keys` is its coalesced steps' prims' keys, sharing its one
 step's `effects.keys` `Arc` when it has one; `edges_between(a, b)`
 walks up from both ends until the lines meet, and `path_keys` and undo,
 redo and jump (`move_to`) take its versions' keys (`keys_along`).
 `Commit.change`; `undo`, `redo` and `jump` return
 `Result<Option<ChangeSet>, EditError>` (a jump to the current version
 `Ok(None)`); `record_save`, `record_unchanged` and `Written::record_in`
 return `Result<ChangeSet>` (`Saved`, from and to the current version,
 "Save"); `ready` returns `(ViewFacts, ChangeSet)`.
   - **Stale bases** (step 2). `Base { Current, At(VersionId) }`, `Stale {
 Validate, Relower }`, `CommitOpts { label, base, stale, group }` with
 `Default`, `labelled(label)` and (step 6) `based(v, stale, label)`;
 `Preview { lines, effects, created }` and `Tx::preview()`;
 `EditError::Conflict { key, by: Option<Arc<str>> }`;
 `Document::tx_at(snapshot, cx)`. A transaction keeps its ops as entries
 (`Entry::Op { op, drawn, positional }`, `Entry::Base` for
 `set_page_base`) and draws every id through `Lower` (`page_id`,
 `outline_id`, `field_id`, `annot_id`, `gen_id`: the next pinned first,
 then the allocator, the rng or `GenId::draw`, each recorded), where
 tx/pages.rs, tx/forms.rs, tx/bookmarks.rs and tx/content.rs drew their
 own; `outline_from_flat` and `nest` take an id-drawing closure.
 `model/stale.rs`'s `onto_cursor`, called by `settle` before it
 coalesces: a base the caller states wrongly is `Internal`; every
 decided read must hold by value (`prim::same`); `Validate` checks
 `seen` and the first before of each written key, then `merge` puts the
 steps' prims on a `Rec` over the current state (a misfit a Conflict),
 says each line again and works its effects out again, and runs the
 post-merge check (`cascade::unsettled`, one round of every rule over
 what the merge wrote; `DocState::unique_after`: one generator per kind
 and place by `GenSpec::same_place`, a source page's copy shown once,
 `/NM` per page, full names; `resolves_after`: what written slots name
 is there); `Relower` needs the base's order unchanged when an entry
 resolved a position, then lowers the entries again on a fresh
 transaction with the same context, grants, names and pins, each op's
 drawn ids pinned. `conflict()` names the newest edit on the current
 line, down to where the base's line meets it, whose keys hold the slot.
 The refusal in `settle` is deleted.
   - **Groups** (step 3). `GroupId`, `GroupKind::{Ocr, PluginCall {plugin
 }}`, `Document::{begin_group, end_group}`, live groups only in
 `Document.groups` (`Group { kind, label, latest }`), `Version.group`
 and `VersionView.group` (the id and the group's label),
 `CommitOpts.group` and `EditError::GroupEnded`. `joins()` (GroupEnded
 once the group's latest isn't the current version or before it, or the
 group ended; Internal for an actor its kind doesn't take), `amends()`
 (the current version is the group's, childless, never saved, not
 sealed, not the saved version, and has a parent), `amend()` (the old
 job amend, its change the commit's own keys, `Cause::Amend`; netting to
 nothing goes back to the parent, `latest` the nearest of the group's
 before it) and `of_group()` ("{first label} (continued)").
 `revert_group(g, label)`: the group's versions on the line
 (`group_line`), their inverse prims applied newest first on a `Rec`
 with every before checked, coalesced by `Rec::finish` into one step of
 `ResolvedOp::Revert { label }`, then `merge(steps, made, ours, theirs)`
 with `theirs` the state before the group's first version on the line,
 `conflict` skipping the group's own versions; pushed as "Undo all of
 {label}" by the user, `Cause::Revert`, and the group ends.
 `rollback_group(g)` truncates when the group's only version is the
 current one and `amends()` holds, else reverts by the group's actor
 (`Cause::Rollback`), a saved version staying; `Ok(None)` with nothing
 on the line; a Conflict leaves the group live. Amends count their new
 steps' bytes only when their keys don't meet the version's (`join`,
 `step_bytes`, `steps_bytes`, `keys_bytes`). The app's OCR run holds its
 group and the run's start snapshot: each flush is `tx_at(base)`,
 `tx.read(Key::Page(p))` per page, `SetOcrText`, committed with
 `Validate` in the group; a Conflict retries page by page and counts the
 changed pages (", N changed during the run (their text wasn't added)");
 GroupEnded cancels the run ("OCR stopped: its results were undone");
 `end_group` on Finished and Aborted. History's version rows of a group
 (not undone, not damaged) have "Undo all of {label}".
   - **The history budget** (step 4). `HistoryStats.droppable`,
 `untrimmable_bytes()`, `HistoryPolicy::{could_act, prunes}`
 (`MemoryBudget::prunes` is `untrimmable_bytes() > budget`;
 `MemoryBudget::trim` stops at `droppable`). `trim()` weighs history
 only when the policy could act, prunes one version at a time while past
 `MAX_VERSIONS` or the policy prunes (weighing again after each), then
 drops snapshots. `prune_one()` takes the oldest leaf off the line,
 never the saved or a sealed one, else `advance_root()`: never past the
 current version or a sealed root, nor while a branch off the root holds
 a sealed version; it detaches the saved version when it's the root or
 on a branch going (`SavedVersion::{At, Detached(Box<Anchor { state,
 record, keys_to_root }>)}`, a version made in memory detaching to
 None), adds each new root's keys to a detached anchor, clears a group's
 `latest` it passes, and empties the new root's prims, its bytes counted
 again. `input` and `recovery_input` give path keys of `keys_to_root` ∪
 the root-to-v keys, the root's own edge left out; `saved_at()` is the
 in-tree id, and every reader of the saved version handles the anchor.
 `Held { snapshots, snapshot_bytes, step_bytes, undone }` is kept by
 `insert_version`, `take_version` and `set_state`, through which every
 change goes; `Version.on_line` is set by `move_to` along both sides of
 the path (`sides()`); and `stats_for`/`droppable` went over the held
 snapshots only, then (step 8) became one pass, `weigh`.
   - **The app's one door and one refresh** (step 5). `DocView::lower`,
 `lower_in`, `commit` (the notice, `commit_notice`, `reveal_bookmarks`
 walking up from each made bookmark, then `on_change`) and `on_change`
 (`Doc::follow`: `Doc.changes` every change, selection and marks pruned
 only by what the change touched, `drop_stale_images` over `content ∪
 pages_added`, `Doc.layout` and the panes' layouts only on order,
 geometry or a page now shown from another source page; the region
 pruned, autosave unless Saved or Opened, `Doc.last_change`, one
 `DocEvent::Changed { change }` or `Opened`). Ready, undo, redo,
 History's `jump_to` and `undo_group`, `finish_save` (then
 `DocEvent::Saved { path }`) and OCR's flushes go through `on_change`
 (OCR commits on the model, not through `DocView::commit`: Deviations).
 `Doc.version` split into `changes` (the Comments `ViewKey`, History
 rows, the labels cache) and `layout` (pane layouts,
 `invalidate_tiles`). Sidecars are keyed by `VersionId`;
 `autosave_again` folded into `schedule_autosave`. Every caller lowers
 and commits itself (adopt, blank review, bookmark tools, the calc tape,
 comments and their import, markup tools, outline, page insert,
 patterns, set status). `dispatch_event` maps `Changed` to the plugins'
 `Edited` ("Undo: …", "Redo: …", "Go to: …" or the label; none for Saved
 or Opened), `from` the plugin when `change.actor` is it. The script's
 `dump` gains `"version"` and `"changes"`.
   - **Planners and dialogs** (step 6). `DocView::run_planner(name, work,
 status, land)` replaces `run_job_then`: any number at once
 (`DocView.planners`), the job's progress or the newest planner's in the
 status bar, Escape stopping the newest (`cancel_planner`, before
 `cancel_job`), `forget_planners` on open and recover; `run_job` only
 for Export and Split, sharing `drive` and `ended()`. page_insert.rs
 (`insert_file` moved there): `plan_insert`/`plan_replace` over
 `bring_in` off the UI thread (read the file, `ready_to_insert`, pin,
 open the engine, lower on `tx_at(snapshot, lower_cx(Job(Insert)))`)
 into a `PlannedInsert { src, handle, tx, on }`, landed by
 `DocView::land` with `Relower`, the engine and `take_in` only after the
 commit; `insert_blank` and `insert_images` through `insert_made`, the
 generated PDF built off the UI thread. Crop, Watermark and Header &
 Footer contexts carry `opened: Snapshot`, resolve indexes to ids on it
 (`ids_at`), plan the copies as a planner and land with
 `user_tx_at(opened)`, `show_copies`, the op,
 `CommitOpts::based(opened.version, Validate, ..)` and `keep_copies`
 after; Remove Header & Footer names the opened pages by id. The labels
 dialog holds `opened`, `now` and a `LowerCx`, previews through
 `Tx::preview` on `now` (`labels_op`, the free `preview()`;
 `labels::label_at` split out), follows each change through
 `Workspace::watch` (every build now) and `LabelsDialog::follow`, and
 commits on `opened` with `Validate`. Blank review's delete is
 `tx_at(planning)`, a `read(Key::Page(p))` per confirmed page,
 `DeletePages` by id, `Relower`. Comment import is a planner (`tx_at`,
 `Job(Import)`, `Relower`).
   - **Plugins** (step 7). `arc-swap` 1.9.2 (workspace dependency, resolved
 offline; sheaf-plugin's and, under `plugins`, sheaf-app's).
 `DocContext.cell: Arc<ArcSwap<Snapshot>>`; `HostState.base` (a
 `OnceCell` loaded at the call's first read or submit) and `reads`,
 reset per call; `HostState::{base, read, page_base}`; reads record
 `Order` and each page's `Page` and `Geom` (`pages()`), `PageAnnots(p)`
 and each mark shown (`annotations`), `OutlineRoots` and each bookmark
 (`outline`), `Page(p)` (text, searches, words, text boxes, `recognize`,
 `page_text_layer`, a renderer's `new`), and an update's `Annot(id)`
 (`Adapter::finish` returns the reads); `event_to_wit` loads the base
 only for a Region event; `CallOutput.{base, reads}` (a hand-written
 Debug); `EditBatch::lowered(tx, doc)`. The app: `Doc.plugin_cell`, made
 in `Doc::new` and stored by `on_change`; `plugin_tx` replaced by a
 `Door` trait (`model`, `commit`, `follow`), the view's `View` door and
 `land(door, id, version, out) -> Landed {failed, kept }` with
 `DocView::land_call`: a `PluginCall` group, the first batch on
 `tx_at(base)` with each read and `Relower` when the call read anything
 (else at the current version), later batches at the current version in
 the group, `rollback_group` on the first failure (its change to
 `follow`; `end_group` and "kept" when it conflicts). A call whose cell
 isn't the view's changed nothing ("{label}: its document was closed, so
 it changed nothing"). docs/ plugins.md and wit/sheaf.wit's 0.3 `edits`
 doc say a call is one undo step, applied to the document as it read it,
 all or nothing.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy: `begin_job`, `apply_in_job`,
 `job_is_current`, `job_is_undone`, `Version.job`, `JobId`, `next_job`,
 the fixture's `commit_ops_in_job` (now `commit_ops_in_group`) and
 `OcrRun.job`; `Document::{prune, holds_protected, remove_subtree}` and
 the root's "stop at the saved version or its branch point" rule;
 `Document::{droppable, stats_for}` (step 8's `weigh`);
 `DocView::{apply, apply_batch, submit, finish_edit, after_model_change,
 prune_selection, edit_source, edits, autosave_again}`, `Doc::touched`,
 `Doc.version`, `Had`, `reveal_new_bookmarks`, `DocEvent::Edited` and
 page_insert's `rebase` (step 5's lowering-only `rebased` went in step
 6); `jobs::run_job_then`, `DocView::{header_footer, has_page_labels}`,
 every `NoProgress` and bare `background_executor().spawn` in
 page_insert.rs; `DocContext.snapshot` and `plugins::plugin_tx`; the
 "commits are at the current one" refusal in `settle`.
   - **Ledgers** (step 8). retired.rs gains 33 slice-12 rows (the job API's
 nine; pruning's three and `droppable` and `stats_for`; the app's
 funnels and refreshes' eleven; its plugin echo and autosave three; the
 planners' and dialogs' three; the plugin snapshot's two) and
 `QUALIFIED_ONLY` nine names (`job`, `prune`, `droppable`,
 `header_footer`, `submit`, `touched`, `rebase`, `version`, `snapshot`);
 temporary probe files (in sheaf-ops, at each row's module) confirmed
 every row catches its definition and its call, path or field form, and
 the ledger's self-tests pass. `docview::DocEvent::Edited` has no row:
 sheaf-plugin's live `DocEvent::Edited` holds the same last two
 segments. The transitional ledger marks the job amends (1, 11 → 12) and
 commits only at the cursor (11 → 12) deleted, notes on the WIT adapter
 row (11 → 20) the recorded 0.2/0.3 reads, `EditBatch::lowered(tx, doc)`
 and the call's first batch committed with `Relower` in a `PluginCall`
 group, and adds `Tx::read(Key::Page(p))` for `read_render` (12 → 13)
 and OCR flushes conflicting with crops (12 → 15). Slice 20's list says
 what slice 12 did early. The gate table gains the snapshot and the flat
 pushes. No lint ban lands.
   - **Docs:** `docs/document-model.md` gains "One funnel and the history
 budget (M10.5 slice 12)" after the slice 11 section (the sketch,
 ChangeSets and version keys, stale bases, groups, the budget, the one
 door and refresh, planners and dialogs, OCR, plugins, the numbers); the
 opening, Undo/redo (the Version, amends by group, the saved version
 detaching, the path kept by the anchor, the property, the costs), the
 slice 11 section's forward references, Saving's sealing, path and
 Append closure (the connector row), the Decisions' policy and budget,
 and Text layers' coalesced undo say what changed. README: the Undo and
 History panel rows. docs/ plugins.md: a call reads at its first read or
 submit, its reads are checked when it lands, partial edits kept, a
 closed document, and the `edited` event's texts.
   - **The 7k harness** (step 8): `snapshot()`, the median of 21, asserted
 under 50 µs on every fixture (`SNAPSHOT_GATE`); the crop of every page
 but the last landed with `Validate` on the version it was planned on
 after that page's delete, beside the crop's slice-15 gate; in the
 history bench, the 7,000-op commit as an OCR group's version, an
 amend's median over 700 flushes, and an insert of 10 pages lowered
 again after a page move; flat pushes asserted in
 `mark_adds_on_the_heavy_7k` (the blank's last-100 median within `FLAT`,
 1.5×, of its first-100's); `history_of_200_ocr_versions_at_1000_pages`
 asserted since step 4 (`HISTORY_BUDGET`; `OCR_200_NEEDS` and its skip
 gone). The module doc says so.

   Deviations from the plan (the planner's list first, as resolved):
   - **Already done in slice 9:** `keeps_snapshot` for every version with
 saves (it keeps the root, the current, the saved and sealed versions)
 and older records reduced to `SaveNote`s; slice 12 extends the latter
 to the anchor, which the next save replaces.
   - **Relower resolves the ops again,** with position reads checked by
 value: an op whose resolve read the order (a position, positions, a
 span, every page, a bookmark's span, an `/NM` with no page) makes the
 transaction positional, and under `Relower` the base's order must equal
 the current one's, else `Conflict { key: Order }`. Conservative: a
 stale "rotate every page" after an insert conflicts. Each entry keeps
 its own drawn ids, so a relowered op draws its own even when an earlier
 one draws a different number now. Under `Validate` the order is in
 `seen` already.
   - **The Conflict's shape** is `Conflict { key, by }`, one slot and the
 label of the edit that changed it, where §6.4 has `Conflict { since,
 keys }`. A uniqueness or reference conflict names the other side's slot
 (the generator, annotation, node or page already there), so `by` names
 the edit that made the clash; an undo-caused one has `by: None`.
   - **The budget rule** is `untrimmable_bytes() > budget` (snapshots that
 must stay + steps + undone sources), where §8.7 has `step_bytes +
 blob_bytes`: `blob_bytes` counts every blob, the primary included, so a
 500 MB scan would have pruned all history. Slice 3's note gives the
 intent: what pruning frees. Versions are pruned first, one at a time,
 then snapshots trimmed, so the budget holds by construction unless the
 snapshots that must stay exceed it alone. `could_act` builds no stats
 when no policy could act.
   - **Anchors off the line:** the saved version also detaches when it's on
 a branch the root advance removes, so save, undo and new edits can be
 pruned; off-line leaf pruning never takes it, so a branch whose only
 leaf is the saved version stays until the root reaches its branch
 point, then goes whole. The enum is `SavedVersion`, since
 `model::Saved` is a slice-9 row. A saved version made in memory has no
 record and detaches to None (NoSaveRecord, dirty).
   - **Split counters:** `Doc.version` keyed the comments panel's and
 History's caches too, so it split into `changes` (every change) and
 `layout` (order, geometry, or a page now shown from another source
 page: `set_base` onto a same-size copy writes no `Geom` prim, and a
 layout's refs hold each page's source, so the geometry-only rule would
 keep drawing the old source). `Doc::new` fills `shown` from the model's
 pages, so the rule works from the first change.
   - **Plugin batches after the first** are committed at the current version
 in the group (`Base::Current`), joining the first's version: WIT
 0.2/0.3 batches arrive only at the end of a call, so only the first can
 be stale (§14.3's "the next Tx based on the committed version"). A call
 that read nothing commits its first batch at the current version too.
 The batches stop at the first failure: a call is all or nothing, where
 each batch landed or failed alone before.
   - **Slice 20 items done early:** docview's `edit_source` (a plugin's own
 edits told by `ChangeSet.actor`), "Reads record decision reads" for
 0.2/0.3, "built lazily from the cell" and `DocContext`'s per-call
 `Snapshot`; slice 20's list says so. Bug 27's version check, which
 slice 11 left here, is the first batch's decision reads.
   - **Watermarks aren't generators until slice 16,** so the "two watermarks
 of the same z" check is two header/footers from one base, and the rule
 is per (kind, place): a header and footer has no z.
   - **`Tx::read_render` is slice 13's:** OCR's and blank review's decision
 reads are `Tx::read(Key::Page(p))` on the snapshot their pages were
 rendered from, so a crop during OCR conflicts until slice 15 (ledger
 rows).
   - **Deferred:** `Tx::mark_handed_out`, `handed_out` and ChangeSet
 `depth`, which have no consumer until WIT 0.4 (slice 20); Relower still
 pins ids. Dialogs' rendered previews (slice 13): only the labels dialog
 has a model preview line.
   - **OCR's flushes commit on the model,** not through `DocView::commit`
 (§14.1 lists `apply_in_job` among the funnels that become one): the
 door's notice would replace the run's own each flush, and a
 page-by-page retry's conflicts (pages changed since the run began,
 left out and counted) would each be told as an error; OCR makes no
 bookmarks to reveal. Every flush's change still goes to `on_change`,
 so the one refresh holds; `DocView::commit`'s doc and
 docs/document-model.md say so. Found by the review (below).
   - **Additions:** `ChangeSet.label` and `Cause::Opened`;
 `ChangeSet::{empty, opened, is_empty}`; `CommitOpts::{labelled, based}`
 (the 13 `CommitOpts { label }` literals use `labelled`);
 `GroupKind::takes`, which makes a commit in a group by another actor
 `Internal` (so no group inserts pages, which need `Cap::User`);
 `DocView::{lower_in, forget_planners, lowering_now, undo_group}`;
 `Preview` without caps or consequences. From the review:
 `Snapshot.instance` (§8.5's "instance", a process-wide counter per
 `Document`, kept by `Tx` and `TxParts`), `Document.cut_groups`,
 `Imported.reads`, `Workspace::close_dialogs_of_another` and
 plugins.rs's `plugin_event`.
   - **Reads through `Rec`'s Deref aren't recorded** (`NextCopy`, the
 generators lookup, the cascades), so the post-merge check also scans
 page copies, and `resolves_after` (a reference check the plan didn't
 list) catches what a stale transaction's slots name that the current
 version no longer has: without it the 4,096-case property found a label
 range on a page deleted since.
   - **The `--script` Check** is the headless
 `each_transition_is_one_change` (the harness needs a display), plus the
 `"version"` and `"changes"` dumps and a recorded run.
   - **The sidecar skip key** is the `VersionId` alone: an amend mints an id
 now, so `edits` served nothing else.
   - **The cell holds a `Snapshot`, not `(Snapshot, LowerCx)`** (§14.1): a
 call's transactions are lowered with `lower_cx` of the plugin's actor
 when it lands. The cell is the `Doc`'s, one per document, and
 `land_call` checks `Arc::ptr_eq` against the view's current cell:
 `VersionId`s are counted per document, so a `Base::At(v)` from another
 document could otherwise land on one whose cursor is `v`. From the
 review the model refuses that too (`Snapshot.instance`), as a bug;
 the check keeps a call on a replaced document a plain refusal.
 `CallOutput.base` is an `Arc<Snapshot>`.
   - **Two plugin reads beyond §14.3's:** an update's `Annot(id)` (the
 0.2/0.3 patch keeps the base mark's tape rows, look and a 0.2 line's
 start arrow) and `page_text_layer`'s and a renderer's `Page(p)`. Frame
 mappings (`page_frame`, a Region's rect, an outline edit's frame)
 record nothing.
   - **Trim White** (the crop dialog's method) takes no explicit
 `read(Key::Page(p))`: `Validate` checks the before of every `Page` and
 `Geom` slot it writes, and the pages it doesn't rebase don't change.

   Found while building (each in its step's notes):
   - **Step 1:** `ChangeSet::of` looks a `Page(p)` slot up in both states
 only when `Key::Order` is among the keys (every page is in the order),
 a debug_assert checking presence;
 `a_page_s_slot_is_judged_by_whether_the_order_changed`, fault-checked;
 the 7,000-op OCR commit went from 7.45 back to 5.9 ms. `geometry` takes
 every `Geom(p)`, added and removed pages too; Saved changes go from and
 to the current version. A jump to the current version returns before
 touching `redo_to`. `Version.keys` counts in `step_bytes` unless it
 shares its step's set (4,254 → 4,257 MB on the 200-OCR bench before the
 budget).
   - **Step 2:** the op in an entry is unboxed
 (`#[allow(clippy::large_enum_variant)]`): boxed, the 7,000-op row was
 lowered 40.5 ms and committed 6.9; unboxed 39.6 and 6.31; with no
 entries 39.0 and 5.9. `TxParts` carries `base_state` and `cx`; its
 `actor` field is a method. Validate checks the first before of every
 key the steps write before coalescing, a key put back included. `/NM`
 and full-name uniqueness can't be reached by any op today (each add
 also writes `PageAnnots` or `Form`, which conflict first), so they're
 unit-tested on states built for them.
   - **Step 3:** an amend keeps its version's label (the group's first
 commit's, or "… (continued)"), and "(continued)" also follows a sealed
 or saved version (`an_amend_on_a_sealed_version_makes_a_child`,
 `a_group_never_amends_a_saved_version`). Revert and rollback end the
 group on success; on a Conflict it stays live and the caller ends it.
 Both are labelled "Undo all of {label}", a rollback's by the actor of
 the group's latest version. The amend's per-step byte count (700
 flushes went from 1,235 ms, 3.34 ms a flush at the end, to 79 ms,
 0.137): `accounting` dedups `Arc`s within a step, not a version, a
 documented over-count that leaves the 200-OCR bench unchanged. The OCR
 flush refreshes whenever the document moved, an amend that nets to
 nothing included. The History row's meta line is a flex row with the
 link.
   - **Step 4:** the cached undone bytes are cleared on every cursor move
 and whenever a version bringing sources comes or goes; per the spec,
 leaf pruning never takes the saved version.
 `pruning_keeps_the_saved_branch_whole` became
 `pruning_takes_the_saved_branch_last`. Two kinds of memory are
 uncounted by design: the values the root's state holds (only its
 marginal estimate counts) and the anchor's state; so RSS growth on the
 200-OCR bench (324–328 MB) is above the 256 MB estimate. `push` went
 from a walk of history to O(snapshots held): the last 100 adds 0.134 →
 0.040 ms.
   - **Step 5:** `on_change` schedules no autosave for `Opened` either, or a
 sidecar found at open would be replaced before the reader answers the
 Recover prompt. `refresh_history` stays for error paths: History's rows
 key on `(changes, is_dirty)`. A no-op commit and a revert with nothing
 to undo still emit one `Changed`. `DocEvent::Saved { path }` follows a
 save's `Changed`, since plugins' `saved` needs the path.
 `invalidate_tiles` bumps only `layout`. The notice's bookmark count is
 the O(1) `bookmark_count()` difference (ChangeSet has `outline: bool`
 only); `reveal_bookmarks` maps kid → parent from the commit's
 `Bookmark` keys, else `DocState::bookmark_place`.
   - **Step 6:** dialogs hand their snapshot over through `opened()` rather
 than in each event; `PlannedInsert` (not `Incoming`, a slice-11 row);
 the labels preview lowers on the UI thread at each render after a
 change (one `SetPageLabels` is cheap), "already" read from
 `Preview.effects.keys.is_empty()`; Remove Header & Footer from the
 dialog names pages by id; `forget_planners`, so a planner can't land a
 `Base::At(v)` on a recovery's document; the planner's start notice is
 its label and an ellipsis ("Insert x.pdf…").
   - **Step 7:** see the cell, the reads and the batches above; a
 pre-existing dead-code warning (`add_panel`) shows in the
 no-default-features build only.
   - **Step 8:** a push weighed history in two passes of four lookups per
 held snapshot (`droppable`, `stats_for`, `keeps_snapshot`): 20 µs at
 200 snapshots, so the blank 7k's last 100 adds took 1.8× its first 100
 (0.034 against 0.019 ms) and the new flatness gate failed. `weigh` is
 one pass with one lookup each (2 µs): 0.021 against 0.019 ms. It yields
 the same order and bytes, which `counted()` checks against an
 independent recount after every action of the history property; with
 the off-line and on-line halves swapped, the property and
 `a_run_applies_every_kind` fail.

   Measured (AMD EPYC 7302P, 32 threads, release builds; the load
   average given with each run):
   - **The 7k harness** (`gates_on_the_*_7k`, three whole runs at a load of
 1.0–3.6):

 | | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | a snapshot (median of 21) | 0.221 µs, asserted < 50 µs | 0.260–0.290 µs, asserted | 0.221 µs, asserted |
 | `Tx::check`, deleting every page by id | 0.383–0.389 ms, asserted < 1 | 0.387–0.390 ms, asserted | 0.386–0.389 ms, asserted |
 | incremental save, one mark | 22.2–22.9 ms | 41.1–42.1 ms | 22.9–23.8 ms, asserted < 50 |
 | incremental save, every page turned, a connector on each (the review's row, one run at a load of 4–35) | 708–993 ms | 928 ms | 730 ms |
 | crop all pages, the edit | 14.9–15.4 ms | 14.1–14.7 ms | 14.1–14.5 ms |
 | the same, landed with `Validate` after a delete | 29.9–32.0 ms | 31.0–32.2 ms | 26.1–28.2 ms |
 | crop all pages (slice 15: < 200 ms) | 169–174 ms | 713–714 ms | 1,056–1,129 ms |
 | geometry walk | 48.7–52.7 ms, asserted < 100 | 71.4–83.8 ms, asserted < 250 | 61.7–62.3 ms |
 | ready | 63.5–67.2 ms | 344–348 ms, asserted < 1 s | 446–536 ms |
 | Rewrite, in the fixture's process | 147–148 ms | 656–673 ms | 529–553 ms |
 | Bates apply, all pages | 150–152 ms | 204–205 ms | 76.3–76.5 ms |

 The snapshot clones the state's tables (im roots and `Arc`s) and the
 primary's, the name's, the derived table's and the file's `Arc`s; the
 Validate crop is the merge of 6,999 pages' `Page` and `Geom` prims,
 each checked by value, and the post-merge check over them, about twice
 the edit at the current version. Every gate passes. The synthetic's
 Bates apply, 150–152 ms against slice 11's 133–139 (ungated until slice
 17's 50 ms), is mostly classifying every page cold in sheaf-ops, which
 slice 12 didn't change; its commit names one generator.
   - **History** (`history_at_7k_pages_and_20k_marks`, three runs; slice
 11's in brackets): AddMark 0.018–0.019 ms [0.022], a full reorder
 0.937–0.944 ms [0.94–0.99], asserted under 1 and 2 ms. 7,000
 `SetOcrText` lowered in 35.1–35.8 ms [38.4–38.7] and committed as an
 OCR group's version in 9.53–9.74 ms [5.66–5.73]: about 4.5 ms of it is
 pruning the previous run's undone version, which the budget no longer
 keeps beside the new one (the bench shares one layer `Arc` across the
 pages, and each step counts it, about 140 MB a version). An OCR run of
 700 flushes, each made on the run's first version, reading its 10 pages
 and landed with `Validate` in the group: 78.4–86.8 ms in all, an
 amend's median 0.102–0.106 ms, the last 50's 0.122 ms (3.34 ms at the
 end before step 3's per-step byte count). An insert of 10 pages lowered
 again after a page move, its commit: 1.47–1.56 ms.
 `mark_adds_on_the_heavy_7k`: heavy 0.032–0.033 ms (the first 100 0.029,
 the last 0.027–0.028), blank pages 0.020–0.021 ms (0.019 and 0.021),
 asserted < 1 ms, ≤ 2× the blank's, and the blank's last 100 ≤ 1.5× its
 first 100; before step 8's `weigh` the blank's were 0.033, 0.019 and
 0.034 (1.8×), slice 8's last 100 0.183. Ready on 20,000 sheaf marks
 525–535 ms (< 1 s); their Rewrite 1,289–1,564 ms and 250–265 MB added.
 200 OCR versions of 1,000 pages, the same in every run: 12 versions and
 12 snapshots kept (0.198 MB), 235 MB of steps, at most 256 MB held
 after any run (asserted ≤ 256 MB), RSS growth 327 MB (slice 2: 201
 versions, 4,076 MB of steps, 4,738 MB of RSS; slice 11: 4,254 MB of
 steps).
   - **The Rewrite against slice 8's writer**
 (`rewrites_on_the_synthetic_and_heavy_7k`'s part; seven processes of
 each writer, run alternately, slice 8's from its own build's binary at
 `target/slice8-gate/`, each process's time the median of three runs;
 load 1.0–4.8):

 | | Synthetic | Heavy |
 |---|---|---|
 | slice 8's writer today | 133.0 ms (131.6–137.3) | 695 ms (586–700) |
 | `materialize`, slice 12 | 149 ms (145–152); 49.9–50.8 MB added | 782 ms (765–787); 250–254 MB added |
 | ratio to the gate's constant (134 and 707 ms) | 1.11× | 1.11× |
 | ratio to today's slice 8 median | 1.12× | 1.13× |
 | RSS added, to slice 8's 61.7 and 251 MB | 0.81× | 1.00× |

 No flag: both under 1.2× the gate's constant, the gate at 1.3×. Nothing
 on the Rewrite's path changed in slice 12 but the Append closure's
 connector row, which a Rewrite doesn't read.
   - **The corpus** (../pdf-corpus), every count as slice 11 left it:
 materialize_corpus's identity 4,389 of 4,392 written, 2,269,722 primary
 objects surviving, 919,479 copied as they were, 436 changed by
 `keep_only`, 0 failed; 2,901 forms without their first page, 0 failed
 (551 findings of the allowed kinds); every page duplicated, 4,389, 0
 failed; inserted twice into the 1040, 1,098, 0 failed; four times, 0
 differ; the 2932995 logo, 0 of 2,037,600 bytes differ. append_corpus:
 4,389 marked, 3,770 appended, 259 rewritten (211 RemovalCheck, 34
 BaseObjectsMissing, 14 BaseUnsound), 360 asked about signatures, 0
 failed, 0 derived vetoes and 211 on the files' own objects; the oracle
 finds text gone from 2,899 of 2,901 page-delete Rewrites (none from
 0016064 and 0112654). encrypted_corpus: 573 encrypted files, the
 Rewrite keeps 573 and the Append 572 (0630899 declined, its table
 rebuilt). The default tiers took 3.4 minutes (load 2–15). The ignored
 tier, deleted means deleted over every file, six features both ways, 0
 failed: the first Append declined [28, 68, 68, 68, 0, 30] (a page: 17
 BaseUnsound and 11 BaseObjectsMissing; a mark, a reply and a mark's
 text: 30 and 38; another tool's comment, 30 files: none; a bookmark: 30
 BaseUnsound), a page's content run found in 2,031 Rewrites and 2,022
 Appends (352 s in release at a load of 4–20).
   - **Suites:** `cargo test --workspace` 1,246 passed and 31 ignored after
 the review's fixes (1,237 before them) in 53
 test binaries and 13 doc-test runs (5.7 minutes, load 0.8–28; slice 11:
 1,175 and 31); fmt and clippy (`-D warnings`, all targets) are clean;
 `cargo check --target x86_64-pc-windows-msvc` passes for sheaf-doc,
 sheaf-ops, sheaf-edit and sheaf-util with their tests and for
 sheaf-render and sheaf-print (sheaf-plugin, sheaf-app and sheaf-cli
 can't be checked here: wasmtime-fiber's, ring's and onig_sys's build
 scripts need MSVC's headers). The ignored tier in release, the 7k
 harness apart (`-- --ignored --skip 7k --skip 200_ocr`, with qpdf and
 pdf.js 6.4.299 on node 24.22, and `SHEAF_FUZZ_SEEDS` set so the seed
 writer wrote op_json's 57 seeds; without it that test fails, as it
 always has), 10.6 minutes: 22 passed, 0 failed. The replay property at
 4,096 cases and Append ≡ Rewrite at 1,024 took 61 s together; interop's
 `qpdf --check` is clean on 84 Rewrite and 14 Append outputs, three
 Appends onto each of 14 bases and a delete saved and undone on each of
 14, and pdf.js reads the records file both ways (the Append half keeps
 the comment only its Rewrite half deletes); the OCR engines, PDFium's
 frames on every page and the lexer-vs-PDFium differential pass.
   - **Manual runs** (Xwayland, `--script`, f1040s1.pdf). Step 5, the change
 count rising by one a transition:

 | Step | Version | Changes | Layout | Last change |
 |---|---|---|---|---|
 | Opened | 1 | 1 | 1 | — |
 | Rotate | 2 | 2 | 2 | Commit 1→2 "Rotate page": 1 key, geometry 1 |
 | Undo | 1 | 3 | 3 | Undo 2→1 |
 | Redo | 2 | 4 | 4 | Redo 1→2 |
 | Square mark | 3 | 5 | 4 (unchanged) | Commit 2→3 "Rect": 2 keys, annots 1 |
 | Undo ×2 | 1 | 7 | 5 | — |

 Step 6: Insert PDF ("Inserted 1 page from one.pdf…", Commit 1→2 by
 `Job(Insert)`), Insert Blank Pages (2→3), Crop by Trim ("Cropped 1
 page", 3→4 by the user, geometry 1), Watermark "DRAFT" ("Watermark: 4
 pages", 4→5, content 4), Header & Footer "Page <<1>>" (5→6, generators)
 and Page Labels (6→7, labels) each landed as one version; Find Blank
 Pages found none on the watermarked pages, and on a fresh document with
 a blank page inserted, "1 page to review", whose Delete gave "Delete 1
 blank page — Ctrl+Z brings them back" (2→3, 3→2 pages); an XFDF import
 gave "Imported from notes.xfdf: 1 added, 0 updated" (1→2 by
 `Job(Import)`, annots 1, the layout count unchanged).
   - **The review:** the plan was reviewed before the steps started (its
 findings are the planner's list above, as resolved); no review step ran
 within the slice's eight steps. Each step checked its new checks by
 switching their behavior off (the fault checks above). The review of
 the whole slice found 13 findings (two pairs the same), each checked
 against the code:
 - *OCR flushes bypass `DocView::commit`* (two findings): true, and
   the summary and step 5's note claimed otherwise; kept, recorded
   under Deviations, the claims corrected.
 - *`Snapshot` has no instance* (two findings): true, and a dialog's
   `opened` or blank review's `planning` surviving a recovery could
   land the replaced document's state on the new one whose cursor had
   the same number. Fixed as §8.5 says: `Snapshot.instance`, kept by
   the transaction, and `onto_cursor` refuses another document's as
   `Internal` (`a_transaction_made_on_another_document_is_a_bug`);
   `forget_planners` also drops blank review's pages, and the
   workspace closes the snapshot dialogs of a replaced document on
   `Opened`. The cell's `ptr_eq` and `forget_planners` stay as the
   app's own guards.
 - *The 0.3 WIT doc and plugins.md overstate "none of them stay" and
   "changes elsewhere don't matter"*: true; both now say a rollback's
   edits stay once the user changed them, and that listing the pages
   reads their order.
 - *"Undo all of …" takes back part of a group once pruning took its
   first fragment* (major): true, reproduced as described. Fixed with
   `Document.cut_groups` (a group whose version or first-kept prims
   pruning took): `revert` and `rollback_group` refuse it as
   `Invalid` ("older than the history kept"), and its rows offer no
   link. The history property's `REVERT` and `ROLLBACK` now check a
   landed one against every version the run saw made, pruned ones
   included (`Run.ever`, `taken_back`); with the check put back as it
   was, the 4,096-case run fails, and the same check then found a
   second path: `rollback_group`'s truncate of the only version left
   on the line, after pruning took the earlier one, also took back
   only part (shrunk to GROUP, two ops, PRUNE, GROUP, ROLLBACK),
   now refused too. `a_revert_of_a_group_pruning_cut_is_invalid` and
   `a_rollback_of_a_group_pruning_cut_changes_nothing`; eight 4,096-case
   runs pass.
 - *`connectors_on` makes the Append closure quadratic* (two findings):
   true, and worse: with the gate row the review asked for (a
   connector pair on every two pages of a 7k fixture, one group, every
   page turned, then the incremental save), the closure overflowed the
   stack, since each connector taken took its group again,
   recursing as deep as the group is long. `Deps` now takes a group
   once (`connected`, `every_connector`), which writes the same keys:
   the corpus counts, Append ≡ Rewrite at 1,024 cases and the DELETED
   tier hold. `append_rewrite_a_long_connector_group_is_taken_once`
   (2,000 connectors on a 1 MiB stack) aborts without it in debug and
   release; the gate row is informational (708–928 ms).
 - *The comment import decides without reads*: true; two imports of
   one file planned together both added its comments. `import` now
   says what it decided by (`Imported.reads`: the order, each
   destination page's `Geom` and `PageAnnots`, each comment updated)
   and `ops_in` reads them;
   `imports_planned_together_conflict_over_what_they_matched_by`
   fails without the reads.
 - *The labels preview says "already" where Apply conflicts*: true
   (the old test asserted it). The preview now says "Page labels:
   changed since this dialog opened" once the labels slot differs from
   `opened`'s, which is the only slot the op writes and so what
   `Validate` checks; `the_preview_follows_the_document` covers it.
 - *No positive test of `ChangeSet::of`'s labels, form, field and
   generator mapping*: true;
   `labels_generators_and_fields_are_named` (commit and undo of each),
   which each of the four mutations fails.
 - *No test of `Doc::prune`, `dispatch_event`'s actor rule or
   `edited_label`*: true; `what_a_change_removes_leaves_the_selection`
   (headless: an undone add, a mark added elsewhere, a page deleted;
   fails with either prune half off), `an_edit_is_told_by_its_label`
   and `a_plugin_s_own_edit_names_it` (the mapping moved into
   `plugin_event` to be testable).
 - *`merge` checks each key's first before twice*: true; the first
   loop went, `written` is built in the second.

   Found:
   - **A connector's next after a page turn** (step 3, pre-existing): the
 1,024-case Append ≡ Rewrite property, reaching new paths through
 `GROUP`'s draws, found an Append that differed from the Rewrite after a
 turn of a page holding part of a connector group (a group's order is
 how its connectors show on their pages); it reproduced with the
 connector committed as a plain user op. `deps::closure` re-emits every
 connector of each group with one on p for `Geom(p)`
 (`Deps::connectors_on`; the dependency table says so);
 `append_rewrite_a_connector_s_next_follows_a_turned_page` fails without
 it.
   - **A push weighed history at four lookups a snapshot** (step 8, above),
 fixed with `weigh`.
   - **From the review** (above): a transaction from a replaced document
 could land; "Undo all of …" and a rollback took back part of a group
 pruning had cut; the Append closure recursed as deep as a connector
 group is long (a stack overflow at 7,000 connectors); two comment
 imports planned together both added their comments.
   - **Proptest seeds:** `history_tests.txt`'s two seeds decoded to other
 cases once the alphabet changed and were deleted, their failures
 covered by `a_batch_keeps_each_slot_s_change_on_its_last_step` (slice
 1's coalesce fault) and `a_page_listed_twice_is_replaced_once` (slice
 8); append_rewrite's `6dfbb569` was deleted in step 4 when `PRUNE`
 joined its alphabet, its shrunk case
 `append_rewrite_a_reply_made_editable_then_undone_removes_nothing`; the
 connector seed (`d0c9d3e4`) has its named test; every seed a fault run
 wrote was deleted, the review's included (its partial-rollback seed
 `1e94cd3a` is `a_rollback_of_a_group_pruning_cut_changes_nothing`).
 Both regression files are deleted, and no seed is committed.

   Checked by tests:
   - **The funnel:** one ChangeSet per transition,
 `each_transition_is_one_change` (headless, Linux: ready gives only
 `Opened`; a commit, a group commit, an amend, undo, redo, a jump each
 way, `finish_save` and "Undo all" one `Changed` each with its cause;
 from step 7 the plugin cell holds the current version after each), and
 the `--script` runs below; a mark edit lays nothing out,
 `a_mark_edit_lays_nothing_out` (the layout counter and the pane's
 layout unchanged by an AddMark while `changes` moves and the Comments
 view has the mark; a rotate and a same-size rebase lay out again); a
 snapshot under 50 µs, `gates_on_the_*_7k`.
   - **Relower and Validate:** an insert whose anchor page went,
 `a_relowered_insert_before_a_page_deleted_since_is_not_found` and, over
 the planner's plan and land,
 `an_insert_whose_anchor_went_says_so_and_changes_nothing`; an insert
 during a move,
 `a_relowered_insert_lands_beside_a_move_with_the_ids_it_drew`
 (`created` equal to the first lowering's),
 `a_relowered_insert_keeps_every_id_it_drew` (a form, marks, other
 tools' comments, an outline and a header) and
 `an_insert_during_a_move_lands_both`; a crop beside a delete lands and
 against a crop of its page conflicts,
 `a_crop_lands_beside_another_page_s_delete_and_conflicts_with_its_own_page_s_crop`;
 a dialog on V after an OCR amend of V keeps the layers,
 `a_dialog_on_v_keeps_an_ocr_amend_of_v`, and the labels dialog's
 `labels_land_on_the_version_their_dialog_opened_on` and
 `the_preview_follows_the_document`; a stale blank-review delete,
 `a_stale_delete_is_relowered_with_the_cascades_of_what_came_since`
 (`page.generators`, `page.bookmarks`, `annot.partner`) and
 `blank_review_deletes_what_its_search_measured`; a Validate commit the
 cascades would add to,
 `a_validated_delete_of_a_page_given_a_connector_since_conflicts`, and
 two header/footers from one base,
 `a_second_header_footer_from_one_base_conflicts`, both failing with the
 post-merge check removed. Beside them:
 `what_names_a_page_deleted_since_conflicts`,
 `two_pages_shown_as_one_copy_of_a_page_conflict`,
 `a_name_given_twice_by_a_merge_breaks_uniqueness_unless_it_was_so`,
 `a_changed_decision_read_conflicts_either_way`,
 `a_conflict_by_an_undo_names_no_edit`,
 `a_change_put_back_since_is_no_conflict`,
 `a_base_pruned_since_still_lands`,
 `a_relowered_position_conflicts_once_the_order_moved`,
 `a_preview_says_what_its_commit_does`,
 `a_merged_step_is_said_against_the_document_it_lands_on` and
 `a_transaction_on_another_version_said_to_be_current_is_a_bug`;
 `a_stopped_planner_lands_nothing`.
   - **Groups:** `undo_during_ocr_stops_its_flushes`;
 `ocr_around_a_user_edit_is_fragments_undone_all_at_once` (History's
 link calls `revert_group`);
 `a_plugin_rollback_reverts_only_its_versions` and, through the app's
 loop, `a_failing_second_batch_takes_the_first_back` and
 `a_call_whose_edits_were_written_over_keeps_them_when_it_fails`;
 `a_rollback_after_a_save_keeps_the_saved_version`. Beside them:
 `a_rollback_of_a_call_s_only_version_takes_it_out`,
 `a_revert_over_a_later_edit_names_it`,
 `a_revert_names_the_edit_between_its_fragments`,
 `a_revert_that_would_leave_a_dangling_bookmark_conflicts`,
 `a_revert_of_what_isn_t_here_is_invalid`,
 `pruning_a_group_s_latest_ends_it_or_leaves_it_none` (both prune paths)
 and `a_commit_in_a_group_by_another_actor_is_a_bug`; the plugin calls'
 `a_call_on_a_page_changed_since_conflicts_and_changes_nothing`,
 `a_call_lands_after_an_edit_to_a_page_it_didn_t_read`,
 `a_call_s_two_batches_on_a_page_it_read_are_one_undo_step`,
 `a_call_lands_only_on_the_document_it_read`, and sheaf-plugin's
 `a_call_s_reads_are_the_slots_it_decided_by` and
 `a_call_s_base_is_the_document_at_its_first_read`.
   - **The history budget:** `history_of_200_ocr_versions_at_1000_pages`
 (the gate) and `two_hundred_ocr_versions_keep_to_a_budget` (scaled,
 default run); a save after pruning past the saved version,
 `append_rewrite_a_save_after_the_root_passes_the_saved_version_appends`
 (path keys equal the union of the edits' ChangeSet keys) and
 `append_rewrite_a_saved_version_on_an_undone_branch_detaches` (the
 budget asserted after every run), and `PRUNE` in the Append ≡ Rewrite
 property; `pruning_takes_the_saved_branch_last`,
 `the_root_keeps_lines_not_prims`, the ported
 `pruning_an_undone_400_page_scan_frees_its_bytes` and
 `pruning_keeps_a_version_being_saved`, and history.rs's policy unit
 tests (`could_act == false` never trims or prunes).
   - **The properties:** history_tests' new actions `STALE` (a transaction
 made on a version still kept, sometimes reading a page, committed with
 either policy; a failure changes nothing and is never `Internal` or
 `Mismatch`), `GROUP` (OCR flushes made on the group's first version
 with `Validate`, or a plugin's at the cursor), `REVERT`, `ROLLBACK` and
 `PRUNE` (a random budget below what history holds); after every action
 `counted()` (the held totals, snapshots, line flags, cached undone
 bytes and, from step 8, the weighed order against a recount) and the
 path-keys check (the anchor's keys included); from the review, a
 landed `REVERT` or `ROLLBACK` puts back every slot any version of the
 group on the line wrote, pruned ones included, and changes no other
 (`taken_back`, against `Run.ever`); `check()` asserts every
 version's keys are its prims' (the root's prims empty), its steps
 coalesced and its byte counts a recount's. A temporary count over the
 release runs: about 1,800 stale merges of both policies and every kind
 of conflict; 1,547 amends, 296 fragments, 80 GroupEnded; reverts 326
 landed, 450 conflicts, 104 Invalid; rollbacks 68 truncated, 232
 reverted, 90 with nothing, 429 conflicts; 22,691 root advances, 3,157
 leaf removals, 2,081 detaches at the root and 65 on a branch, 765 with
 a record among the 1,024 Append ≡ Rewrite cases.
   - **Also:** change_tests' ten (a commit's keys are its prims' and its
 version's `Arc`; a mark moved to another page names both; inserts and
 deletes; geometry and content by the job's actor; undo, redo and a jump
 across a branch name the keys to where the lines meet; the order
 shortcut; saves; a no-op; an amend's keys; from the review, labels, a
 header and a form's page, each committed and undone);
 `new_bookmarks_are_revealed` (a new heading over a moved bookmark);
 `ready_replaces_the_opened_version` (the Opened change). From the
 review, the tests named in its findings above.
   - **Fault checks** (each step's, restored from its own copy and checked
 with `cmp`): the turning version in `edges_between`; the order shortcut
 forced; the post-merge check, `resolves_after`, the page-copy scan, the
 `/NM` and full-name rules and each kind of pin removed; the amend's `-
 was` and the fast path forced on overlapping keys; the anchor not
 growing, detaching without the new root's edge, `on_line` not set on
 the joined side, the root keeping its prims, snapshot bytes not counted
 when set; relayout on every change, Opened emitted for Saved, a made
 heading's kids or the walk past one parent, `|| rebased`, `shown` not
 filled; the cell's `ptr_eq`; `weigh`'s halves swapped. The review's:
 `cut_groups` never filled, then the revert's check put back as it was
 (`first == root`), and the rollback's check off; the connector memo
 off (the stack overflows); the import's reads off; the labels
 preview's check off; each of `ChangeSet::of`'s labels, form, field and
 generator arms; either half of `Doc::prune`. Each failed the tests
 named, and passed restored.

   For Greg:
   - **Plugin calls are all or nothing now:** a call's batches land as one
 undo step on the document as the call read it; a change by the user to
 anything it read (the pages it listed, a page's annotations, text or
 render, the outline) refuses the whole call, with "“X” changed the same
 data"; a later batch's failure takes the earlier ones back.
 docs/plugins.md and the 0.3 WIT doc say so; wit/0.2's doc, frozen until
 slice 20 deletes it, still says a batch each.
   - **What users see change:** an insert, crop, watermark, header and
 footer, labels change, blank-page delete or comment import planned
 while they edit lands on their edits, or says what it conflicts with
 and changes nothing; several can run at once, and Escape stops the
 newest; OCR results for a page cropped during the run aren't added ("N
 changed during the run"), until slice 15; undoing OCR stops it; History
 has "Undo all of …" for an OCR run or a plugin command, whose parts
 after an edit say "(continued)", until pruning takes part of it
 (then neither the link nor a failed call's rollback can take it all
 back, and a failed call says its edits were kept); long sessions
 keep within 256 MB, the oldest versions going (the History panel
 shows fewer of a long session's versions); a mark edit no longer
 lays the pages out again.
   - **The History panel's "Undo all of …" link and the labels preview line
 weren't checked by eye:** the screenshots grabbed stale frames. The
 headless tests and the `--script` dumps cover what they do.
   - **No manual run of a plugin call in the app:** its loop is tested at
 the Document level (`impl Door for Document`) and once through the
 view; the notices made inside `run`'s closure aren't.

   Left for later slices:
   - **Slice 13:** `Tx::read_render` in place of `Tx::read(Key::Page(p))`
 (OCR flushes, blank review, plugins' text and render reads); render
 keys from `ChangeSet.content`; dialogs' rendered previews, on a
 background executor with a generation token.
   - **Slice 15:** crop as geometry (`SetPageBoxes`), so an OCR flush no
 longer conflicts with a crop of its page.
   - **Slice 16:** watermarks as generators, under the per-(kind, z)
 uniqueness rule.
   - **Slice 19:** Combine without its write and reopen.
   - **Slice 20:** `mark_handed_out` and `created` equality for Relower,
 ChangeSet `depth`, 0.4's reads and events (merged coalescing, scopes
 from ChangeSet, undo-triggered edits refused), and the lazy Tx's
 semantics for 0.4 (`commit()` mid-call, the next Tx on the version it
 committed).
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed: an
 unrecorded `/Annots` entry's `/Parent`, `/IRT` and `/Popup`); slice 12
 didn't touch that walk.
