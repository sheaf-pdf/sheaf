# M5 slice 2: Documents with no file yet

Status: done · Part of [M5: Scan & OCR](../../m05-scan/README.md)

**Documents with no file yet** (proposal 2, ask 1; B11).
- **Builds:** `DocView` with no path ("Untitled scan 1", "Untitled
  scan 2"); Save is Save As there; closing asks; the recovery sidecar
  in the state folder as a Full sidecar, offered at the next launch;
  Create from Scans streams its pages into an untitled document in a
  new or empty window, as they're cleaned.
  - As built: the sidecar is a kind of its own, `untitled`, the whole
    document as Full's is, since a Full sidecar's head names a base
    file (proposal 2, and the slice notes).
- **Deletes:** `create_from_scans`' save-and-`open_paths` (B11), the
  dialog's ask-for-a-path step.
- **Checks** (headless app tests, Linux only, as the GPUI tests are):
  - Create from Scans of three images: an untitled window, three
    pages, one undo step, no file anywhere; Save As writes the file,
    and the window takes its name and path.
  - A process killed after the first batch of a 20-image scan: the
    next launch offers the untitled document's recovery with those
    pages.
  - Closing an untitled document with pages asks; one with none
    doesn't.
  - B11 moves to Closed in the backlog, with this slice.

## Notes

Done. Documents with no file yet (proposal 2, ask 1; B11): Create
from Scans no longer writes a file and opens it again. The dialog hands
its images and choices to the window (`ScanEvent::Scan`), which shows a
new untitled document ("Untitled scan 1", "Untitled scan 2": the first
number no view in any sheaf claims and no sidecar has) in the current window
when it's empty, else in a new one, and the pages stream in as they're
cleaned: batches of `Batching::DEFAULT` in one `GroupKind::Scan` group,
one undo step, each planned on the intake's thread on a version the
view hands it and committed on the UI thread with `Base::At` and
`Stale::Relower`. Save on an untitled document is Save As, suggesting
"<name>.pdf" in the images' folder, and afterwards the window takes the
file's name and path. Closing asks only when it's dirty, which means it
has pages. Its crash-recovery sidecar is a third kind of format 2,
`untitled` (its Save As folder and the whole document), in
`state_dir()/recovery/`, claimed by name while its view lives, its
write started as each batch lands (or as the write in flight ends) and
removed when nothing is unsaved; the process that owns sheaf's state
offers each one no live view holds at the next launch. The dialog's
ask-for-a-path step, its own scan run and the save-and-`open_paths`
hand-off are gone. Every asserted gate holds, the corpus counts are
the baseline's, and B11 is in the backlog's Closed.
- **Groundwork** (step 1). sheaf-doc recovery.rs: the `untitled` kind
  (`sheaf-recovery 2\nuntitled\n` and the whole document), written by
  `write_untitled(input, sidecar, p) -> Result<usize>` (its target
  `same_file: false` with no stamp, through `durable::write`, the
  folder made first) through a private `whole(input, target, p)`
  (`forced_rewrite` plus `materialize`) that Full's arm of `write`
  shares; `untitled_path(dir, name)` (`<dir>/<name>.sheaf-recovery`);
  `pub struct Untitled { sidecar, name, bytes }`; `find_untitled(dir)`
  (sorted by name; a `*.sheaf-recovery` not of the kind removed;
  unreadable files, `.sheaf-tmp` temps and other files left; no folder,
  none); `restore_untitled(store, rec)` (`BlobKind::recovered(None)`)
  and `discard_untitled(sidecar)`; `parse` beside a file still refuses
  the kind (a comment on its arm); the module doc. model.rs:
  `Document::never_saved()` (`saved = None`), and the `saved` field's
  doc corrected (a memory-made document opens saved with no record;
  None is `never_saved`, or a memory document after pruning took its
  saved version). sheaf-ops: `Session::recovered(&Untitled, p)` (a new
  store, `restore_untitled`, `open::finish`, `open::start` under the
  sidecar's name, `ready` keeping its ViewFacts, then `never_saved`);
  intake.rs' `batch_source(store, pages, fit) -> Result<Src>` and
  `apply_batch(tx, src, words, engine, p)` (the insert at the end, then
  each page's `SetOcrText`, with the count check) over a pub(crate)
  `insert_at_end(src, name)`, which `SessionLanding::land` and images.rs
  use; `made_pages` deleted; scan.rs' `pub fn engine(opts, ocr)`, which
  `scan()` uses. Tests: recovery.rs' `an_untitled_sidecar_round_trips`,
  `find_untitled_removes_what_it_can_t_read`,
  `find_beside_a_file_takes_no_untitled_sidecar`; tests/intake.rs'
  `an_intake_s_untitled_sidecar_comes_back_as_a_session`.
  docs/document-model.md's recovery entry, Images to PDF, the scan
  entry and Sessions.
- **The view's file behind accessors** (step 2, no change in
  behavior). docview.rs: the field private (it was `pub(crate)`), read
  outside the open, recovery and save code through `file_name()`,
  `path() -> Option<&Path>`, `folder()` and `stem()` (`stem_of`), and
  `title()` through `file_name()`; their readers in export.rs, jobs.rs,
  comments_panel.rs, attachments_panel.rs and workspace.rs' `shows`
  switched. properties_dialog.rs' `PropertiesContext.path` an `Option`,
  with a `name` for File when there's no path (Location "Not saved
  yet"). sheaf-plugin's `DocContext.path` an `Option` (WIT's
  `document.path()` was already `option<string>`; its doc and
  docs/plugins.md now say so). Test: host.rs'
  `plugin_sees_the_call_context` with a document of no file
  (`path=None`).
- **Create from Scans streams into an untitled document** (step 3,
  B11). untitled.rs (new): `Untitled { name, dir, sidecar }`,
  `next(kind, cx)` over a pure `first_free`, and `for_scan(files, cx)`
  (the first image's folder for Save As, the sidecar at
  `untitled_path(dirs::recovery_dir(), name)`). sheaf-ops dirs.rs:
  `recovery_dir()`. docview.rs: `pub(crate) enum Place { File,
  Untitled }` in place of the field (`unopened(path, ..)` keeps its
  signature over a private `placed`); the accessors by place;
  `open`, `recover` and `offer_recovery` for a file only, and
  `recent::add` from `install` and `adopt` for a file only; `save()`
  returning `Task<bool>`, `save_to` for a file and `save_as()` (now
  resolving to whether it saved) for an untitled document;
  `finish_save` making the place the file; `adopt`/`adopting` taking a
  `Place`; `DocView::untitled(u, who, scale, timing, launched, cx)`,
  which reads no global; a `scan: Option<ScanRun>` field, with
  `cancel_scan` in Escape's chain after `cancel_ocr` and
  `scan_progress` for the status bar. scan_run.rs (new):
  `DocView::scan_untitled` (the workspace's and the tests' way in) and
  `start_scan(open, opts, ocr, batching, cx)`: `begin_group(Scan)`, the
  intake through `drive` (`open()`, `scan::engine`, `intake` with a
  `ViewLanding`), each batch's source made by `batch_source`, planned
  on the intake's thread on the version and the scan job's lowering
  the view hands it (`Request::Base`: `tx_at`, `pin`, `apply_batch`),
  and committed by the view (`Request::Commit`, `land_batch`) with
  `Base::At(on)`, `Stale::Relower` and the group; the group ended and
  `scan` cleared whatever ends `drive`; the notices "Scanned N pages
  (k left out as blank) — Ctrl+Z undoes", "Every page came out blank",
  "Scan stopped: its pages were undone" and `jobs::ended("Scan")`, and
  "Scanning page k of N — Esc stops" while it runs. scan_dialog.rs:
  its save-path step, `run()`, `running`, `status`, `error`, the bar,
  the Stop states and their imports deleted; the button "Create".
  workspace.rs: the handler's `show_untitled_scan(origin, files, opts,
  cx)` (shaped like `show_made`) through `Workspace::scan_untitled`,
  its `open_paths` gone. The comment summary of an untitled document
  is titled with its name. Tests (headless, Linux only):
  `create_from_scans_streams_into_an_untitled_document`,
  `closing_an_untitled_document_asks_only_with_pages`,
  `undoing_a_scan_in_the_view_stops_it`; untitled.rs'
  `untitled_names_take_the_first_free_number` (every platform);
  stale_tests.rs'
  `a_relowered_insert_at_the_end_lands_beside_an_insert_a_delete_and_a_move`.
  README's "Create PDF from images" row, docs/document-model.md's
  Create from Scans paragraph, scan.rs' module doc.
- **Untitled recovery** (step 4). docview.rs: a
  `sidecar_unwanted: Arc<AtomicBool>` shared with the writes;
  `write_recovery` (now pub(crate)) through
  `write_untitled_recovery(sidecar, cx)` for an untitled place (nothing
  once unwanted; discarded and `sidecar_of` cleared when clean;
  rescheduled through autosave while a save or a write runs; skipped
  when `sidecar_of` is the current version; else written in the
  background with the `sidecar_writing` guard, the write removing what
  it wrote when the flag is set by then, and the completion keeping it
  only while the place is still that sidecar, the flag clear and the
  document dirty, removing it and calling `write_recovery` again
  otherwise); `discard_recovery`'s untitled arm setting the flag first;
  `show_session` (`adopting`'s shared part); `recovered_untitled(rec,
  ..)` (place `Untitled { name, dir: PathBuf::new(), sidecar }`,
  `Session::recovered` in the background, "Recovered unsaved pages —
  Ctrl+S to save them", `State::Failed("… (its recovery data is
  kept)")` on failure); `placed`'s `cx.on_release` calling `released()`,
  which discards a ready, clean untitled document's sidecar;
  `test_support`'s `sidecar_writing`, `sidecar_of`,
  `sidecar_unwanted`. scan_run.rs: `land_batch` calls `write_recovery`
  after `on_change`. untitled.rs: `next` skips a name a sidecar in
  `recovery_dir()` has (`kept`). workspace.rs:
  `offer_untitled_recoveries(first, cx)` (nothing unless
  `instance::owns_state()`; `find_untitled` in the background, then
  "Recover the unsaved scan \"<name>\"?", Recover or Discard, one at a
  time in `first`), `show_recovered` (an empty window, else a new one,
  through `Workspace::recovered_untitled`); main.rs calls it after the
  launch windows open, but not in a `--script` run. Tests:
  `a_batch_s_sidecar_is_written_as_it_lands`,
  `a_scan_killed_after_its_first_batch_is_offered_at_the_next_launch`,
  `an_untitled_document_s_sidecar_follows_it`; the create test and the
  names test extended. docs/document-model.md's untitled recovery,
  README's Crash recovery paragraph.
- **Wrap-up** (step 5). gate_7k.rs: after each profile's row of
  `scan_of_500_black_and_white_pages`, outside `with_mem`, the untitled
  sidecar of the document scanned written once (timed, then removed)
  and its size, and what sidecars written as each batch lands would
  write in all (`sidecars_in_all`: the sum over batches of the
  document's size so far, its pages' share of the whole sidecar), in
  the command line's batches of 8 and in the app's at the row's pace
  (`app_batch`: as many pages as come within `Batching::DEFAULT`'s 2 s,
  up to 8); then the same for `COLOR_SAMPLE` (8) pages in color under
  the scanner profile (JPEG at the intake's quality of 80), scaled to
  500 pages; no gate on either; `scan_into`, the landing both use;
  the module doc and bench/README.md's harness paragraph. The backlog:
  B11 to the bottom of Closed, F14's Closed line with `4481573`. Docs:
  README's Configuration (`recovery/`), docs/document-model.md's dirty
  tracking (`never_saved`) and the app's saves (`Place`, Save As, the
  window taking the name), dirs.rs' doc (`recovery/` isn't safe to
  delete), this plan's finding 2, slice 2's Builds and the Risks'
  untitled documents, as built.
- **Deleted,** confirmed dead by `cargo check --workspace
  --all-targets` and clippy, the names in retired.rs' `RETIRED_M5`
  with comments: `intake::made_pages` (`batch_source` and
  `apply_batch`); `scan_dialog::ScanEvent::Created` (`ScanEvent::Scan`),
  "Created" in `QUALIFIED_ONLY` for Combine's `CombineEvent::Created`;
  the dialog's `file_dialog::save` step and its `run()` of
  `sheaf_ops::scan::scan` with `running`, `status`, `error`, the bar
  and the Stop states (`scan::scan` stays, the command line's); the
  workspace's `open_paths` of the scan's file; the `DocView.path`
  field (`place`). `rg` finds `made_pages` and `ScanEvent::Created`
  only in retired.rs, no `scan::scan`, `file_dialog::save`, `Tracker`
  or `error` field in scan_dialog.rs, and no `path:` field in
  `DocView`.

Deviations from the plan (the planner's list first, as resolved):
- **The sidecar is a kind of its own:** the plan said "a Full
  sidecar", but Full's head names a base file (its length, stamp and
  flags), and an untitled document has none. `untitled` keeps format 2
  and writes the body as Full's is; `find` beside a file never takes
  one (it's removed as malformed there, Found). Proposal 2 and slice
  2's Builds say so now. Added beyond the step list:
  `recovery::untitled_path`, so the naming `find_untitled` parses back
  is in one place, and `write_untitled` making its folder, so
  `recovery/` needn't exist before the first batch.
- **`never_saved`:** model.rs said `saved` is None for a document made
  in memory, but `Document::loading` sets it to the first version and
  `detach` to None when pruning takes a version with no record. A
  restored untitled document needs it None, so `never_saved()`; the
  field's doc is corrected. Revert to saved exists only in the docs
  (`cursor = saved`); every match on `self.saved` reads None as no
  saved side: `saved_at` None, `input()` no saved side (so a Rewrite,
  `NoSaveRecord`), `saved_record`/`file` None, pruning's `_ => {}`,
  `attach` setting `At(v)`. On a file-backed document it would leave
  the saved version's record unused; it's for memory-made documents
  only. A long-edited untitled document reads dirty after pruning,
  which is harmless.
- **A recovered untitled document opens as a file does** (review): step
  1's `Session::recovered` (every page's facts read, then `ready`,
  before anything showed) is gone, never committed. `recovered_untitled`
  reads the body in the background (`restore_untitled`), then shows it
  through `show_opening` (`install`'s shared part, taking who and the
  scale, since `install` reads `prefs::get` and `state::get`, globals no
  headless test sets): its pages first, `finish_open` after them, and
  `ready` calls `never_saved` for an untitled place, with "Recovered
  unsaved pages — Ctrl+S to save them". While it's still reading,
  `released()` and `write_untitled_recovery` leave its sidecar alone.
  tests/intake.rs' test opens the sidecar the same way at sheaf-doc's
  level (renamed `an_intake_s_untitled_sidecar_comes_back`).
- **`batch_source` and `apply_batch` in place of `plan_made`:**
  `plan_made` (page_insert.rs) makes the insert alone, while a batch
  needs its `SetOcrText`s in the same transaction. The two are shared
  by `SessionLanding`, images.rs and the app; `plan_made` isn't used.
  `apply_batch` names its insert "scan".
- **Planned on the intake's thread:** the plan had each batch planned
  in a background task the UI starts. Planning in a second background
  task needs a free executor thread while the intake's blocks on the
  reply, and GPUI starts max(2, cores) threads, so with as many scans
  running as threads none would be free. The landing asks the view for
  the version and the lowering (`Request::Base`), plans there, and
  sends the planned Tx back (`Request::Commit`); the view commits on
  the UI thread as the plan said. It plans with `NoProgress`, so a
  cancel never cuts a batch's landing short (`SessionLanding` passes
  its progress). Proposal 1 says so.
- **No Conflict re-plan:** the plan kept a bounded re-plan if an end
  insert read the order. It doesn't: the new sheaf-doc test commits an
  end insert planned before a user's insert, delete and move with
  `Relower`, and it lands beside each; an insert that recorded an
  order read makes it a Conflict (its fault check). So no edit of the
  user's conflicts with a batch, and none is planned again.
- **Checks at the view:** headless GPUI opens no windows and has no
  Workspace test, so "an untitled window" is checked at the view the
  workspace builds, through the same constructor and naming code
  (`untitled::for_scan`, `DocView::scan_untitled`); the window choice,
  the prompts and the real picker are hand checks. The create test
  runs in a process of its own (`alone`, through `test_support`) with
  `SHEAF_CONFIG_DIR` and `SHEAF_SCRIPT_SAVE` in a `sheaf-` temporary
  directory, so `save()` itself runs as Save As, and no test touches
  the user's recent.json or state folder.
- **"No file anywhere":** read as nothing written beside the images
  and nothing where it's saved until Save As, and (from step 4) only
  its sidecar in the recovery folder, none after.
- **The sidecar at landing, not by autosave:** autosave writes 3 s
  after the last change, or at once once the changes began 30 s ago, so
  batches every 2 s would keep putting it off. `land_batch` starts the
  write as each batch lands, and `a_batch_s_sidecar_is_written_as_it_lands`
  checks it by state in the same update (`sidecar_writing`, or
  `sidecar_of` the current version), not by a deadline. A batch that
  lands while a write runs (review) sets `sidecar_due`
  (`write_recovery_at_once`), and the write's completion starts the
  next at once, not on autosave's 3 s timer, which each landing put
  off again: a crash loses at most the pages landed since the last
  write finished, which the module doc, `land_batch`'s and README's
  Crash recovery now say (they said none). Only the untitled
  completion does so; a file's sidecar has no landing yet (slice 7,
  Left).
- **The dialog's run moved to the window:** the dialog ran
  `sheaf_ops::scan::scan` itself after its save step, with `running`,
  `status`, the bar, the Stop states and `error` (whose only setters
  were in `run()`). All of it went; the view's job shows the progress
  and the outcome, and the dialog closes once it emits
  `ScanEvent::Scan`. `ScanRun` keeps the task answering the intake
  (`_answers`): dropped with the view, it leaves the intake's question
  unanswered, which stops it ("the document was closed").
- **No retired row for `DocView::path`:** the ledger treats `impl
  DocView { fn path() }` as a definition of `DocView::path`, so a row
  for the old field would fail against the new accessor. The field is
  private and gone; the name lives on as the accessor.
- **The field private, not `pub(crate)`,** so the compiler, not only
  `rg`, keeps other modules off it. `offer_recovery` still reads it
  (its Discard drops the file's sidecar; the offer half of recover).
  The dialogs' suggestions now come from `stem()` (`stem_of`), so a
  file whose stem isn't UTF-8 gets "document - pages.pdf" and the like
  where it got none. `locked_card` takes the name, worked out before
  `render` borrows the state.
- **The owner-only offer:** `main.rs` reaches `app.run` in Standalone
  processes too (another sheaf running, `--new-instance`), so the
  launch offer runs only when `instance::owns_state()` (`workspace::
  offered(dir, owns)`), leaving the state folder, discards included,
  to the owner. Every process writes its own untitled documents'
  sidecars there, a Standalone one's too, so a crash never loses their
  pages; claims keep them apart (Names are claimed, below), and
  instance.rs' doc says so. The plan's risk
  note was wrong in part: `Mode::Separate` (debug builds, the dev
  flags) owns the state when no other sheaf is running (sheaf-instance
  lib.rs:191 goes standalone only beside a running one), so a debug
  build run alone does offer them.
- **The offer in the first window,** one sidecar at a time, and not
  over a `--script` run's window, whose keys a prompt would take (those
  wait for the next unscripted launch). A recovered document opens in
  an empty window or a new one, its Save As starting in the images'
  folder, which the sidecar's head keeps (review: a third head line,
  the path as UTF-8 hex, empty when it isn't UTF-8); an untitled
  document with no folder starts Save As and the exports in the home
  folder (`dirs::home`, now pub), not the process's working one.
- **Orphan sidecars,** beyond the step list: the view's release
  (`cx.on_release`) discards a ready, clean untitled document's
  sidecar, so a scan undone to nothing and closed while a write runs
  leaves none; and a write's completion removes what it wrote when the
  document became clean meanwhile. A dirty view is always asked about
  before it closes, and a recovered view still loading (or failed)
  keeps its sidecar.
- **Names are claimed** (review; it was "names skip sidecars", by
  open views' names and the sidecars on disk): `recovery::Claim`, a
  lock (`File::try_lock`: flock, LockFileEx, so per open file, in this
  process too) on `recovery/.<name>.sheaf-lock`, taken and let go under
  the folder's `.lock` so a lock file is removed (as its claim is let
  go) only while no one else has it open. `untitled::next(kind)` gives
  the first name it can claim that has no sidecar once claimed (no
  window is read, so none may be being updated), the claim kept in
  `Untitled.claim` (an `Arc`, cloned into each write in flight, so a
  closed view's last write holds the name until it's done) and None,
  with a warning, when the folder can't take claims (no sidecar
  then). `write_untitled` and `discard_untitled` take the claim, not a
  path; `find_untitled` claims what it returns, so a live document's
  sidecar, in this sheaf or another, is never offered, discarded or
  recovered twice, and two processes never share a name. The lock
  files stay only while their names are held (or after a crash, until
  the next `find_untitled`); `.lock` stays.
- **The model goes by the place's name** (review; it was
  `Session::new_empty`'s "Untitled" as planned, while a recovered one
  went by its sidecar's name): `Session::new_empty_named(name)`, which
  `new_empty` calls with "Untitled", so a header's `<<file>>`
  (`lower_cx`) says "Untitled scan 1" before a crash and after its
  recovery alike.
- **`DocContext.path` and `PropertiesContext.path` as `Option`s,** a
  public struct of sheaf-plugin the host tests build (seven literals
  in tests/host.rs, none in plugins.rs); `PropertiesContext` gained a
  `name` for File with no path.
- **Step order:** README's row and Crash recovery paragraph and
  document-model.md's paragraphs changed in the steps that made them
  untrue (3 and 4), not in the wrap-up.
- **Test helpers:** `ALONE`, `alone` and `shown` moved into
  `#[cfg(all(test, target_os = "linux"))] pub(crate) mod test_support`
  in docview.rs rather than making docview's `tests` pub(crate):
  `nonlinux-clippy.sh` switches off Linux-only tests only inside
  `#[cfg(test)]\nmod tests {`, and a `pub(crate) mod tests` fell
  outside it. scan_run.rs' test module is gated whole.
- **The kill test's child** calls `DocView::untitled` plus `start_scan`
  with the gated pages (`Gated::new(&files, 8)`), since `scan_untitled`
  takes files only and is exactly those two calls over `Files`; the
  parent's `find_untitled` runs before its headless app, and the
  pages are compared by their image XObjects' bytes in the saved
  files (lopdf), against a `SessionLanding` intake of the same 8
  images.
- **The gate rows:** the untitled sidecar is measured once a profile,
  outside `with_mem`, and what per-batch sidecars write in all is
  computed (each the whole sidecar's share by pages), not written 63
  or 500 times over. The color figure comes from an 8-page sample
  scaled to 500 pages. Each row's Session now outlives its
  measurement for the sidecar write, so the pages/s row no longer
  counts the Session's drop (microseconds against minutes).

Found while building:
- **An untitled sidecar is never encrypted:** a memory-made primary has
  none, so pages inserted from an encrypted PDF into an untitled
  document sit in the clear in the state folder, as a Save As would
  write them.
- **`find` beside a file removes an untitled sidecar** as malformed, so
  opening a file named `state_dir()/recovery/Untitled scan 1` (no
  extension) would delete that recovery. An extreme edge case.
- **A file's sidecar can outlive an undo to its saved version** (before
  this slice): in `write_recovery`'s file path, the clean-document
  discard runs before the `sidecar_writing` check, and the completion
  keeps a sidecar while its base is unchanged, so a write taking over
  3 s that lands after the document was undone to its saved version
  (or closed clean) stays, and the next open offers undone edits. The
  untitled path checks dirtiness when a write completes.
- **`find` beside a file reads the whole sidecar** at open and holds
  it until its prompt is answered (before this slice); `find_untitled`
  now reads heads only (review), so only a file's large Full sidecar
  still costs that.
- **Scratch folders in /tmp:** sheaf-doc's model.rs tests
  (`sheaf-model-<pid>-<n>`) and rewrite_tests (`sheaf-save-…`) leave
  theirs on every run: 7,041 `sheaf-model-*` and 38,367 `sheaf-*`
  entries at the wrap-up.
- **`fuzz_bodies_survive_mutated_fixtures` asserts 60 s of wall time,**
  which a loaded machine misses (8 s alone).
- **A stray doc comment** in workspace.rs (before this slice): `///
  Document > Recognize Text (OCR)…: open the options card…` sits above
  `open_recent`'s own.
- **Not new:** `contrib.rs`' `add_panel` dead under
  `--no-default-features` is backlog B6.
- **Tooling, not sheaf:** `nonlinux-clippy.sh` recognizes only
  `#[cfg(test)]\nmod tests {`, so a `pub(crate) mod tests` of
  Linux-only tests would fall outside its simulation (Deviations); and
  sheaf-plugin can't be checked for Windows here (wasmtime's fiber and
  ring build C with `lib.exe`).

What review found and fixed: the slice plan was reviewed adversarially
before the steps started, and its findings are the deviations above,
as resolved (the planner's list). Each step checked its new checks by
switching their behavior off (Checked by tests). The wrap-up read the
app's landing, the untitled sidecar's writes and the launch offer again
against the plan and found nothing to fix in the code; the plan's text
was behind it in finding 2, slice 2's Builds and the Risks' untitled
documents, now as built. The slice's review (20 findings) then found,
all fixed but one rejected in part:
- **Untitled sidecars were tied to their documents by name alone**
  (findings 1, 6, 7, 15, and 2): a Standalone sheaf wrote into the
  owner's `recovery/` with names only its own windows reserved, so two
  processes could share a sidecar and remove each other's, a
  relaunched owner could offer (and Discard) a live Standalone's, and
  a closed view's write still in flight could write over and remove a
  new document's of the same name; and `next` skipped a window being
  updated without a word. Now names are claimed (Deviations, "Names
  are claimed"): the claim, not the window list, says a name is
  taken, so `next` reads no window at all. Standalone processes keep
  their sidecars (a crash never loses their pages) and the owner alone
  offers and discards. Finding 15's "unique file names" option wasn't
  needed: the lock gives the same isolation and keeps the names.
- **The crash-safety wording overstated it** (3, 13): a batch landing
  during a write waited for autosave, so the docs' "loses none" was
  wrong. The next write now starts as the last ends, and the docs say
  a crash loses at most the pages since the last finished write.
- **`<<file>>` said "Untitled" before a crash and "Untitled scan 1"
  after it** (4): both go by the place's name now.
- **No test ran the launch offer** (5): its choice is `offered(dir,
  owns)`, which the kill test runs on both sides of the kill and for a
  non-owner; the prompt, Recover's window choice and main.rs' call stay
  hand checks (headless GPUI has no windows to prompt in).
- **Untitled sidecars were read whole at launch and held through every
  prompt** (8, 11, 18), with a second copy: `find_untitled` reads heads
  only, and `restore_untitled` reads the body once Recover is chosen,
  straight into the shared buffer (`sheaf_util::read_shared`, which
  `read_file_and_metadata` now uses too), no second copy.
- **Each sidecar write held the document twice** (9, 12): the head and
  body go out in turn through `durable::write_parts`, which `write`
  now calls with one part; Full's arm too, its bytes step 1's
  baseline's (cmp).
- **A recovered document's Save As started in the working folder**
  (10): the head keeps the folder, and a document with none starts in
  the home folder.
- **A recovered document showed nothing until every page's facts were
  read** (14): it opens as a file does now (Deviations); no gate row
  was added, since it's the staged path the `ready` gate measures.
- **Save at close mid-scan dropped the pages landing during the save**
  (16): `save_to_close` stops the scan and waits for it to end, then
  saves, and closes only if nothing is unsaved after.
- **The close flow had no test** (17): rejected in part. The decision
  is `is_dirty()` itself, shared by both paths, so no `must_ask` was
  factored out; the Save arm is tested through `save_to_close` (a
  mid-scan save, and a cancelled Save As, which `SHEAF_SCRIPT_SAVE`
  set empty now scripts); the prompt and its wiring are hand checks,
  and the closing test's doc says so.
- **A crashed write's temp file stayed forever** (19): `find_untitled`
  removes an unheld name's temp files (and a stale lock file).
- **The scan tests' helpers were `pub(crate)`** (20): private now.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The gates** (`gate_7k`, release, `--ignored --test-threads=1` in
  one run, started once the 1-minute load was under 4 on two checks 30
  s apart: 2.9 at its start and 3.0–3.5 through the heavy's timed gates;
  a spike to 17.8 during the scanned's, whose thumbnail strip isn't
  asserted, and 10.3 then 5.6 during the synthetic's, every asserted row
  passing with room; 12 of 12 tests pass, every asserted gate, both
  memory rows):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 49.3 ms; 55.1 ms | 79.3 ms; 368 ms | 62.2 ms; 468 ms |
  | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 11.9 ms (slice 1: 11.9) | 22.1 ms (21.3) | 33.3 ms (33.0) |
  | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s) | 423 pages/s | 352 pages/s | 141 pages/s (the load's spike) |
  | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 9.86 s | 1, 7,000; 9.97 s | 1, 7,000; 10.18 s |
  | one-mark Append on the scanned (asserted < 50 ms) | | | 24.4 ms (slice 1: 25.6) |
  | the Rewrite against slice 8's writer, the gates' own part (fails past 1.3×, flagged past 1.2×) | 149 ms, 1.11× | 803 ms, 1.14× | |

  The rest where slice 1 left them: history's 200 OCR versions of 1,000
  pages 236 MB (≤ 256 MB, asserted); snapshot 0.24–0.28 µs and
  `Tx::check` of a delete of every page 0.42 ms; engines after 220
  groups 28.9–29.0 MB weighed, ≤ 6 open; crop all pages 10.4–13.1 ms;
  Bates apply 0.62–0.70 ms, one font; AddMark 0.021 ms and a full
  reorder 0.97 ms at 20,000 marks; ready on 20,000 sheaf marks 519 ms;
  "Page n of N" moved after a save, the Append 471–686 ms (≤ 1 s),
  sections 1.36–3.33 MB (≤ 8 MB). Proposal 8's accepted misses
  (`SHEAF_GATE_PROPOSAL_8`): synthetic 22.7, 20.4, 19.2 and 29.8 ms,
  heavy 36.3, 32.4, 32.4 and 42.5 ms (slice 1: 21.6, 20.3, 18.9, 28.6;
  37.8, 35.2, 32.1, 42.1), the Appends on the file opened again 1,073
  and 1,517 ms (1,068 and 1,448; the scanned's 888 ms, asserted). The
  flatten export's RSS (`SHEAF_GATE_FLATTEN`): the heavy's peak 515 MB
  (slice 1: 513), the synthetic's 88.7 MB, the scanned's 1,076 MB. This
  slice's paths reach none of them, and the differences are the
  machine's. The Rewrite against slice 8's writer in the processes of
  `rewrites_on_the_synthetic_and_heavy_7k`: synthetic 137 ms (1.03×),
  heavy 786 ms (1.11×), under the 1.2× flag; this slice doesn't touch
  the writer, so no five-run median was taken.
- **The memory gate** (`scan_of_500_black_and_white_pages`, in the run
  above; load 5–13 through it): Photo 0.306 pages/s, 233 MB peak, 197
  MB added (asserted < 400 MB); Scanner 1.12 pages/s, 236 MB peak, 198
  MB added; both 10.4 MB written. The test's 2,963 s run (12 tests).
- **The untitled sidecars** (the gate's rows; no gate, the plan names
  none):

  | Scan | Untitled sidecar, written once | Sidecars in all, a batch of 8 | The app's in all (its batch at that pace) |
  |---|---|---|---|
  | 500 black and white pages, photo profile (0.306 pages/s) | 10.4 MB in 217 ms | 334 MB | 2,597 MB (1 a batch) |
  | 500 black and white pages, scanner profile (1.12 pages/s) | 10.4 MB in 33.2 ms | 334 MB | 872 MB (3 a batch) |
  | 500 color pages, scanner profile, JPEG 80, from 8 (1.54 pages/s) | 382 MB (the 8 pages' 6.10 MB in 107 ms, scaled) | 12,304 MB | 24,037 MB (4 a batch) |

  Each figure is one write (the two black and white times, of the same
  10.4 MB, vary with the disk). The app writes no sidecar while one is
  being written, so its figures are at most these: a color document's
  whole sidecar near its end takes about 6.7 s at the sample's rate, so
  later batches share writes, but the writes run back to back. In black
  and white a whole sidecar a batch is cheap; in color it writes 12–24
  GB over a 500-page scan. An incremental untitled sidecar is slice 7's
  (Left).
- **Step 1's baselines** (debug sheaf-cli, the old binary at
  `4481573`): `scan` of a PNG, of a two-page TIFF, `--mode bw`, `--mode
  gray`, and two images-to-pdf runs, all six outputs equal (cmp) old and
  new; the Full sidecar, written by a temporary test with the file's
  mtime pinned, for a memory-made document with no record and for one
  `opened_from` a file with its record unsound, equal old and new (the
  test removed, its file restored and checked with cmp). The wrap-up's
  final debug binary: the same six outputs equal the old's (cmp).
- **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
  no corpus" line), every count the baseline's: materialize_corpus's
  identity 4,389 of 4,392 written, 2,269,688 primary objects surviving,
  919,465 copied as they were, 432 changed by `keep_only`, 0 failed;
  2,901 forms without their first page, 0 failed; every page
  duplicated, 4,389, 0 failed; inserted twice into the 1040, 1,098, 0
  failed; four times, 0 differ; the logo, 0 of 2,037,600 bytes differ.
  append_corpus: 4,389 marked, 3,770 appended, 259 rewritten (211
  RemovalCheck, 34 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0
  failed, 0 derived vetoes and 211 on the files' own objects; the
  oracle finds text gone from 2,899 of 2,901 (none from 0016064 and
  0112654). encrypted_corpus: the Rewrite keeps 573 of 573 and the
  Append 572 (0630899 declined). source_corpus: 4,392 files, 24,251
  pages compared, 0 differ; forms_corpus passes. The ignored tiers are
  the pre-merge run's (Greg, 2026-10-20); this slice adds and changes
  none, and touches neither the writer nor the realizer (the Full
  sidecar's body is the same function's, step 1's cmp).
- **Suites:** `cargo test --workspace` 1,694 passed, 0 failed and 57
  ignored (slice 1: 1,682 and 57; this slice's 12 new tests), no
  "skipped: no corpus". fmt and clippy (`-D warnings`, all targets)
  clean, and clippy with sheaf-app's Linux-only gates switched off
  (`nonlinux-clippy.sh`); clippy (`-D warnings`, all targets) on
  `x86_64-pc-windows-msvc` passes for sheaf-doc and sheaf-ops;
  sheaf-plugin can't be built for Windows here, neither clippy nor
  check (wasmtime-internal-fiber and ring need `lib.exe`), and its
  change is a field made an `Option`; sheaf-app can't be checked for
  Windows here, as before. The interop tier (qpdf; pdf.js 6.4.299 on
  node 24.22): 19 of 19 with `--include-ignored`. `qpdf --check`: no
  errors on a Save As output (the create test's `scans.pdf`, 3 pages)
  and on an untitled sidecar's body with its two head lines stripped
  (the same test's sidecar before Save As, 3 pages), both copied aside
  by two temporary lines in the test, which was restored from its copy
  and checked with cmp. No proptest seed added.
- **After the review's fixes:** `cargo test --workspace` 1,698 passed,
  0 failed, 57 ignored (four new tests; tests/intake.rs' renamed), no
  "skipped: no corpus" (`fuzz_bodies_survive_mutated_fixtures` missed
  its 60 s once at a load of 71, then passed, Found); fmt, clippy
  (`-D warnings`, all targets), the non-Linux simulation, and Windows
  clippy for sheaf-util, sheaf-doc and sheaf-ops clean; materialize,
  append and encrypted corpus counts the baseline's, word for word;
  interop 19 of 19 with `--include-ignored`. The gates weren't run
  again then (the 1-minute load was 34–71 throughout, the other
  checkout's runs): `durable::write_parts` is now under every save's
  write (one `write_all` a part, the same bytes), and the untitled
  rows' sidecar goes to a `recovery/` beside the fixtures under a
  claim.
- **The final check's gate run** (after the review's fixes; release,
  `--ignored --test-threads=1` in one run, started at a load of 2.8
  after two checks under 4, 30 s apart): 11 of 12 pass. The miss is
  the scanned's reorder Append on the file opened again, 1,071 ms
  (asserted ≤ 1 s; 888 ms before the fixes), which `write_parts` can't
  reach (an Append in place goes on the file through
  `append_in_place`, not `durable::write`); `gates_on_the_scanned_7k`
  alone, three more times at loads under 4: 976, 838 and 889 ms, all
  passing, within slice 17's 0.89–1.07 s spread on the synthetic's
  same row. The rest: geometry walk 47.7 and 75.4 ms (< 100 and <
  250); ready on the heavy 360 ms (< 1 s); realize + open + render
  12.0 and 22.0 ms (< 15 and < 25); thumbnail strip 425 and 349
  pages/s (≥ 310); recognized text census 1 font, 7,000 blocks, the
  heavy's Append 9.95 s; the scanned's one-mark Append 39.2 ms in the
  run, 22.6 ms alone (< 50); history's 200 OCR versions 236 MB (≤ 256);
  the Rewrite against slice 8's writer 1.14× and 1.18× synthetic, 1.10×
  and 1.08× heavy (one run each, under the 1.2× flag; no five-run
  median, the writer's bytes unchanged); the scan memory gate 178 MB
  (photo) and 181 MB (scanner) added (< 400 MB); the untitled
  sidecars' rows 153 ms (photo), 41.8 ms (scanner) and 39.8 ms (the
  color sample), 10.4 MB each for 500 pages.

Checked by tests:
- **Untitled sidecars:** recovery.rs' `an_untitled_sidecar_round_trips`
  (the same page sizes and page 2's OCR word, `Memory(Recovered)`,
  dirty with nothing to undo, no record or file, still dirty after
  `forget_before_cursor`, a first save that's a Rewrite and leaves it
  clean), `find_untitled_removes_what_it_can_t_read`,
  `find_beside_a_file_takes_no_untitled_sidecar` (a real one, and an
  untitled head over lines that read as the file's own base);
  tests/intake.rs' `an_intake_s_untitled_sidecar_comes_back_as_a_session`
  (a 10-page gray OCR intake, its sidecar found and recovered: the
  name, 10 pages, dirty with nothing to undo, a Rewrite save, every page
  equal to the intake's save as PDFium reads it: render, text and
  character boxes).
- **No file yet in the plugin host:** `plugin_sees_the_call_context`
  (`path=None` with its selection and current page).
- **The end insert and the user's edits:**
  `a_relowered_insert_at_the_end_lands_beside_an_insert_a_delete_and_a_move`.
- **The app** (headless, Linux only):
  `create_from_scans_streams_into_an_untitled_document` (the title
  "Untitled scan 1 •", `path()` None, Properties with no path and the
  name, 3 pages, the empty version and one "Scan" by `Job(Scan)`, undo
  to 0 pages and clean and redo; nothing beside the images or in
  `out/`, the sidecar alone in the recovery folder; `save()` resolving
  true as Save As, 3 pages in the file, `path()`, `model.file()` and
  the title "scans.pdf" following it, clean, the recovery folder
  empty); `closing_an_untitled_document_asks_only_with_pages`
  (`is_dirty()` false with no pages, true after a scan, false after
  the undo; `file_name()` "Untitled scan 1");
  `undoing_a_scan_in_the_view_stops_it` (20 gated images, exactly 4
  pages, the undone notice); `a_batch_s_sidecar_is_written_as_it_lands`
  (9 images gated after 8: the write started in the read that first
  sees 8 pages, then the sidecar holding 8, and 9 once the gate opens;
  then a write in flight and an edit landed as a batch lands: no look
  finds no write running with the sidecar behind, until it's current);
  `a_scan_killed_after_its_first_batch_is_offered_at_the_next_launch`
  (while the child runs with its 8-page sidecar, the parent can't claim
  "Untitled scan 1" and `offered(dir, true)` is empty; the child
  SIGKILLed, signal 9 asserted; `offered(dir, false)` empty, and
  `offered(dir, true)` exactly ["Untitled scan 1"] with the images'
  folder; `recovered_untitled` read with 8 pages, "Untitled scan 1 •",
  dirty, no path, `folder()` the images'; saved, the sidecar gone; the
  image XObjects' bytes a `SessionLanding` intake's);
  `saving_to_close_mid_scan_keeps_every_page` (in a process of its own:
  20 gated images, `save_to_close` at 3 pages, then the gate opened:
  it resolves true, the scan over short of 20, the saved file holding
  every page the view does, clean); `a_cancelled_save_to_close_keeps_it_open`
  (`SHEAF_SCRIPT_SAVE` empty, which now cancels a save picker: false,
  untitled, dirty, the sidecar kept);
  `an_untitled_document_of_no_folder_saves_from_home` (`folder()` the
  home folder);
  `an_untitled_document_s_sidecar_follows_it` (there after a scan,
  gone after undo, back after redo; a write in flight, undo twice and
  the view dropped: none once the write ends, and its name claimable
  only once the write is done; a write in flight, Don't Save and the
  view dropped: the same; `save_to`: the recovery folder empty but for
  claims, and an edit writes the file's own sidecar); untitled.rs'
  `untitled_names_take_the_first_free_number` (every platform, real
  claims in a temp folder: gaps once a claim is let go, other kinds
  and files take none, a crash's sidecar takes its name, as a claim
  does); recovery.rs' `a_claimed_name_is_no_one_else_s` (a second claim
  refused while one's held, its sidecar and temp file not found then;
  let go, its lock file gone, found with no folder, the dead write's
  temp gone, claimed by the find until it's dropped).
  `an_untitled_sidecar_round_trips` now writes with a claim and a
  folder ("images ü") and reads it back, not found while claimed;
  `find_untitled_removes_what_it_can_t_read` adds a head cut short, a
  folder not hex, a body that isn't a PDF (found by its head, failing
  only at `restore_untitled`), an unheld name's temp and lock file
  (removed) and a temp whose name was cut short (left).
- **Fault checks,** each file restored from the step's own copy and
  checked with cmp, and no proptest seed added (only
  materialize/append_rewrite_tests.txt is there):
  - `never_saved` a no-op fails `an_untitled_sidecar_round_trips` and
    the session test at their dirty asserts; `parse` taking
    `"full" | UNTITLED` fails `find_beside_a_file_takes_no_untitled_sidecar`
    at its like-a-base case;
  - `path()` giving `Some("")` with no path fails the host test
    (`path=Some("")`);
  - committing without the group fails the create test (a timeline of
    4, not 2); the landing always replying Ok fails the undo test
    ("Scanned 20 pages…", not the undone notice); an untitled save
    written to `<dir>/<name>.pdf` fails the create test's path assert;
    `finish_save` leaving the place untitled fails it (`path()` None);
    `title()` ignoring the place fails it ("? •"); an end insert
    recording an order read makes the stale test a Conflict;
  - no `write_recovery` in `land_batch` (autosave alone) fails only the
    landing test ("the sidecar is written as the batch lands"); no
    `never_saved` in `Session::recovered` fails the kill test at the
    restored title ("Untitled scan 1", not "Untitled scan 1 •");
    `finish_save` not discarding fails the follow test ("saved, it
    keeps none"), the create test and the kill test; no flag store in
    `discard_recovery` fails the follow test ("none once it's
    written"), and so do no flag check in the background write and no
    release hook (its first case);
  - `made_pages` put back (a `pub(crate) fn made_pages() {}` in
    intake.rs) fails `nothing_retired_is_back` with
    "crates/sheaf-ops/src/intake.rs:432: intake::made_pages (retired in
    M5 slice 2)"; a `Created` variant put back in `ScanEvent` fails it
    with "scan_dialog::ScanEvent::Created (retired in M5 slice 2)".

- **Review's fault checks** (each file restored from the review's own
  copy and checked with cmp; no proptest seed added): a claim that
  ignores a held lock fails `a_claimed_name_is_no_one_else_s`,
  `an_untitled_sidecar_round_trips`, the names test, the kill test
  ("the live child's") and the follow test ("claimed while its write
  runs"); no temp sweep fails the claim test and the find test; an
  empty folder line fails the round trip, the find test and the kill
  test (its folder); no chained write fails the batch test ("the next
  write starts as the last ends"); no `stop_scan` in `save_to_close`
  fails the mid-scan test ("stopped"); `save_to_close` resolving true
  whatever the save did fails the cancelled test ("it stays open");
  `offered` ignoring `owns` fails the kill test; no `never_saved` in
  `ready` fails the kill test (its title); no home fallback fails the
  home test. The Full sidecar, through `write_parts` now, equals step
  1's baseline (the temporary test again, its file restored and
  checked with cmp). Not checked by a fault: `released()` and
  `write_untitled_recovery` leaving a recovered sidecar alone while
  it's still reading (closing in that window is a race no test can
  hold), and `save_to_close`'s dirtiness check after a save (an edit
  can't be put between a scripted picker and the save's end).

For Greg:
- **Hand checks** (a release build, or `SHEAF_SINGLE_INSTANCE=1` with
  a debug one, so the process owns sheaf's state; no test reaches
  these):
  - File > Create > From Scans/Photos… from an empty window: the pages
    stream into that window, titled "Untitled scan 1 •"; from a window
    showing a document: a new window. The status line reads "Scanning
    page k of N — Esc stops", with the bar, and ends "Scanned N pages —
    Ctrl+Z undoes".
  - Two scans: "Untitled scan 1" and "Untitled scan 2", each in its own
    window.
  - Ctrl+S on an untitled document opens Save As in the images' folder
    suggesting "Untitled scan 1.pdf"; Cancel keeps it untitled (title,
    no file); a save makes the title the file's name, and the file is
    in Open Recent.
  - Closing the window or quitting asks about an untitled document only
    when it has pages, naming "Untitled scan 1"; its Save goes to Save
    As (mid-scan, it stops the scan first, then asks where; a
    cancelled Save As keeps the window open, and a quit stops); Don't Save closes it and leaves nothing under
    `~/.local/state/sheaf/recovery/`; a scan whose every page came out
    blank closes without asking.
  - Esc mid-scan stops it and keeps the pages so far; Ctrl+Z mid-scan
    undoes it and stops it ("Scan stopped: its pages were undone");
    closing the window mid-scan stops it.
  - Properties on an untitled document: File "Untitled scan 1",
    Location "Not saved yet".
  - Kill -9 sheaf mid-scan, then start it again: "Recover the unsaved
    scan \"Untitled scan 1\"?" in the first window. Recover shows it in
    an empty window or a new one, "Untitled scan 1 •", with "Recovered
    unsaved pages — Ctrl+S to save them", its pages those landed before
    the kill; Ctrl+S opens the picker in the images' folder. Discard
    removes it. With several, the prompts come one at a time; a fully
    undone scan closed leaves none to offer; a second sheaf started
    beside a running one (`--new-instance`) offers nothing, and its
    own scan is "Untitled scan 2" while the first sheaf's "Untitled
    scan 1" is open (before its first batch too); quit the first and
    start sheaf again while the second runs: the second's scan isn't
    offered.
  - An OCR scan with the OCR data missing: the scan stops at once and
    the notice says why (`jobs::ended("Scan", …)`), with the untitled
    window left empty.
- **The untitled sidecar's cost on a long scan** (Measured): a whole
  document a batch is cheap in black and white and large in color;
  slice 7 is where an incremental untitled sidecar would go (Left).

Left for later slices:
- **Slice 4:** File > Create > Separator Sheets… makes a new untitled
  document: `untitled::next(kind)` takes the kind ("Untitled
  separators", say) and gives the name's claim, and a constructor like
  `for_scan` gives it a folder (none: the sheets come from no files,
  so Save As starts in the home folder);
  `DocView::untitled` and the recovery code take any `Untitled`. The
  launch offer's text says "scan" today ("Recover the unsaved scan …"),
  which a second kind should change.
- **Slice 7:** Insert from Scanner streams into the open document:
  `start_scan(open, opts, ocr, batching, cx)` lands at the end only
  (`apply_batch`'s `insert_at_end`); a `before` page means an insert
  that reads the order, so the user's delete or move of that page meets
  a `Relower` commit, and the Conflict re-plan this slice didn't need
  comes back. `land_batch` already calls `write_recovery_at_once`, so a
  file's sidecar is written as each batch lands too, the Append section
  of every page scanned so far; but only the untitled write's
  completion starts the next write at once (`sidecar_due`), so a
  file's completion should do the same once its batches land. And the untitled sidecar's cost: each
  write is the whole document, so over a long color scan the writes
  add up to many gigabytes (Measured); an incremental untitled sidecar
  (the batches since the last one, as a file's Append section is) is
  the fix if the figures stand.
- **Slices 6 and 7:** a device's scan into an untitled document is
  recoverable after its first batch, through this slice's sidecar; a
  device's page in flight at a crash is lost, as slice 1 left it.
